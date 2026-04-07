use std::collections::HashMap;

use crux_core::Command;

use crate::{
    Effect, WebSocketCmd,
    events::{Event, WebSocketEvent},
    model::Model,
    parse_ods_update,
    types::{
        NetworkFormData, NetworkFormState,
        ods::{
            OdsFactoryReset, OdsNetworkStatus, OdsOnlineStatus, OdsSystemInfo, OdsTimeouts,
            OdsUpdateValidationStatus,
        },
    },
    update_field,
};

/// Handle WebSocket and WebSocket-related events
pub fn handle(event: WebSocketEvent, model: &mut Model) -> Command<Effect, Event> {
    match event {
        WebSocketEvent::SubscribeToChannels => {
            // Issue WebSocket effect (shell sends WebSocket data as events directly)
            WebSocketCmd::subscribe_all()
                .build()
                .then_send(|_| Event::WebSocket(WebSocketEvent::Connected))
        }

        WebSocketEvent::UnsubscribeFromChannels => {
            // Issue WebSocket effect
            WebSocketCmd::unsubscribe_all()
                .build()
                .then_send(|_| Event::WebSocket(WebSocketEvent::Disconnected))
        }

        WebSocketEvent::SystemInfoUpdated(json) => {
            parse_ods_update!(model, json, OdsSystemInfo, system_info, "SystemInfo")
        }
        WebSocketEvent::NetworkStatusUpdated(json) => {
            parse_ods_update!(
                model,
                json,
                OdsNetworkStatus,
                "NetworkStatus",
                |m, status| {
                    m.network_status = Some(status.into());
                    m.update_current_connection_adapter();
                    sync_network_form_from_status(m);
                    crux_core::render::render()
                }
            )
        }
        WebSocketEvent::OnlineStatusUpdated(json) => {
            parse_ods_update!(model, json, OdsOnlineStatus, online_status, "OnlineStatus")
        }
        WebSocketEvent::FactoryResetUpdated(json) => {
            parse_ods_update!(model, json, OdsFactoryReset, factory_reset, "FactoryReset")
        }
        WebSocketEvent::UpdateValidationStatusUpdated(json) => {
            parse_ods_update!(
                model,
                json,
                OdsUpdateValidationStatus,
                update_validation_status,
                "UpdateValidationStatus"
            )
        }
        WebSocketEvent::TimeoutsUpdated(json) => {
            parse_ods_update!(model, json, OdsTimeouts, timeouts, "Timeouts")
        }

        WebSocketEvent::Connected => update_field!(model.is_connected, true),
        WebSocketEvent::Disconnected => update_field!(model.is_connected, false),
    }
}

/// When the form has no unsaved changes, re-initialize it from freshly updated adapter data.
/// This keeps the form in sync when external events (e.g. `WiFi` connecting) change the
/// adapter's IP/config at the OS level.
fn sync_network_form_from_status(m: &mut Model) {
    let maybe_adapter_name = if m.network_form_dirty {
        None
    } else if let NetworkFormState::Editing { adapter_name, .. } = &m.network_form_state {
        Some(adapter_name.clone())
    } else {
        None
    };
    if let Some(adapter_name) = maybe_adapter_name
        && let Some(form_data) = m
            .network_status
            .as_ref()
            .and_then(|ns| ns.network_status.iter().find(|n| n.name == adapter_name))
            .map(NetworkFormData::from)
    {
        m.network_form_state = NetworkFormState::Editing {
            adapter_name,
            form_data: form_data.clone(),
            original_data: form_data,
            errors: HashMap::new(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        WebSocketOperation,
        types::{
            FactoryReset, FactoryResetStatus, OnlineStatus, OsInfo, SystemInfo,
            UpdateValidationStatus,
        },
    };

    mod subscribe {
        use super::*;

        #[test]
        fn subscribe_to_channels_emits_subscribe_all_effect() {
            let mut model = Model::default();
            let mut cmd = handle(WebSocketEvent::SubscribeToChannels, &mut model);

            // SubscribeToChannels produces a single WebSocket effect (no render wrapper)
            let (operation, _) = cmd.expect_one_effect().expect_web_socket().split();
            assert!(matches!(operation, WebSocketOperation::SubscribeAll));
        }

        #[test]
        fn unsubscribe_from_channels_emits_unsubscribe_all_effect() {
            let mut model = Model::default();
            let mut cmd = handle(WebSocketEvent::UnsubscribeFromChannels, &mut model);

            let (operation, _) = cmd.expect_one_effect().expect_web_socket().split();
            assert!(matches!(operation, WebSocketOperation::UnsubscribeAll));
        }
    }

    mod system_info {
        use super::*;

        #[test]
        fn updates_system_info() {
            let mut model = Model::default();

            let json = r#"{"os": {"name": "Linux", "version": "5.10"}, "azure_sdk_version": "1.0", "omnect_device_service_version": "2.0", "boot_time": "2024-01-01T00:00:00Z", "hostname": "omnect-device"}"#;

            let expected_info = SystemInfo {
                os: OsInfo {
                    name: "Linux".into(),
                    version: "5.10".into(),
                },
                azure_sdk_version: "1.0".into(),
                omnect_device_service_version: "2.0".into(),
                boot_time: Some("2024-01-01T00:00:00Z".into()),
                hostname: "omnect-device".into(),
            };

            let _ = handle(WebSocketEvent::SystemInfoUpdated(json.into()), &mut model);

            assert_eq!(model.system_info, Some(expected_info));
        }
    }

    mod online_status {
        use super::*;

        #[test]
        fn updates_online_status_to_online() {
            let mut model = Model::default();

            let _ = handle(
                WebSocketEvent::OnlineStatusUpdated(r#"{"iothub": true}"#.into()),
                &mut model,
            );

            assert_eq!(model.online_status, Some(OnlineStatus { iothub: true }));
        }
    }

    mod factory_reset {
        use super::*;

        #[test]
        fn updates_factory_reset_status() {
            let mut model = Model::default();

            let json = r#"{"keys": ["test_key"], "result": null}"#;

            let expected_status = FactoryReset {
                keys: vec!["test_key".into()],
                result: None,
            };

            let _ = handle(WebSocketEvent::FactoryResetUpdated(json.into()), &mut model);

            assert_eq!(model.factory_reset, Some(expected_status));
        }

        #[test]
        fn parses_integer_status_from_ods() {
            let mut model = Model::default();

            // ODS sends status as integer (serde_repr): 0=ModeSupported, 1=ModeUnsupported, etc.
            let json = r#"{"keys":["network"],"result":{"status":0,"error":"0","paths":["/etc/systemd/network/"]}}"#;

            let _ = handle(WebSocketEvent::FactoryResetUpdated(json.into()), &mut model);

            let factory_reset = model.factory_reset.expect("factory_reset should be set");
            let result = factory_reset.result.expect("result should be set");
            assert_eq!(result.status, FactoryResetStatus::ModeSupported);
            assert_eq!(result.error, "0");
            assert_eq!(result.paths, vec!["/etc/systemd/network/"]);
        }
    }

    mod update_validation {
        use super::*;

        #[test]
        fn updates_validation_status() {
            let mut model = Model::default();

            let json = r#"{"status": "Succeeded"}"#;

            let expected_status = UpdateValidationStatus {
                status: "Succeeded".into(),
            };

            let _ = handle(
                WebSocketEvent::UpdateValidationStatusUpdated(json.into()),
                &mut model,
            );

            assert_eq!(model.update_validation_status, Some(expected_status));
        }
    }

    mod timeouts {
        use super::*;
        use crate::types::{TimeoutDuration, Timeouts};

        #[test]
        fn updates_timeouts() {
            let mut model = Model::default();

            let json = r#"{"wait_online_timeout": {"nanos": 0, "secs": 300}}"#;

            let expected_timeouts = Timeouts {
                wait_online_timeout: TimeoutDuration {
                    nanos: 0,
                    secs: 300,
                },
            };

            let _ = handle(WebSocketEvent::TimeoutsUpdated(json.into()), &mut model);

            assert_eq!(model.timeouts, Some(expected_timeouts));
        }
    }

    mod network_status {
        use super::*;
        use crate::types::{DeviceNetwork, InternetProtocol, IpAddress, NetworkStatus};

        #[test]
        fn updates_network_status() {
            let mut model = Model::default();

            let json = r#"{
                "network_status": [{
                    "name": "eth0",
                    "mac": "00:11:22:33:44:55",
                    "online": true,
                    "file": "/etc/network/interfaces",
                    "ipv4": {
                        "addrs": [{
                            "addr": "192.168.1.100",
                            "dhcp": false,
                            "prefix_len": 24
                        }],
                        "dns": [],
                        "gateways": []
                    }
                }]
            }"#;

            let expected_status = NetworkStatus {
                network_status: vec![DeviceNetwork {
                    name: "eth0".to_string(),
                    mac: "00:11:22:33:44:55".to_string(),
                    online: true,
                    file: Some("/etc/network/interfaces".to_string()),
                    ipv4: InternetProtocol {
                        addrs: vec![IpAddress {
                            addr: "192.168.1.100".to_string(),
                            dhcp: false,
                            prefix_len: 24,
                        }],
                        dns: vec![],
                        gateways: vec![],
                    },
                }],
            };

            let _ = handle(
                WebSocketEvent::NetworkStatusUpdated(json.into()),
                &mut model,
            );

            assert_eq!(model.network_status, Some(expected_status));
        }
    }
}
