use crux_core::Command;

use crate::{
    Effect, TimeCmd,
    events::{DeviceEvent, Event, UiEvent},
    http_get, http_get_silent,
    model::Model,
    types::{HealthcheckInfo, NetworkChangeState, OverlaySpinnerState},
    unauth_post,
};

/// New IP poll interval
const NEW_IP_POLL_INTERVAL_MS: u64 = 5000;

/// New IP countdown tick interval (1 second)
const NEW_IP_COUNTDOWN_INTERVAL_MS: u64 = 1000;

pub(super) fn schedule_new_ip_check_timeout(secs: u64) -> Command<Effect, Event> {
    let (timer, handle) = TimeCmd::notify_after(std::time::Duration::from_secs(secs));
    std::mem::forget(handle);
    timer.then_send(|_| Event::Device(DeviceEvent::NewIpCheckTimeout))
}

pub(super) fn schedule_new_ip_countdown_tick() -> Command<Effect, Event> {
    let (timer, handle) = TimeCmd::notify_after(std::time::Duration::from_millis(
        NEW_IP_COUNTDOWN_INTERVAL_MS,
    ));
    std::mem::forget(handle);
    timer.then_send(|_| Event::Device(DeviceEvent::NewIpCountdownTick))
}

/// Handle new IP countdown tick - decrements the displayed countdown each second
pub fn handle_new_ip_countdown_tick(model: &mut Model) -> Command<Effect, Event> {
    let is_active = matches!(
        model.network_change_state,
        NetworkChangeState::WaitingForNewIp { .. }
    );
    if is_active && model.overlay_spinner.countdown_seconds() > Some(0) {
        model.overlay_spinner.decrement_countdown();
        Command::all([
            crux_core::render::render(),
            schedule_new_ip_countdown_tick(),
        ])
    } else {
        Command::done()
    }
}

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

    let timeout_u32 = u32::try_from(rollback_timeout_seconds).unwrap_or(u32::MAX);
    model.overlay_spinner = if rollback_enabled {
        spinner.with_countdown(timeout_u32)
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
            if *switching_to_dhcp {
                crux_core::render::render()
            } else {
                // Try to reach the new IP, then reschedule
                let url = format!("https://{new_ip}:{ui_port}/healthcheck");
                Command::all([
                    http_get_silent!(
                        url,
                        on_success: Event::Device(DeviceEvent::HealthcheckResponse(Ok(
                            HealthcheckInfo::default()
                        ))),
                        on_error: Event::Ui(UiEvent::ClearSuccess)
                    ),
                    schedule_new_ip_poll(),
                ])
            }
        }
        NetworkChangeState::WaitingForOldIp {
            old_ip,
            ui_port,
            attempt,
        } => {
            *attempt += 1;
            // Poll the old IP to see if rollback completed, then reschedule
            let url = format!("https://{old_ip}:{ui_port}/healthcheck");
            Command::all([
                http_get!(
                    Device,
                    DeviceEvent,
                    &url,
                    HealthcheckResponse,
                    crate::types::HealthcheckInfo
                ),
                schedule_new_ip_poll(),
            ])
        }
        _ => crux_core::render::render(),
    }
}

/// Handle new IP check timeout - new IP didn't become reachable in time
pub fn handle_new_ip_check_timeout(model: &mut Model) -> Command<Effect, Event> {
    let mut start_old_ip_poll = false;

    if let NetworkChangeState::WaitingForNewIp {
        new_ip,
        old_ip,
        ui_port,
        rollback_timeout_seconds,
        switching_to_dhcp,
        ..
    } = &model.network_change_state
    {
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
            start_old_ip_poll = true;
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

    if start_old_ip_poll {
        Command::all([crux_core::render::render(), schedule_new_ip_poll()])
    } else {
        crux_core::render::render()
    }
}

pub(super) fn schedule_new_ip_poll() -> Command<Effect, Event> {
    let (timer, handle) =
        TimeCmd::notify_after(std::time::Duration::from_millis(NEW_IP_POLL_INTERVAL_MS));
    std::mem::forget(handle);
    timer.then_send(|_| Event::Device(DeviceEvent::NewIpCheckTick))
}

/// Handle acknowledge factory reset result - clear the result
pub fn handle_ack_factory_reset_result(model: &mut Model) -> Command<Effect, Event> {
    if let Some(factory_reset) = &mut model.factory_reset {
        factory_reset.result = None;
    }

    unauth_post!(
        Device,
        DeviceEvent,
        model,
        "/ack-factory-reset-result",
        AckFactoryResetResultResponse,
        "Acknowledge factory reset result"
    )
}

/// Handle acknowledge update validation - clear the status
pub fn handle_ack_update_validation(model: &mut Model) -> Command<Effect, Event> {
    model.update_validation_status = None;

    unauth_post!(
        Device,
        DeviceEvent,
        model,
        "/ack-update-validation",
        AckUpdateValidationResponse,
        "Acknowledge update validation"
    )
}

/// Handle acknowledge network rollback - clear the rollback occurred flag
pub fn handle_ack_rollback(model: &mut Model) -> Command<Effect, Event> {
    // Clear the rollback status in the model
    if let Some(healthcheck) = &mut model.healthcheck {
        healthcheck.network_rollback_occurred = false;
    }

    // Send POST request to backend to clear the marker file
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
                ..Default::default()
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
                    network_rollback_occurred: true,
                    ..Default::default()
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

    mod factory_reset_result_acknowledgment {
        use super::*;
        use crate::types::{FactoryReset, FactoryResetResult, FactoryResetStatus};

        #[test]
        fn clears_factory_reset_result_in_model() {
            let mut model = Model {
                factory_reset: Some(FactoryReset {
                    keys: vec!["key1".to_string()],
                    result: Some(FactoryResetResult {
                        status: FactoryResetStatus::ModeSupported,
                        context: None,
                        error: String::new(),
                        paths: vec![],
                    }),
                }),
                ..Default::default()
            };

            let _ = handle_ack_factory_reset_result(&mut model);

            assert!(model.factory_reset.as_ref().unwrap().result.is_none());
        }

        #[test]
        fn handles_missing_factory_reset_gracefully() {
            let mut model = Model {
                factory_reset: None,
                ..Default::default()
            };

            let _ = handle_ack_factory_reset_result(&mut model);

            assert!(model.factory_reset.is_none());
        }

        #[test]
        fn response_ok_stops_loading() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = crate::update::device::handle(
                DeviceEvent::AckFactoryResetResultResponse(Ok(())),
                &mut model,
            );

            assert!(!model.is_loading);
        }

        #[test]
        fn response_error_sets_error_message() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = crate::update::device::handle(
                DeviceEvent::AckFactoryResetResultResponse(Err("Failed".to_string())),
                &mut model,
            );

            assert!(!model.is_loading);
            assert!(model.error_message.is_some());
        }
    }

    mod update_validation_acknowledgment {
        use super::*;

        #[test]
        fn clears_update_validation_status_in_model() {
            let mut model = Model {
                update_validation_status: Some(UpdateValidationStatus {
                    status: "Succeeded".to_string(),
                }),
                ..Default::default()
            };

            let _ = handle_ack_update_validation(&mut model);

            assert!(model.update_validation_status.is_none());
        }

        #[test]
        fn response_ok_stops_loading() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = crate::update::device::handle(
                DeviceEvent::AckUpdateValidationResponse(Ok(())),
                &mut model,
            );

            assert!(!model.is_loading);
        }

        #[test]
        fn response_error_sets_error_message() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = crate::update::device::handle(
                DeviceEvent::AckUpdateValidationResponse(Err("Failed".to_string())),
                &mut model,
            );

            assert!(!model.is_loading);
            assert!(model.error_message.is_some());
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
        fn schedule_new_ip_check_timeout_emits_time_effect_with_correct_duration() {
            let mut cmd = schedule_new_ip_check_timeout(120);
            let time_request = find_time_effect(&mut cmd).expect("expected Time effect");
            let TimeRequest::NotifyAfter { duration, .. } = time_request else {
                panic!("expected NotifyAfter");
            };
            assert_eq!(duration, TimeDuration::from_secs(120));
        }

        #[test]
        fn schedule_new_ip_countdown_tick_emits_1000ms_time_effect() {
            let mut cmd = schedule_new_ip_countdown_tick();
            let time_request = find_time_effect(&mut cmd).expect("expected Time effect");
            let TimeRequest::NotifyAfter { duration, .. } = time_request else {
                panic!("expected NotifyAfter");
            };
            assert_eq!(
                duration,
                TimeDuration::from_millis(NEW_IP_COUNTDOWN_INTERVAL_MS)
            );
        }

        #[test]
        fn schedule_new_ip_poll_emits_5000ms_time_effect() {
            let mut cmd = schedule_new_ip_poll();
            let time_request = find_time_effect(&mut cmd).expect("expected Time effect");
            let TimeRequest::NotifyAfter { duration, .. } = time_request else {
                panic!("expected NotifyAfter");
            };
            assert_eq!(duration, TimeDuration::from_millis(NEW_IP_POLL_INTERVAL_MS));
        }

        #[test]
        fn new_ip_countdown_tick_reschedules_when_waiting_and_nonzero() {
            let mut model = Model {
                network_change_state: NetworkChangeState::WaitingForNewIp {
                    new_ip: "192.168.1.101".to_string(),
                    old_ip: "192.168.1.100".to_string(),
                    attempt: 0,
                    rollback_timeout_seconds: 60,
                    ui_port: 443,
                    switching_to_dhcp: false,
                },
                overlay_spinner: OverlaySpinnerState::new("Test").with_countdown(30),
                ..Default::default()
            };
            let mut cmd = handle_new_ip_countdown_tick(&mut model);
            let time_request = find_time_effect(&mut cmd).expect("expected Time effect");
            let TimeRequest::NotifyAfter { duration, .. } = time_request else {
                panic!("expected NotifyAfter");
            };
            assert_eq!(
                duration,
                TimeDuration::from_millis(NEW_IP_COUNTDOWN_INTERVAL_MS)
            );
        }

        #[test]
        fn new_ip_countdown_tick_stops_when_idle() {
            let mut model = Model {
                network_change_state: NetworkChangeState::Idle,
                overlay_spinner: OverlaySpinnerState::new("Test").with_countdown(30),
                ..Default::default()
            };
            let mut cmd = handle_new_ip_countdown_tick(&mut model);
            assert!(find_time_effect(&mut cmd).is_none());
        }
    }
}
