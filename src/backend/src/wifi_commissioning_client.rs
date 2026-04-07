#![cfg_attr(feature = "mock", allow(dead_code, unused_imports))]

use crate::http_client::{handle_http_response, unix_socket_client};
use anyhow::{Context, Result};
use log::info;
#[cfg(feature = "mock")]
use mockall::automock;
pub use omnect_ui_core::types::{
    WifiAvailability, WifiConnectRequest, WifiConnectResponse, WifiDisconnectResponse,
    WifiForgetRequest, WifiForgetResponse, WifiSavedNetworksResponse, WifiScanResultsResponse,
    WifiScanStartedResponse, WifiStatusResponse, WifiVersionResponse,
};
use reqwest::Client;
use serde::Serialize;
use std::{fmt::Debug, path::Path};
use trait_variant::make;

// --- Client trait ---

#[make(Send)]
#[cfg_attr(feature = "mock", automock)]
pub trait WifiCommissioningClient {
    async fn scan(&self) -> Result<WifiScanStartedResponse>;
    async fn scan_results(&self) -> Result<WifiScanResultsResponse>;
    async fn connect(&self, request: WifiConnectRequest) -> Result<WifiConnectResponse>;
    async fn disconnect(&self) -> Result<WifiDisconnectResponse>;
    async fn status(&self) -> Result<WifiStatusResponse>;
    async fn saved_networks(&self) -> Result<WifiSavedNetworksResponse>;
    async fn forget_network(&self, request: WifiForgetRequest) -> Result<WifiForgetResponse>;
    async fn version(&self) -> Result<WifiVersionResponse>;
}

#[cfg(feature = "mock")]
impl Clone for MockWifiCommissioningClient {
    fn clone(&self) -> Self {
        Self::new()
    }
}

// --- Client implementation ---

#[derive(Clone)]
pub struct WifiCommissioningServiceClient {
    client: Client,
}

impl WifiCommissioningServiceClient {
    const SCAN_ENDPOINT: &str = "/api/v1/scan";
    const SCAN_RESULTS_ENDPOINT: &str = "/api/v1/scan/results";
    const CONNECT_ENDPOINT: &str = "/api/v1/connect";
    const DISCONNECT_ENDPOINT: &str = "/api/v1/disconnect";
    const STATUS_ENDPOINT: &str = "/api/v1/status";
    const NETWORKS_ENDPOINT: &str = "/api/v1/networks";
    const FORGET_ENDPOINT: &str = "/api/v1/networks/forget";
    const VERSION_ENDPOINT: &str = "/api/v1/version";

    pub const MIN_REQUIRED_VERSION: semver::Version = semver::Version::new(0, 1, 0);

    pub async fn check_availability(&self) -> WifiAvailability {
        let status_result = self.status().await;
        let version_result = self.version().await;

        match (status_result, version_result) {
            (Ok(status), Ok(version_response)) => {
                let is_version_compatible = match semver::Version::parse(&version_response.version)
                {
                    Ok(v) => v >= Self::MIN_REQUIRED_VERSION,
                    Err(e) => {
                        log::warn!(
                            "Failed to parse WiFi service version '{}': {}",
                            version_response.version,
                            e
                        );
                        false
                    }
                };

                if is_version_compatible {
                    log::info!(
                        "WiFi service available (version {})",
                        version_response.version
                    );

                    if let Some(interface_name) = status.interface_name {
                        return WifiAvailability::Available {
                            version: version_response.version,
                            interface_name,
                        };
                    }
                    log::error!("WiFi service reported OK status but no interface name");
                } else {
                    log::warn!(
                        "WiFi service version '{}' is lower than required minimum {}",
                        version_response.version,
                        Self::MIN_REQUIRED_VERSION
                    );
                }

                WifiAvailability::Unavailable {
                    socket_present: true,
                    version: Some(version_response.version),
                    min_required_version: Self::MIN_REQUIRED_VERSION.to_string(),
                }
            }
            (Err(e), _) => {
                log::error!("WiFi service status probe failed: {e:#}");
                WifiAvailability::Unavailable {
                    socket_present: true,
                    version: None,
                    min_required_version: Self::MIN_REQUIRED_VERSION.to_string(),
                }
            }
            (_, Err(e)) => {
                log::error!("WiFi service version probe failed: {e:#}");
                WifiAvailability::Unavailable {
                    socket_present: true,
                    version: None,
                    min_required_version: Self::MIN_REQUIRED_VERSION.to_string(),
                }
            }
        }
    }

    /// Try to create a client. Returns `None` if the socket does not exist.
    #[must_use]
    pub fn try_new(socket_path: &Path) -> Option<Self> {
        let path_str = socket_path.to_string_lossy();

        if !socket_path.exists() {
            info!("WiFi socket not found at {path_str}, WiFi management disabled");
            return None;
        }

        match unix_socket_client(&path_str) {
            Ok(client) => {
                info!("WiFi commissioning client created for socket {path_str}");
                Some(Self { client })
            }
            Err(e) => {
                log::error!("Failed to create WiFi socket client at {path_str}: {e:#}");
                None
            }
        }
    }

    fn build_url(path: &str) -> String {
        let normalized = path.trim_start_matches('/');
        format!("http://localhost/{normalized}")
    }

    async fn get(&self, path: &str) -> Result<String> {
        let url = Self::build_url(path);
        info!("WiFi GET {url}");

        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context(format!("failed to send GET to {url}"))?;

        handle_http_response(res, &format!("WiFi GET {url}")).await
    }

    async fn post(&self, path: &str) -> Result<String> {
        let url = Self::build_url(path);
        info!("WiFi POST {url}");

        let res = self
            .client
            .post(&url)
            .send()
            .await
            .context(format!("failed to send POST to {url}"))?;

        handle_http_response(res, &format!("WiFi POST {url}")).await
    }

    async fn post_json(&self, path: &str, body: impl Debug + Serialize) -> Result<String> {
        let url = Self::build_url(path);
        info!("WiFi POST {url} with body: {body:?}");

        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context(format!("failed to send POST to {url}"))?;

        handle_http_response(res, &format!("WiFi POST {url}")).await
    }
}

impl WifiCommissioningClient for WifiCommissioningServiceClient {
    async fn scan(&self) -> Result<WifiScanStartedResponse> {
        let body = self.post(Self::SCAN_ENDPOINT).await?;
        serde_json::from_str(&body).context("failed to parse scan response")
    }

    async fn scan_results(&self) -> Result<WifiScanResultsResponse> {
        let body = self.get(Self::SCAN_RESULTS_ENDPOINT).await?;
        serde_json::from_str(&body).context("failed to parse scan results")
    }

    async fn connect(&self, request: WifiConnectRequest) -> Result<WifiConnectResponse> {
        let body = self.post_json(Self::CONNECT_ENDPOINT, request).await?;
        serde_json::from_str(&body).context("failed to parse connect response")
    }

    async fn disconnect(&self) -> Result<WifiDisconnectResponse> {
        let body = self.post(Self::DISCONNECT_ENDPOINT).await?;
        serde_json::from_str(&body).context("failed to parse disconnect response")
    }

    async fn status(&self) -> Result<WifiStatusResponse> {
        let body = self.get(Self::STATUS_ENDPOINT).await?;
        serde_json::from_str(&body).context("failed to parse status response")
    }

    async fn saved_networks(&self) -> Result<WifiSavedNetworksResponse> {
        let body = self.get(Self::NETWORKS_ENDPOINT).await?;
        serde_json::from_str(&body).context("failed to parse saved networks response")
    }

    async fn forget_network(&self, request: WifiForgetRequest) -> Result<WifiForgetResponse> {
        let body = self.post_json(Self::FORGET_ENDPOINT, request).await?;
        serde_json::from_str(&body).context("failed to parse forget response")
    }

    async fn version(&self) -> Result<WifiVersionResponse> {
        let body = self.get(Self::VERSION_ENDPOINT).await?;
        serde_json::from_str(&body).context("failed to parse version response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod build_url {
        use super::*;

        #[test]
        fn normalizes_path_with_leading_slash() {
            let url = WifiCommissioningServiceClient::build_url("/api/v1/scan");
            assert_eq!(url, "http://localhost/api/v1/scan");
        }

        #[test]
        fn normalizes_path_without_leading_slash() {
            let url = WifiCommissioningServiceClient::build_url("api/v1/scan");
            assert_eq!(url, "http://localhost/api/v1/scan");
        }
    }

    mod dto_serialization {
        use super::*;

        #[test]
        fn connect_request_serializes_correctly() {
            let req = WifiConnectRequest {
                ssid: "MyNetwork".to_string(),
                psk: "a".repeat(64),
            };
            let json = serde_json::to_string(&req).unwrap();
            assert!(json.contains("\"ssid\":\"MyNetwork\""));
            assert!(json.contains("\"psk\":\""));
        }

        #[test]
        fn forget_request_serializes_correctly() {
            let req = WifiForgetRequest {
                ssid: "OldNetwork".to_string(),
            };
            let json = serde_json::to_string(&req).unwrap();
            assert!(json.contains("\"ssid\":\"OldNetwork\""));
        }

        #[test]
        fn status_response_deserializes_with_all_fields() {
            let json = r#"{"status":"ok","state":"connected","ssid":"MyNet","ip_address":"192.168.1.100","interface_name":"wlan0"}"#;
            let resp: WifiStatusResponse = serde_json::from_str(json).unwrap();
            assert_eq!(resp.state, "connected");
            assert_eq!(resp.ssid.as_deref(), Some("MyNet"));
            assert_eq!(resp.ip_address.as_deref(), Some("192.168.1.100"));
            assert_eq!(resp.interface_name.as_deref(), Some("wlan0"));
        }

        #[test]
        fn status_response_deserializes_without_optional_fields() {
            let json = r#"{"status":"ok","state":"idle","ssid":null,"ip_address":null,"interface_name":"wlan0"}"#;
            let resp: WifiStatusResponse = serde_json::from_str(json).unwrap();
            assert_eq!(resp.state, "idle");
            assert!(resp.ssid.is_none());
            assert!(resp.ip_address.is_none());
            assert_eq!(resp.interface_name.as_deref(), Some("wlan0"));
        }

        #[test]
        fn scan_results_deserializes_network_list() {
            let json = r#"{"status":"ok","state":"finished","networks":[{"ssid":"Net1","mac":"aa:bb:cc:dd:ee:ff","ch":6,"rssi":-55}]}"#;
            let resp: WifiScanResultsResponse = serde_json::from_str(json).unwrap();
            assert_eq!(resp.state, "finished");
            assert_eq!(resp.networks.len(), 1);
            assert_eq!(resp.networks[0].ssid, "Net1");
            assert_eq!(resp.networks[0].ch, 6);
            assert_eq!(resp.networks[0].rssi, -55);
        }

        #[test]
        fn saved_networks_deserializes_with_flags() {
            let json = r#"{"status":"ok","networks":[{"ssid":"Home","flags":"[CURRENT]"},{"ssid":"Work","flags":""}]}"#;
            let resp: WifiSavedNetworksResponse = serde_json::from_str(json).unwrap();
            assert_eq!(resp.networks.len(), 2);
            assert_eq!(resp.networks[0].flags, "[CURRENT]");
            assert!(resp.networks[1].flags.is_empty());
        }
    }

    mod try_new {
        use super::*;

        #[test]
        fn returns_none_for_nonexistent_socket() {
            let result =
                WifiCommissioningServiceClient::try_new(Path::new("/tmp/nonexistent.sock"));
            assert!(result.is_none());
        }
    }

    mod constants {
        use super::*;

        #[test]
        fn api_endpoints_are_correctly_defined() {
            assert_eq!(
                WifiCommissioningServiceClient::SCAN_ENDPOINT,
                "/api/v1/scan"
            );
            assert_eq!(
                WifiCommissioningServiceClient::SCAN_RESULTS_ENDPOINT,
                "/api/v1/scan/results"
            );
            assert_eq!(
                WifiCommissioningServiceClient::CONNECT_ENDPOINT,
                "/api/v1/connect"
            );
            assert_eq!(
                WifiCommissioningServiceClient::DISCONNECT_ENDPOINT,
                "/api/v1/disconnect"
            );
            assert_eq!(
                WifiCommissioningServiceClient::STATUS_ENDPOINT,
                "/api/v1/status"
            );
            assert_eq!(
                WifiCommissioningServiceClient::NETWORKS_ENDPOINT,
                "/api/v1/networks"
            );
            assert_eq!(
                WifiCommissioningServiceClient::FORGET_ENDPOINT,
                "/api/v1/networks/forget"
            );
            assert_eq!(
                WifiCommissioningServiceClient::VERSION_ENDPOINT,
                "/api/v1/version"
            );
        }
    }
}
