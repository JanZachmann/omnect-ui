use crux_core::Command;
use std::collections::HashMap;

use crate::{
    Effect,
    events::Event,
    model::Model,
    types::{NetworkFormData, NetworkFormState, is_valid_ipv4, subnet_to_cidr},
};

/// Handle network form start edit - initialize form with current network adapter data
pub fn handle_network_form_start_edit(
    adapter_name: &str,
    model: &mut Model,
) -> Command<Effect, Event> {
    // Find the network adapter and copy its data to form state
    if let Some(network_status) = &model.network_status
        && let Some(adapter) = network_status
            .network_status
            .iter()
            .find(|n| n.name == adapter_name)
    {
        let form_data = NetworkFormData::from(adapter);

        model.network_form_state = NetworkFormState::Editing {
            adapter_name: adapter_name.to_string(),
            form_data: form_data.clone(),
            original_data: form_data,
            errors: HashMap::new(),
        };
        // Clear dirty flag when starting a fresh edit
        model.network_form_dirty = false;
        // Clear rollback modal flags
        model.should_show_rollback_modal = false;
        model.default_rollback_enabled = false;
    }

    crux_core::render::render()
}

/// Handle network form update - update form data from user input
pub fn handle_network_form_update(
    form_data_json: &str,
    model: &mut Model,
) -> Command<Effect, Event> {
    // Parse the JSON form data
    let parsed: Result<NetworkFormData, _> = serde_json::from_str(form_data_json);

    match parsed {
        Ok(form_data) => {
            if let NetworkFormState::Editing {
                adapter_name,
                original_data,
                ..
            } = &model.network_form_state
            {
                // Validate that this update is for the adapter currently being edited
                if adapter_name != &form_data.name {
                    // Silently ignore updates from non-active adapters
                    return crux_core::render::render();
                }

                let mut errors = HashMap::new();

                // Validate IP Address (only if not DHCP)
                if !form_data.dhcp && !is_valid_ipv4(&form_data.ip_address) {
                    errors.insert("ipAddress".to_string(), "Invalid IPv4-Address".to_string());
                }

                // Validate Subnet Mask (only if not DHCP)
                if !form_data.dhcp && subnet_to_cidr(&form_data.subnet_mask).is_none() {
                    errors.insert("subnetMask".to_string(), "Invalid Subnet Mask".to_string());
                }

                let is_dirty = form_data != *original_data;

                // Compute rollback modal flags
                let (should_show_modal, default_enabled) =
                    compute_rollback_modal_state(&form_data, original_data, adapter_name, model);

                model.network_form_state = NetworkFormState::Editing {
                    adapter_name: adapter_name.clone(),
                    form_data,
                    original_data: original_data.clone(),
                    errors,
                };
                model.network_form_dirty = is_dirty;
                model.should_show_rollback_modal = should_show_modal;
                model.default_rollback_enabled = default_enabled;
            }
            crux_core::render::render()
        }
        Err(e) => model.set_error_and_render(format!("Invalid form data: {e}")),
    }
}

/// Compute whether to show rollback modal and default checkbox state
fn compute_rollback_modal_state(
    form_data: &NetworkFormData,
    original_data: &NetworkFormData,
    adapter_name: &str,
    model: &Model,
) -> (bool, bool) {
    // Check if this adapter is the current connection
    if !model.is_current_adapter(adapter_name) {
        return (false, false);
    }

    // Check if switching to DHCP
    let switching_to_dhcp = !original_data.dhcp && form_data.dhcp;

    // Show modal when any setting changed on current adapter
    let should_show = form_data != original_data;

    // Default rollback enabled: true UNLESS switching to DHCP (then false)
    let default_enabled = !switching_to_dhcp;

    (should_show, default_enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DeviceNetwork, InternetProtocol, IpAddress, NetworkStatus};

    fn create_test_network_adapter(name: &str, ip: &str, dhcp: bool) -> DeviceNetwork {
        DeviceNetwork {
            name: name.to_string(),
            mac: "00:11:22:33:44:55".to_string(),
            online: true,
            file: Some("/etc/network/interfaces".to_string()),
            ipv4: InternetProtocol {
                addrs: vec![IpAddress {
                    addr: ip.to_string(),
                    dhcp,
                    prefix_len: 24,
                }],
                dns: vec!["8.8.8.8".to_string()],
                gateways: vec!["192.168.1.1".to_string()],
            },
        }
    }

    mod network_form {
        use super::*;

        #[test]
        fn start_edit_transitions_to_editing_state() {
            let adapter = create_test_network_adapter("eth0", "192.168.1.100", false);
            let mut model = Model {
                network_status: Some(NetworkStatus {
                    network_status: vec![adapter.clone()],
                }),
                ..Default::default()
            };

            let _ = handle_network_form_start_edit("eth0", &mut model);

            assert!(matches!(
                model.network_form_state,
                NetworkFormState::Editing { .. }
            ));
            if let NetworkFormState::Editing {
                adapter_name,
                form_data,
                original_data,
                ..
            } = &model.network_form_state
            {
                assert_eq!(adapter_name, "eth0");
                assert_eq!(form_data.ip_address, "192.168.1.100");
                assert!(!form_data.dhcp);
                assert_eq!(form_data, original_data);
            }
            assert!(!model.network_form_dirty);
        }

        #[test]
        fn update_with_unchanged_data_keeps_clean_flag() {
            let form_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.100".to_string(),
                dhcp: false,
                subnet_mask: "255.255.255.0".to_string(),
                dns: vec!["8.8.8.8".to_string()],
                gateways: vec!["192.168.1.1".to_string()],
            };

            let mut model = Model {
                network_form_state: NetworkFormState::Editing {
                    adapter_name: "eth0".to_string(),
                    form_data: form_data.clone(),
                    original_data: form_data.clone(),
                    errors: HashMap::new(),
                },
                network_form_dirty: false,
                ..Default::default()
            };

            let _ =
                handle_network_form_update(&serde_json::to_string(&form_data).unwrap(), &mut model);

            assert!(!model.network_form_dirty);
        }

        #[test]
        fn update_with_changed_ip_sets_dirty_flag_with_subnet_mask() {
            let original_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.100".to_string(),
                dhcp: false,
                subnet_mask: "255.255.255.0".to_string(),
                dns: vec!["8.8.8.8".to_string()],
                gateways: vec!["192.168.1.1".to_string()],
            };

            let mut changed_data = original_data.clone();
            changed_data.ip_address = "192.168.1.101".to_string();

            let mut model = Model {
                network_form_state: NetworkFormState::Editing {
                    adapter_name: "eth0".to_string(),
                    form_data: original_data.clone(),
                    original_data: original_data.clone(),
                    errors: HashMap::new(),
                },
                network_form_dirty: false,
                ..Default::default()
            };

            let _ = handle_network_form_update(
                &serde_json::to_string(&changed_data).unwrap(),
                &mut model,
            );

            assert!(model.network_form_dirty);
        }

        #[test]
        fn reset_restarts_edit_from_original_adapter_data() {
            let adapter = create_test_network_adapter("eth0", "192.168.1.100", false);

            let modified_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.200".to_string(),
                dhcp: false,
                subnet_mask: "255.255.255.0".to_string(),
                dns: vec!["1.1.1.1".to_string()],
                gateways: vec!["192.168.1.254".to_string()],
            };

            let mut model = Model {
                network_status: Some(NetworkStatus {
                    network_status: vec![adapter.clone()],
                }),
                network_form_state: NetworkFormState::Editing {
                    adapter_name: "eth0".to_string(),
                    form_data: modified_data,
                    original_data: NetworkFormData::from(&adapter),
                    errors: HashMap::new(),
                },
                network_form_dirty: true,
                ..Default::default()
            };

            let _ = handle_network_form_start_edit("eth0", &mut model);

            if let NetworkFormState::Editing {
                form_data,
                original_data,
                ..
            } = &model.network_form_state
            {
                assert_eq!(form_data.ip_address, "192.168.1.100");
                assert_eq!(original_data.ip_address, "192.168.1.100");
            }
            assert!(!model.network_form_dirty);
        }

        #[test]
        fn ignores_updates_from_non_active_adapter() {
            let eth0_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.100".to_string(),
                dhcp: false,
                subnet_mask: "255.255.255.0".to_string(),
                dns: vec!["8.8.8.8".to_string()],
                gateways: vec!["192.168.1.1".to_string()],
            };

            let wlan0_data = NetworkFormData {
                name: "wlan0".to_string(),
                ip_address: "192.168.2.100".to_string(),
                dhcp: false,
                subnet_mask: "255.255.255.0".to_string(),
                dns: vec!["8.8.8.8".to_string()],
                gateways: vec!["192.168.2.1".to_string()],
            };

            let mut model = Model {
                network_form_state: NetworkFormState::Editing {
                    adapter_name: "eth0".to_string(),
                    form_data: eth0_data.clone(),
                    original_data: eth0_data.clone(),
                    errors: HashMap::new(),
                },
                network_form_dirty: false,
                ..Default::default()
            };

            let _ = handle_network_form_update(
                &serde_json::to_string(&wlan0_data).unwrap(),
                &mut model,
            );

            if let NetworkFormState::Editing {
                adapter_name,
                form_data,
                ..
            } = &model.network_form_state
            {
                assert_eq!(adapter_name, "eth0");
                assert_eq!(form_data.ip_address, "192.168.1.100");
            }
            assert!(!model.network_form_dirty);
        }
    }

    mod rollback_modal_flags {
        use super::*;

        fn create_network_status_with_adapter(name: &str, ip: &str) -> NetworkStatus {
            NetworkStatus {
                network_status: vec![DeviceNetwork {
                    name: name.to_string(),
                    mac: "00:11:22:33:44:55".to_string(),
                    online: true,
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
                }],
            }
        }

        #[test]
        fn shows_modal_when_ip_changed_on_current_adapter() {
            let network_status = create_network_status_with_adapter("eth0", "192.168.1.100");

            let original_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.100".to_string(),
                dhcp: false,
                subnet_mask: "255.255.255.0".to_string(),
                dns: vec![],
                gateways: vec![],
            };

            let mut model = Model {
                network_status: Some(network_status),
                current_connection_adapter: Some("eth0".to_string()),
                network_form_state: NetworkFormState::Editing {
                    adapter_name: "eth0".to_string(),
                    form_data: original_data.clone(),
                    original_data: original_data.clone(),
                    errors: HashMap::new(),
                },
                ..Default::default()
            };

            let mut changed_data = original_data.clone();
            changed_data.ip_address = "192.168.1.101".to_string();

            let _ = handle_network_form_update(
                &serde_json::to_string(&changed_data).unwrap(),
                &mut model,
            );

            assert!(model.should_show_rollback_modal);
            assert!(model.default_rollback_enabled);
        }
    }
}
