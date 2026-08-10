use serde::{Deserialize, Serialize};
use std::fmt;

/// Factory reset operation status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum FactoryResetStatus {
    /// No result received yet. Distinct from `Unrecognized` so the UI can stay
    /// silent while a result is missing and still report one it cannot name.
    #[default]
    Unknown,
    Success,
    Invalid,
    Error,
    ConfigError,
    /// Reset succeeded, but a partition needed a second format attempt.
    Warning,
    /// A status code this version does not know.
    Unrecognized,
}

impl FactoryResetStatus {
    /// `Warning` counts as success: the reset completed, only a partition
    /// needed a retry.
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success | Self::Warning)
    }
}

impl fmt::Display for FactoryResetStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => write!(f, "unknown"),
            Self::Success => write!(f, "success"),
            Self::Invalid => write!(f, "invalid"),
            Self::Error => write!(f, "error"),
            Self::ConfigError => write!(f, "configError"),
            Self::Warning => write!(f, "warning"),
            Self::Unrecognized => write!(f, "unrecognized"),
        }
    }
}

/// Result of factory reset operation
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FactoryResetResult {
    pub status: FactoryResetStatus,
    pub context: Option<String>,
    pub error: Option<String>,
    pub paths: Vec<String>,
    /// `true` once the reset started wiping data. On a failure this separates a
    /// safe abort from one that left the device half wiped.
    pub data_wiped: bool,
}

/// Factory reset state from WebSocket
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FactoryReset {
    pub keys: Vec<String>,
    #[serde(default)]
    pub result: Option<FactoryResetResult>,
}

/// Request to initiate factory reset
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FactoryResetRequest {
    pub mode: u8,
    pub preserve: Vec<String>,
}
