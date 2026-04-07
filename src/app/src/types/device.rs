use serde::{Deserialize, Serialize};

/// State of long-running device operations (reboot, factory reset, update)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DeviceOperationState {
    #[default]
    Idle,
    Rebooting,
    FactoryResetting,
    Updating,
    WaitingReconnection {
        operation: String,
        attempt: u32,
    },
    ReconnectionFailed {
        operation: String,
        reason: String,
    },
    ReconnectionSuccessful {
        operation: String,
    },
}

impl DeviceOperationState {
    #[must_use]
    pub fn operation_name(&self) -> String {
        match self {
            Self::Rebooting => "Reboot".to_string(),
            Self::FactoryResetting => "Factory Reset".to_string(),
            Self::Updating => "Update".to_string(),
            Self::WaitingReconnection { operation, .. }
            | Self::ReconnectionFailed { operation, .. }
            | Self::ReconnectionSuccessful { operation } => operation.clone(),
            Self::Idle => "Unknown".to_string(),
        }
    }
}
