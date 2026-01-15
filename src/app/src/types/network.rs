use serde::{Deserialize, Serialize};

/// IP address configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpAddress {
    pub addr: String,
    pub dhcp: bool,
    pub prefix_len: u32,
}

/// Internet protocol configuration (IPv4/IPv6)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InternetProtocol {
    pub addrs: Vec<IpAddress>,
    pub dns: Vec<String>,
    pub gateways: Vec<String>,
}

/// Network adapter information
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceNetwork {
    pub ipv4: InternetProtocol,
    pub mac: String,
    pub name: String,
    pub online: bool,
    #[serde(default)]
    pub file: Option<String>,
}

/// Network status from WebSocket
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkStatus {
    pub network_status: Vec<DeviceNetwork>,
}

/// Network configuration request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfigRequest {
    pub is_server_addr: bool,
    pub ip_changed: bool,
    pub name: String,
    pub dhcp: bool,
    pub ip: Option<String>,
    pub previous_ip: Option<String>,
    pub netmask: Option<u32>,
    pub gateway: Vec<String>,
    pub dns: Vec<String>,
    /// Whether to enable automatic rollback protection.
    /// Only applicable when is_server_addr=true AND ip_changed=true.
    #[serde(default)]
    pub enable_rollback: Option<bool>,
    /// Whether this change is switching to DHCP (for UI messaging)
    #[serde(default)]
    pub switching_to_dhcp: bool,
}

/// Form data for network configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkFormData {
    pub name: String,
    pub ip_address: String,
    pub dhcp: bool,
    pub prefix_len: u32,
    pub dns: Vec<String>,
    pub gateways: Vec<String>,
}

impl From<&DeviceNetwork> for NetworkFormData {
    fn from(adapter: &DeviceNetwork) -> Self {
        Self {
            name: adapter.name.clone(),
            ip_address: adapter
                .ipv4
                .addrs
                .first()
                .map(|a| a.addr.clone())
                .unwrap_or_default(),
            dhcp: adapter.ipv4.addrs.first().map(|a| a.dhcp).unwrap_or(false),
            prefix_len: adapter
                .ipv4
                .addrs
                .first()
                .map(|a| a.prefix_len)
                .unwrap_or(24),
            dns: adapter.ipv4.dns.clone(),
            gateways: adapter.ipv4.gateways.clone(),
        }
    }
}

/// State of network form (to prevent WebSocket interference)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NetworkFormState {
    #[default]
    Idle,
    Editing {
        adapter_name: String,
        form_data: NetworkFormData,
        original_data: NetworkFormData,
    },
    Submitting {
        adapter_name: String,
        form_data: NetworkFormData,
        original_data: NetworkFormData,
    },
}

impl NetworkFormState {
    /// Transition from Editing to Submitting state
    pub fn to_submitting(&self) -> Option<Self> {
        if let Self::Editing {
            adapter_name,
            form_data,
            original_data,
        } = self
        {
            Some(Self::Submitting {
                adapter_name: adapter_name.clone(),
                form_data: form_data.clone(),
                original_data: original_data.clone(),
            })
        } else {
            None
        }
    }

    /// Transition from Submitting back to Editing state
    pub fn to_editing(&self) -> Option<Self> {
        if let Self::Submitting {
            adapter_name,
            form_data,
            original_data,
        } = self
        {
            Some(Self::Editing {
                adapter_name: adapter_name.clone(),
                form_data: form_data.clone(),
                original_data: original_data.clone(),
            })
        } else {
            None
        }
    }
}

/// State machine for network IP change after configuration.
///
/// This state machine tracks the progress of network configuration changes
/// that affect the device's IP address, including automatic rollback handling.
///
/// # State Machine Diagram
///
/// ```text
///                              ┌─────────────────────────────────────────────────────────┐
///                              │                     START                                │
///                              └────────────────────────┬────────────────────────────────┘
///                                                       │
///                                                       ▼
///                                              ┌────────────────┐
///                              ┌───────────────│      Idle      │───────────────┐
///                              │               └────────────────┘               │
///                              │                       │                        │
///                              │     User applies      │                        │
///                              │    network config     │                        │
///                              │                       ▼                        │
///                              │              ┌────────────────┐                │
///                              │              │ ApplyingConfig │                │
///                              │              └────────────────┘                │
///                              │                       │                        │
///                              │   Backend responds    │                        │
///                              │    successfully       │                        │
///                              │                       ▼                        │
///                              │             ┌─────────────────┐                │
///                              │             │ WaitingForNewIp │                │
///                              │             └─────────────────┘                │
///                              │              │               │                 │
///                              │   Healthcheck│               │Timeout expires  │
///                              │    succeeds  │               │(rollback enabled)│
///                              │              ▼               ▼                 │
///                              │   ┌────────────────┐  ┌─────────────────┐      │
///                              │   │ NewIpReachable │  │ WaitingForOldIp │      │
///                              │   └────────────────┘  └─────────────────┘      │
///                              │          │                    │                │
///                              │   Redirect to │    Healthcheck │                │
///                              │     new IP    │    on old IP   │                │
///                              │          │    │    succeeds    │                │
///                              │          │    │       │        │                │
///                              │          ▼    │       ▼        │                │
///                              │   ┌───────────┴───────────┐    │                │
///                              └───│        SUCCESS        │◄───┘                │
///                                  └───────────────────────┘                     │
///                                                                                │
///                              ┌─────────────────────────────────────────────────┘
///                              │ Timeout expires (rollback disabled)
///                              ▼
///                     ┌────────────────┐
///                     │ NewIpTimeout   │  (Shows manual navigation message)
///                     └────────────────┘
/// ```
///
/// # State Descriptions
///
/// - **Idle**: No network change in progress
/// - **ApplyingConfig**: Configuration request sent to backend, waiting for response
/// - **WaitingForNewIp**: Polling new IP to verify reachability before rollback timeout
/// - **NewIpReachable**: New IP confirmed reachable, will redirect browser
/// - **NewIpTimeout**: Timeout expired without rollback enabled, show manual nav message
/// - **WaitingForOldIp**: Rollback assumed, now polling old IP to verify device is back
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NetworkChangeState {
    /// No network change in progress
    #[default]
    Idle,
    /// Configuration request sent to backend, waiting for response
    ApplyingConfig {
        is_server_addr: bool,
        ip_changed: bool,
        new_ip: String,
        old_ip: String,
        switching_to_dhcp: bool,
    },
    /// Polling new IP to verify reachability before rollback timeout
    WaitingForNewIp {
        new_ip: String,
        old_ip: String,
        attempt: u32,
        rollback_timeout_seconds: u64,
        ui_port: u16,
        switching_to_dhcp: bool,
    },
    /// New IP confirmed reachable, browser will redirect
    NewIpReachable {
        new_ip: String,
        ui_port: u16,
    },
    /// Timeout expired without confirming new IP (rollback disabled case)
    NewIpTimeout {
        new_ip: String,
        old_ip: String,
        ui_port: u16,
        switching_to_dhcp: bool,
    },
    /// Rollback assumed complete, polling old IP to verify device is accessible
    WaitingForOldIp {
        old_ip: String,
        ui_port: u16,
        attempt: u32,
    },
}

/// Response from backend when setting network configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetNetworkConfigResponse {
    pub rollback_timeout_seconds: u64,
    pub ui_port: u16,
    pub rollback_enabled: bool,
}
