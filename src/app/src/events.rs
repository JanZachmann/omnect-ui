use serde::{Deserialize, Serialize};
use std::fmt;

use crate::types::{
    AuthToken, HealthcheckInfo, UpdateManifest, WifiAvailability, WifiSavedNetworksResponse,
    WifiScanResultsResponse, WifiStatusResponse,
};

/// Authentication events
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum AuthEvent {
    Login {
        password: String,
    },
    Logout,
    SetPassword {
        password: String,
    },
    UpdatePassword {
        current_password: String,
        password: String,
    },
    CheckRequiresPasswordSet,
    RestoreSession(String),
    #[serde(skip)]
    LoginResponse(Result<AuthToken, String>),
    #[serde(skip)]
    LogoutResponse(Result<(), String>),
    #[serde(skip)]
    SetPasswordResponse(Result<AuthToken, String>),
    #[serde(skip)]
    UpdatePasswordResponse(Result<(), String>),
    #[serde(skip)]
    CheckRequiresPasswordSetResponse(Result<bool, String>),
}

/// Device operation events
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    Reboot,
    FactoryResetRequest {
        mode: String,
        preserve: Vec<String>,
    },
    SetNetworkConfig {
        config: String,
    },
    NetworkFormStartEdit {
        adapter_name: String,
    },
    NetworkFormUpdate {
        form_data: String,
    },
    NetworkFormReset {
        adapter_name: String,
    },
    LoadUpdate {
        file_path: String,
    },
    UploadStarted,
    UploadProgress(u8),
    UploadCompleted(String),
    UploadFailed(String),
    RunUpdate {
        validate_iothub_connection: bool,
    },
    FetchInitialHealthcheck,
    ReconnectionCheckTick,
    ReconnectionCountdownTick,
    ReconnectionTimeout,
    NewIpCheckTick,
    NewIpCountdownTick,
    NewIpCheckTimeout,
    AckRollback,
    AckFactoryResetResult,
    AckUpdateValidation,
    #[serde(skip)]
    RebootResponse(Result<(), String>),
    #[serde(skip)]
    FactoryResetResponse(Result<(), String>),
    #[serde(skip)]
    SetNetworkConfigResponse(Result<crate::types::SetNetworkConfigResponse, String>),
    #[serde(skip)]
    LoadUpdateResponse(Result<UpdateManifest, String>),
    #[serde(skip)]
    RunUpdateResponse(Result<(), String>),
    #[serde(skip)]
    HealthcheckResponse(Result<HealthcheckInfo, String>),
    #[serde(skip)]
    AckRollbackResponse(Result<(), String>),
    #[serde(skip)]
    AckFactoryResetResultResponse(Result<(), String>),
    #[serde(skip)]
    AckUpdateValidationResponse(Result<(), String>),
}

/// WebSocket/WebSocket events
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum WebSocketEvent {
    SubscribeToChannels,
    UnsubscribeFromChannels,
    SystemInfoUpdated(String),
    NetworkStatusUpdated(String),
    OnlineStatusUpdated(String),
    FactoryResetUpdated(String),
    UpdateValidationStatusUpdated(String),
    TimeoutsUpdated(String),
    Connected,
    Disconnected,
}

/// `WiFi` management events
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum WifiEvent {
    // User actions
    CheckAvailability,
    Scan,
    Connect {
        ssid: String,
        password: String,
    },
    Disconnect,
    GetStatus,
    GetSavedNetworks,
    ForgetNetwork {
        ssid: String,
    },
    // Timer ticks (driven by Shell)
    ScanPollTick,
    ConnectPollTick,
    // Responses from HTTP effects
    #[serde(skip)]
    CheckAvailabilityResponse(Result<WifiAvailability, String>),
    #[serde(skip)]
    ScanResponse(Result<(), String>),
    #[serde(skip)]
    ScanResultsResponse(Result<WifiScanResultsResponse, String>),
    #[serde(skip)]
    ConnectResponse(Result<(), String>),
    #[serde(skip)]
    DisconnectResponse(Result<(), String>),
    #[serde(skip)]
    StatusResponse(Result<WifiStatusResponse, String>),
    #[serde(skip)]
    SavedNetworksResponse(Result<WifiSavedNetworksResponse, String>),
    #[serde(skip)]
    ForgetNetworkResponse(Result<(), String>),
}

/// Custom Debug for `WifiEvent` to redact password
impl fmt::Debug for WifiEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect { ssid, .. } => f
                .debug_struct("Connect")
                .field("ssid", ssid)
                .field("password", &"<redacted>")
                .finish(),
            Self::CheckAvailability => write!(f, "CheckAvailability"),
            Self::Scan => write!(f, "Scan"),
            Self::Disconnect => write!(f, "Disconnect"),
            Self::GetStatus => write!(f, "GetStatus"),
            Self::GetSavedNetworks => write!(f, "GetSavedNetworks"),
            Self::ForgetNetwork { ssid } => {
                f.debug_struct("ForgetNetwork").field("ssid", ssid).finish()
            }
            Self::ScanPollTick => write!(f, "ScanPollTick"),
            Self::ConnectPollTick => write!(f, "ConnectPollTick"),
            Self::CheckAvailabilityResponse(r) => {
                f.debug_tuple("CheckAvailabilityResponse").field(r).finish()
            }
            Self::ScanResponse(r) => f.debug_tuple("ScanResponse").field(r).finish(),
            Self::ScanResultsResponse(r) => f.debug_tuple("ScanResultsResponse").field(r).finish(),
            Self::ConnectResponse(r) => f.debug_tuple("ConnectResponse").field(r).finish(),
            Self::DisconnectResponse(r) => f.debug_tuple("DisconnectResponse").field(r).finish(),
            Self::StatusResponse(r) => f.debug_tuple("StatusResponse").field(r).finish(),
            Self::SavedNetworksResponse(r) => {
                f.debug_tuple("SavedNetworksResponse").field(r).finish()
            }
            Self::ForgetNetworkResponse(r) => {
                f.debug_tuple("ForgetNetworkResponse").field(r).finish()
            }
        }
    }
}

/// UI action events
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum UiEvent {
    ClearError,
    ClearSuccess,
    SetBrowserHostname(String),
    LoadSettings,
    SaveSettings(crate::types::TimeoutSettings),
    #[serde(skip)]
    LoadSettingsResponse(Result<crate::types::TimeoutSettings, String>),
    #[serde(skip)]
    SaveSettingsResponse(Result<(), String>),
}

/// Main event enum - wraps domain events
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Event {
    Initialize,
    Auth(AuthEvent),
    Device(DeviceEvent),
    WebSocket(WebSocketEvent),
    Ui(UiEvent),
    Wifi(WifiEvent),
}

/// Custom Debug implementation for `AuthEvent` to redact sensitive data
impl fmt::Debug for AuthEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Login { .. } => f
                .debug_struct("Login")
                .field("password", &"<redacted>")
                .finish(),
            Self::SetPassword { .. } => f
                .debug_struct("SetPassword")
                .field("password", &"<redacted>")
                .finish(),
            Self::UpdatePassword { .. } => f
                .debug_struct("UpdatePassword")
                .field("current_password", &"<redacted>")
                .field("password", &"<redacted>")
                .finish(),
            Self::LoginResponse(result) => match result {
                Ok(_) => f
                    .debug_tuple("LoginResponse")
                    .field(&"Ok(<redacted token>)")
                    .finish(),
                Err(e) => f
                    .debug_tuple("LoginResponse")
                    .field(&format!("Err({e})"))
                    .finish(),
            },
            Self::Logout => write!(f, "Logout"),
            Self::CheckRequiresPasswordSet => write!(f, "CheckRequiresPasswordSet"),
            Self::RestoreSession(_) => write!(f, "RestoreSession(<redacted token>)"),
            Self::LogoutResponse(r) => f.debug_tuple("LogoutResponse").field(r).finish(),
            Self::SetPasswordResponse(result) => match result {
                Ok(_) => f
                    .debug_tuple("SetPasswordResponse")
                    .field(&"Ok(<redacted token>)")
                    .finish(),
                Err(e) => f
                    .debug_tuple("SetPasswordResponse")
                    .field(&format!("Err({e})"))
                    .finish(),
            },
            Self::UpdatePasswordResponse(r) => {
                f.debug_tuple("UpdatePasswordResponse").field(r).finish()
            }
            Self::CheckRequiresPasswordSetResponse(r) => f
                .debug_tuple("CheckRequiresPasswordSetResponse")
                .field(r)
                .finish(),
        }
    }
}

/// Custom Debug implementation for Event
impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialize => write!(f, "Initialize"),
            Self::Auth(e) => write!(f, "Auth({e:?})"),
            Self::Device(e) => write!(f, "Device({e:?})"),
            Self::WebSocket(e) => write!(f, "WebSocket({e:?})"),
            Self::Ui(e) => write!(f, "Ui({e:?})"),
            Self::Wifi(e) => write!(f, "Wifi({e:?})"),
        }
    }
}
