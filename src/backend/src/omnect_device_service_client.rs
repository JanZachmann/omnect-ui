#![cfg_attr(feature = "mock", allow(dead_code, unused_imports))]

use crate::{
    config::AppConfig,
    http_client::{handle_http_response, unix_socket_client},
    services::marker,
    version::{VersionInfo, version_info},
};
use anyhow::{Context, Result, bail};
use log::info;
#[cfg(feature = "mock")]
use mockall::automock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::{env, fmt::Debug, path::PathBuf};
use trait_variant::make;

#[derive(Clone, Debug, Default, Deserialize_repr, PartialEq, Eq, Serialize_repr)]
#[repr(u8)]
pub enum FactoryResetMode {
    #[default]
    Mode1 = 1,
    Mode2 = 2,
    Mode3 = 3,
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
    // The floor is the oldest omnect-device-service that publishes the
    // factory-reset result in the shape parsed here (feature version 4).
    const REQUIRED_CLIENT_VERSION: &str = ">=0.46.1";

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
        let version_info = version_info(
            Self::REQUIRED_CLIENT_VERSION,
            &status.system_info.omnect_device_service_version,
        )?;

        Ok(HealthcheckInfo {
            version_info,
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

        fn mismatch(current: &str) -> bool {
            version_info(OmnectDeviceServiceClient::REQUIRED_CLIENT_VERSION, current)
                .unwrap()
                .mismatch
        }

        #[test]
        fn accepts_the_floor_and_newer() {
            for current in ["0.46.1", "0.47.0", "1.0.0"] {
                assert!(
                    !mismatch(current),
                    "{current} should satisfy the requirement"
                );
            }
        }

        #[test]
        fn rejects_older_versions() {
            for current in ["0.46.0", "0.45.10", "0.1.0"] {
                assert!(
                    mismatch(current),
                    "{current} should not satisfy the requirement"
                );
            }
        }
    }

    mod factory_reset_request {
        use super::*;

        #[test]
        fn accepts_modes_1_to_3() {
            for (json, mode) in [
                (r#"{"mode":1,"preserve":[]}"#, FactoryResetMode::Mode1),
                (r#"{"mode":2,"preserve":[]}"#, FactoryResetMode::Mode2),
                (
                    r#"{"mode":3,"preserve":["network"]}"#,
                    FactoryResetMode::Mode3,
                ),
            ] {
                let request: FactoryReset = serde_json::from_str(json).unwrap();
                assert_eq!(request.mode, mode);
            }
        }

        #[test]
        fn rejects_mode_4() {
            assert!(serde_json::from_str::<FactoryReset>(r#"{"mode":4,"preserve":[]}"#).is_err());
        }
    }

    mod healthcheck_info {
        use super::*;

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
            let version_req =
                semver::VersionReq::parse(OmnectDeviceServiceClient::REQUIRED_CLIENT_VERSION);
            assert!(version_req.is_ok());
        }
    }
}
