//! External Data Transfer Objects (DTOs) for omnect-device-service (ODS)
//!
//! These types represent the "wire format" of JSON payloads received from ODS
//! over WebSocket/WebSocket.
//!
//! ### Why separate types?
//! 1. **Wire Format Isolation**: ODS uses `snake_case` variants, while our internal
//!    and UI models use `camelCase`. These types handle the translation.
//! 2. **Decoupling**: By mapping ODS types to internal domain models (via `From` traits),
//!    we protect our application logic and UI from breaking changes in the ODS API.
//! 3. **Validation**: These types provide a clear boundary for parsing raw data
//!    before it enters the application's business logic.

use serde::{Deserialize, Serialize};
use serde_repr::Deserialize_repr;

use crate::types::{
    DeviceNetwork, FactoryReset, FactoryResetResult, FactoryResetStatus, InternetProtocol,
    IpAddress, NetworkStatus, OnlineStatus, OsInfo, SystemInfo, TimeoutDuration, Timeouts,
    UpdateValidationStatus,
};

/// Online status update from ODS
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsOnlineStatus {
    pub iothub: bool,
}

impl From<OdsOnlineStatus> for OnlineStatus {
    fn from(ods: OdsOnlineStatus) -> Self {
        Self { iothub: ods.iothub }
    }
}

/// OS version information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsOsInfo {
    pub name: String,
    pub version: String,
}

/// System information update from ODS
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsSystemInfo {
    pub os: OdsOsInfo,
    pub azure_sdk_version: String,
    pub omnect_device_service_version: String,
    pub boot_time: Option<String>,
    pub hostname: String,
}

impl From<OdsSystemInfo> for SystemInfo {
    fn from(ods: OdsSystemInfo) -> Self {
        Self {
            os: OsInfo {
                name: ods.os.name,
                version: ods.os.version,
            },
            azure_sdk_version: ods.azure_sdk_version,
            omnect_device_service_version: ods.omnect_device_service_version,
            boot_time: ods.boot_time,
            hostname: ods.hostname,
        }
    }
}

/// Duration in seconds and nanoseconds
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsDuration {
    pub nanos: i32,
    pub secs: u64,
}

/// Timeouts configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsTimeouts {
    pub wait_online_timeout: OdsDuration,
}

impl From<OdsTimeouts> for Timeouts {
    fn from(ods: OdsTimeouts) -> Self {
        Self {
            wait_online_timeout: TimeoutDuration {
                nanos: ods.wait_online_timeout.nanos.unsigned_abs(),
                secs: ods.wait_online_timeout.secs,
            },
        }
    }
}

/// IP address configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsIpAddress {
    pub addr: String,
    pub dhcp: bool,
    pub prefix_len: u32,
}

impl From<OdsIpAddress> for IpAddress {
    fn from(ods: OdsIpAddress) -> Self {
        Self {
            addr: ods.addr,
            dhcp: ods.dhcp,
            prefix_len: ods.prefix_len,
        }
    }
}

/// Internet protocol configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsInternetProtocol {
    pub addrs: Vec<OdsIpAddress>,
    pub dns: Vec<String>,
    pub gateways: Vec<String>,
}

impl From<OdsInternetProtocol> for InternetProtocol {
    fn from(ods: OdsInternetProtocol) -> Self {
        Self {
            addrs: ods.addrs.into_iter().map(Into::into).collect(),
            dns: ods.dns,
            gateways: ods.gateways,
        }
    }
}

/// Network adapter information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsDeviceNetwork {
    pub ipv4: OdsInternetProtocol,
    pub mac: String,
    pub name: String,
    pub online: bool,
    #[serde(default)]
    pub file: Option<String>,
}

impl From<OdsDeviceNetwork> for DeviceNetwork {
    fn from(ods: OdsDeviceNetwork) -> Self {
        Self {
            ipv4: ods.ipv4.into(),
            mac: ods.mac,
            name: ods.name,
            online: ods.online,
            file: ods.file,
        }
    }
}

/// Network status update from ODS
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsNetworkStatus {
    pub network_status: Vec<OdsDeviceNetwork>,
}

impl From<OdsNetworkStatus> for NetworkStatus {
    fn from(ods: OdsNetworkStatus) -> Self {
        Self {
            network_status: ods.network_status.into_iter().map(Into::into).collect(),
        }
    }
}

/// Factory reset result status — ODS sends numeric values (`serde_repr`)
#[derive(Debug, Clone, Deserialize_repr, PartialEq, Eq)]
#[repr(u8)]
pub enum OdsFactoryResetResultStatus {
    ModeSupported = 0,
    ModeUnsupported = 1,
    BackupRestoreError = 2,
    ConfigurationError = 3,
}

impl From<OdsFactoryResetResultStatus> for FactoryResetStatus {
    fn from(ods: OdsFactoryResetResultStatus) -> Self {
        match ods {
            OdsFactoryResetResultStatus::ModeSupported => Self::ModeSupported,
            OdsFactoryResetResultStatus::ModeUnsupported => Self::ModeUnsupported,
            OdsFactoryResetResultStatus::BackupRestoreError => Self::BackupRestoreError,
            OdsFactoryResetResultStatus::ConfigurationError => Self::ConfigurationError,
        }
    }
}

/// Factory reset result
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct OdsFactoryResetResult {
    pub status: OdsFactoryResetResultStatus,
    pub context: Option<String>,
    pub error: String,
    pub paths: Vec<String>,
}

impl From<OdsFactoryResetResult> for FactoryResetResult {
    fn from(ods: OdsFactoryResetResult) -> Self {
        Self {
            status: ods.status.into(),
            context: ods.context,
            error: ods.error,
            paths: ods.paths,
        }
    }
}

/// Factory reset status update from ODS
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct OdsFactoryReset {
    pub keys: Vec<String>,
    pub result: Option<OdsFactoryResetResult>,
}

impl From<OdsFactoryReset> for FactoryReset {
    fn from(ods: OdsFactoryReset) -> Self {
        Self {
            keys: ods.keys,
            result: ods.result.map(Into::into),
        }
    }
}

/// Update validation status update from ODS
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OdsUpdateValidationStatus {
    pub status: String,
}

impl From<OdsUpdateValidationStatus> for UpdateValidationStatus {
    fn from(ods: OdsUpdateValidationStatus) -> Self {
        Self { status: ods.status }
    }
}
