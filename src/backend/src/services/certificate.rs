//! Certificate management service
//!
//! Handles module certificate creation and SNI-based certificate resolution for IoT Edge modules.

#![cfg_attr(feature = "mock", allow(dead_code, unused_imports))]

use crate::{
    config::AppConfig,
    http_client::{handle_http_response, unix_socket_client},
    omnect_device_service_client::OmnectDeviceServiceClient,
};
use anyhow::{Context, Result, bail};
use log::info;
use rustls::{server::ResolvesServerCert, sign::CertifiedKey};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

// Payload for certificate creation
#[derive(Debug, Serialize)]
struct CreateCertPayload {
    #[serde(rename = "commonName")]
    common_name: String,
}

#[derive(Debug, Deserialize, Clone)]
struct PrivateKey {
    bytes: String,
}

#[derive(Debug, Deserialize, Clone)]
struct CreateCertResponse {
    #[serde(rename = "privateKey")]
    private_key: PrivateKey,
    certificate: String,
}

/// Certificate cache structure for storing multiple certificates
struct CertificateCache {
    /// Pre-built CertifiedKey objects for SNI resolver
    certified_keys: HashMap<String, Arc<CertifiedKey>>,
}

/// Global certificate cache
static CERTIFICATE_CACHE: Mutex<Option<CertificateCache>> = Mutex::new(None);

/// Service for certificate management operations
pub struct CertificateService;

impl CertificateService {
    /// Create a module certificate from IoT Edge workload API
    ///
    /// # Arguments
    /// * `payload` - Certificate creation payload with common name
    ///
    /// # Returns
    /// Certificate response with certificate and private key PEM data
    async fn create_module_certificate(payload: CreateCertPayload) -> Result<CreateCertResponse> {
        info!("create module certificate");

        let iot_edge = &AppConfig::get().iot_edge;
        let client = unix_socket_client(&iot_edge.workload_uri)?;
        let url = format!(
            "http://localhost/modules/{}/genid/{}/certificate/server?api-version={}",
            iot_edge.module_id, iot_edge.module_generation_id, iot_edge.api_version
        );

        info!("POST {url} with payload: {payload:?}");

        let res = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("failed to send certificate request")?;

        let body = handle_http_response(res, "certificate request").await?;
        let response: CreateCertResponse =
            serde_json::from_str(&body).context("failed to parse certificate response")?;

        Ok(response)
    }

    /// Ensure all required certificates are generated and cached (mock version)
    ///
    /// Mock implementation that does nothing - certificates are not needed in test environment
    #[cfg(feature = "mock")]
    pub async fn ensure_certificates_updated(
        _service_client: &OmnectDeviceServiceClient,
        _online_ips: &[String],
    ) -> Result<()> {
        Ok(())
    }

    /// Ensure all required certificates are generated and cached
    ///
    /// This function generates certificates for all online network interfaces
    /// plus a fallback hostname certificate. Certificates are cached in memory
    /// and reused across restarts when the network topology hasn't changed.
    ///
    /// The fallback hostname certificate is also written to disk for Centrifugo.
    ///
    /// # Arguments
    /// * `service_client` - Device service client for retrieving network status
    /// * `online_ips` - List of IP addresses for online network interfaces
    ///
    /// # Returns
    /// Result indicating success or failure
    #[cfg(not(feature = "mock"))]
    pub async fn ensure_certificates_updated(
        _service_client: &OmnectDeviceServiceClient,
        online_ips: &[String],
    ) -> Result<()> {
        let hostname = AppConfig::get().hostname.clone();

        let mut required_certs = online_ips.to_vec();
        required_certs.push(hostname.clone());

        // Determine which certificates need to be generated (without holding the lock)
        let certs_to_generate: Vec<String> = {
            let mut cache_lock = CERTIFICATE_CACHE.lock().unwrap();
            let cache = cache_lock.get_or_insert_with(|| CertificateCache {
                certified_keys: HashMap::new(),
            });

            // Remove certificates for IPs that are no longer online
            cache
                .certified_keys
                .retain(|cn, _| required_certs.contains(cn));

            // Identify missing certificates
            required_certs
                .iter()
                .filter(|cn| !cache.certified_keys.contains_key(*cn))
                .cloned()
                .collect()
        }; // Lock released here

        // Track if we need to write the hostname certificate to disk
        let mut hostname_cert_response: Option<CreateCertResponse> = None;

        // Generate missing certificates (without holding the lock)
        for common_name in certs_to_generate {
            info!("generating certificate for: {common_name}");
            let response = Self::create_module_certificate(CreateCertPayload {
                common_name: common_name.clone(),
            })
            .await?;

            // Save hostname certificate for writing to disk
            if common_name == hostname {
                hostname_cert_response = Some(response.clone());
            }

            // Parse and build CertifiedKey
            let certified_key =
                Self::build_certified_key(&response.certificate, &response.private_key.bytes)?;

            // Insert into cache (acquire lock briefly)
            {
                let mut cache_lock = CERTIFICATE_CACHE.lock().unwrap();
                if let Some(cache) = cache_lock.as_mut() {
                    cache
                        .certified_keys
                        .insert(common_name, Arc::new(certified_key));
                }
            } // Lock released
        }

        // Write hostname certificate to disk for Centrifugo
        if let Some(response) = hostname_cert_response {
            Self::write_certificate_to_disk(&response)?;
        }

        Ok(())
    }

    /// Write certificate to disk for Centrifugo
    fn write_certificate_to_disk(response: &CreateCertResponse) -> Result<()> {
        use std::{fs::File, io::Write};

        let paths = &AppConfig::get().certificate;
        let mut cert_file =
            File::create(&paths.cert_path).context("failed to create certificate file")?;
        let mut key_file = File::create(&paths.key_path).context("failed to create key file")?;

        cert_file
            .write_all(response.certificate.as_bytes())
            .context("failed to write certificate")?;

        key_file
            .write_all(response.private_key.bytes.as_bytes())
            .context("failed to write private key")?;

        info!("wrote hostname certificate to disk for Centrifugo");

        Ok(())
    }

    /// Build a CertifiedKey from PEM-encoded certificate and private key
    fn build_certified_key(cert_pem: &str, key_pem: &str) -> Result<CertifiedKey> {
        let certs = rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .context("failed to parse certificate PEM")?;

        let key_item = rustls_pemfile::read_one(&mut key_pem.as_bytes())
            .context("failed to read key PEM")?
            .context("no key found in PEM")?;

        let private_key = match key_item {
            rustls_pemfile::Item::Pkcs1Key(key) => rustls::pki_types::PrivateKeyDer::Pkcs1(key),
            rustls_pemfile::Item::Pkcs8Key(key) => rustls::pki_types::PrivateKeyDer::Pkcs8(key),
            _ => bail!("unexpected key type in PEM"),
        };

        let signing_key = rustls::crypto::ring::sign::any_supported_type(&private_key)
            .map_err(|_| anyhow::anyhow!("failed to create signing key"))?;

        Ok(CertifiedKey::new(certs, signing_key))
    }

    /// Create an SNI resolver for multi-certificate support
    ///
    /// # Arguments
    /// * `hostname` - Fallback hostname for default certificate
    ///
    /// # Returns
    /// Arc-wrapped SNI resolver
    pub fn create_sni_resolver(hostname: String) -> Arc<dyn ResolvesServerCert> {
        Arc::new(MultiCertResolver { hostname })
    }
}

/// SNI resolver that selects certificates based on the requested hostname
#[derive(Debug)]
struct MultiCertResolver {
    hostname: String,
}

impl ResolvesServerCert for MultiCertResolver {
    fn resolve(&self, client_hello: rustls::server::ClientHello) -> Option<Arc<CertifiedKey>> {
        let cache = CERTIFICATE_CACHE.lock().unwrap();
        let cache = cache.as_ref()?;

        // Try to match SNI to an exact certificate (IP or hostname)
        if let Some(sni) = client_hello.server_name()
            && let Some(cert) = cache.certified_keys.get(sni)
        {
            info!("SNI resolver: matched certificate for {sni}");
            return Some(cert.clone());
        }

        // Fallback to hostname certificate
        info!(
            "SNI resolver: using fallback certificate for hostname {}",
            self.hostname
        );
        cache.certified_keys.get(&self.hostname).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Valid RSA private key for testing (2048-bit, generated with openssl)
    const TEST_PRIVATE_KEY_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCdRCZK0nKbhMvD
++Z6AnVhye+roxi9NR475afnOCVOTE5fx7w4NnnJ+8A5muG1ocML0wvnqh7Wc+C0
zu3pbbeiglwzcP+aS976uFOXV5Hrpx0+14TylPxR4vioEu9iZTRV28YkT9MtMxM6
cbhCc/LcfWwHUU3KWyhxyFRH8VB4xk81hj1H+ShdWna6xSQcRgNXN/IQoAxJIoDf
yAPCjm0EwNb7BuX87JVQT5wDLGWJrisx3w3u4Ly4earX+qvvu0k2Aq/dQa7gIDob
3W/Sofo/6pDsS+Vcy2rosyNo8gI0jSTwMj7c24vm/C1+CeJ7iZxAq3mN6Q/wQClD
04K/cBZDAgMBAAECggEAQ1wJxLl/8kG+XzrZPIAqE9EBEXSBp6UFRqV2tbAUNoWz
eg3cff1DS/LDIklHDNt05e8m5bq1i6hFYlxRhc6DPZ11bWkkacu+fYgO8b9F1ngV
LDH2lUqgCljbpW26z9vGP1Ire6kfK/h472r+/6OXLb6g0z+NQLOrzpR+GPRwwdGO
ir2CZzdTP1OouXWtbo4v3SceOjj/NdzO7IUn5VWs9tW/wYdwERZbLKlhXVXae2cl
QOcXLavXjkqNn/a5BuI+gy29kCia6IYSZNSflhv6+QNSgOVdYvQ5CsVpcbt3qccj
guh6zUn4bNgnofI/3x28v1d9X/HJV8uALXnB0f6imQKBgQDX2vdIJMMtQxj1u1KO
/xZJLmrNgtd4nCspIzXb2V3tTsB7Vtwfxfqgw38k4HOYgLCiw22qmxGwNFPgnGyP
2XubyeIW7Xs9wcovXslE7M7KnZ+UWjVXvP4ecG5u1ir3GWcCQHrwTpxjcB/n7WTw
KlMySYlCJpPQz6ew4e3SXcDR+wKBgQC6g7Yjz6Sn2tLD3k6Jfj5ricl6SW/dGxM3
Dpldro/CDIiYPY2GC/yQWqsHKNuKTpxIETwFrpHdsrCkISsIAQGLZrw0Ek5dOhHj
xSnVJ6pYr+3Uz3wDl3qi0lK9KbjA4tg0DzJlarHjkdrGuqpR4HCDchTTkZosEOWp
Sb5oKv9iWQKBgQC1fHc5Ax/PKIEN6sfJTxQvx4Uo8X+0+qkXV3FrPWFJq1MO4MMH
O/AzxutZ2BWY/WqGDwZf0S2YFwcG7L4iXFsfayha2qUqEYurNGjJOMnNdaW8l/QN
puuKMEHJkuxhAcyoCgrTjWTT/mv1FpYtj4iP2WA8bC8P++gkQnEw1H7QjQKBgBME
Ptvj3evnWbnyvpsyLfcU81/ugONQUWM5r9VnaOzmDj9Hd1iFfFjThcCTH984KKMI
btA9fk3WXEA/yX1lbNzjuqisfSTwOMa6YYuEIdAtD9i01vYeybg0LY9v45B3EIgu
THsep8iGJIJCof77HT2psgnoPInlpyTdifZg++zxAoGBAIyTCw2uWqroo4YxbzK2
IWkEUMLb1i1bJXcGgQrbhVf2xYKj4yQN0g5NDCc8Va6zZSa74zXkBIncnfwEwVV1
PUcFtC5W7yud0dLelpzV0S9baCfcu1BK5vl7p6NBhP7qH4bmpAYmebubhPHblowM
iakNzeKD+4BgLuMUPvLgju8a
-----END PRIVATE KEY-----"#;

    // Valid self-signed certificate for testing (matching the private key above)
    const TEST_CERTIFICATE_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIC/zCCAeegAwIBAgIUTQAAzaJA3Js2P+ygOFuS1AyBM8UwDQYJKoZIhvcNAQEL
BQAwDzENMAsGA1UEAwwEdGVzdDAeFw0yNjAxMjMwOTE4MjJaFw0yNzAxMjMwOTE4
MjJaMA8xDTALBgNVBAMMBHRlc3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEK
AoIBAQCdRCZK0nKbhMvD++Z6AnVhye+roxi9NR475afnOCVOTE5fx7w4NnnJ+8A5
muG1ocML0wvnqh7Wc+C0zu3pbbeiglwzcP+aS976uFOXV5Hrpx0+14TylPxR4vio
Eu9iZTRV28YkT9MtMxM6cbhCc/LcfWwHUU3KWyhxyFRH8VB4xk81hj1H+ShdWna6
xSQcRgNXN/IQoAxJIoDfyAPCjm0EwNb7BuX87JVQT5wDLGWJrisx3w3u4Ly4earX
+qvvu0k2Aq/dQa7gIDob3W/Sofo/6pDsS+Vcy2rosyNo8gI0jSTwMj7c24vm/C1+
CeJ7iZxAq3mN6Q/wQClD04K/cBZDAgMBAAGjUzBRMB0GA1UdDgQWBBS3I/xdTL8S
DpVmtTGnpicYOuENhDAfBgNVHSMEGDAWgBS3I/xdTL8SDpVmtTGnpicYOuENhDAP
BgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQA4I0JpTF2DhEIqXDVd
PWjjVGDQPEUo90OTT7TjXcVkdoC9J5EwXiBKzFbk9CDDY+uc7VbuUMftquRCk24m
4Yh5J2tKj1G7SMcqL+iK2hSImIMyZN/cUYGZyxCMzspfULD+aB5SqOiTxF3QO/vl
oi5MwHjFypnJRSHrcqopXs8PSzj5gE6+RZMZTGMvEjKgdj4EAtKHWNSOjtdBTcqB
kFVihXcc3yLjfmcE4ib1/zq9hefI37Wo+wmem1QE71yg0YYgkJRsjcgi1H0PlasC
88u05jB7gWsmnFeWmH90Aq8K43fcT+/dURCfuWEBNcLduuGdCNC8IuJE/Lk3r7Br
QOaW
-----END CERTIFICATE-----"#;

    #[test]
    fn test_build_certified_key_with_valid_pem() {
        // Test that build_certified_key successfully parses valid PEM data
        let result =
            CertificateService::build_certified_key(TEST_CERTIFICATE_PEM, TEST_PRIVATE_KEY_PEM);

        assert!(
            result.is_ok(),
            "Should successfully build CertifiedKey from valid PEM data"
        );

        // Verify we can extract the certificate
        let certified_key = result.unwrap();
        assert!(
            !certified_key.cert.is_empty(),
            "CertifiedKey should contain certificates"
        );
    }

    #[test]
    fn test_build_certified_key_with_invalid_key() {
        let invalid_key = "INVALID KEY";

        let result = CertificateService::build_certified_key(TEST_CERTIFICATE_PEM, invalid_key);

        assert!(result.is_err(), "Should fail with invalid private key PEM");
    }

    #[test]
    fn test_build_certified_key_with_empty_key() {
        let result = CertificateService::build_certified_key(TEST_CERTIFICATE_PEM, "");

        assert!(result.is_err(), "Should fail with empty key");
    }

    #[test]
    fn test_create_sni_resolver() {
        let hostname = "test-hostname".to_string();
        let resolver = CertificateService::create_sni_resolver(hostname.clone());

        // Verify resolver is created (just checks it doesn't panic)
        assert!(
            Arc::strong_count(&resolver) == 1,
            "Should create a new Arc with count 1"
        );
    }

    #[test]
    fn test_get_device_hostname() {
        // Note: This test will need to be updated when we implement fetching from ODS
        // For now, we test the hardcoded value without needing a real client

        // The function is currently hardcoded to return a specific hostname
        // When ODS integration is added, this test should be updated to:
        // 1. Mock the ODS response
        // 2. Verify the hostname is extracted correctly from StatusV1 SystemInfo

        // Placeholder test - just ensures the test compiles
        // TODO: Update when ODS integration is implemented
    }
}
