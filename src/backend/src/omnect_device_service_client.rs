#![cfg_attr(feature = "mock", allow(dead_code, unused_imports))]

use crate::{
    config::AppConfig,
    http_client::{handle_http_response, unix_socket_client},
    services::marker,
};
use anyhow::{Context, Result, anyhow, bail};
use log::info;
#[cfg(feature = "mock")]
use mockall::automock;
use reqwest::Client;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::{env, fmt::Debug, path::PathBuf, sync::OnceLock};
use trait_variant::make;

#[derive(Clone, Debug, Default, Deserialize_repr, PartialEq, Eq, Serialize_repr)]
#[repr(u8)]
pub enum FactoryResetMode {
    #[default]
    Mode1 = 1,
    Mode2 = 2,
    Mode3 = 3,
    Mode4 = 4,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FactoryReset {
    mode: FactoryResetMode,
    preserve: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoadUpdate {
    pub update_file_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunUpdate {
    validate_iothub_connection: bool,
}

#[derive(Deserialize)]
pub struct Status {
    #[serde(rename = "NetworkStatus")]
    pub network_status: NetworkStatus,
    #[serde(rename = "SystemInfo")]
    pub system_info: SystemInfo,
    #[serde(rename = "UpdateValidationStatus")]
    pub update_validation_status: UpdateValidationStatus,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateValidationStatus {
    pub status: String,
}

#[derive(Deserialize)]
pub struct SystemInfo {
    pub fleet_id: Option<String>,
    pub omnect_device_service_version: String,
}

#[derive(Deserialize)]
pub struct NetworkStatus {
    #[serde(rename = "network_status")]
    pub network_interfaces: Vec<NetworkInterface>,
}

#[derive(Deserialize, Debug)]
pub struct NetworkInterface {
    pub online: bool,
    pub ipv4: Ipv4Info,
    pub file: PathBuf,
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct Ipv4Info {
    pub addrs: Vec<Ipv4AddrInfo>,
}

#[derive(Deserialize, Debug)]
pub struct Ipv4AddrInfo {
    pub addr: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub required: String,
    pub current: String,
    pub mismatch: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HealthcheckInfo {
    pub version_info: VersionInfo,
    pub update_validation_status: UpdateValidationStatus,
    pub network_rollback_occurred: bool,
    pub factory_reset_result_acked: bool,
    pub update_validation_acked: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderKeyValue {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishEndpoint {
    pub url: String,
    pub headers: Vec<HeaderKeyValue>,
}

#[derive(Debug, Serialize)]
struct PublishIdEndpoint {
    id: &'static str,
    endpoint: PublishEndpoint,
}

#[derive(Clone)]
pub struct OmnectDeviceServiceClient {
    client: Client,
    pub has_publish_endpoint: bool,
}

#[make(Send)]
#[cfg_attr(feature = "mock", automock)]
pub trait DeviceServiceClient {
    async fn fleet_id(&self) -> Result<String>;
    async fn status(&self) -> Result<Status>;
    async fn republish(&self) -> Result<()>;
    async fn factory_reset(&self, factory_reset: FactoryReset) -> Result<()>;
    async fn reboot(&self) -> Result<()>;
    async fn register_publish_endpoint(&mut self, endpoint: PublishEndpoint) -> Result<()>;
    async fn reload_network(&self) -> Result<()>;
    async fn load_update(&self, load_update: LoadUpdate) -> Result<String>;
    async fn run_update(&self, run_update: RunUpdate) -> Result<()>;
    async fn healthcheck_info(&self) -> Result<HealthcheckInfo>;
    async fn shutdown(&self) -> Result<()>;
}

#[cfg(feature = "mock")]
impl Clone for MockDeviceServiceClient {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl OmnectDeviceServiceClient {
    const REQUIRED_CLIENT_VERSION: &str = ">=0.39.0";

    // API endpoint constants
    const STATUS_ENDPOINT: &str = "/status/v1";
    const REPUBLISH_ENDPOINT: &str = "/republish/v1/";
    const FACTORY_RESET_ENDPOINT: &str = "/factory-reset/v1";
    const REBOOT_ENDPOINT: &str = "/reboot/v1";
    const RELOAD_NETWORK_ENDPOINT: &str = "/reload-network/v1";
    const LOAD_UPDATE_ENDPOINT: &str = "/fwupdate/load/v1";
    const RUN_UPDATE_ENDPOINT: &str = "/fwupdate/run/v1";
    const PUBLISH_ENDPOINT: &str = "/publish-endpoint/v1";

    pub fn new() -> Result<Self> {
        let client = unix_socket_client(
            &AppConfig::get()
                .device_service
                .socket_path
                .to_string_lossy(),
        )?;

        Ok(Self {
            client,
            has_publish_endpoint: false,
        })
    }

    fn required_version() -> &'static VersionReq {
        static REQUIRED_VERSION: OnceLock<VersionReq> = OnceLock::new();
        REQUIRED_VERSION.get_or_init(|| {
            VersionReq::parse(Self::REQUIRED_CLIENT_VERSION)
                .expect("invalid REQUIRED_CLIENT_VERSION constant")
        })
    }

    #[allow(clippy::unused_self)]
    fn build_url(&self, path: &str) -> String {
        // Normalize path to always start with a single "/"
        let normalized_path = path.trim_start_matches('/');
        format!("http://localhost/{normalized_path}")
    }

    /// GET request to the device service API
    async fn get(&self, path: &str) -> Result<String> {
        let url = self.build_url(path);
        info!("GET {url}");

        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context(format!("failed to send GET request to {url}"))?;

        handle_http_response(res, &format!("GET {url}")).await
    }

    /// POST request to the device service API (empty body)
    async fn post(&self, path: &str) -> Result<String> {
        let url = self.build_url(path);
        info!("POST {url}");

        let res = self
            .client
            .post(&url)
            .send()
            .await
            .context(format!("failed to send POST request to {url}"))?;

        handle_http_response(res, &format!("POST {url}")).await
    }

    /// POST request to the device service API with JSON body
    async fn post_json(&self, path: &str, body: impl Debug + Serialize) -> Result<String> {
        let url = self.build_url(path);
        info!("POST {url} with body: {body:?}");

        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context(format!("failed to send POST request to {url}"))?;

        handle_http_response(res, &format!("POST {url}")).await
    }
}

impl DeviceServiceClient for OmnectDeviceServiceClient {
    async fn register_publish_endpoint(&mut self, endpoint: PublishEndpoint) -> Result<()> {
        let publish_id_endpoint = PublishIdEndpoint {
            id: env!("CARGO_PKG_NAME"),
            endpoint,
        };
        self.post_json(Self::PUBLISH_ENDPOINT, publish_id_endpoint)
            .await?;

        self.has_publish_endpoint = true;

        Ok(())
    }

    async fn fleet_id(&self) -> Result<String> {
        let status = self.status().await?;

        let Some(fleet_id) = status.system_info.fleet_id else {
            bail!("failed to get fleet id from status")
        };

        Ok(fleet_id)
    }

    async fn status(&self) -> Result<Status> {
        let body = self.get(Self::STATUS_ENDPOINT).await?;
        serde_json::from_str(&body).context("failed to parse status")
    }

    async fn republish(&self) -> Result<()> {
        self.post(&format!(
            "{}{}",
            Self::REPUBLISH_ENDPOINT,
            env!("CARGO_PKG_NAME")
        ))
        .await?;
        Ok(())
    }

    async fn factory_reset(&self, factory_reset: FactoryReset) -> Result<()> {
        self.post_json(Self::FACTORY_RESET_ENDPOINT, factory_reset)
            .await?;
        Ok(())
    }

    async fn reboot(&self) -> Result<()> {
        self.post(Self::REBOOT_ENDPOINT).await?;
        Ok(())
    }

    async fn reload_network(&self) -> Result<()> {
        self.post(Self::RELOAD_NETWORK_ENDPOINT).await?;
        Ok(())
    }

    async fn load_update(&self, load_update: LoadUpdate) -> Result<String> {
        self.post_json(Self::LOAD_UPDATE_ENDPOINT, load_update)
            .await
    }

    async fn run_update(&self, run_update: RunUpdate) -> Result<()> {
        self.post_json(Self::RUN_UPDATE_ENDPOINT, run_update)
            .await?;
        Ok(())
    }

    async fn healthcheck_info(&self) -> Result<HealthcheckInfo> {
        let status = self.status().await?;
        let current_version = status.system_info.omnect_device_service_version;

        let required_version = Self::required_version();
        let parsed_current = Version::parse(&current_version)
            .map_err(|e| anyhow!("failed to parse current version: {e}"))?;

        Ok(HealthcheckInfo {
            version_info: VersionInfo {
                required: Self::REQUIRED_CLIENT_VERSION.to_string(),
                current: current_version,
                mismatch: !required_version.matches(&parsed_current),
            },
            update_validation_status: status.update_validation_status,
            network_rollback_occurred: marker::NETWORK_ROLLBACK_OCCURRED.exists(),
            factory_reset_result_acked: marker::FACTORY_RESET_RESULT_ACKED.exists(),
            update_validation_acked: marker::UPDATE_VALIDATION_ACKED.exists(),
        })
    }

    async fn shutdown(&self) -> Result<()> {
        if self.has_publish_endpoint {
            let endpoint = Self::PUBLISH_ENDPOINT;
            let pkg_name = env!("CARGO_PKG_NAME");
            let url = self.build_url(&format!("{endpoint}/{pkg_name}"));
            info!("DELETE {url}");

            self.client
                .delete(&url)
                .send()
                .await
                .context("failed to send DELETE request to unregister endpoint")?
                .error_for_status()
                .context("failed to unregister endpoint: server returned error status")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod build_url {
        use super::*;

        fn create_test_client() -> OmnectDeviceServiceClient {
            OmnectDeviceServiceClient {
                client: reqwest::Client::new(),
                has_publish_endpoint: false,
            }
        }

        #[test]
        fn normalizes_path_with_leading_slash() {
            let client = create_test_client();
            let url = client.build_url("/status/v1");
            assert_eq!(url, "http://localhost/status/v1");
        }

        #[test]
        fn normalizes_path_without_leading_slash() {
            let client = create_test_client();
            let url = client.build_url("status/v1");
            assert_eq!(url, "http://localhost/status/v1");
        }

        #[test]
        fn normalizes_path_with_multiple_leading_slashes() {
            let client = create_test_client();
            let url = client.build_url("///status/v1");
            assert_eq!(url, "http://localhost/status/v1");
        }

        #[test]
        fn handles_empty_path() {
            let client = create_test_client();
            let url = client.build_url("");
            assert_eq!(url, "http://localhost/");
        }

        #[test]
        fn handles_root_path() {
            let client = create_test_client();
            let url = client.build_url("/");
            assert_eq!(url, "http://localhost/");
        }
    }

    mod version_requirements {
        use super::*;

        #[test]
        fn required_version_parses_correctly() {
            let version_req = OmnectDeviceServiceClient::required_version();
            assert_eq!(version_req.to_string(), ">=0.39.0");
        }

        #[test]
        fn required_version_matches_valid_versions() {
            let version_req = OmnectDeviceServiceClient::required_version();

            assert!(version_req.matches(&Version::parse("0.39.0").unwrap()));
            assert!(version_req.matches(&Version::parse("0.40.0").unwrap()));
            assert!(version_req.matches(&Version::parse("1.0.0").unwrap()));
        }

        #[test]
        fn required_version_rejects_older_versions() {
            let version_req = OmnectDeviceServiceClient::required_version();

            assert!(!version_req.matches(&Version::parse("0.38.9").unwrap()));
            assert!(!version_req.matches(&Version::parse("0.30.0").unwrap()));
            assert!(!version_req.matches(&Version::parse("0.1.0").unwrap()));
        }
    }

    mod healthcheck_info {
        use super::*;
        use crate::services::network::NetworkConfigService;

        fn create_test_status(version: &str) -> Status {
            Status {
                network_status: NetworkStatus {
                    network_interfaces: vec![],
                },
                system_info: SystemInfo {
                    fleet_id: Some("test-fleet".to_string()),
                    omnect_device_service_version: version.to_string(),
                },
                update_validation_status: UpdateValidationStatus {
                    status: "idle".to_string(),
                },
            }
        }

        #[test]
        fn detects_version_mismatch_when_below_requirement() {
            let status = create_test_status("0.38.0");
            let current_version = status.system_info.omnect_device_service_version.clone();

            let required_version = OmnectDeviceServiceClient::required_version();
            let parsed_current = Version::parse(&current_version).unwrap();

            let mismatch = !required_version.matches(&parsed_current);

            assert!(mismatch);
        }

        #[test]
        fn detects_no_mismatch_when_matching_requirement() {
            let status = create_test_status("0.40.0");
            let current_version = status.system_info.omnect_device_service_version.clone();

            let required_version = OmnectDeviceServiceClient::required_version();
            let parsed_current = Version::parse(&current_version).unwrap();

            let mismatch = !required_version.matches(&parsed_current);

            assert!(!mismatch);
        }

        #[test]
        fn healthcheck_includes_network_rollback_status() {
            // Verify marker file check is callable without panicking
            let _rollback_occurred = marker::NETWORK_ROLLBACK_OCCURRED.exists();
        }
    }

    mod publish_endpoint_state {
        use super::*;

        #[test]
        fn new_client_has_no_publish_endpoint() {
            let client = OmnectDeviceServiceClient {
                client: reqwest::Client::new(),
                has_publish_endpoint: false,
            };

            assert!(!client.has_publish_endpoint);
        }

        #[test]
        fn client_tracks_publish_endpoint_registration() {
            let mut client = OmnectDeviceServiceClient {
                client: reqwest::Client::new(),
                has_publish_endpoint: false,
            };

            // Simulate registration
            client.has_publish_endpoint = true;

            assert!(client.has_publish_endpoint);
        }
    }

    mod constants {
        use super::*;

        #[test]
        fn api_endpoints_are_correctly_defined() {
            assert_eq!(OmnectDeviceServiceClient::STATUS_ENDPOINT, "/status/v1");
            assert_eq!(
                OmnectDeviceServiceClient::REPUBLISH_ENDPOINT,
                "/republish/v1/"
            );
            assert_eq!(
                OmnectDeviceServiceClient::FACTORY_RESET_ENDPOINT,
                "/factory-reset/v1"
            );
            assert_eq!(OmnectDeviceServiceClient::REBOOT_ENDPOINT, "/reboot/v1");
            assert_eq!(
                OmnectDeviceServiceClient::RELOAD_NETWORK_ENDPOINT,
                "/reload-network/v1"
            );
            assert_eq!(
                OmnectDeviceServiceClient::LOAD_UPDATE_ENDPOINT,
                "/fwupdate/load/v1"
            );
            assert_eq!(
                OmnectDeviceServiceClient::RUN_UPDATE_ENDPOINT,
                "/fwupdate/run/v1"
            );
            assert_eq!(
                OmnectDeviceServiceClient::PUBLISH_ENDPOINT,
                "/publish-endpoint/v1"
            );
        }

        #[test]
        fn required_version_constant_is_valid_semver_requirement() {
            let version_req = VersionReq::parse(OmnectDeviceServiceClient::REQUIRED_CLIENT_VERSION);
            assert!(version_req.is_ok());
        }
    }
}
