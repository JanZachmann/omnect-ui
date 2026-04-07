use anyhow::{Context, Result};
use std::{env, path::PathBuf, sync::OnceLock};
use uuid::Uuid;

/// Application configuration loaded and validated at startup
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// UI server configuration
    pub ui: UiConfig,

    /// Internal Publish endpoint configuration
    pub publish: PublishConfig,

    /// Keycloak SSO configuration
    pub keycloak: KeycloakConfig,

    /// Device service client configuration
    pub device_service: DeviceServiceConfig,

    /// TLS certificate configuration
    pub certificate: CertificateConfig,

    /// `IoT` Edge workload API configuration
    #[cfg_attr(feature = "mock", allow(dead_code))]
    pub iot_edge: IoTEdgeConfig,

    /// `WiFi` commissioning service configuration
    pub wifi: WifiConfig,

    /// Path configuration
    pub paths: PathConfig,

    /// Tenant identifier
    pub tenant: String,
}

#[derive(Clone, Debug)]
pub struct UiConfig {
    pub port: u16,
}

#[derive(Clone, Debug)]
pub struct PublishConfig {
    pub api_key: String,
    pub endpoint: crate::omnect_device_service_client::PublishEndpoint,
}

#[derive(Clone, Debug)]
pub struct KeycloakConfig {
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct DeviceServiceConfig {
    pub socket_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CertificateConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "mock", allow(dead_code))]
pub struct IoTEdgeConfig {
    pub module_id: String,
    pub module_generation_id: String,
    pub api_version: String,
    pub workload_uri: String,
}

#[derive(Clone, Debug)]
pub struct PathConfig {
    pub app_config_path: PathBuf,
    pub data_dir: PathBuf,
    pub password_file: PathBuf,
    pub host_update_file: PathBuf,
    pub local_update_file: PathBuf,
    pub session_key_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct WifiConfig {
    pub socket_path: PathBuf,
}

impl AppConfig {
    /// Get or load the application configuration
    ///
    /// Returns a reference to the cached configuration. On first call, it loads
    /// and validates all configuration from environment variables. Subsequent
    /// calls return the cached instance.
    ///
    /// # Panics
    /// Panics if configuration loading fails. This is intentional as the
    /// application cannot function without valid configuration.
    pub fn get() -> &'static Self {
        static APP_CONFIG: OnceLock<AppConfig> = OnceLock::new();
        APP_CONFIG.get_or_init(|| {
            Self::load_internal().expect("failed to load application configuration")
        })
    }

    /// Internal function to load and validate all configuration from environment variables
    ///
    /// This should only be called once via `get()`. It validates all
    /// required environment variables and returns an error if any are missing
    /// or invalid.
    fn load_internal() -> Result<Self> {
        #[cfg(not(any(test, feature = "mock")))]
        anyhow::ensure!(
            PathBuf::from("/data").try_exists().unwrap_or(false),
            "failed to find required data directory: /data is missing"
        );

        let ui = UiConfig::load()?;
        let publish = PublishConfig::load();
        let keycloak = KeycloakConfig::load()?;
        let device_service = DeviceServiceConfig::load();
        let certificate = CertificateConfig::load();
        let iot_edge = IoTEdgeConfig::load()?;
        let wifi = WifiConfig::load();
        let paths = PathConfig::load()?;
        let tenant = env::var("TENANT").unwrap_or_else(|_| "cp".to_string());

        Ok(Self {
            ui,
            publish,
            keycloak,
            device_service,
            certificate,
            iot_edge,
            wifi,
            paths,
            tenant,
        })
    }
}

impl UiConfig {
    fn load() -> Result<Self> {
        let port = env::var("UI_PORT")
            .unwrap_or_else(|_| "1977".to_string())
            .parse::<u16>()
            .context("failed to parse UI_PORT: invalid format")?;

        Ok(Self { port })
    }
}

impl PublishConfig {
    fn load() -> Self {
        let port = env::var("PUBLISH_PORT").unwrap_or_else(|_| "8000".to_string());
        // Generate a unique token for this instance to protect the internal publish endpoint
        let api_key = Uuid::new_v4().to_string();

        let endpoint = crate::omnect_device_service_client::PublishEndpoint {
            url: format!("http://localhost:{port}/api/internal/publish"),
            headers: vec![
                crate::omnect_device_service_client::HeaderKeyValue {
                    name: String::from("Content-Type"),
                    value: String::from("application/json"),
                },
                crate::omnect_device_service_client::HeaderKeyValue {
                    name: String::from("X-API-Key"),
                    value: api_key.clone(),
                },
            ],
        };

        Self { api_key, endpoint }
    }
}

impl KeycloakConfig {
    fn load() -> Result<Self> {
        let url = if cfg!(any(test, feature = "mock")) {
            env::var("KEYCLOAK_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080/realms/omnect".to_string())
        } else {
            env::var("KEYCLOAK_URL")?
        };

        Ok(Self { url })
    }
}

impl DeviceServiceConfig {
    fn load() -> Self {
        let socket_path = env::var("DEVICE_SERVICE_SOCKET_PATH")
            .unwrap_or_else(|_| "/omnect-device-service/api.sock".to_string())
            .into();

        Self { socket_path }
    }
}

impl CertificateConfig {
    fn load() -> Self {
        let cert_path = env::var("CERT_PATH")
            .unwrap_or_else(|_| "/cert/cert.pem".to_string())
            .into();

        let key_path = env::var("KEY_PATH")
            .unwrap_or_else(|_| "/cert/key.pem".to_string())
            .into();

        Self {
            cert_path,
            key_path,
        }
    }
}

impl IoTEdgeConfig {
    #[cfg_attr(
        any(test, feature = "mock"),
        allow(clippy::unnecessary_wraps) // mock branch provides defaults; non-mock uses ?
    )]
    fn load() -> Result<Self> {
        #[cfg(any(test, feature = "mock"))]
        {
            Ok(Self {
                module_id: env::var("IOTEDGE_MODULEID")
                    .unwrap_or_else(|_| "test-module".to_string()),
                module_generation_id: env::var("IOTEDGE_MODULEGENERATIONID")
                    .unwrap_or_else(|_| "1".to_string()),
                api_version: env::var("IOTEDGE_APIVERSION")
                    .unwrap_or_else(|_| "2021-12-07".to_string()),
                workload_uri: env::var("IOTEDGE_WORKLOADURI")
                    .unwrap_or_else(|_| "unix:///var/run/iotedge/workload.sock".to_string()),
            })
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            let module_id =
                env::var("IOTEDGE_MODULEID").context("failed to get IOTEDGE_MODULEID")?;
            let module_generation_id = env::var("IOTEDGE_MODULEGENERATIONID")
                .context("failed to get IOTEDGE_MODULEGENERATIONID")?;
            let api_version =
                env::var("IOTEDGE_APIVERSION").context("failed to get IOTEDGE_APIVERSION")?;
            let workload_uri =
                env::var("IOTEDGE_WORKLOADURI").context("failed to get IOTEDGE_WORKLOADURI")?;

            Ok(Self {
                module_id,
                module_generation_id,
                api_version,
                workload_uri,
            })
        }
    }
}

impl WifiConfig {
    fn load() -> Self {
        let socket_path = env::var("WIFI_COMMISSIONING_SOCKET_PATH")
            .unwrap_or_else(|_| "/wifi-commissioning-service/wlan0/api.sock".to_string())
            .into();

        Self { socket_path }
    }
}

impl PathConfig {
    fn load() -> Result<Self> {
        #[cfg(not(any(test, feature = "mock")))]
        let (data_dir, host_data_dir) = (
            PathBuf::from("/data/"),
            PathBuf::from("/var/lib/").join(env!("CARGO_PKG_NAME")),
        );

        #[cfg(any(test, feature = "mock"))]
        let (data_dir, host_data_dir) = {
            let data_dir = std::env::temp_dir().join("omnect-ui-test");

            std::fs::create_dir_all(&data_dir)
                .context("failed to create data directory")
                .unwrap();

            (data_dir.clone(), data_dir)
        };

        // Ensure config directory exists
        let config_dir = data_dir.join("config");
        std::fs::create_dir_all(&config_dir).context("failed to create config directory")?;

        let app_config_path = config_dir.join("app_config.js");
        let password_file = config_dir.join("password");
        let host_update_file = host_data_dir.join("update.tar");
        let local_update_file = data_dir.join("update.tar");
        let session_key_path = data_dir.join("session.key");

        Ok(Self {
            app_config_path,
            data_dir,
            password_file,
            host_update_file,
            local_update_file,
            session_key_path,
        })
    }
}
