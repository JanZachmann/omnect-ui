#![cfg_attr(feature = "mock", allow(dead_code, unused_imports))]

use crate::http_client::{handle_http_response, unix_socket_client};
use anyhow::{Context, Result};
use log::info;
#[cfg(feature = "mock")]
use mockall::automock;
pub use omnect_ui_core::types::{
    VersionInfo, WifiAvailability, WifiConnectRequest, WifiConnectResponse, WifiDisconnectResponse,
    WifiForgetRequest, WifiForgetResponse, WifiSavedNetworksResponse, WifiScanResultsResponse,
    WifiScanStartedResponse, WifiServiceInfoResponse, WifiStatusResponse, WifiVersionResponse,
};
use reqwest::Client;
use semver::{Version, VersionReq};
use serde::Serialize;
use std::{fmt::Debug, path::Path, sync::OnceLock};
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
    async fn service_info(&self) -> Result<WifiServiceInfoResponse>;
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
    const SERVICE_INFO_ENDPOINT: &str = "/api/v1/service-info";

    // The floor is the oldest wifi-commissioning-service serving
    // /api/v1/service-info, which is the availability probe used here.
    const REQUIRED_CLIENT_VERSION: &str = ">=0.2.1";

    fn required_version() -> &'static VersionReq {
        static REQUIRED_VERSION: OnceLock<VersionReq> = OnceLock::new();
        REQUIRED_VERSION.get_or_init(|| {
            VersionReq::parse(Self::REQUIRED_CLIENT_VERSION)
                .expect("invalid REQUIRED_CLIENT_VERSION constant")
        })
    }

    /// A version that does not parse counts as a mismatch: it cannot be shown
    /// to satisfy the requirement.
    fn version_info(current: &str) -> VersionInfo {
        let mismatch = match Version::parse(current) {
            Ok(parsed) => !Self::required_version().matches(&parsed),
            Err(e) => {
                log::warn!("failed to parse WiFi service version '{current}': {e:#}");
                true
            }
        };

        VersionInfo {
            required: Self::REQUIRED_CLIENT_VERSION.to_string(),
            current: current.to_string(),
            mismatch,
        }
    }

    pub async fn check_availability(&self) -> WifiAvailability {
        // A service older than the floor has no /api/v1/service-info and answers
        // 404, which lands here as an error — expected, not a fault.
        let info = match self.service_info().await {
            Ok(info) => info,
            Err(e) => {
                log::warn!("WiFi service-info probe failed: {e:#}");
                return WifiAvailability::Unavailable {
                    socket_present: true,
                    version_info: None,
                };
            }
        };

        let version_info = Self::version_info(&info.version);

        if version_info.mismatch {
            log::warn!(
                "WiFi service version '{}' does not satisfy {}",
                info.version,
                Self::REQUIRED_CLIENT_VERSION
            );
            return WifiAvailability::Unavailable {
                socket_present: true,
                version_info: Some(version_info),
            };
        }

        if info.interface_name.is_empty() {
            log::error!("WiFi service reported no interface name");
            return WifiAvailability::Unavailable {
                socket_present: true,
                version_info: Some(version_info),
            };
        }

        log::info!(
            "WiFi service available (version {}, interface {}, BLE {})",
            info.version,
            info.interface_name,
            if info.ble_enabled {
                "enabled"
            } else {
                "disabled"
            }
        );

        WifiAvailability::Available {
            version: info.version,
            interface_name: info.interface_name,
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

    async fn service_info(&self) -> Result<WifiServiceInfoResponse> {
        let body = self.get(Self::SERVICE_INFO_ENDPOINT).await?;
        serde_json::from_str(&body).context("failed to parse service info response")
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
            assert_eq!(
                WifiCommissioningServiceClient::SERVICE_INFO_ENDPOINT,
                "/api/v1/service-info"
            );
        }
    }

    mod version_requirements {
        use super::*;

        #[test]
        fn required_version_parses_correctly() {
            let version_req = WifiCommissioningServiceClient::required_version();
            assert_eq!(version_req.to_string(), ">=0.2.1");
        }

        #[test]
        fn accepts_the_floor_and_newer() {
            for current in ["0.2.1", "0.3.0", "1.0.0"] {
                let info = WifiCommissioningServiceClient::version_info(current);
                assert!(!info.mismatch, "{current} should satisfy the requirement");
                assert_eq!(info.current, current);
                assert_eq!(info.required, ">=0.2.1");
            }
        }

        #[test]
        fn rejects_versions_below_the_floor() {
            // 0.2.0 introduced /api/v1/service-info, 0.1.0 has no such endpoint.
            for current in ["0.2.0", "0.1.0"] {
                let info = WifiCommissioningServiceClient::version_info(current);
                assert!(
                    info.mismatch,
                    "{current} should not satisfy the requirement"
                );
            }
        }

        #[test]
        fn unparseable_version_counts_as_mismatch() {
            let info = WifiCommissioningServiceClient::version_info("not-a-version");
            assert!(info.mismatch);
            assert_eq!(info.current, "not-a-version");
        }
    }

    mod service_info_response {
        use super::*;

        #[test]
        fn deserializes_all_fields() {
            let json =
                r#"{"status":"ok","ble_enabled":true,"interface_name":"wlan0","version":"0.2.1"}"#;
            let resp: WifiServiceInfoResponse = serde_json::from_str(json).unwrap();
            assert_eq!(resp.status, "ok");
            assert!(resp.ble_enabled);
            assert_eq!(resp.interface_name, "wlan0");
            assert_eq!(resp.version, "0.2.1");
        }

        #[test]
        fn deserializes_with_ble_disabled() {
            let json =
                r#"{"status":"ok","ble_enabled":false,"interface_name":"wlan0","version":"0.2.1"}"#;
            let resp: WifiServiceInfoResponse = serde_json::from_str(json).unwrap();
            assert!(!resp.ble_enabled);
        }
    }
}
