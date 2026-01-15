use crux_core::Command;

use crate::events::{Event, WebSocketEvent};
use crate::model::Model;
use crate::update_field;
use crate::{CentrifugoCmd, Effect};

/// Handle WebSocket and Centrifugo-related events
pub fn handle(event: WebSocketEvent, model: &mut Model) -> Command<Effect, Event> {
    match event {
        WebSocketEvent::SubscribeToChannels => {
            // Issue Centrifugo effect (shell sends WebSocket data as events directly)
            CentrifugoCmd::subscribe_all()
                .build()
                .then_send(|_| Event::WebSocket(WebSocketEvent::Connected))
        }

        WebSocketEvent::UnsubscribeFromChannels => {
            // Issue Centrifugo effect
            CentrifugoCmd::unsubscribe_all()
                .build()
                .then_send(|_| Event::WebSocket(WebSocketEvent::Disconnected))
        }

        WebSocketEvent::SystemInfoUpdated(info) => update_field!(model.system_info, Some(info)),
        WebSocketEvent::NetworkStatusUpdated(status) => {
            update_field!(model.network_status, Some(status))
        }
        WebSocketEvent::OnlineStatusUpdated(status) => {
            update_field!(model.online_status, Some(status))
        }
        WebSocketEvent::FactoryResetUpdated(reset) => {
            update_field!(model.factory_reset, Some(reset))
        }
        WebSocketEvent::UpdateValidationStatusUpdated(status) => {
            update_field!(model.update_validation_status, Some(status))
        }
        WebSocketEvent::TimeoutsUpdated(timeouts) => update_field!(model.timeouts, Some(timeouts)),
        WebSocketEvent::Connected => update_field!(model.is_connected, true),
        WebSocketEvent::Disconnected => update_field!(model.is_connected, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FactoryReset, OsInfo, SystemInfo, Timeouts, UpdateValidationStatus};
    use crate::{App, OnlineStatus};
    use crux_core::testing::AppTester;

    mod system_info {
        use super::*;

        #[test]
        fn updates_system_info() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let info = SystemInfo {
                os: OsInfo {
                    name: "Linux".into(),
                    version: "5.10".into(),
                },
                azure_sdk_version: "1.0".into(),
                omnect_device_service_version: "2.0".into(),
                boot_time: Some("2024-01-01".into()),
            };

            let _ = app.update(
                Event::WebSocket(WebSocketEvent::SystemInfoUpdated(info.clone())),
                &mut model,
            );

            assert_eq!(model.system_info, Some(info));
        }

        #[test]
        fn replaces_previous_system_info() {
            let app = AppTester::<App>::default();
            let old_info = SystemInfo {
                os: OsInfo {
                    name: "Linux".into(),
                    version: "5.9".into(),
                },
                azure_sdk_version: "0.9".into(),
                omnect_device_service_version: "1.9".into(),
                boot_time: None,
            };
            let mut model = Model {
                system_info: Some(old_info),
                ..Default::default()
            };

            let new_info = SystemInfo {
                os: OsInfo {
                    name: "Linux".into(),
                    version: "5.10".into(),
                },
                azure_sdk_version: "1.0".into(),
                omnect_device_service_version: "2.0".into(),
                boot_time: Some("2024-01-01".into()),
            };

            let _ = app.update(
                Event::WebSocket(WebSocketEvent::SystemInfoUpdated(new_info.clone())),
                &mut model,
            );

            assert_eq!(model.system_info, Some(new_info));
        }
    }

    mod online_status {
        use super::*;

        #[test]
        fn updates_online_status_to_online() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let _ = app.update(
                Event::WebSocket(WebSocketEvent::OnlineStatusUpdated(OnlineStatus {
                    iothub: true,
                })),
                &mut model,
            );

            assert_eq!(model.online_status, Some(OnlineStatus { iothub: true }));
        }

        #[test]
        fn updates_online_status_to_offline() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                online_status: Some(OnlineStatus { iothub: true }),
                ..Default::default()
            };

            let _ = app.update(
                Event::WebSocket(WebSocketEvent::OnlineStatusUpdated(OnlineStatus {
                    iothub: false,
                })),
                &mut model,
            );

            assert_eq!(model.online_status, Some(OnlineStatus { iothub: false }));
        }

        #[test]
        fn transitions_from_offline_to_online() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                online_status: Some(OnlineStatus { iothub: false }),
                ..Default::default()
            };

            let _ = app.update(
                Event::WebSocket(WebSocketEvent::OnlineStatusUpdated(OnlineStatus {
                    iothub: true,
                })),
                &mut model,
            );

            assert_eq!(model.online_status, Some(OnlineStatus { iothub: true }));
        }
    }

    mod factory_reset {
        use super::*;

        #[test]
        fn updates_factory_reset_status() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let status = FactoryReset {
                keys: vec!["test_key".into()],
                result: None,
            };

            let _ = app.update(
                Event::WebSocket(WebSocketEvent::FactoryResetUpdated(status.clone())),
                &mut model,
            );

            assert_eq!(model.factory_reset, Some(status));
        }
    }

    mod update_validation {
        use super::*;

        #[test]
        fn updates_validation_status() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let status = UpdateValidationStatus {
                status: "Succeeded".into(),
            };

            let _ = app.update(
                Event::WebSocket(WebSocketEvent::UpdateValidationStatusUpdated(
                    status.clone(),
                )),
                &mut model,
            );

            assert_eq!(model.update_validation_status, Some(status));
        }
    }

    mod timeouts {
        use super::*;
        use crate::types::Duration;

        #[test]
        fn updates_timeouts() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let timeouts = Timeouts {
                wait_online_timeout: Duration {
                    nanos: 0,
                    secs: 300,
                },
            };

            let _ = app.update(
                Event::WebSocket(WebSocketEvent::TimeoutsUpdated(timeouts.clone())),
                &mut model,
            );

            assert_eq!(model.timeouts, Some(timeouts));
        }
    }

    mod connection {
        use super::*;

        #[test]
        fn connected_sets_is_connected() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let _ = app.update(Event::WebSocket(WebSocketEvent::Connected), &mut model);

            assert!(model.is_connected);
        }

        #[test]
        fn disconnected_clears_is_connected() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_connected: true,
                ..Default::default()
            };

            let _ = app.update(Event::WebSocket(WebSocketEvent::Disconnected), &mut model);

            assert!(!model.is_connected);
        }
    }
}
