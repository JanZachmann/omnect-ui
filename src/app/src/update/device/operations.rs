use crux_core::Command;

use crate::{
    Effect, TimeCmd,
    events::{DeviceEvent, Event},
    model::Model,
    types::{DeviceOperationState, OverlaySpinnerState},
};

use super::reconnection::schedule_reconnection_poll;

const RECONNECTION_COUNTDOWN_INTERVAL_MS: u64 = 1000;

pub(super) fn schedule_reconnection_timeout(secs: u32) -> Command<Effect, Event> {
    let (timer, handle) = TimeCmd::notify_after(std::time::Duration::from_secs(u64::from(secs)));
    std::mem::forget(handle);
    timer.then_send(|_| Event::Device(DeviceEvent::ReconnectionTimeout))
}

pub(super) fn schedule_reconnection_countdown_tick() -> Command<Effect, Event> {
    let (timer, handle) = TimeCmd::notify_after(std::time::Duration::from_millis(
        RECONNECTION_COUNTDOWN_INTERVAL_MS,
    ));
    std::mem::forget(handle);
    timer.then_send(|_| Event::Device(DeviceEvent::ReconnectionCountdownTick))
}

/// Check if an error message indicates a network error
pub fn is_network_error(error: &str) -> bool {
    let e_lower = error.to_lowercase();
    e_lower.contains("failed to fetch")
        || e_lower.contains("networkerror")
        || error.contains("IO error")
}

/// Check if an update is complete based on healthcheck status
pub fn is_update_complete(info: &crate::types::HealthcheckInfo) -> bool {
    let status = &info.update_validation_status.status;
    status == "Succeeded" || status == "Recovered" || status == "NoUpdate"
}

/// Returns true when an actual firmware update completed (succeeded or rolled back).
/// Distinct from `is_update_complete`, which also matches `"NoUpdate"` for polling purposes.
pub fn is_actual_update_result(info: &crate::types::HealthcheckInfo) -> bool {
    let status = &info.update_validation_status.status;
    status == "Succeeded" || status == "Recovered"
}

/// Generic handler for device operation responses (reboot, factory reset, update)
/// Schedules reconnection poll, operation timeout, and countdown tick on success.
pub fn handle_device_operation_response(
    result: Result<(), String>,
    model: &mut Model,
    operation: DeviceOperationState,
    success_msg: &str,
    connection_lost_msg: &str,
    overlay_title: &str,
    overlay_text: Option<String>,
) -> Command<Effect, crate::Event> {
    model.stop_loading();

    let is_network_err = result.as_ref().is_err_and(|e| is_network_error(e));

    if result.is_ok() || is_network_err {
        model.success_message = Some(if is_network_err {
            connection_lost_msg.to_string()
        } else {
            success_msg.to_string()
        });
        let timeout_secs = match &operation {
            DeviceOperationState::FactoryResetting => {
                model.timeout_settings.factory_reset_timeout_secs
            }
            DeviceOperationState::Updating => model.timeout_settings.firmware_update_timeout_secs,
            _ => model.timeout_settings.reboot_timeout_secs,
        };
        model.device_operation_state = operation;
        model.reconnection_attempt = 0;
        model.device_went_offline = false;
        let mut spinner = OverlaySpinnerState::new(overlay_title).with_countdown(timeout_secs);
        if let Some(text) = overlay_text {
            spinner = spinner.with_text(text);
        }
        model.overlay_spinner = spinner;
        return Command::all([
            crux_core::render::render(),
            schedule_reconnection_poll(),
            schedule_reconnection_timeout(timeout_secs),
            schedule_reconnection_countdown_tick(),
        ]);
    } else if let Err(e) = result {
        model.set_error(e);
        model.overlay_spinner.clear();
    }

    crux_core::render::render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DEFAULT_FACTORY_RESET_TIMEOUT_SECS, DEFAULT_REBOOT_TIMEOUT_SECS};
    use crux_time::protocol::{Duration as TimeDuration, TimeRequest};

    fn collect_time_effects(cmd: &mut Command<Effect, Event>) -> Vec<TimeRequest> {
        cmd.effects()
            .filter_map(|e| {
                if let Effect::Time(_) = e {
                    let (time_request, _) = e.expect_time().split();
                    Some(time_request)
                } else {
                    None
                }
            })
            .collect()
    }

    fn has_notify_after(effects: &[TimeRequest], expected: TimeDuration) -> bool {
        effects.iter().any(
            |r| matches!(r, TimeRequest::NotifyAfter { duration, .. } if *duration == expected),
        )
    }

    #[test]
    fn schedule_reconnection_timeout_emits_time_effect_with_correct_duration() {
        let mut cmd = schedule_reconnection_timeout(90);
        let effects = collect_time_effects(&mut cmd);
        assert_eq!(effects.len(), 1);
        let TimeRequest::NotifyAfter { duration, .. } = effects[0] else {
            panic!("expected NotifyAfter");
        };
        assert_eq!(duration, TimeDuration::from_secs(90));
    }

    #[test]
    fn schedule_reconnection_countdown_tick_emits_1000ms_time_effect() {
        let mut cmd = schedule_reconnection_countdown_tick();
        let effects = collect_time_effects(&mut cmd);
        assert_eq!(effects.len(), 1);
        let TimeRequest::NotifyAfter { duration, .. } = effects[0] else {
            panic!("expected NotifyAfter");
        };
        assert_eq!(
            duration,
            TimeDuration::from_millis(RECONNECTION_COUNTDOWN_INTERVAL_MS)
        );
    }

    #[test]
    fn reboot_success_schedules_poll_timeout_and_countdown_timers() {
        let mut model = Model::default();
        let mut cmd = handle_device_operation_response(
            Ok(()),
            &mut model,
            DeviceOperationState::Rebooting,
            "Reboot initiated",
            "Reboot initiated (connection lost)",
            "Rebooting",
            None,
        );
        let time_effects = collect_time_effects(&mut cmd);
        // poll (5000ms) + timeout (reboot_timeout_secs) + countdown tick (1000ms)
        assert_eq!(time_effects.len(), 3);
        assert!(
            has_notify_after(&time_effects, TimeDuration::from_millis(5_000)),
            "poll timer missing"
        );
        assert!(
            has_notify_after(
                &time_effects,
                TimeDuration::from_secs(u64::from(DEFAULT_REBOOT_TIMEOUT_SECS))
            ),
            "timeout timer missing"
        );
        assert!(
            has_notify_after(&time_effects, TimeDuration::from_millis(1_000)),
            "countdown timer missing"
        );
    }

    #[test]
    fn factory_reset_success_schedules_correct_timeout() {
        let mut model = Model::default();
        let mut cmd = handle_device_operation_response(
            Ok(()),
            &mut model,
            DeviceOperationState::FactoryResetting,
            "Factory reset initiated",
            "Factory reset initiated (connection lost)",
            "Factory Resetting",
            None,
        );
        let time_effects = collect_time_effects(&mut cmd);
        assert!(
            has_notify_after(
                &time_effects,
                TimeDuration::from_secs(u64::from(DEFAULT_FACTORY_RESET_TIMEOUT_SECS))
            ),
            "factory reset timeout timer missing"
        );
    }
}
