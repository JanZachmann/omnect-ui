mod auth;
mod device;
mod ui;
mod websocket;
mod wifi;

use crux_core::{Command, render::render};

use crate::{Effect, events::Event, model::Model};

/// Main update dispatcher - routes events to domain-specific handlers
pub fn update(event: Event, model: &mut Model) -> Command<Effect, Event> {
    // Log to browser console for debugging
    log::debug!("Crux Core update: {event:?}");

    match event {
        Event::Initialize => {
            // Init fans out to deliberately silent background fetches (LoadSettings,
            // CheckAvailability). It must NOT touch the global `is_loading` flag:
            // none of their completion handlers clear it, so setting it here leaves
            // it stuck true and spins loading-bound buttons (e.g. Settings "Save").
            Command::all([
                render(),
                wifi::handle(crate::events::WifiEvent::CheckAvailability, model),
                ui::handle(crate::events::UiEvent::LoadSettings, model),
            ])
        }
        Event::Auth(auth_event) => auth::handle(auth_event, model),
        Event::Device(device_event) => device::handle(device_event, model),
        Event::WebSocket(ws_event) => websocket::handle(ws_event, model),
        Event::Ui(ui_event) => ui::handle(ui_event, model),
        Event::Wifi(wifi_event) => wifi::handle(wifi_event, model),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events::{UiEvent, WifiEvent},
        types::{TimeoutSettings, WifiAvailability},
    };

    /// Initialization must not leave the global `is_loading` flag stuck on.
    ///
    /// Regression: the Settings "Save" button (bound to `is_loading`) showed a
    /// spinner permanently after load because the init fan-out set
    /// `is_loading=true` but no completion handler ever cleared it.
    /// Exercises the real repro path: authenticated + WiFi available.
    #[test]
    fn initialize_clears_loading_when_wifi_available_and_authenticated() {
        let mut model = Model {
            is_authenticated: true,
            auth_token: Some("test-token".into()),
            ..Default::default()
        };

        let _ = update(Event::Initialize, &mut model);
        // Init itself must never set the global loading flag — guards against
        // reintroducing start_loading() that a later response happens to clear.
        assert!(
            !model.is_loading,
            "Initialize must not set the global loading flag"
        );

        let _ = update(
            Event::Ui(UiEvent::LoadSettingsResponse(
                Ok(TimeoutSettings::default()),
            )),
            &mut model,
        );
        let _ = update(
            Event::Wifi(WifiEvent::CheckAvailabilityResponse(Ok(
                WifiAvailability::Available {
                    version: "1.0.0".into(),
                    interface_name: "wlan0".into(),
                },
            ))),
            &mut model,
        );

        assert!(
            !model.is_loading,
            "is_loading must be cleared after init settles"
        );
    }

    /// Isolates the Initialize + CheckAvailability loading sources on the
    /// WiFi-unavailable path (no GetStatus/GetSavedNetworks follow-up).
    #[test]
    fn initialize_clears_loading_when_wifi_unavailable() {
        let mut model = Model::default();

        let _ = update(Event::Initialize, &mut model);
        assert!(
            !model.is_loading,
            "Initialize must not set the global loading flag"
        );

        let _ = update(
            Event::Wifi(WifiEvent::CheckAvailabilityResponse(Ok(
                WifiAvailability::Unavailable {
                    socket_present: false,
                    version_info: None,
                },
            ))),
            &mut model,
        );

        assert!(
            !model.is_loading,
            "is_loading must be cleared after init settles"
        );
    }
}
