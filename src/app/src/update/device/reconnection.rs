use crux_core::Command;

use crate::{
    Effect, TimeCmd,
    events::Event,
    http_get,
    http_helpers::build_url,
    model::Model,
    types::{DeviceOperationState, NetworkChangeState, OverlaySpinnerState, UploadState},
};

/// Reconnection poll interval
const RECONNECTION_POLL_INTERVAL_MS: u64 = 5000;

use super::operations::{
    is_actual_update_result, is_update_complete, schedule_reconnection_countdown_tick,
};

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

    Command::all([
        http_get!(
            Device,
            DeviceEvent,
            &build_url("/healthcheck"),
            HealthcheckResponse,
            crate::types::HealthcheckInfo
        ),
        schedule_reconnection_poll(),
    ])
}

pub(super) fn schedule_reconnection_poll() -> Command<Effect, Event> {
    use crate::events::DeviceEvent;
    let (timer, handle) = TimeCmd::notify_after(std::time::Duration::from_millis(
        RECONNECTION_POLL_INTERVAL_MS,
    ));
    std::mem::forget(handle);
    timer.then_send(|_| Event::Device(DeviceEvent::ReconnectionCheckTick))
}

/// Handle reconnection countdown tick - decrements the displayed countdown each second
pub fn handle_reconnection_countdown_tick(model: &mut Model) -> Command<Effect, Event> {
    let is_active = matches!(
        model.device_operation_state,
        DeviceOperationState::Rebooting
            | DeviceOperationState::FactoryResetting
            | DeviceOperationState::Updating
            | DeviceOperationState::WaitingReconnection { .. }
    );
    if is_active && model.overlay_spinner.countdown_seconds() > Some(0) {
        model.overlay_spinner.decrement_countdown();
        Command::all([
            crux_core::render::render(),
            schedule_reconnection_countdown_tick(),
        ])
    } else {
        Command::done()
    }
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

    let timeout_msg =
        "Device did not come back online. You may need to re-accept the security certificate.";

    model.device_operation_state = DeviceOperationState::ReconnectionFailed {
        operation,
        reason: timeout_msg.to_string(),
    };

    // Update overlay spinner to show timeout
    model.overlay_spinner.set_text(timeout_msg);
    model.overlay_spinner.set_timed_out();

    crux_core::render::render()
}

/// Handle healthcheck response - manages reconnection and network change state machines
#[allow(clippy::needless_pass_by_value)]
pub fn handle_healthcheck_response(
    result: Result<crate::types::HealthcheckInfo, String>,
    model: &mut Model,
) -> Command<Effect, Event> {
    if let Ok(info) = &result {
        update_version_info(info, model);
        model.healthcheck = Some(info.clone());
    }
    advance_device_operation_state(&result, model);
    advance_network_change_state(&result, model);
    crux_core::render::render()
}

fn update_version_info(info: &crate::types::HealthcheckInfo, model: &mut Model) {
    model.version_mismatch = info.version_info.mismatch;
    model.version_mismatch_message = if info.version_info.mismatch {
        Some(format!(
            "Current version: {}. Required version {}. Please consider to update omnect Secure OS.",
            info.version_info.current, info.version_info.required,
        ))
    } else {
        None
    };
}

fn advance_device_operation_state(
    result: &Result<crate::types::HealthcheckInfo, String>,
    model: &mut Model,
) {
    match &model.device_operation_state {
        DeviceOperationState::Rebooting
        | DeviceOperationState::FactoryResetting
        | DeviceOperationState::Updating => {
            let is_updating =
                matches!(model.device_operation_state, DeviceOperationState::Updating);

            // For updates, completion requires a terminal status field (not just reachability)
            let update_done = if is_updating {
                result.as_ref().ok().is_some_and(is_update_complete)
            } else {
                result.is_ok()
            };

            if result.is_err() {
                model.device_went_offline = true;
                let operation = model.device_operation_state.operation_name();
                model.device_operation_state = DeviceOperationState::WaitingReconnection {
                    operation,
                    attempt: model.reconnection_attempt,
                };
            } else if (update_done || !is_updating) && model.device_went_offline {
                let operation = model.device_operation_state.operation_name();
                model.device_operation_state =
                    DeviceOperationState::ReconnectionSuccessful { operation };
                // Invalidate session as backend restart clears tokens
                model.invalidate_session();
                model.overlay_spinner.clear();

                // Clear stale firmware page state so the update page is fresh on re-login.
                // Only when an actual update ran — "NoUpdate" preserves the loaded manifest.
                if is_updating
                    && let Ok(info) = &result
                    && is_actual_update_result(info)
                {
                    model.update_manifest = None;
                    model.firmware_upload_state = UploadState::Idle;
                }
            }
            // else: device never went offline - keep checking
        }
        DeviceOperationState::WaitingReconnection { operation, .. } => {
            let is_update = operation == "Update";

            if result.is_err() {
                model.device_went_offline = true;
                model.device_operation_state = DeviceOperationState::WaitingReconnection {
                    operation: operation.clone(),
                    attempt: model.reconnection_attempt,
                };
            } else {
                // For updates, completion requires a terminal status field
                let update_done = if is_update {
                    result.as_ref().ok().is_some_and(is_update_complete)
                } else {
                    true
                };

                if update_done && model.device_went_offline {
                    model.device_operation_state = DeviceOperationState::ReconnectionSuccessful {
                        operation: operation.clone(),
                    };
                    // Invalidate session as backend restart clears tokens
                    model.invalidate_session();
                    model.overlay_spinner.clear();

                    // Clear stale firmware page state so the update page is fresh on re-login.
                    // Only when an actual update ran — "NoUpdate" preserves the loaded manifest.
                    if is_update
                        && let Ok(info) = &result
                        && is_actual_update_result(info)
                    {
                        model.update_manifest = None;
                        model.firmware_upload_state = UploadState::Idle;
                    }
                }
                // else: device never went offline - keep checking
            }
        }
        _ => {}
    }
}

fn advance_network_change_state(
    result: &Result<crate::types::HealthcheckInfo, String>,
    model: &mut Model,
) {
    match &model.network_change_state {
        NetworkChangeState::WaitingForNewIp {
            new_ip, ui_port, ..
        } => {
            if result.is_ok() {
                // Clone values before reassigning state to avoid borrow conflict
                let new_ip = new_ip.clone();
                let port = *ui_port;
                model.network_change_state = NetworkChangeState::NewIpReachable {
                    new_ip: new_ip.clone(),
                    ui_port: port,
                };
                model.success_message = None;
                model.error_message = None;
                model.overlay_spinner = OverlaySpinnerState::new("Network settings applied")
                    .with_text(format!("Redirecting to new IP: {new_ip}:{port}"));
            }
        }
        NetworkChangeState::WaitingForOldIp { .. } => {
            if result.is_ok() {
                // Old IP is reachable — rollback successful.
                // No success message here: the "Network Settings Rolled Back" modal
                // is triggered by the `network_rollback_occurred` flag in the healthcheck response.
                model.network_change_state = NetworkChangeState::Idle;
                model.overlay_spinner.clear();
                model.invalidate_session();
                model.success_message = None;
                model.error_message = None;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::Model,
        types::{
            DeviceOperationState, HealthcheckInfo, NetworkChangeState, UpdateValidationStatus,
            VersionInfo,
        },
    };

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
            ..Default::default()
        }
    }

    mod reconnection_check_tick {
        use super::*;

        #[test]
        fn increments_attempt_counter_when_rebooting() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Rebooting,
                reconnection_attempt: 0,
                ..Default::default()
            };

            let _ = handle_reconnection_check_tick(&mut model);

            assert_eq!(model.reconnection_attempt, 1);
        }

        #[test]
        fn increments_attempt_counter_when_factory_resetting() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::FactoryResetting,
                reconnection_attempt: 5,
                ..Default::default()
            };

            let _ = handle_reconnection_check_tick(&mut model);

            assert_eq!(model.reconnection_attempt, 6);
        }

        #[test]
        fn increments_attempt_counter_when_updating() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Updating,
                reconnection_attempt: 0,
                ..Default::default()
            };

            let _ = handle_reconnection_check_tick(&mut model);

            assert_eq!(model.reconnection_attempt, 1);
        }

        #[test]
        fn does_not_increment_when_idle() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Idle,
                reconnection_attempt: 0,
                ..Default::default()
            };

            let _ = handle_reconnection_check_tick(&mut model);

            assert_eq!(model.reconnection_attempt, 0);
        }

        #[test]
        fn sends_get_to_healthcheck_endpoint() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Rebooting,
                ..Default::default()
            };
            let mut cmd = handle_reconnection_check_tick(&mut model);

            // Command::all([http_get!(...), schedule_poll()]) produces Http + Time effects;
            // find the Http one.
            let http_effect = cmd
                .effects()
                .find_map(|e| {
                    if let Effect::Http(_) = e {
                        Some(e.expect_http())
                    } else {
                        None
                    }
                })
                .expect("expected Http effect");
            let (http_request, _) = http_effect.split();

            assert_eq!(http_request.url, "https://relative/healthcheck");
            assert_eq!(http_request.method, "GET");
        }
    }

    mod reconnection_timeout {
        use super::*;

        #[test]
        fn transitions_reboot_to_failed_state() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Rebooting,
                ..Default::default()
            };

            let _ = handle_reconnection_timeout(&mut model);

            assert!(matches!(
                model.device_operation_state,
                DeviceOperationState::ReconnectionFailed { .. }
            ));
            if let DeviceOperationState::ReconnectionFailed { operation, reason } =
                &model.device_operation_state
            {
                assert_eq!(operation, "Reboot");
                assert!(reason.contains("security certificate"));
            }
            assert!(model.overlay_spinner.timed_out());
        }

        #[test]
        fn transitions_factory_reset_to_failed_with_longer_timeout() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::FactoryResetting,
                ..Default::default()
            };

            let _ = handle_reconnection_timeout(&mut model);

            assert!(matches!(
                model.device_operation_state,
                DeviceOperationState::ReconnectionFailed { .. }
            ));
            if let DeviceOperationState::ReconnectionFailed { operation, reason } =
                &model.device_operation_state
            {
                assert_eq!(operation, "Factory Reset");
                assert!(reason.contains("security certificate"));
            }
        }

        #[test]
        fn transitions_update_to_failed_state() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Updating,
                ..Default::default()
            };

            let _ = handle_reconnection_timeout(&mut model);

            assert!(matches!(
                model.device_operation_state,
                DeviceOperationState::ReconnectionFailed { .. }
            ));
            if let DeviceOperationState::ReconnectionFailed { operation, .. } =
                &model.device_operation_state
            {
                assert_eq!(operation, "Update");
            }
        }

        #[test]
        fn does_nothing_when_idle() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Idle,
                ..Default::default()
            };

            let _ = handle_reconnection_timeout(&mut model);

            assert_eq!(model.device_operation_state, DeviceOperationState::Idle);
        }
    }

    mod healthcheck_response {
        use super::*;

        mod reboot {
            use super::*;

            #[test]
            fn error_marks_device_offline_and_transitions_to_waiting() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Rebooting,
                    device_went_offline: false,
                    reconnection_attempt: 2,
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Err("Connection failed".to_string()), &mut model);

                assert!(model.device_went_offline);
                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
                if let DeviceOperationState::WaitingReconnection { operation, attempt } =
                    &model.device_operation_state
                {
                    assert_eq!(operation, "Reboot");
                    assert_eq!(*attempt, 2);
                }
            }

            #[test]
            fn success_after_offline_transitions_to_successful() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Rebooting,
                    device_went_offline: true,
                    is_authenticated: true,
                    auth_token: Some("test_token".to_string()),
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Ok(create_healthcheck("valid", false)), &mut model);

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
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Rebooting,
                    device_went_offline: false,
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Ok(create_healthcheck("valid", false)), &mut model);

                assert_eq!(
                    model.device_operation_state,
                    DeviceOperationState::Rebooting
                );
            }
        }

        mod factory_reset {
            use super::*;

            #[test]
            fn error_marks_device_offline() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::FactoryResetting,
                    device_went_offline: false,
                    reconnection_attempt: 3,
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Err("Connection failed".to_string()), &mut model);

                assert!(model.device_went_offline);
                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
            }

            #[test]
            fn success_after_offline_transitions_to_successful() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::FactoryResetting,
                    device_went_offline: true,
                    is_authenticated: true,
                    auth_token: Some("test_token".to_string()),
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Ok(create_healthcheck("valid", false)), &mut model);

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
                if let DeviceOperationState::ReconnectionSuccessful { operation } =
                    &model.device_operation_state
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
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: false,
                    reconnection_attempt: 1,
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Err("Connection failed".to_string()), &mut model);

                assert!(model.device_went_offline);
                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
            }

            #[test]
            fn success_with_succeeded_status_after_offline_completes() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    is_authenticated: true,
                    auth_token: Some("test_token".to_string()),
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("Succeeded", false)),
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
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("Recovered", false)),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
            }

            #[test]
            fn success_with_no_update_status_after_offline_completes() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("NoUpdate", false)),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
            }

            #[test]
            fn success_with_incomplete_status_keeps_checking() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("InProgress", false)),
                    &mut model,
                );

                assert_eq!(model.device_operation_state, DeviceOperationState::Updating);
            }

            #[test]
            fn clears_manifest_and_upload_state_on_succeeded() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    update_manifest: Some(crate::types::UpdateManifest::default()),
                    firmware_upload_state: crate::types::UploadState::Completed,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("Succeeded", false)),
                    &mut model,
                );

                assert!(model.update_manifest.is_none());
                assert_eq!(model.firmware_upload_state, crate::types::UploadState::Idle);
            }

            #[test]
            fn clears_manifest_and_upload_state_on_recovered() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    update_manifest: Some(crate::types::UpdateManifest::default()),
                    firmware_upload_state: crate::types::UploadState::Completed,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("Recovered", false)),
                    &mut model,
                );

                assert!(model.update_manifest.is_none());
                assert_eq!(model.firmware_upload_state, crate::types::UploadState::Idle);
            }

            #[test]
            fn does_not_clear_manifest_on_no_update() {
                let manifest = crate::types::UpdateManifest::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::Updating,
                    device_went_offline: true,
                    update_manifest: Some(manifest.clone()),
                    firmware_upload_state: crate::types::UploadState::Completed,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("NoUpdate", false)),
                    &mut model,
                );

                assert_eq!(model.update_manifest, Some(manifest));
                assert_eq!(
                    model.firmware_upload_state,
                    crate::types::UploadState::Completed
                );
            }
        }

        mod waiting_reconnection {
            use super::*;

            #[test]
            fn error_updates_attempt_count() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Reboot".to_string(),
                        attempt: 5,
                    },
                    reconnection_attempt: 10,
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Err("Connection failed".to_string()), &mut model);

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
                if let DeviceOperationState::WaitingReconnection { attempt, .. } =
                    &model.device_operation_state
                {
                    assert_eq!(*attempt, 10);
                }
            }

            #[test]
            fn success_for_non_update_operation_completes() {
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

                let _ =
                    handle_healthcheck_response(Ok(create_healthcheck("valid", false)), &mut model);

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
                assert!(!model.is_authenticated);
            }

            #[test]
            fn success_for_update_with_completed_status() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Update".to_string(),
                        attempt: 3,
                    },
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("Succeeded", false)),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::ReconnectionSuccessful { .. }
                ));
            }

            #[test]
            fn success_for_update_with_incomplete_status_keeps_waiting() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Update".to_string(),
                        attempt: 3,
                    },
                    device_went_offline: true,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("InProgress", false)),
                    &mut model,
                );

                assert!(matches!(
                    model.device_operation_state,
                    DeviceOperationState::WaitingReconnection { .. }
                ));
            }

            #[test]
            fn clears_manifest_and_upload_state_on_succeeded_from_waiting() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Update".to_string(),
                        attempt: 3,
                    },
                    device_went_offline: true,
                    update_manifest: Some(crate::types::UpdateManifest::default()),
                    firmware_upload_state: crate::types::UploadState::Completed,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("Succeeded", false)),
                    &mut model,
                );

                assert!(model.update_manifest.is_none());
                assert_eq!(model.firmware_upload_state, crate::types::UploadState::Idle);
            }

            #[test]
            fn clears_manifest_and_upload_state_on_recovered_from_waiting() {
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Update".to_string(),
                        attempt: 3,
                    },
                    device_went_offline: true,
                    update_manifest: Some(crate::types::UpdateManifest::default()),
                    firmware_upload_state: crate::types::UploadState::Completed,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("Recovered", false)),
                    &mut model,
                );

                assert!(model.update_manifest.is_none());
                assert_eq!(model.firmware_upload_state, crate::types::UploadState::Idle);
            }

            #[test]
            fn does_not_clear_manifest_on_no_update_from_waiting() {
                let manifest = crate::types::UpdateManifest::default();
                let mut model = Model {
                    device_operation_state: DeviceOperationState::WaitingReconnection {
                        operation: "Update".to_string(),
                        attempt: 3,
                    },
                    device_went_offline: true,
                    update_manifest: Some(manifest.clone()),
                    firmware_upload_state: crate::types::UploadState::Completed,
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(
                    Ok(create_healthcheck("NoUpdate", false)),
                    &mut model,
                );

                assert_eq!(model.update_manifest, Some(manifest));
                assert_eq!(
                    model.firmware_upload_state,
                    crate::types::UploadState::Completed
                );
            }
        }

        mod network_change {
            use super::*;

            #[test]
            fn successful_healthcheck_transitions_to_reachable() {
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

                let _ =
                    handle_healthcheck_response(Ok(create_healthcheck("valid", false)), &mut model);

                assert!(matches!(
                    model.network_change_state,
                    NetworkChangeState::NewIpReachable { .. }
                ));
                if let NetworkChangeState::NewIpReachable { new_ip, ui_port } =
                    &model.network_change_state
                {
                    assert_eq!(new_ip, "192.168.1.101");
                    assert_eq!(*ui_port, 443);
                }
                assert!(model.overlay_spinner.is_visible());
            }

            #[test]
            fn failed_healthcheck_keeps_waiting() {
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

                let _ =
                    handle_healthcheck_response(Err("Connection failed".to_string()), &mut model);

                assert!(matches!(
                    model.network_change_state,
                    NetworkChangeState::WaitingForNewIp { .. }
                ));
            }
        }

        mod derived_state {
            use super::*;

            #[test]
            fn version_mismatch_sets_model_fields() {
                let mut model = Model::default();
                let info = HealthcheckInfo {
                    version_info: VersionInfo {
                        required: ">=1.0.0".to_string(),
                        current: "0.9.0".to_string(),
                        mismatch: true,
                    },
                    ..Default::default()
                };

                let _ = handle_healthcheck_response(Ok(info), &mut model);

                assert!(model.version_mismatch);
                let msg = model.version_mismatch_message.unwrap();
                assert!(msg.contains("0.9.0"));
                assert!(msg.contains(">=1.0.0"));
            }

            #[test]
            fn no_version_mismatch_clears_fields() {
                let mut model = Model {
                    version_mismatch: true,
                    version_mismatch_message: Some("old".to_string()),
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Ok(create_healthcheck("valid", false)), &mut model);

                assert!(!model.version_mismatch);
                assert!(model.version_mismatch_message.is_none());
            }

            #[test]
            fn error_response_does_not_change_derived_state() {
                let mut model = Model {
                    version_mismatch: true,
                    ..Default::default()
                };

                let _ =
                    handle_healthcheck_response(Err("Connection failed".to_string()), &mut model);

                // Derived state unchanged on error
                assert!(model.version_mismatch);
            }
        }
    }

    mod schedule_timers {
        use super::*;
        use crux_time::protocol::{Duration as TimeDuration, TimeRequest};

        fn find_time_effect(cmd: &mut Command<Effect, Event>) -> Option<TimeRequest> {
            cmd.effects().find_map(|e| {
                if let Effect::Time(_) = e {
                    let (time_request, _) = e.expect_time().split();
                    Some(time_request)
                } else {
                    None
                }
            })
        }

        #[test]
        fn schedule_reconnection_poll_emits_5000ms_time_effect() {
            let mut cmd = schedule_reconnection_poll();
            let time_request = find_time_effect(&mut cmd).expect("expected Time effect");
            let TimeRequest::NotifyAfter { duration, .. } = time_request else {
                panic!("expected NotifyAfter");
            };
            assert_eq!(
                duration,
                TimeDuration::from_millis(RECONNECTION_POLL_INTERVAL_MS)
            );
        }

        #[test]
        fn check_tick_reschedules_poll() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Rebooting,
                ..Default::default()
            };
            let mut cmd = handle_reconnection_check_tick(&mut model);
            let time_request = find_time_effect(&mut cmd).expect("expected Time effect");
            let TimeRequest::NotifyAfter { duration, .. } = time_request else {
                panic!("expected NotifyAfter");
            };
            assert_eq!(
                duration,
                TimeDuration::from_millis(RECONNECTION_POLL_INTERVAL_MS)
            );
        }

        #[test]
        fn countdown_tick_reschedules_when_active_and_nonzero() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Rebooting,
                overlay_spinner: OverlaySpinnerState::new("Test").with_countdown(30),
                ..Default::default()
            };
            let mut cmd = handle_reconnection_countdown_tick(&mut model);
            let time_request = find_time_effect(&mut cmd).expect("expected Time effect");
            let TimeRequest::NotifyAfter { duration, .. } = time_request else {
                panic!("expected NotifyAfter");
            };
            assert_eq!(duration, TimeDuration::from_millis(1_000));
        }

        #[test]
        fn countdown_tick_stops_when_countdown_is_zero() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Rebooting,
                overlay_spinner: OverlaySpinnerState::new("Test").with_countdown(0),
                ..Default::default()
            };
            let mut cmd = handle_reconnection_countdown_tick(&mut model);
            assert!(find_time_effect(&mut cmd).is_none());
        }

        #[test]
        fn countdown_tick_stops_when_idle() {
            let mut model = Model {
                device_operation_state: DeviceOperationState::Idle,
                overlay_spinner: OverlaySpinnerState::new("Test").with_countdown(30),
                ..Default::default()
            };
            let mut cmd = handle_reconnection_countdown_tick(&mut model);
            assert!(find_time_effect(&mut cmd).is_none());
        }
    }
}
