use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use std::collections::HashMap;

/// Validate IPv4 address format
#[must_use]
pub fn is_valid_ipv4(ip: &str) -> bool {
    if ip.is_empty() {
        return true; // Empty is considered valid (for optional fields)
    }

    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }

    parts.iter().all(|part| {
        if let Ok(num) = part.parse::<u32>() {
            num <= 255
        } else {
            false
        }
    })
}

/// Convert CIDR prefix length to Dotted Decimal Subnet Mask
#[must_use]
pub fn cidr_to_subnet(cidr: u32) -> String {
    if cidr > 32 {
        return String::new();
    }
    let mask = if cidr == 0 {
        0
    } else {
        0xffff_ffff_u32 << (32 - cidr)
    };
    format!(
        "{}.{}.{}.{}",
        (mask >> 24) & 0xff,
        (mask >> 16) & 0xff,
        (mask >> 8) & 0xff,
        mask & 0xff
    )
}

/// Convert Dotted Decimal Subnet Mask to CIDR prefix length
#[must_use]
pub fn subnet_to_cidr(subnet: &str) -> Option<u32> {
    let parts: Vec<u32> = subnet
        .split('.')
        .filter_map(|p| p.parse::<u32>().ok())
        .collect();

    if parts.len() != 4 || parts.iter().any(|&p| p > 255) {
        return None;
    }

    let mut mask: u32 = 0;
    for &part in &parts {
        mask = (mask << 8) | part;
    }

    // A valid subnet mask must be a sequence of 1s followed by a sequence of 0s.
    // We check this by ensuring that (not mask) + 1 is a power of 2.
    // e.g. 255.255.255.0 is 11111111.11111111.11111111.00000000
    // not mask is 00000000.00000000.00000000.11111111 (255)
    // 255 + 1 = 256 (power of 2)
    let inverted = !mask;
    if (inverted.wrapping_add(1) & inverted) == 0 {
        Some(mask.count_ones())
    } else {
        None
    }
}

/// Validate and parse netmask value
/// Accepts "/24" or "24" format, returns prefix length if valid
#[must_use]
pub fn parse_netmask(mask: &str) -> Option<u32> {
    let cleaned = mask.trim_start_matches('/');
    if let Ok(prefix_len) = cleaned.parse::<u32>()
        && prefix_len <= 32
    {
        return Some(prefix_len);
    }
    None
}

/// IP address configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpAddress {
    pub addr: String,
    pub dhcp: bool,
    pub prefix_len: u32,
}

/// Internet protocol configuration (IPv4/IPv6)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InternetProtocol {
    pub addrs: Vec<IpAddress>,
    pub dns: Vec<String>,
    pub gateways: Vec<String>,
}

/// Network adapter information
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    pub network_status: Vec<DeviceNetwork>,
}

impl NetworkStatus {
    /// Determine which adapter is the current connection based on browser hostname
    #[must_use]
    pub fn current_connection_adapter(
        &self,
        browser_hostname: Option<&str>,
    ) -> Option<&DeviceNetwork> {
        let hostname = browser_hostname?;

        // First, try to find a direct IP match
        for adapter in &self.network_status {
            for ip in &adapter.ipv4.addrs {
                if ip.addr == hostname {
                    return Some(adapter);
                }
            }
        }

        // Special case: if we are on localhost, and an adapter has "localhost" IP, match it
        if hostname == "localhost" || hostname == "127.0.0.1" {
            for adapter in &self.network_status {
                if adapter
                    .ipv4
                    .addrs
                    .iter()
                    .any(|ip| ip.addr == "localhost" || ip.addr == "127.0.0.1")
                {
                    return Some(adapter);
                }
            }
        }

        // If hostname is a domain name (not an IP), we can't determine which adapter
        // is the current connection without DNS resolution, so return None
        None
    }
}

/// Network configuration request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Validate)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfigRequest {
    pub is_server_addr: bool,
    pub ip_changed: bool,
    #[validate(min_length = 1)]
    pub name: String,
    pub dhcp: bool,
    pub ip: Option<String>,
    pub previous_ip: Option<String>,
    #[validate(maximum = 32)]
    #[validate(minimum = 0)]
    pub netmask: Option<u32>,
    pub gateway: Vec<String>,
    pub dns: Vec<String>,
    /// Whether to enable automatic rollback protection.
    /// Only applicable when `is_server_addr=true` AND `ip_changed=true`.
    #[serde(default)]
    pub enable_rollback: Option<bool>,
    /// Whether this change is switching to DHCP
    #[serde(default)]
    pub switching_to_dhcp: bool,
}

/// Form data for network configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkFormData {
    pub name: String,
    pub ip_address: String,
    pub dhcp: bool,
    pub subnet_mask: String,
    pub dns: Vec<String>,
    pub gateways: Vec<String>,
}

impl From<&DeviceNetwork> for NetworkFormData {
    fn from(adapter: &DeviceNetwork) -> Self {
        let addr = adapter.ipv4.addrs.first();
        Self {
            name: adapter.name.clone(),
            ip_address: addr.map(|a| a.addr.clone()).unwrap_or_default(),
            dhcp: addr.is_some_and(|a| a.dhcp),
            subnet_mask: cidr_to_subnet(addr.map_or(24, |a| a.prefix_len)),
            dns: adapter.ipv4.dns.clone(),
            gateways: adapter.ipv4.gateways.clone(),
        }
    }
}

/// State of network form
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum NetworkFormState {
    #[default]
    Idle,
    Editing {
        adapter_name: String,
        form_data: NetworkFormData,
        original_data: NetworkFormData,
        #[serde(default)]
        errors: HashMap<String, String>,
    },
    Submitting {
        adapter_name: String,
        form_data: NetworkFormData,
        original_data: NetworkFormData,
        #[serde(default)]
        errors: HashMap<String, String>,
    },
}

impl NetworkFormState {
    /// Transition from Editing to Submitting state
    #[must_use]
    pub fn to_submitting(&self, target_adapter_name: &str) -> Option<Self> {
        if let Self::Editing {
            adapter_name,
            form_data,
            original_data,
            errors,
        } = self
            && adapter_name == target_adapter_name
        {
            Some(Self::Submitting {
                adapter_name: adapter_name.clone(),
                form_data: form_data.clone(),
                original_data: original_data.clone(),
                errors: errors.clone(),
            })
        } else {
            None
        }
    }

    /// Transition from Submitting back to Editing state
    #[must_use]
    pub fn to_editing(&self) -> Option<Self> {
        if let Self::Submitting {
            adapter_name,
            form_data,
            original_data,
            errors,
        } = self
        {
            Some(Self::Editing {
                adapter_name: adapter_name.clone(),
                form_data: form_data.clone(),
                original_data: original_data.clone(),
                errors: errors.clone(),
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
///                    ┌───────┐
///                    │ START │
///                    └───┬───┘
///                        │
///                        ▼
///              ┌─────────────────┐◄──────────────────────────────────────────┐
///              │      Idle       │◄──────────────────────────────────────┐   │
///              └────────┬────────┘                                       │   │
///                       │ [user applies config]                          │   │
///                       ▼                                                │   │
///              ┌─────────────────┐ [backend error] ──────────────────────────┘
///              │  ApplyingConfig │
///              └────────┬────────┘ [DHCP + no rollback] ─────────────────┘
///                       │ [backend ok]
///                       ▼
///             ┌──────────────────┐
///             │  WaitingForNewIp │
///             └────────┬─────────┘
///          ┌───────────┼────────────────┐
///          │           │                │
///  [new IP │  [timeout,│rollback]  [timeout,
///  reachable]          │           no rollback]
///          │           ▼                │
///          │  ┌─────────────────┐       ▼
///          │  │ WaitingForOldIp │  ┌─────────────┐
///          │  └────────┬────────┘  │ NewIpTimeout│
///          │           │[old IP ok] └──────┬──────┘
///          │           │                   │ [user navigates
///          │           ▼                   │  manually → page reload]
///          │          Idle ◄───────────────┘
///          │
///          ▼
///   ┌────────────────┐
///   │ NewIpReachable │  [browser redirects → page reload → Idle]
///   └────────────────┘
/// ```
///
/// # State Descriptions
///
/// - **Idle**: No network change in progress
/// - **`ApplyingConfig`**: Configuration request sent to backend, waiting for response
/// - **`WaitingForNewIp`**: Polling new IP to verify reachability before rollback timeout
/// - **`NewIpReachable`**: New IP confirmed reachable; browser redirect causes page reload → Idle
/// - **`NewIpTimeout`**: Timeout expired, rollback disabled; user manually navigates → page reload → Idle
/// - **`WaitingForOldIp`**: Rollback assumed, polling old IP; on success → Idle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
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
    NewIpReachable { new_ip: String, ui_port: u16 },
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

#[cfg(test)]
mod tests {
    use super::*;

    mod validation {
        use super::*;

        #[test]
        fn is_valid_ipv4_accepts_valid_addresses() {
            assert!(is_valid_ipv4("192.168.1.1"));
            assert!(is_valid_ipv4("10.0.0.1"));
            assert!(is_valid_ipv4("172.16.0.1"));
            assert!(is_valid_ipv4("0.0.0.0"));
            assert!(is_valid_ipv4("255.255.255.255"));
        }

        #[test]
        fn is_valid_ipv4_accepts_empty_string() {
            assert!(is_valid_ipv4(""));
        }

        #[test]
        fn is_valid_ipv4_rejects_invalid_addresses() {
            assert!(!is_valid_ipv4("256.1.1.1"));
            assert!(!is_valid_ipv4("192.168.1"));
            assert!(!is_valid_ipv4("192.168.1.1.1"));
            assert!(!is_valid_ipv4("abc.def.ghi.jkl"));
            assert!(!is_valid_ipv4("192.168.-1.1"));
        }

        #[test]
        fn parse_netmask_accepts_valid_values() {
            assert_eq!(parse_netmask("24"), Some(24));
            assert_eq!(parse_netmask("/24"), Some(24));
            assert_eq!(parse_netmask("0"), Some(0));
            assert_eq!(parse_netmask("32"), Some(32));
            assert_eq!(parse_netmask("/8"), Some(8));
        }

        #[test]
        fn parse_netmask_rejects_invalid_values() {
            assert_eq!(parse_netmask("33"), None);
            assert_eq!(parse_netmask("abc"), None);
            assert_eq!(parse_netmask("-1"), None);
            assert_eq!(parse_netmask("24.5"), None);
        }

        #[test]
        fn cidr_to_subnet_converts_correctly() {
            assert_eq!(cidr_to_subnet(24), "255.255.255.0");
            assert_eq!(cidr_to_subnet(16), "255.255.0.0");
            assert_eq!(cidr_to_subnet(8), "255.0.0.0");
            assert_eq!(cidr_to_subnet(32), "255.255.255.255");
            assert_eq!(cidr_to_subnet(0), "0.0.0.0");
        }

        #[test]
        fn subnet_to_cidr_converts_correctly() {
            assert_eq!(subnet_to_cidr("255.255.255.0"), Some(24));
            assert_eq!(subnet_to_cidr("255.255.0.0"), Some(16));
            assert_eq!(subnet_to_cidr("255.0.0.0"), Some(8));
            assert_eq!(subnet_to_cidr("255.255.255.255"), Some(32));
            assert_eq!(subnet_to_cidr("0.0.0.0"), Some(0));
        }

        #[test]
        fn subnet_to_cidr_rejects_invalid_masks() {
            assert_eq!(subnet_to_cidr("255.255.255.1"), None); // Non-contiguous
            assert_eq!(subnet_to_cidr("255.255.256.0"), None); // Out of range
            assert_eq!(subnet_to_cidr("255.255.0"), None); // Too few parts
            assert_eq!(subnet_to_cidr("abc.def.ghi.jkl"), None);
        }
    }

    mod current_connection {
        use super::*;

        fn create_adapter(name: &str, ip: &str, online: bool) -> DeviceNetwork {
            DeviceNetwork {
                name: name.to_string(),
                mac: "00:11:22:33:44:55".to_string(),
                online,
                file: Some("/etc/network/interfaces".to_string()),
                ipv4: InternetProtocol {
                    addrs: vec![IpAddress {
                        addr: ip.to_string(),
                        dhcp: false,
                        prefix_len: 24,
                    }],
                    dns: vec![],
                    gateways: vec![],
                },
            }
        }

        #[test]
        fn returns_adapter_with_matching_ip() {
            let status = NetworkStatus {
                network_status: vec![
                    create_adapter("eth0", "192.168.1.100", true),
                    create_adapter("eth1", "192.168.2.100", true),
                ],
            };

            let adapter = status.current_connection_adapter(Some("192.168.1.100"));
            assert_eq!(adapter.map(|a| &a.name), Some(&"eth0".to_string()));
        }

        #[test]
        fn returns_none_for_hostname_without_ip_match() {
            let status = NetworkStatus {
                network_status: vec![
                    create_adapter("eth0", "192.168.1.100", false),
                    create_adapter("eth1", "192.168.2.100", true),
                    create_adapter("eth2", "192.168.3.100", true),
                ],
            };

            let adapter = status.current_connection_adapter(Some("omnect-device"));
            assert_eq!(adapter, None);
        }

        #[test]
        fn returns_none_for_no_hostname() {
            let status = NetworkStatus {
                network_status: vec![create_adapter("eth0", "192.168.1.100", true)],
            };

            let adapter = status.current_connection_adapter(None);
            assert_eq!(adapter, None);
        }

        #[test]
        fn returns_none_for_no_match() {
            let status = NetworkStatus {
                network_status: vec![create_adapter("eth0", "192.168.1.100", true)],
            };

            let adapter = status.current_connection_adapter(Some("192.168.99.99"));
            assert_eq!(adapter, None);
        }

        #[test]
        fn returns_none_when_no_online_adapters() {
            let status = NetworkStatus {
                network_status: vec![create_adapter("eth0", "192.168.1.100", false)],
            };

            let adapter = status.current_connection_adapter(Some("omnect-device"));
            assert_eq!(adapter, None);
        }

        #[test]
        fn returns_adapter_with_matching_localhost() {
            let status = NetworkStatus {
                network_status: vec![create_adapter("eth0", "localhost", true)],
            };

            let adapter = status.current_connection_adapter(Some("localhost"));
            assert_eq!(adapter.map(|a| &a.name), Some(&"eth0".to_string()));
        }
    }
}
