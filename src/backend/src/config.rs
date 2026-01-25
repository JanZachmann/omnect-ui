use anyhow::{Context, Result};
use std::{env, path::PathBuf, sync::OnceLock};
use uuid::Uuid;

/// Application configuration loaded and validated at startup
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// UI server configuration
    pub ui: UiConfig,

    /// Centrifugo WebSocket server configuration
    pub centrifugo: CentrifugoConfig,

    /// Keycloak SSO configuration
    pub keycloak: KeycloakConfig,

    /// Device service client configuration
    pub device_service: DeviceServiceConfig,

    /// TLS certificate configuration
    pub certificate: CertificateConfig,

    /// IoT Edge workload API configuration
    #[cfg_attr(feature = "mock", allow(dead_code))]
    pub iot_edge: IoTEdgeConfig,

    /// Path configuration
    pub paths: PathConfig,

    /// Device hostname
    pub hostname: String,

    /// Tenant identifier
    pub tenant: String,
}

#[derive(Clone, Debug)]
pub struct UiConfig {
    pub port: u16,
}

#[derive(Clone, Debug)]
pub struct CentrifugoConfig {
    pub port: String,
    pub client_token: String,
    pub api_key: String,
    pub publish_endpoint: crate::omnect_device_service_client::PublishEndpoint,
    pub log_level: String,
    pub binary_path: PathBuf,
    pub config_path: PathBuf,
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
    pub tmp_update_file: PathBuf,
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
    /// This should only be called once via get(). It validates all
    /// required environment variables and returns an error if any are missing
    /// or invalid.
    fn load_internal() -> Result<Self> {
        // Validate critical paths exist before proceeding (skip in test/mock mode)
        #[cfg(not(any(test, feature = "mock")))]
        anyhow::ensure!(
            PathBuf::from("/data").try_exists().unwrap_or(false),
            "failed to find required data directory: /data is missing"
        );

        let ui = UiConfig::load()?;
        let centrifugo = CentrifugoConfig::load()?;
        let keycloak = KeycloakConfig::load()?;
        let device_service = DeviceServiceConfig::load()?;
        let certificate = CertificateConfig::load()?;
        let iot_edge = IoTEdgeConfig::load()?;
        let paths = PathConfig::load()?;
        let tenant = env::var("TENANT").unwrap_or_else(|_| "cp".to_string());
        #[cfg(any(test, feature = "mock"))]
        let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "omnect-device".to_string());
        #[cfg(not(any(test, feature = "mock")))]
        let hostname = env::var("HOSTNAME").context("HOSTNAME env not set")?;

        Ok(Self {
            ui,
            centrifugo,
            keycloak,
            device_service,
            certificate,
            iot_edge,
            paths,
            hostname,
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

impl CentrifugoConfig {
    fn load() -> Result<Self> {
        let port = env::var("CENTRIFUGO_HTTP_SERVER_PORT").unwrap_or_else(|_| "8000".to_string());
        let log_level = env::var("CENTRIFUGO_LOG_LEVEL").unwrap_or_else(|_| "none".to_string());

        // Generate unique tokens for this instance
        let client_token = Uuid::new_v4().to_string();
        let api_key = Uuid::new_v4().to_string();

        let publish_endpoint = crate::omnect_device_service_client::PublishEndpoint {
            url: format!("https://localhost:{port}/api/publish"),
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

        // In test/mock mode, use the local centrifugo instance
        #[cfg(any(test, feature = "mock"))]
        let binary_path = PathBuf::from("tools/centrifugo");
        #[cfg(not(any(test, feature = "mock")))]
        let binary_path =
            std::fs::canonicalize("centrifugo").context("failed to find centrifugo binary")?;

        #[cfg(any(test, feature = "mock"))]
        let config_path = PathBuf::from("src/backend/config/centrifugo_config.json");
        #[cfg(not(any(test, feature = "mock")))]
        let config_path = PathBuf::from("/centrifugo_config.json");

        Ok(Self {
            port,
            client_token,
            api_key,
            publish_endpoint,
            log_level,
            binary_path,
            config_path,
        })
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
    fn load() -> Result<Self> {
        let socket_path = env::var("SOCKET_PATH")
            .unwrap_or_else(|_| "/socket/api.sock".to_string())
            .into();

        Ok(Self { socket_path })
    }
}

impl CertificateConfig {
    fn load() -> Result<Self> {
        let cert_path = env::var("CERT_PATH")
            .unwrap_or_else(|_| "/data/cert.pem".to_string())
            .into();

        let key_path = env::var("KEY_PATH")
            .unwrap_or_else(|_| "/data/key.pem".to_string())
            .into();

        Ok(Self {
            cert_path,
            key_path,
        })
    }
}

impl IoTEdgeConfig {
    fn load() -> Result<Self> {
        #[cfg(any(test, feature = "mock"))]
        {
            let module_id =
                env::var("IOTEDGE_MODULEID").unwrap_or_else(|_| "test-module".to_string());
            let module_generation_id =
                env::var("IOTEDGE_MODULEGENERATIONID").unwrap_or_else(|_| "1".to_string());
            let api_version =
                env::var("IOTEDGE_APIVERSION").unwrap_or_else(|_| "2021-12-07".to_string());
            let workload_uri = env::var("IOTEDGE_WORKLOADURI")
                .unwrap_or_else(|_| "unix:///var/run/iotedge/workload.sock".to_string());

            Ok(Self {
                module_id,
                module_generation_id,
                api_version,
                workload_uri,
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

impl PathConfig {
    fn load() -> Result<Self> {
        #[cfg(not(any(test, feature = "mock")))]
        let (data_dir, host_data_dir) = (
            PathBuf::from("/data/"),
            PathBuf::from("/var/lib/").join(env!("CARGO_PKG_NAME")),
        );

        // In test mode, use temp directory as default
        #[cfg(any(test, feature = "mock"))]
        let (data_dir, host_data_dir) = {
            let data_dir = std::env::temp_dir().join("omnect-ui-test");

            std::fs::create_dir_all(&data_dir)
                .context("failed to create data directory")
                .unwrap();

            (data_dir.clone(), data_dir)
        };

        // Ensure config directory exists (skip in test/mock mode as it may not have permissions)
        let config_dir = data_dir.join("config");
        std::fs::create_dir_all(&config_dir).context("failed to create config directory")?;

        let app_config_path = config_dir.join("app_config.js");
        let password_file = config_dir.join("password");
        let host_update_file = host_data_dir.join("update.tar");
        let local_update_file = data_dir.join("update.tar");
        let tmp_update_file = std::env::temp_dir().join("update.tar");

        Ok(Self {
            app_config_path,
            data_dir,
            password_file,
            host_update_file,
            local_update_file,
            tmp_update_file,
        })
    }
}
