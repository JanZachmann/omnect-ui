pub mod capabilities;
pub mod events;
pub mod http_helpers;
pub mod macros;
pub mod model;
pub mod types;
pub mod update;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

use crux_core::Command;

// Using deprecated Capabilities API for Http (kept for Effect enum generation)
#[allow(deprecated)]
use crux_http::Http;

// Re-export core types
pub use crate::capabilities::centrifugo::{CentrifugoOperation, CentrifugoOutput};
pub use crate::events::Event;
pub use crate::http_helpers::{
    build_url, check_response_status, extract_error_message, extract_string_response,
    handle_auth_error, handle_request_error, is_response_success, parse_json_response,
    process_json_response, process_status_response, BASE_URL,
};
pub use crate::model::Model;
pub use crate::types::*;
pub use crux_http::Result as HttpResult;

/// Capabilities - side effects the app can perform
///
/// Note: We keep the old deprecated capabilities in this struct ONLY for Effect enum generation.
/// The #[derive(crux_core::macros::Effect)] macro generates the Effect enum with proper
/// From<Request<Operation>> implementations based on what's declared here.
/// Actual usage goes through the Command-based APIs (HttpCmd, CentrifugoCmd).
#[allow(deprecated)]
#[cfg_attr(feature = "typegen", derive(crux_core::macros::Export))]
#[derive(crux_core::macros::Effect)]
pub struct Capabilities {
    pub render: crux_core::render::Render<Event>,
    pub http: Http<Event>,
    pub centrifugo: crate::capabilities::centrifugo::Centrifugo<Event>,
}

pub type CentrifugoCmd = crate::capabilities::centrifugo_command::Centrifugo<Effect, Event>;
pub type HttpCmd = crux_http::command::Http<Effect, Event>;

/// The Core application
#[derive(Default)]
pub struct App;

impl crux_core::App for App {
    type Event = Event;
    type Model = Model;
    type ViewModel = Model;
    type Capabilities = Capabilities;
    type Effect = Effect;

    fn update(
        &self,
        event: Self::Event,
        model: &mut Self::Model,
        _caps: &Self::Capabilities,
    ) -> Command<Effect, Event> {
        update::update(event, model)
    }

    fn view(&self, model: &Self::Model) -> Self::ViewModel {
        model.clone()
    }
}
