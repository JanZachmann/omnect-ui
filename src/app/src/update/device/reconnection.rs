use crux_core::Command;

use crate::events::Event;
use crate::http_get;
use crate::http_helpers::build_url;
use crate::model::Model;
use crate::types::{DeviceOperationState, NetworkChangeState, OverlaySpinnerState};
use crate::Effect;

use super::operations::is_update_complete;

/// Handle reconnection check tick - polls healthcheck endpoint
pub fn handle_reconnection_check_tick(model: &mut Model) -> Command<Effect, Event> {
    // Only check if we're waiting for reconnection
    if !matches!(
        model.device_operation_state,
        DeviceOperationState::Rebooting
            | DeviceOperationState::FactoryResetting
            | DeviceOperationState::Updating
            | DeviceOperationState::WaitingReconnection { .. }
    ) {
        return crux_core::render::render();
    }

    model.reconnection_attempt += 1;

    // Send healthcheck request
    http_get!(
        Device,
        DeviceEvent,
        &build_url("/healthcheck"),
        HealthcheckResponse,
        crate::types::HealthcheckInfo
    )
}

/// Handle reconnection timeout - device didn't come back online
pub fn handle_reconnection_timeout(model: &mut Model) -> Command<Effect, Event> {
    // Early return if not in a device operation state
    if !matches!(
        &model.device_operation_state,
        DeviceOperationState::Rebooting
            | DeviceOperationState::FactoryResetting
            | DeviceOperationState::Updating
            | DeviceOperationState::WaitingReconnection { .. }
    ) {
        return crux_core::render::render();
    }

    let operation = model.device_operation_state.operation_name();

    let timeout_msg = if matches!(
        model.device_operation_state,
        DeviceOperationState::FactoryResetting
    ) {
        "Device did not come back online after 10 minutes. Please check the device manually."
    } else {
        "Device did not come back online after 5 minutes. Please check the device manually."
    };

    model.device_operation_state = DeviceOperationState::ReconnectionFailed {
        operation: operation.clone(),
        reason: timeout_msg.to_string(),
    };

    // Update overlay spinner to show timeout
    model.overlay_spinner.set_text(timeout_msg);
    model.overlay_spinner.set_timed_out();

    crux_core::render::render()
}

/// Handle healthcheck response - manages reconnection and network change state machines
pub fn handle_healthcheck_response(
    result: Result<crate::types::HealthcheckInfo, String>,
    model: &mut Model,
) -> Command<Effect, Event> {
    // Update healthcheck info if success
    if let Ok(info) = &result {
        model.healthcheck = Some(info.clone());
    }

    // Handle reconnection state machine
    match &model.device_operation_state {
        DeviceOperationState::Rebooting
        | DeviceOperationState::FactoryResetting
        | DeviceOperationState::Updating => {
            // First check - if it fails, mark as "waiting"
            let is_updating =
                matches!(model.device_operation_state, DeviceOperationState::Updating);

            // For updates, we also check the status field
            // Consider update done when status is Succeeded, Recovered, or NoUpdate
            // (NoUpdate means there's no pending update, so previous one completed)
            let update_done = if is_updating {
                result.as_ref().ok().is_some_and(is_update_complete)
            } else {
                result.is_ok()
            };

            if result.is_err() {
                // Device went offline - mark it
                model.device_went_offline = true;
                // Transition to waiting
                let operation = model.device_operation_state.operation_name();
                model.device_operation_state = DeviceOperationState::WaitingReconnection {
                    operation,
                    attempt: model.reconnection_attempt,
                };
            } else if (update_done || !is_updating) && model.device_went_offline {
                // Device came back online after going offline - reconnection successful
                let operation = model.device_operation_state.operation_name();
                model.device_operation_state =
                    DeviceOperationState::ReconnectionSuccessful { operation };

                // Invalidate session as backend restart clears tokens
                model.invalidate_session();

                // Clear overlay spinner
                model.overlay_spinner.clear();
            }
            // else: healthcheck succeeded but device never went offline - keep checking
        }
        DeviceOperationState::WaitingReconnection { operation, .. } => {
            let is_update = operation == "Update";

            if result.is_err() {
                // Still offline - mark it
                model.device_went_offline = true;
                // Update attempt count
                model.device_operation_state = DeviceOperationState::WaitingReconnection {
                    operation: operation.clone(),
                    attempt: model.reconnection_attempt,
                };
            } else {
                // Consider update done when status is Succeeded, Recovered, or NoUpdate
                let update_done = if is_update {
                    result.as_ref().ok().is_some_and(is_update_complete)
                } else {
                    true
                };

                if update_done && model.device_went_offline {
                    // Success! Device is back online (or update finished) AND it went offline
                    model.device_operation_state = DeviceOperationState::ReconnectionSuccessful {
                        operation: operation.clone(),
                    };

                    // Invalidate session as backend restart clears tokens
                    model.invalidate_session();

                    // Clear overlay spinner
                    model.overlay_spinner.clear();
                }
                // else: healthcheck succeeded but device never went offline - keep checking
            }
        }
        _ => {} // Do nothing for other states
    }

    // Handle network change state machine for IP change polling
    match &model.network_change_state {
        NetworkChangeState::WaitingForNewIp {
            new_ip, ui_port, ..
        } => {
            if result.is_ok() {
                // Clone values before reassigning state to avoid borrow conflict
                let new_ip = new_ip.clone();
                let port = *ui_port;
                // New IP is reachable
                model.network_change_state = NetworkChangeState::NewIpReachable {
                    new_ip: new_ip.clone(),
                    ui_port: port,
                };
                // Update overlay for redirect
                model.overlay_spinner = OverlaySpinnerState::new("Network settings applied")
                    .with_text(format!("Redirecting to new IP: {new_ip}:{port}"));
            }
        }
        NetworkChangeState::WaitingForOldIp { .. } => {
            if result.is_ok() {
                // Old IP is reachable - Rollback successful
                model.network_change_state = NetworkChangeState::Idle;
                model.overlay_spinner.clear();
                model.invalidate_session();
                model.success_message =
                    Some("Automatic network rollback successful. Please log in.".to_string());
            }
        }
        _ => {}
    }

    crux_core::render::render()
}

#[cfg(test)]
mod tests {
    use crate::events::{DeviceEvent, Event};
    use crate::model::Model;
    use crate::types::{
        DeviceOperationState, HealthcheckInfo, NetworkChangeState, UpdateValidationStatus,
        VersionInfo,
    };
    use crate::App;
    use crux_core::testing::AppTester;

    fn create_healthcheck(status: &str, mismatch: bool) -> HealthcheckInfo {
        HealthcheckInfo {
            version_info: VersionInfo {
                required: "1.0.0".to_string(),
                current: if mismatch { "0.9.0" } else { "1.0.0" }.to_string(),
                mismatch,
            },
            update_validation_status: UpdateValidationStatus {
                status: status.to_string(),
            },
            network_rollback_occurred: false,
        }
    }

    mod reconnection_check_tick {
        use super::*;

        #[test]
        fn increments_attempt_counter_when_rebooting() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                device_operation_state: DeviceOperationState::Rebooting,
                reconnection_attempt: 0,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::ReconnectionCheckTick),
                &mut model,
            );

            assert_eq!(model.reconnection_attempt, 1);
        }

        #[test]
        fn increments_attempt_counter_when_factory_resetting() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                device_operation_state: DeviceOperationState::FactoryResetting,
                reconnection_attempt: 5,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::ReconnectionCheckTick),
                &mut model,
            );

            assert_eq!(model.reconnection_attempt, 6);
        }

        #[test]
        fn increments_attempt_counter_when_updating() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                device_operation_state: DeviceOperationState::Updating,
                reconnection_attempt: 0,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::ReconnectionCheckTick),
                &mut model,
            );

            assert_eq!(model.reconnection_attempt, 1);
        }

        #[test]
        fn does_not_increment_when_idle() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                device_operation_state: DeviceOperationState::Idle,
                reconnection_attempt: 0,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::ReconnectionCheckTick),
                &mut model,
            );

            assert_eq!(model.reconnection_attempt, 0);
        }
    }

    mod reconnection_timeout {
        use super::*;

        #[test]
        fn transitions_reboot_to_failed_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                device_operation_state: DeviceOperationState::Rebooting,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::ReconnectionTimeout),
                &mut model,
            );

            assert!(matches!(
                model.device_operation_state,
                DeviceOperationState::ReconnectionFailed { .. }
            ));
            if let DeviceOperationState::ReconnectionFailed { operation, reason } =
                model.device_operation_state
            {
                assert_eq!(operation, "Reboot");
                assert!(reason.contains("5 minutes"));
            }
            assert!(model.overlay_spinner.timed_out());
        }

        #[test]
        fn transitions_factory_reset_to_failed_with_longer_timeout() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                device_operation_state: DeviceOperationState::FactoryResetting,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::ReconnectionTimeout),
                &mut model,
            );

            assert!(matches!(
                model.device_operation_state,
                DeviceOperationState::ReconnectionFailed { .. }
            ));
            if let DeviceOperationState::ReconnectionFailed { operation, reason } =
                model.device_operation_state
            {
                assert_eq!(operation, "Factory Reset");
                assert!(reason.contains("10 minutes"));
            }
        }

        #[test]
        fn transitions_update_to_failed_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                device_operation_state: DeviceOperationState::Updating,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::ReconnectionTimeout),
                &mut model,
            );

            assert!(matches!(
                model.device_operation_state,
                DeviceOperationState::ReconnectionFailed { .. }
            ));
            if let DeviceOperationState::ReconnectionFailed { operation, .. } =
                model.device_operation_state
            {
                assert_eq!(operation, "Update");
            }
        }

        #[test]
        fn does_nothing_when_idle() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                device_operation_state: DeviceOperationState::Idle,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::ReconnectionTimeout),
                &mut model,
            );

            assert_eq!(model.device_operation_state, DeviceOperationState::Idle);
        }
    }

    mod healthcheck_response {
        use super::*;

        mod reboot {
            use super::*;

            #[test]
            fn error_marks_device_offline_and_transitions_to_waiting() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Rebooting,
                    device_went_offline: false,
                    reconnection_attempt: 2,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Err(
                        "Connection failed".to_string(),
                    ))),
                    &mut model,
                );

                assert!(model.device_went_offline);
                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
                if let DeviceOperationState::WaitingReconnection { operation, attempt } =
                    model.device_operation_state
                {
                    assert_eq!(operation, "Reboot");
                    assert_eq!(attempt, 2);
                }
            }

            #[test]
            fn success_after_offline_transitions_to_successful() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Rebooting,
                    device_went_offline: true,
                    is_authenticated: true,
                    auth_token: Some("test_token".to_string()),
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "valid", false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
                assert!(!model.is_authenticated);
                assert_eq!(model.auth_token, None);
                assert!(!model.overlay_spinner.is_visible());
            }

            #[test]
            fn success_without_offline_keeps_checking() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Rebooting,
                    device_went_offline: false,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "valid", false,
                    )))),
                    &mut model,
                );

                assert_eq!(model.device_operation_state, DeviceOperationState::Rebooting);
            }
        }

        mod factory_reset {
            use super::*;

            #[test]
            fn error_marks_device_offline() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::FactoryResetting,
                    device_went_offline: false,
                    reconnection_attempt: 3,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Err(
                        "Connection failed".to_string(),
                    ))),
                    &mut model,
                );

                assert!(model.device_went_offline);
                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
            }

            #[test]
            fn success_after_offline_transitions_to_successful() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::FactoryResetting,
                    device_went_offline: true,
                    is_authenticated: true,
                    auth_token: Some("test_token".to_string()),
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "valid", false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
                if let DeviceOperationState::ReconnectionSuccessful { operation } =
                    model.device_operation_state
                {
                    assert_eq!(operation, "Factory Reset");
                }
                assert!(!model.is_authenticated);
            }
        }

        mod update {
            use super::*;

            #[test]
            fn error_marks_device_offline() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: false,
                    reconnection_attempt: 1,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Err(
                        "Connection failed".to_string(),
                    ))),
                    &mut model,
                );

                assert!(model.device_went_offline);
                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
            }

            #[test]
            fn success_with_succeeded_status_after_offline_completes() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    is_authenticated: true,
                    auth_token: Some("test_token".to_string()),
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "Succeeded",
                        false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
                assert!(!model.is_authenticated);
            }

            #[test]
            fn success_with_recovered_status_after_offline_completes() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "Recovered",
                        false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
            }

            #[test]
            fn success_with_no_update_status_after_offline_completes() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "NoUpdate",
                        false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
            }

            #[test]
            fn success_with_incomplete_status_keeps_checking() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "InProgress",
                        false,
                    )))),
                    &mut model,
                );

                assert_eq!(model.device_operation_state, DeviceOperationState::Updating);
            }
        }

        mod waiting_reconnection {
            use super::*;

            #[test]
            fn error_updates_attempt_count() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Reboot".to_string(),
                        attempt: 5,
                    },
                    reconnection_attempt: 10,
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Err(
                        "Connection failed".to_string(),
                    ))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
                if let DeviceOperationState::WaitingReconnection { attempt, .. } =
                    model.device_operation_state
                {
                    assert_eq!(attempt, 10);
                }
            }

            #[test]
            fn success_for_non_update_operation_completes() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Reboot".to_string(),
                        attempt: 5,
                    },
                    device_went_offline: true,
                    is_authenticated: true,
                    auth_token: Some("test_token".to_string()),
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "valid", false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
                assert!(!model.is_authenticated);
            }

            #[test]
            fn success_for_update_with_completed_status() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Update".to_string(),
                        attempt: 3,
                    },
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "Succeeded",
                        false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
            }

            #[test]
            fn success_for_update_with_incomplete_status_keeps_waiting() {
                let app = AppTester::<App>::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Update".to_string(),
                        attempt: 3,
                    },
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "InProgress",
                        false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
            }
        }

        mod network_change {
            use super::*;

            #[test]
            fn successful_healthcheck_transitions_to_reachable() {
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

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Ok(create_healthcheck(
                        "valid", false,
                    )))),
                    &mut model,
                );

                assert!(matches!(
                    model.network_change_state,
                    NetworkChangeState::NewIpReachable { .. }
                ));
                if let NetworkChangeState::NewIpReachable { new_ip, ui_port } =
                    model.network_change_state
                {
                    assert_eq!(new_ip, "192.168.1.101");
                    assert_eq!(ui_port, 443);
                }
                assert!(model.overlay_spinner.is_visible());
            }

            #[test]
            fn failed_healthcheck_keeps_waiting() {
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

                let _ = app.update(
                    Event::Device(DeviceEvent::HealthcheckResponse(Err(
                        "Connection failed".to_string(),
                    ))),
                    &mut model,
                );

                assert!(matches!(
                    model.network_change_state,
                    NetworkChangeState::WaitingForNewIp { .. }
                ));
            }
        }
    }
}
