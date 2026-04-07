//! Certificate management service
//!
//! Handles module certificate creation and persistence for `IoT` Edge modules.

#![cfg_attr(feature = "mock", allow(dead_code, unused_imports))]

use crate::{
    config::AppConfig,
    http_client::{handle_http_response, unix_socket_client},
};
use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Write};

// Public payload for passing to certificate creation
#[derive(Debug, Serialize)]
pub struct CreateCertPayload {
    #[serde(rename = "commonName")]
    pub common_name: String,
}

#[derive(Debug, Deserialize)]
struct PrivateKey {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    type_name: String,
    bytes: String,
}

#[derive(Debug, Deserialize)]
struct CreateCertResponse {
    #[serde(rename = "privateKey")]
    private_key: PrivateKey,
    certificate: String,
    #[allow(dead_code)]
    expiration: String,
}

/// Service for certificate management operations
pub struct CertificateService;

impl CertificateService {
    /// Create a module certificate from IoT Edge workload API
    ///
    /// # Arguments
    /// * `service_client` - Device service client for retrieving IP address
    ///
    /// # Returns
    /// Result indicating success or failure
    #[cfg(feature = "mock")]
    #[allow(clippy::unused_async)] // signature must match non-mock async version
    pub async fn create_module_certificate(_payload: CreateCertPayload) -> Result<()> {
        Ok(())
    }

    /// Create a module certificate from `IoT` Edge workload API
    ///
    /// # Arguments
    /// * `service_client` - Device service client for retrieving IP address
    ///
    /// # Returns
    /// Result indicating success or failure
    #[cfg(not(feature = "mock"))]
    pub async fn create_module_certificate(payload: CreateCertPayload) -> Result<()> {
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
        let paths = &AppConfig::get().certificate;
        let mut cert_file =
            File::create(&paths.cert_path).context("failed to create certificate file")?;
        let mut key_file = File::create(&paths.key_path).context("failed to create key file")?;

        cert_file
            .write_all(response.certificate.as_bytes())
            .context("failed to write certificate")?;

        key_file
            .write_all(response.private_key.bytes.as_bytes())
            .context("failed to write private key")
    }
}
