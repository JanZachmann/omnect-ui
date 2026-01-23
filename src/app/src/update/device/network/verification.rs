use crux_core::Command;

use crate::{
    events::{DeviceEvent, Event, UiEvent},
    http_get_silent,
    model::Model,
    types::{HealthcheckInfo, NetworkChangeState, OverlaySpinnerState},
    unauth_post, Effect,
};

/// Helper to update network state and spinner based on configuration response
pub fn update_network_state_and_spinner(
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
            "Applying network configuration. Find the new IP via DHCP server or console, then log in to prevent automatic rollback."
        } else {
            "Network configuration applied. Find the new IP via DHCP server or console."
        }
    } else if rollback_enabled {
        "Applying network configuration. Log in at the new address to confirm the change and prevent automatic rollback."
    } else {
        "Network configuration applied. Your connection will be interrupted."
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
            // Use http_get! to parse the response body (needed for network_rollback_occurred flag)
            use crate::http_get;
            http_get!(
                Device,
                DeviceEvent,
                &url,
                HealthcheckResponse,
                crate::types::HealthcheckInfo
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
            model
                .overlay_spinner
                .set_text("Rollback in progress. Verifying original address...");
            // Ensure spinner is spinning (not timed out state)
            model.overlay_spinner.set_loading();
        } else {
            model.network_change_state = NetworkChangeState::NewIpTimeout {
                new_ip: new_ip.clone(),
                old_ip: old_ip.clone(),
                ui_port: *ui_port,
                switching_to_dhcp: *switching_to_dhcp,
            };

            // Update overlay spinner to show timeout with manual link
            model.overlay_spinner.set_text(
                "Unable to reach new address automatically. Click below to navigate manually.",
            );
            model.overlay_spinner.set_timed_out();
        }
    }

    crux_core::render::render()
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
    use super::*;
    use crate::types::{HealthcheckInfo, UpdateValidationStatus, VersionInfo};

    mod ip_change_detection {
        use super::*;

        #[test]
        fn tick_increments_attempt_counter() {
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

            let _ = handle_new_ip_check_tick(&mut model);

            // Verify attempt counter was incremented
            if let NetworkChangeState::WaitingForNewIp { attempt, .. } = model.network_change_state
            {
                assert_eq!(attempt, 1);
            } else {
                panic!("Expected WaitingForNewIp state");
            }
        }

        #[test]
        fn tick_skips_polling_when_switching_to_dhcp() {
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

            let _ = handle_new_ip_check_tick(&mut model);

            if let NetworkChangeState::WaitingForNewIp { attempt, .. } = model.network_change_state
            {
                assert_eq!(attempt, 1);
            }
        }

        #[test]
        fn timeout_transitions_to_waiting_for_old_ip_if_rollback_enabled() {
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

            let _ = handle_new_ip_check_timeout(&mut model);

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

            let _ = handle_new_ip_check_timeout(&mut model);

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

            let _ = crate::update::device::handle(
                DeviceEvent::HealthcheckResponse(Ok(healthcheck.clone())),
                &mut model,
            );

            assert_eq!(model.healthcheck, Some(healthcheck));
        }
    }

    mod rollback_acknowledgment {
        use super::*;

        #[test]
        fn clears_rollback_flag_in_healthcheck() {
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

            let _ = handle_ack_rollback(&mut model);

            if let Some(healthcheck) = &model.healthcheck {
                assert!(!healthcheck.network_rollback_occurred);
            }
        }

        #[test]
        fn handles_missing_healthcheck_gracefully() {
            let mut model = Model {
                healthcheck: None,
                ..Default::default()
            };

            let _ = handle_ack_rollback(&mut model);

            assert!(model.healthcheck.is_none());
        }

        #[test]
        fn ack_rollback_response_stops_loading() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ =
                crate::update::device::handle(DeviceEvent::AckRollbackResponse(Ok(())), &mut model);

            assert!(!model.is_loading);
        }

        #[test]
        fn ack_rollback_response_error_sets_error_message() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = crate::update::device::handle(
                DeviceEvent::AckRollbackResponse(Err("Failed to acknowledge rollback".to_string())),
                &mut model,
            );

            assert!(!model.is_loading);
            assert!(model.error_message.is_some());
        }
    }
}
