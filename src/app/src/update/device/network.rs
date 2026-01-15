use crux_core::Command;

use crate::auth_post;
use crate::events::{DeviceEvent, Event, UiEvent};
use crate::http_get_silent;
use crate::model::Model;
use crate::types::{
    HealthcheckInfo, NetworkChangeState, NetworkConfigRequest, NetworkFormData, NetworkFormState,
    OverlaySpinnerState,
};
use crate::unauth_post;
use crate::Effect;

/// Success message for network configuration update
const NETWORK_CONFIG_SUCCESS: &str = "Network configuration updated";

/// Handle network configuration request
pub fn handle_set_network_config(config: String, model: &mut Model) -> Command<Effect, Event> {
    // Parse the JSON config to extract metadata
    let parsed_config: Result<NetworkConfigRequest, _> = serde_json::from_str(&config);

    match parsed_config {
        Ok(config_req) => {
            // Store network change state for later use
            // Show modal for: IP changed OR switching to DHCP on current adapter
            if config_req.is_server_addr && (config_req.ip_changed || config_req.switching_to_dhcp)
            {
                model.network_change_state = NetworkChangeState::ApplyingConfig {
                    is_server_addr: true,
                    ip_changed: config_req.ip_changed || config_req.switching_to_dhcp,
                    new_ip: config_req.ip.clone().unwrap_or_default(),
                    old_ip: config_req.previous_ip.clone().unwrap_or_default(),
                    switching_to_dhcp: config_req.switching_to_dhcp,
                };
            }

            // Transition network form to submitting state
            if let Some(submitting) = model.network_form_state.to_submitting() {
                model.network_form_state = submitting;
            }

            // Clear dirty flag when submitting
            model.network_form_dirty = false;

            // Send the request to backend
            auth_post!(
                Device,
                DeviceEvent,
                model,
                "/network",
                SetNetworkConfigResponse,
                "Set network config",
                body_string: config,
                expect_json: crate::types::SetNetworkConfigResponse
            )
        }
        Err(e) => model.set_error_and_render(format!("Invalid network config: {e}")),
    }
}

/// Helper to update network state and spinner based on configuration response
fn update_network_state_and_spinner(
    model: &mut Model,
    new_ip: String,
    old_ip: String,
    ui_port: u16,
    rollback_timeout_seconds: u64,
    switching_to_dhcp: bool,
    rollback_enabled: bool,
) {
    // Determine target state
    // If switching to DHCP without rollback, we go to Idle
    if !rollback_enabled && switching_to_dhcp {
        model.network_change_state = NetworkChangeState::Idle;
    } else {
        model.network_change_state = NetworkChangeState::WaitingForNewIp {
            new_ip,
            old_ip,
            attempt: 0,
            rollback_timeout_seconds: if rollback_enabled {
                rollback_timeout_seconds
            } else {
                0
            },
            ui_port,
            switching_to_dhcp,
        };
    }

    // Determine overlay text
    let overlay_text = if switching_to_dhcp {
        if rollback_enabled {
            "Network configuration is being applied. Your connection will be interrupted. \
             Use your DHCP server or device console to find the new IP address. \
             You must access the new address and log in to cancel the automatic rollback."
        } else {
            "Network configuration has been applied. Your connection will be interrupted. \
             Use your DHCP server or device console to find the new IP address."
        }
    } else if rollback_enabled {
        "Network configuration is being applied. Click the button below to open the new address in a new tab. \
         You must access the new address and log in to cancel the automatic rollback."
    } else {
        "Network configuration has been applied. Your connection will be interrupted. \
         Click the button below to navigate to the new address."
    };

    let spinner = OverlaySpinnerState::new("Applying network settings").with_text(overlay_text);

    model.overlay_spinner = if rollback_enabled && !switching_to_dhcp {
        spinner.with_countdown(rollback_timeout_seconds as u32)
    } else if rollback_enabled && switching_to_dhcp {
        // Show countdown even for DHCP if rollback is enabled
        spinner.with_countdown(rollback_timeout_seconds as u32)
    } else {
        spinner
    };
}

/// Handle network configuration response
pub fn handle_set_network_config_response(
    result: Result<crate::types::SetNetworkConfigResponse, String>,
    model: &mut Model,
) -> Command<Effect, Event> {
    model.stop_loading();

    match result {
        Ok(response) => {
            // Check if we are applying a config that changes IP/DHCP
            if let NetworkChangeState::ApplyingConfig {
                new_ip,
                old_ip,
                switching_to_dhcp,
                ..
            } = &model.network_change_state.clone()
            {
                if response.rollback_enabled {
                    update_network_state_and_spinner(
                        model,
                        new_ip.clone(),
                        old_ip.clone(),
                        response.ui_port,
                        response.rollback_timeout_seconds,
                        *switching_to_dhcp,
                        true,
                    );
                } else {
                    update_network_state_and_spinner(
                        model,
                        new_ip.clone(),
                        old_ip.clone(),
                        response.ui_port,
                        0,
                        *switching_to_dhcp,
                        false,
                    );
                }

                model.success_message = Some(NETWORK_CONFIG_SUCCESS.to_string());
                model.network_form_state = NetworkFormState::Idle;
                crux_core::render::render()
            } else {
                // Not changing current connection's IP - just show success message
                model.success_message = Some(NETWORK_CONFIG_SUCCESS.to_string());
                model.network_change_state = NetworkChangeState::Idle;
                model.network_form_state = NetworkFormState::Idle;
                model.overlay_spinner.clear();
                crux_core::render::render()
            }
        }
        Err(e) => {
            model.set_error(e);
            model.network_change_state = NetworkChangeState::Idle;
            // Reset form state back to editing on failure
            if let Some(editing) = model.network_form_state.to_editing() {
                model.network_form_state = editing;
            }
            crux_core::render::render()
        }
    }
}

/// Handle new IP check tick - polls new IP to see if it's reachable
pub fn handle_new_ip_check_tick(model: &mut Model) -> Command<Effect, Event> {
    match &mut model.network_change_state {
        NetworkChangeState::WaitingForNewIp {
            new_ip,
            attempt,
            ui_port,
            switching_to_dhcp,
            ..
        } => {
            *attempt += 1;

            // If switching to DHCP, we don't know the new IP, so we can't poll it.
            // We just wait for the timeout (rollback) or for the user to manually navigate.
            if !*switching_to_dhcp {
                // Try to reach the new IP (silent GET - no error shown on failure)
                // Use HTTPS since the server only listens on HTTPS
                let url = format!("https://{new_ip}:{ui_port}/healthcheck");
                http_get_silent!(
                    url,
                    on_success: Event::Device(DeviceEvent::HealthcheckResponse(Ok(
                        HealthcheckInfo::default()
                    ))),
                    on_error: Event::Ui(UiEvent::ClearSuccess)
                )
            } else {
                crux_core::render::render()
            }
        }
        NetworkChangeState::WaitingForOldIp {
            old_ip,
            ui_port,
            attempt,
        } => {
            *attempt += 1;
            // Poll the old IP to see if rollback completed
            let url = format!("https://{old_ip}:{ui_port}/healthcheck");
            http_get_silent!(
                url,
                on_success: Event::Device(DeviceEvent::HealthcheckResponse(Ok(
                    HealthcheckInfo::default()
                ))),
                on_error: Event::Ui(UiEvent::ClearSuccess)
            )
        }
        _ => crux_core::render::render(),
    }
}

/// Handle new IP check timeout - new IP didn't become reachable in time
pub fn handle_new_ip_check_timeout(model: &mut Model) -> Command<Effect, Event> {
    if let NetworkChangeState::WaitingForNewIp {
        new_ip,
        old_ip,
        ui_port,
        rollback_timeout_seconds,
        switching_to_dhcp,
        ..
    } = &model.network_change_state
    {
        // If rollback was enabled (timeout > 0), we assume rollback happened on device
        if *rollback_timeout_seconds > 0 {
            model.network_change_state = NetworkChangeState::WaitingForOldIp {
                old_ip: old_ip.clone(),
                ui_port: *ui_port,
                attempt: 0,
            };
            model.overlay_spinner.set_text(
                "Automatic rollback initiated. Verifying connectivity at original address...",
            );
            // Ensure spinner is spinning (not timed out state)
            model.overlay_spinner.set_loading();
        } else {
            let new_ip_url = format!("https://{new_ip}:{ui_port}");
            model.network_change_state = NetworkChangeState::NewIpTimeout {
                new_ip: new_ip.clone(),
                old_ip: old_ip.clone(),
                ui_port: *ui_port,
                switching_to_dhcp: *switching_to_dhcp,
            };

            // Update overlay spinner to show timeout with manual link
            model.overlay_spinner.set_text(
                format!(
                    "Automatic rollback will occur soon. The network settings were not confirmed at the new address. \
                     Please navigate to: {new_ip_url}"
                )
                .as_str(),
            );
            model.overlay_spinner.set_timed_out();
        }
    }

    crux_core::render::render()
}

/// Handle network form start edit - initialize form with current network adapter data
pub fn handle_network_form_start_edit(
    adapter_name: String,
    model: &mut Model,
) -> Command<Effect, Event> {
    // Find the network adapter and copy its data to form state
    if let Some(network_status) = &model.network_status {
        if let Some(adapter) = network_status
            .network_status
            .iter()
            .find(|n| n.name == adapter_name)
        {
            let form_data = NetworkFormData::from(adapter);

            model.network_form_state = NetworkFormState::Editing {
                adapter_name: adapter_name.clone(),
                form_data: form_data.clone(),
                original_data: form_data,
            };
            // Clear dirty flag when starting a fresh edit
            model.network_form_dirty = false;
        }
    }

    crux_core::render::render()
}

/// Handle network form update - update form data from user input
pub fn handle_network_form_update(
    form_data_json: String,
    model: &mut Model,
) -> Command<Effect, Event> {
    // Parse the JSON form data
    let parsed: Result<NetworkFormData, _> = serde_json::from_str(&form_data_json);

    match parsed {
        Ok(form_data) => {
            if let NetworkFormState::Editing {
                adapter_name,
                original_data,
                ..
            } = &model.network_form_state
            {
                let is_dirty = form_data != *original_data;

                model.network_form_state = NetworkFormState::Editing {
                    adapter_name: adapter_name.clone(),
                    form_data,
                    original_data: original_data.clone(),
                };
                model.network_form_dirty = is_dirty;
            }
            crux_core::render::render()
        }
        Err(e) => model.set_error_and_render(format!("Invalid form data: {e}")),
    }
}

/// Handle acknowledge network rollback - clear the rollback occurred flag
pub fn handle_ack_rollback(model: &mut Model) -> Command<Effect, Event> {
    // Clear the rollback status in the model
    if let Some(healthcheck) = &mut model.healthcheck {
        healthcheck.network_rollback_occurred = false;
    }

    // Send POST request to backend to clear the marker file
    // Note: Using unauth_post instead of auth_post because this may be called before login
    // (the rollback notification appears in App.vue onMounted, before authentication)
    unauth_post!(
        Device,
        DeviceEvent,
        model,
        "/ack-rollback",
        AckRollbackResponse,
        "Acknowledge rollback"
    )
}

#[cfg(test)]
mod tests {
    use crate::events::{DeviceEvent, Event};
    use crate::model::Model;
    use crate::types::{
        DeviceNetwork, HealthcheckInfo, InternetProtocol, IpAddress, NetworkChangeState,
        NetworkFormData, NetworkFormState, NetworkStatus, OverlaySpinnerState,
        SetNetworkConfigResponse, UpdateValidationStatus, VersionInfo,
    };
    use crate::App;
    use crux_core::testing::AppTester;

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
            let app = AppTester::<App>::default();
            let adapter = create_test_network_adapter("eth0", "192.168.1.100", false);
            let mut model = Model {
                network_status: Some(NetworkStatus {
                    network_status: vec![adapter.clone()],
                }),
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::NetworkFormStartEdit {
                    adapter_name: "eth0".to_string(),
                }),
                &mut model,
            );

            assert!(matches!(
                model.network_form_state,
                NetworkFormState::Editing { .. }
            ));
            if let NetworkFormState::Editing {
                adapter_name,
                form_data,
                original_data,
            } = model.network_form_state
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
            let app = AppTester::<App>::default();
            let form_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.100".to_string(),
                dhcp: false,
                prefix_len: 24,
                dns: vec!["8.8.8.8".to_string()],
                gateways: vec!["192.168.1.1".to_string()],
            };

            let mut model = Model {
                network_form_state: NetworkFormState::Editing {
                    adapter_name: "eth0".to_string(),
                    form_data: form_data.clone(),
                    original_data: form_data.clone(),
                },
                network_form_dirty: false,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::NetworkFormUpdate {
                    form_data: serde_json::to_string(&form_data).unwrap(),
                }),
                &mut model,
            );

            assert!(!model.network_form_dirty);
        }

        #[test]
        fn update_with_changed_data_sets_dirty_flag() {
            let app = AppTester::<App>::default();
            let original_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.100".to_string(),
                dhcp: false,
                prefix_len: 24,
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
                },
                network_form_dirty: false,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::NetworkFormUpdate {
                    form_data: serde_json::to_string(&changed_data).unwrap(),
                }),
                &mut model,
            );

            assert!(model.network_form_dirty);
        }

        #[test]
        fn reset_restarts_edit_from_original_adapter_data() {
            let app = AppTester::<App>::default();
            let adapter = create_test_network_adapter("eth0", "192.168.1.100", false);

            let modified_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.200".to_string(),
                dhcp: false,
                prefix_len: 24,
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
                },
                network_form_dirty: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::NetworkFormReset {
                    adapter_name: "eth0".to_string(),
                }),
                &mut model,
            );

            if let NetworkFormState::Editing {
                form_data,
                original_data,
                ..
            } = model.network_form_state
            {
                assert_eq!(form_data.ip_address, "192.168.1.100");
                assert_eq!(original_data.ip_address, "192.168.1.100");
            }
            assert!(!model.network_form_dirty);
        }
    }

    mod network_configuration {
        use super::*;

        #[test]
        fn static_ip_with_rollback_enters_waiting_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::ApplyingConfig {
                    is_server_addr: true,
                    ip_changed: true,
                    new_ip: "192.168.1.101".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    switching_to_dhcp: false,
                },
                is_loading: true,
                ..Default::default()
            };

            let response = SetNetworkConfigResponse {
                rollback_timeout_seconds: 60,
                ui_port: 443,
                rollback_enabled: true,
            };

            let _ = app.update(
                Event::Device(DeviceEvent::SetNetworkConfigResponse(Ok(response))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(
                model.success_message,
                Some("Network configuration updated".to_string())
            );
            assert!(matches!(
                model.network_change_state,
                NetworkChangeState::WaitingForNewIp { .. }
            ));
            if let NetworkChangeState::WaitingForNewIp {
                new_ip,
                old_ip,
                rollback_timeout_seconds,
                switching_to_dhcp,
                ..
            } = model.network_change_state
            {
                assert_eq!(new_ip, "192.168.1.101");
                assert_eq!(old_ip, "192.168.1.100");
                assert_eq!(rollback_timeout_seconds, 60);
                assert!(!switching_to_dhcp);
            }
            assert_eq!(model.network_form_state, NetworkFormState::Idle);
        }

        #[test]
        fn static_ip_without_rollback_enters_waiting_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::ApplyingConfig {
                    is_server_addr: true,
                    ip_changed: true,
                    new_ip: "192.168.1.101".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    switching_to_dhcp: false,
                },
                is_loading: true,
                ..Default::default()
            };

            let response = SetNetworkConfigResponse {
                rollback_timeout_seconds: 0,
                ui_port: 443,
                rollback_enabled: false,
            };

            let _ = app.update(
                Event::Device(DeviceEvent::SetNetworkConfigResponse(Ok(response))),
                &mut model,
            );

            assert!(matches!(
                model.network_change_state,
                NetworkChangeState::WaitingForNewIp { .. }
            ));
            if let NetworkChangeState::WaitingForNewIp {
                rollback_timeout_seconds,
                old_ip,
                ..
            } = model.network_change_state
            {
                assert_eq!(old_ip, "192.168.1.100");
                assert_eq!(rollback_timeout_seconds, 0);
            }
        }

        #[test]
        fn dhcp_with_rollback_enters_waiting_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::ApplyingConfig {
                    is_server_addr: true,
                    ip_changed: true,
                    new_ip: "".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    switching_to_dhcp: true,
                },
                is_loading: true,
                ..Default::default()
            };

            let response = SetNetworkConfigResponse {
                rollback_timeout_seconds: 60,
                ui_port: 443,
                rollback_enabled: true,
            };

            let _ = app.update(
                Event::Device(DeviceEvent::SetNetworkConfigResponse(Ok(response))),
                &mut model,
            );

            assert!(matches!(
                model.network_change_state,
                NetworkChangeState::WaitingForNewIp { .. }
            ));
            if let NetworkChangeState::WaitingForNewIp {
                switching_to_dhcp,
                old_ip,
                ..
            } = model.network_change_state
            {
                assert_eq!(old_ip, "192.168.1.100");
                assert!(switching_to_dhcp);
            }
        }

        #[test]
        fn dhcp_without_rollback_goes_to_idle() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::ApplyingConfig {
                    is_server_addr: true,
                    ip_changed: true,
                    new_ip: "".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    switching_to_dhcp: true,
                },
                is_loading: true,
                ..Default::default()
            };

            let response = SetNetworkConfigResponse {
                rollback_timeout_seconds: 0,
                ui_port: 443,
                rollback_enabled: false,
            };

            let _ = app.update(
                Event::Device(DeviceEvent::SetNetworkConfigResponse(Ok(response))),
                &mut model,
            );

            assert_eq!(model.network_change_state, NetworkChangeState::Idle);
            assert!(model.overlay_spinner.is_visible());
            assert!(model.overlay_spinner.countdown_seconds().is_none());
        }

        #[test]
        fn non_server_adapter_returns_to_idle() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::Idle,
                is_loading: true,
                ..Default::default()
            };

            let response = SetNetworkConfigResponse {
                rollback_timeout_seconds: 60,
                ui_port: 443,
                rollback_enabled: true,
            };

            let _ = app.update(
                Event::Device(DeviceEvent::SetNetworkConfigResponse(Ok(response))),
                &mut model,
            );

            assert_eq!(model.network_change_state, NetworkChangeState::Idle);
            assert!(!model.overlay_spinner.is_visible());
        }

        #[test]
        fn error_resets_to_editing_state() {
            let app = AppTester::<App>::default();
            let form_data = NetworkFormData {
                name: "eth0".to_string(),
                ip_address: "192.168.1.100".to_string(),
                dhcp: false,
                prefix_len: 24,
                dns: vec![],
                gateways: vec![],
            };

            let mut model = Model {
                network_form_state: NetworkFormState::Submitting {
                    adapter_name: "eth0".to_string(),
                    form_data: form_data.clone(),
                    original_data: form_data.clone(),
                },
                network_change_state: NetworkChangeState::ApplyingConfig {
                    is_server_addr: true,
                    ip_changed: true,
                    new_ip: "192.168.1.101".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    switching_to_dhcp: false,
                },
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::SetNetworkConfigResponse(Err(
                    "Network error".to_string()
                ))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert!(model.error_message.is_some());
            assert_eq!(model.network_change_state, NetworkChangeState::Idle);
            assert!(matches!(
                model.network_form_state,
                NetworkFormState::Editing { .. }
            ));
        }
    }

    mod ip_change_detection {
        use super::*;

        #[test]
        fn tick_increments_attempt_counter() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::WaitingForNewIp {
                    new_ip: "192.168.1.101".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    attempt: 0,
                    rollback_timeout_seconds: 60,
                    ui_port: 443,
                    switching_to_dhcp: false,
                },
                ..Default::default()
            };

            let _ = app.update(Event::Device(DeviceEvent::NewIpCheckTick), &mut model);

            if let NetworkChangeState::WaitingForNewIp { attempt, .. } = model.network_change_state
            {
                assert_eq!(attempt, 1);
            }
        }

        #[test]
        fn tick_skips_polling_when_switching_to_dhcp() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::WaitingForNewIp {
                    new_ip: "".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    attempt: 0,
                    rollback_timeout_seconds: 60,
                    ui_port: 443,
                    switching_to_dhcp: true,
                },
                ..Default::default()
            };

            let _ = app.update(Event::Device(DeviceEvent::NewIpCheckTick), &mut model);

            if let NetworkChangeState::WaitingForNewIp { attempt, .. } = model.network_change_state
            {
                assert_eq!(attempt, 1);
            }
        }

        #[test]
        fn timeout_transitions_to_waiting_for_old_ip_if_rollback_enabled() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::WaitingForNewIp {
                    new_ip: "192.168.1.101".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    attempt: 10,
                    rollback_timeout_seconds: 60,
                    ui_port: 443,
                    switching_to_dhcp: false,
                },
                overlay_spinner: OverlaySpinnerState::new("Test Spinner"),
                ..Default::default()
            };

            let _ = app.update(Event::Device(DeviceEvent::NewIpCheckTimeout), &mut model);

            assert!(matches!(
                model.network_change_state,
                NetworkChangeState::WaitingForOldIp { .. }
            ));
            if let NetworkChangeState::WaitingForOldIp {
                old_ip, ui_port, ..
            } = model.network_change_state
            {
                assert_eq!(old_ip, "192.168.1.100");
                assert_eq!(ui_port, 443);
            }
            assert!(model.overlay_spinner.is_visible());
            assert!(!model.overlay_spinner.timed_out());
        }

        #[test]
        fn timeout_transitions_to_timeout_state_if_rollback_disabled() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::WaitingForNewIp {
                    new_ip: "192.168.1.101".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    attempt: 10,
                    rollback_timeout_seconds: 0,
                    ui_port: 443,
                    switching_to_dhcp: false,
                },
                ..Default::default()
            };

            let _ = app.update(Event::Device(DeviceEvent::NewIpCheckTimeout), &mut model);

            assert!(matches!(
                model.network_change_state,
                NetworkChangeState::NewIpTimeout { .. }
            ));
            if let NetworkChangeState::NewIpTimeout {
                new_ip, ui_port, ..
            } = model.network_change_state
            {
                assert_eq!(new_ip, "192.168.1.101");
                assert_eq!(ui_port, 443);
            }
            assert!(model.overlay_spinner.timed_out());
        }

        #[test]
        fn successful_healthcheck_on_new_ip() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                network_change_state: NetworkChangeState::WaitingForNewIp {
                    new_ip: "192.168.1.101".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    attempt: 5,
                    rollback_timeout_seconds: 60,
                    ui_port: 443,
                    switching_to_dhcp: false,
                },
                ..Default::default()
            };

            let healthcheck = HealthcheckInfo {
                version_info: VersionInfo {
                    required: "1.0.0".to_string(),
                    current: "1.0.0".to_string(),
                    mismatch: false,
                },
                update_validation_status: UpdateValidationStatus {
                    status: "valid".to_string(),
                },
                network_rollback_occurred: false,
            };

            let _ = app.update(
                Event::Device(DeviceEvent::HealthcheckResponse(Ok(healthcheck.clone()))),
                &mut model,
            );

            assert_eq!(model.healthcheck, Some(healthcheck));
        }
    }

    mod rollback_acknowledgment {
        use super::*;

        #[test]
        fn clears_rollback_flag_in_healthcheck() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                healthcheck: Some(HealthcheckInfo {
                    version_info: VersionInfo {
                        required: "1.0.0".to_string(),
                        current: "1.0.0".to_string(),
                        mismatch: false,
                    },
                    update_validation_status: UpdateValidationStatus {
                        status: "valid".to_string(),
                    },
                    network_rollback_occurred: true,
                }),
                ..Default::default()
            };

            let _ = app.update(Event::Device(DeviceEvent::AckRollback), &mut model);

            if let Some(healthcheck) = &model.healthcheck {
                assert!(!healthcheck.network_rollback_occurred);
            }
        }

        #[test]
        fn handles_missing_healthcheck_gracefully() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                healthcheck: None,
                ..Default::default()
            };

            let _ = app.update(Event::Device(DeviceEvent::AckRollback), &mut model);

            assert!(model.healthcheck.is_none());
        }

        #[test]
        fn ack_rollback_response_stops_loading() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::AckRollbackResponse(Ok(()))),
                &mut model,
            );

            assert!(!model.is_loading);
        }

        #[test]
        fn ack_rollback_response_error_sets_error_message() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::AckRollbackResponse(Err(
                    "Failed to acknowledge rollback".to_string(),
                ))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert!(model.error_message.is_some());
        }
    }
}
