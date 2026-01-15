mod network;
mod operations;
mod reconnection;

pub use network::{
    handle_ack_rollback, handle_network_form_start_edit, handle_network_form_update,
    handle_new_ip_check_tick, handle_new_ip_check_timeout, handle_set_network_config,
    handle_set_network_config_response,
};
pub use operations::handle_device_operation_response;
pub use reconnection::{
    handle_healthcheck_response, handle_reconnection_check_tick, handle_reconnection_timeout,
};

use crux_core::Command;

use crate::auth_post;
use crate::events::{DeviceEvent, Event};
use crate::handle_response;
use crate::model::Model;
use crate::types::{
    DeviceOperationState, FactoryResetRequest, LoadUpdateRequest, OverlaySpinnerState,
    RunUpdateRequest, UpdateManifest, UploadState,
};
use crate::Effect;

/// Handle device action events (reboot, factory reset, network, updates)
pub fn handle(event: DeviceEvent, model: &mut Model) -> Command<Effect, Event> {
    match event {
        DeviceEvent::UploadStarted => {
            model.firmware_upload_state = UploadState::Uploading;
            model.overlay_spinner = OverlaySpinnerState::new("Uploading firmware...")
                .with_text("Please do not close this window.")
                .with_progress(0);
            crux_core::render::render()
        }

        DeviceEvent::UploadProgress(percentage) => {
            model.firmware_upload_state = UploadState::Uploading;
            model.overlay_spinner.set_progress(percentage);
            crux_core::render::render()
        }

        DeviceEvent::UploadCompleted(_) => {
            model.firmware_upload_state = UploadState::Completed;
            model.success_message = Some("Upload successful".to_string());
            model.overlay_spinner.clear();
            crux_core::render::render()
        }

        DeviceEvent::UploadFailed(error) => {
            model.firmware_upload_state = UploadState::Failed(error.clone());
            model.overlay_spinner.clear();
            model.set_error_and_render(format!("Upload failed: {error}"))
        }

        DeviceEvent::Reboot => {
            model.overlay_spinner = OverlaySpinnerState::new("Requesting device reboot...");
            auth_post!(
                Device,
                DeviceEvent,
                model,
                "/reboot",
                RebootResponse,
                "Reboot"
            )
        }

        DeviceEvent::RebootResponse(result) => handle_device_operation_response(
            result,
            model,
            DeviceOperationState::Rebooting,
            "Reboot initiated",
            "Reboot initiated (connection lost)",
            "Device is rebooting",
            None,
        ),

        DeviceEvent::FactoryResetRequest { mode, preserve } => {
            let parsed_mode = match mode.parse::<u8>() {
                Ok(m) => m,
                Err(e) => {
                    model.set_error(format!("Invalid factory reset mode: {e}"));
                    return crux_core::render::render();
                }
            };
            let request = FactoryResetRequest {
                mode: parsed_mode,
                preserve,
            };
            model.overlay_spinner = OverlaySpinnerState::new("Requesting factory reset...");
            auth_post!(Device, DeviceEvent, model, "/factory-reset", FactoryResetResponse, "Factory reset",
                body_json: &request
            )
        }

        DeviceEvent::FactoryResetResponse(result) => handle_device_operation_response(
            result,
            model,
            DeviceOperationState::FactoryResetting,
            "Factory reset initiated",
            "Factory reset initiated (connection lost)",
            "The device is resetting",
            Some(
                "Please wait while the device resets. The app will be temporarily \
                 removed and reinstalled automatically when the device is back online."
                    .to_string(),
            ),
        ),

        DeviceEvent::SetNetworkConfig { config } => handle_set_network_config(config, model),

        DeviceEvent::SetNetworkConfigResponse(result) => {
            handle_set_network_config_response(result, model)
        }

        DeviceEvent::AckRollbackResponse(result) => {
            model.stop_loading();
            if let Err(e) = result {
                model.set_error(e);
            }
            crux_core::render::render()
        }

        DeviceEvent::LoadUpdate { file_path } => {
            let request = LoadUpdateRequest { file_path };
            auth_post!(Device, DeviceEvent, model, "/update/load", LoadUpdateResponse, "Load update",
                body_json: &request,
                expect_json: UpdateManifest
            )
        }

        DeviceEvent::LoadUpdateResponse(result) => handle_response!(model, result, {
            on_success: |model, manifest| {
                model.update_manifest = Some(manifest);
            },
            success_message: "Update loaded",
        }),

        DeviceEvent::RunUpdate {
            validate_iothub_connection,
        } => {
            let request = RunUpdateRequest {
                validate_iothub_connection,
            };
            model.overlay_spinner = OverlaySpinnerState::new("Requesting update...");
            auth_post!(Device, DeviceEvent, model, "/update/run", RunUpdateResponse, "Run update",
                body_json: &request
            )
        }

        DeviceEvent::RunUpdateResponse(result) => handle_device_operation_response(
            result,
            model,
            DeviceOperationState::Updating,
            "Update started",
            "Update started (connection lost)",
            "Installing update",
            Some("Please have some patience, the update may take some time.".to_string()),
        ),

        DeviceEvent::HealthcheckResponse(result) => handle_healthcheck_response(result, model),

        // Device reconnection events (reboot/factory reset/update)
        // Shell sends these tick events based on watching device_operation_state
        DeviceEvent::ReconnectionCheckTick => handle_reconnection_check_tick(model),
        DeviceEvent::ReconnectionTimeout => handle_reconnection_timeout(model),

        // Network IP change events
        // Shell sends these tick events based on watching network_change_state
        DeviceEvent::NewIpCheckTick => handle_new_ip_check_tick(model),
        DeviceEvent::NewIpCheckTimeout => handle_new_ip_check_timeout(model),

        // Acknowledge network rollback
        DeviceEvent::AckRollback => handle_ack_rollback(model),

        // Network form events
        DeviceEvent::NetworkFormStartEdit { adapter_name } => {
            handle_network_form_start_edit(adapter_name, model)
        }
        DeviceEvent::NetworkFormUpdate { form_data } => {
            handle_network_form_update(form_data, model)
        }
        DeviceEvent::NetworkFormReset { adapter_name } => {
            handle_network_form_start_edit(adapter_name, model)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{DeviceEvent, Event};
    use crate::types::{DeviceOperationState, UploadState};
    use crate::{App, UpdateManifest};
    use crux_core::testing::AppTester;

    mod reboot {
        use super::*;

        #[test]
        fn success_sets_rebooting_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::RebootResponse(Ok(()))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(
                model.device_operation_state,
                DeviceOperationState::Rebooting
            );
            assert_eq!(model.success_message, Some("Reboot initiated".into()));
            assert!(model.overlay_spinner.is_visible());
        }

        #[test]
        fn network_error_sets_rebooting_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::RebootResponse(Err("Failed to fetch".into()))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(
                model.device_operation_state,
                DeviceOperationState::Rebooting
            );
            assert_eq!(
                model.success_message,
                Some("Reboot initiated (connection lost)".into())
            );
        }

        #[test]
        fn non_network_error_sets_error() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::RebootResponse(Err("Permission denied".into()))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(model.device_operation_state, DeviceOperationState::Idle);
            assert_eq!(model.error_message, Some("Permission denied".into()));
            assert!(!model.overlay_spinner.is_visible());
        }
    }

    mod factory_reset {
        use super::*;

        #[test]
        fn invalid_mode_sets_error() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let _ = app.update(
                Event::Device(DeviceEvent::FactoryResetRequest {
                    mode: "invalid".into(),
                    preserve: vec![],
                }),
                &mut model,
            );

            assert!(!model.is_loading);
            assert!(model.error_message.is_some());
            assert!(model
                .error_message
                .as_ref()
                .unwrap()
                .contains("Invalid factory reset mode"));
        }

        #[test]
        fn success_sets_factory_resetting_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::FactoryResetResponse(Ok(()))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(
                model.device_operation_state,
                DeviceOperationState::FactoryResetting
            );
            assert_eq!(
                model.success_message,
                Some("Factory reset initiated".into())
            );
            assert!(model.overlay_spinner.is_visible());
        }

        #[test]
        fn network_error_sets_factory_resetting_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::FactoryResetResponse(Err(
                    "NetworkError when attempting to fetch".into(),
                ))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(
                model.device_operation_state,
                DeviceOperationState::FactoryResetting
            );
            assert_eq!(
                model.success_message,
                Some("Factory reset initiated (connection lost)".into())
            );
        }
    }

    mod firmware_upload {
        use super::*;

        #[test]
        fn upload_started_sets_state() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let _ = app.update(Event::Device(DeviceEvent::UploadStarted), &mut model);

            assert_eq!(model.firmware_upload_state, UploadState::Uploading);
            assert!(model.overlay_spinner.is_visible());
        }

        #[test]
        fn upload_progress_updates_spinner() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                firmware_upload_state: UploadState::Uploading,
                overlay_spinner: OverlaySpinnerState::new("Uploading...").with_progress(0),
                ..Default::default()
            };

            let _ = app.update(Event::Device(DeviceEvent::UploadProgress(50)), &mut model);

            assert_eq!(model.firmware_upload_state, UploadState::Uploading);
        }

        #[test]
        fn upload_completed_sets_success() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                firmware_upload_state: UploadState::Uploading,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::UploadCompleted("/tmp/file.swu".into())),
                &mut model,
            );

            assert_eq!(model.firmware_upload_state, UploadState::Completed);
            assert_eq!(model.success_message, Some("Upload successful".into()));
            assert!(!model.overlay_spinner.is_visible());
        }

        #[test]
        fn upload_failed_sets_error() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                firmware_upload_state: UploadState::Uploading,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::UploadFailed("Network timeout".into())),
                &mut model,
            );

            assert!(matches!(
                model.firmware_upload_state,
                UploadState::Failed(_)
            ));
            assert!(model.error_message.is_some());
            assert!(model
                .error_message
                .as_ref()
                .unwrap()
                .contains("Upload failed"));
            assert!(!model.overlay_spinner.is_visible());
        }
    }

    mod load_update {
        use super::*;

        #[test]
        fn success_stores_manifest() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let manifest = UpdateManifest {
                update_id: crate::types::UpdateId {
                    provider: "test".into(),
                    name: "test-update".into(),
                    version: "1.0.0".into(),
                },
                is_deployable: true,
                compatibility: vec![],
                created_date_time: "2024-01-01".into(),
                manifest_version: "1".into(),
            };

            let _ = app.update(
                Event::Device(DeviceEvent::LoadUpdateResponse(Ok(manifest.clone()))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(model.update_manifest, Some(manifest));
            assert_eq!(model.success_message, Some("Update loaded".into()));
        }

        #[test]
        fn failure_sets_error() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::LoadUpdateResponse(
                    Err("File not found".into()),
                )),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(model.error_message, Some("File not found".into()));
            assert!(model.update_manifest.is_none());
        }
    }

    mod run_update {
        use super::*;

        #[test]
        fn success_sets_updating_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::RunUpdateResponse(Ok(()))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(model.device_operation_state, DeviceOperationState::Updating);
            assert_eq!(model.success_message, Some("Update started".into()));
            assert!(model.overlay_spinner.is_visible());
        }

        #[test]
        fn network_error_sets_updating_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Device(DeviceEvent::RunUpdateResponse(Err("IO error".into()))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(model.device_operation_state, DeviceOperationState::Updating);
            assert_eq!(
                model.success_message,
                Some("Update started (connection lost)".into())
            );
        }
    }
}
