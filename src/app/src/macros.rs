/// Macro for model field updates with automatic rendering.
/// Supports both single and multiple field updates.
///
/// # Examples
///
/// Single field update:
/// ```ignore
/// update_field!(model.system_info, Some(info))
/// ```
///
/// Multiple field updates:
/// ```ignore
/// update_field!(
///     model.is_connected, true;
///     model.error_message, None
/// )
/// ```
#[macro_export]
macro_rules! update_field {
    // Multiple field updates (must come first to match the pattern)
    ($($model_field:expr, $value:expr);+ $(;)?) => {{
        $(
            $model_field = $value;
        )+
        crux_core::render::render()
    }};

    // Single field update
    ($model_field:expr, $value:expr) => {{
        $model_field = $value;
        crux_core::render::render()
    }};
}

// Re-export http_helpers functions for macro use
pub use crate::http_helpers::{
    build_url, check_response_status, extract_error_message, extract_string_response,
    handle_auth_error, handle_request_error, is_response_success, parse_json_response,
    process_json_response, process_status_response, BASE_URL,
};

/// Macro for unauthenticated POST requests with standard error handling.
/// Requires domain parameters for event wrapping.
///
/// # Patterns
///
/// Pattern 0: Simple POST without body (status only)
/// ```ignore
/// unauth_post!(Device, DeviceEvent, model, "/ack-rollback", AckRollbackResponse, "Acknowledge rollback")
/// ```
///
/// Pattern 1: POST with JSON body expecting JSON response
/// ```ignore
/// unauth_post!(Auth, AuthEvent, model, "/endpoint", Response, "Action",
///     body_json: &request,
///     expect_json: ResponseType
/// )
/// ```
///
/// Pattern 2: POST with JSON body expecting status only
/// ```ignore
/// unauth_post!(Auth, AuthEvent, model, "/set-password", SetPasswordResponse, "Set password",
///     body_json: &request
/// )
/// ```
///
/// Pattern 3: GET expecting JSON response
/// ```ignore
/// unauth_post!(Auth, AuthEvent, model, "/require-set-password", CheckRequiresPasswordSetResponse, "Check password",
///     method: get,
///     expect_json: bool
/// )
/// ```
#[macro_export]
macro_rules! unauth_post {
    // Pattern 0: Simple POST without body (status only)
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr) => {{
        $model.start_loading();
        let cmd = crux_core::Command::all([
            crux_core::render::render(),
            $crate::HttpCmd::post($crate::build_url($endpoint))
                .build()
                .then_send(|result| {
                    let event_result = $crate::process_status_response($action, result);
                    $crate::events::Event::$domain($crate::events::$domain_event::$response_event(
                        event_result,
                    ))
                }),
        ]);
        cmd
    }};

    // Pattern 1: POST with JSON body expecting JSON response
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr, body_json: $body:expr, expect_json: $response_type:ty) => {{
        $model.start_loading();
        match $crate::HttpCmd::post($crate::build_url($endpoint))
            .header("Content-Type", "application/json")
            .body_json($body)
        {
            Ok(builder) => crux_core::Command::all([
                crux_core::render::render(),
                builder.build().then_send(|result| {
                    let event_result: Result<$response_type, String> = match result {
                        Ok(mut response) => $crate::parse_json_response($action, &mut response),
                        Err(e) => Err(e.to_string()),
                    };
                    $crate::events::Event::$domain($crate::events::$domain_event::$response_event(
                        event_result,
                    ))
                }),
            ]),
            Err(e) => {
                $model.set_error_and_render(format!("Failed to create {} request: {}", $action, e))
            }
        }
    }};

    // Pattern 2: POST with JSON body expecting status only
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr, body_json: $body:expr) => {{
        $model.start_loading();
        match $crate::HttpCmd::post($crate::build_url($endpoint))
            .header("Content-Type", "application/json")
            .body_json($body)
        {
            Ok(builder) => crux_core::Command::all([
                crux_core::render::render(),
                builder.build().then_send(|result| {
                    let event_result = match result {
                        Ok(mut response) => $crate::check_response_status($action, &mut response),
                        Err(e) => Err(e.to_string()),
                    };
                    $crate::events::Event::$domain($crate::events::$domain_event::$response_event(
                        event_result,
                    ))
                }),
            ]),
            Err(e) => {
                $model.set_error_and_render(format!("Failed to create {} request: {}", $action, e))
            }
        }
    }};

    // Pattern 3: GET expecting JSON response
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr, method: get, expect_json: $response_type:ty) => {{
        $model.start_loading();
        crux_core::Command::all([
            crux_core::render::render(),
            $crate::HttpCmd::get($crate::build_url($endpoint))
                .build()
                .then_send(|result| {
                    let event_result: Result<$response_type, String> = match result {
                        Ok(mut response) => $crate::parse_json_response($action, &mut response),
                        Err(e) => Err(e.to_string()),
                    };
                    $crate::events::Event::$domain($crate::events::$domain_event::$response_event(
                        event_result,
                    ))
                }),
        ])
    }};
}

/// Macro for POST requests with Basic authentication (username:password encoded).
///
/// Used for login endpoint which requires Basic auth instead of Bearer token.
/// Returns string body (e.g., auth token) on success, with optional conversion to target type.
///
/// NOTE: Endpoints are prefixed with `http://omnect-device` as a workaround.
/// `crux_http` (v0.15) panics when given a relative URL in some environments (e.g. `cargo test`).
/// The UI shell (`useCore.ts`) strips this prefix before sending the request.
///
/// # Example
/// ```ignore
/// auth_post_basic!(Auth, AuthEvent, model, "/token/login", LoginResponse, "Login",
///     credentials: encoded_credentials,
///     map: |token| AuthToken { token })
/// ```
#[macro_export]
macro_rules! auth_post_basic {
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr, credentials: $credentials:expr, map: $mapper:expr) => {{
        $model.start_loading();
        crux_core::Command::all([
            crux_core::render::render(),
            $crate::HttpCmd::post($crate::build_url($endpoint))
                .header("Authorization", format!("Basic {}", $credentials))
                .build()
                .then_send(|result| {
                    let event_result = match result {
                        Ok(mut response) => {
                            $crate::extract_string_response($action, &mut response).map($mapper)
                        }
                        Err(e) => Err(e.to_string()),
                    };
                    $crate::events::Event::$domain($crate::events::$domain_event::$response_event(
                        event_result,
                    ))
                }),
        ])
    }};
}

/// Macro for authenticated POST requests with standard error handling.
/// Reduces boilerplate for POST requests that require authentication.
///
/// NOTE: Endpoints are prefixed with `http://omnect-device` as a workaround.
/// `crux_http` (v0.15) panics when given a relative URL in some environments (e.g. `cargo test`).
/// The UI shell (`useCore.ts`) strips this prefix before sending the request.
/// This workaround should be removed once `crux_http` supports relative URLs gracefully.
///
/// # Patterns
///
/// Pattern 1: Simple POST without body
/// ```ignore
/// auth_post!(model, "/api/device/reboot", RebootResponse, "Reboot")
/// ```
///
/// Pattern 2: POST with JSON body
/// ```ignore
/// auth_post!(model, "/api/device/factory-reset", FactoryResetResponse, "Factory reset",
///     body_json: &FactoryResetRequest { mode, preserve }
/// )
/// ```
///
/// Pattern 3: POST with string body
/// ```ignore
/// auth_post!(model, "/api/device/network", SetNetworkConfigResponse, "Set network config",
///     body_string: config
/// )
/// ```
///
/// NOTE: The macro now requires a domain parameter to specify the event wrapper.
/// Examples:
/// - Auth domain: `auth_post!(Auth, AuthEvent, model, "/logout", LogoutResponse, "Logout")`
/// - Device domain: `auth_post!(Device, DeviceEvent, model, "/reboot", RebootResponse, "Reboot")`
#[macro_export]
macro_rules! auth_post {
    // Pattern 1: Simple POST without body
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr) => {{
        $model.start_loading();
        if let Some(token) = &$model.auth_token {
            crux_core::Command::all([
                crux_core::render::render(),
                $crate::HttpCmd::post($crate::build_url($endpoint))
                    .header("Authorization", format!("Bearer {token}"))
                    .build()
                    .then_send(|result| {
                        let event_result = $crate::process_status_response($action, result);
                        $crate::events::Event::$domain(
                            $crate::events::$domain_event::$response_event(event_result),
                        )
                    }),
            ])
        } else {
            $crate::handle_auth_error($model, $action)
        }
    }};

    // Pattern 2: POST with JSON body (no JSON response expected)
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr, body_json: $body:expr) => {{
        $model.start_loading();
        if let Some(token) = &$model.auth_token {
            match $crate::HttpCmd::post($crate::build_url($endpoint))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body_json($body)
            {
                Ok(builder) => crux_core::Command::all([
                    crux_core::render::render(),
                    builder.build().then_send(|result| {
                        let event_result = $crate::process_status_response($action, result);
                        $crate::events::Event::$domain(
                            $crate::events::$domain_event::$response_event(event_result),
                        )
                    }),
                ]),
                Err(e) => $crate::handle_request_error($model, $action, e),
            }
        } else {
            $crate::handle_auth_error($model, $action)
        }
    }};

    // Pattern 3: POST with string body (no JSON response expected)
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr, body_string: $body:expr) => {{
        $model.start_loading();
        if let Some(token) = &$model.auth_token {
            crux_core::Command::all([
                crux_core::render::render(),
                $crate::HttpCmd::post($crate::build_url($endpoint))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body_string($body)
                    .build()
                    .then_send(|result| {
                        let event_result = $crate::process_status_response($action, result);
                        $crate::events::Event::$domain(
                            $crate::events::$domain_event::$response_event(event_result),
                        )
                    }),
            ])
        } else {
            $crate::handle_auth_error($model, $action)
        }
    }};

    // Pattern 4: POST with JSON body expecting JSON response
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr, body_json: $body:expr, expect_json: $response_type:ty) => {{
        $model.start_loading();
        if let Some(token) = &$model.auth_token {
            match $crate::HttpCmd::post($crate::build_url($endpoint))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body_json($body)
            {
                Ok(builder) => crux_core::Command::all([
                    crux_core::render::render(),
                    builder.build().then_send(|result| {
                        let event_result: Result<$response_type, String> =
                            $crate::process_json_response($action, result);
                        $crate::events::Event::$domain(
                            $crate::events::$domain_event::$response_event(event_result),
                        )
                    }),
                ]),
                Err(e) => $crate::handle_request_error($model, $action, e),
            }
        } else {
            $crate::handle_auth_error($model, $action)
        }
    }};

    // Pattern 5: POST with string body expecting JSON response
    ($domain:ident, $domain_event:ident, $model:expr, $endpoint:expr, $response_event:ident, $action:expr, body_string: $body:expr, expect_json: $response_type:ty) => {{
        $model.start_loading();
        if let Some(token) = &$model.auth_token {
            crux_core::Command::all([
                crux_core::render::render(),
                $crate::HttpCmd::post($crate::build_url($endpoint))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body_string($body)
                    .build()
                    .then_send(|result| {
                        let event_result: Result<$response_type, String> =
                            $crate::process_json_response($action, result);
                        $crate::events::Event::$domain(
                            $crate::events::$domain_event::$response_event(event_result),
                        )
                    }),
            ])
        } else {
            $crate::handle_auth_error($model, $action)
        }
    }};
}

/// Macro for simple HTTP GET requests expecting JSON response.
/// Does not set loading state or require authentication.
/// Requires domain parameters for event wrapping.
///
/// # Example
/// ```ignore
/// http_get!(Device, DeviceEvent, "http://omnect-device/healthcheck", HealthcheckResponse, HealthcheckInfo)
/// ```
#[macro_export]
macro_rules! http_get {
    ($domain:ident, $domain_event:ident, $url:expr, $response_event:ident, $response_type:ty) => {
        $crate::HttpCmd::get($url).build().then_send(|result| {
            let event_result: Result<$response_type, String> = match result {
                Ok(mut response) => {
                    $crate::parse_json_response(stringify!($response_event), &mut response)
                }
                Err(e) => Err(e.to_string()),
            };
            $crate::events::Event::$domain($crate::events::$domain_event::$response_event(
                event_result,
            ))
        })
    };
}

/// Silent HTTP GET - no loading state, custom success/error event handlers.
///
/// Used for background polling where failures should not show errors to user.
/// Does NOT check for shell hack header (since success returns custom event anyway).
///
/// # Example
/// ```ignore
/// http_get_silent!(
///     url,
///     on_success: Event::Device(DeviceEvent::HealthcheckResponse(Ok(HealthcheckInfo::default()))),
///     on_error: Event::Ui(UiEvent::ClearSuccess)
/// )
/// ```
#[macro_export]
macro_rules! http_get_silent {
    ($url:expr, on_success: $success_event:expr, on_error: $error_event:expr) => {
        $crate::HttpCmd::get($url)
            .build()
            .then_send(move |result| match result {
                Ok(response) if response.status().is_success() => $success_event,
                _ => $error_event,
            })
    };
}

/// Macro for handling response events with standard loading state and error handling.
///
/// # Patterns
///
/// Pattern 1: Only success message (for `Result<(), String>`)
/// ```ignore
/// handle_response!(model, result, {
///     success_message: "Operation successful",
/// })
/// ```
///
/// Pattern 2: Custom success handling
/// ```ignore
/// handle_response!(model, result, {
///     on_success: |m, value| {
///         m.some_field = value;
///     },
/// })
/// ```
///
/// Pattern 3: Custom success handler + success message
/// ```ignore
/// handle_response!(model, result, {
///     on_success: |m, value| {
///         m.some_field = value;
///     },
///     success_message: "Operation successful",
/// })
/// ```
///
/// Pattern 4: Custom success handler without loading state (for responses that don't set loading)
/// ```ignore
/// handle_response!(model, result, {
///     on_success: |m, info| {
///         m.healthcheck = Some(info);
///     },
///     no_loading: true,
/// })
/// ```
#[macro_export]
macro_rules! handle_response {
    // Pattern 1: Only success message (for Result<(), String>)
    ($model:expr, $result:expr, {
        success_message: $msg:expr $(,)?
    }) => {{
        $model.stop_loading();
        match $result {
            Ok(()) => {
                $model.success_message = Some($msg.to_string());
            }
            Err(e) => {
                $model.set_error(e);
            }
        }
        crux_core::render::render()
    }};

    // Pattern 2: Only custom success handler
    ($model:expr, $result:expr, {
        on_success: |$success_model:ident, $value:tt| $success_body:block $(,)?
    }) => {{
        $model.stop_loading();
        match $result {
            Ok($value) => {
                #[allow(clippy::redundant_locals)]
                let $success_model = $model;
                $success_body
            }
            Err(e) => {
                $model.set_error(e);
            }
        }
        crux_core::render::render()
    }};

    // Pattern 3: Custom success handler + success message
    ($model:expr, $result:expr, {
        on_success: |$success_model:ident, $value:tt| $success_body:block,
        success_message: $msg:expr $(,)?
    }) => {{
        $model.stop_loading();
        match $result {
            Ok($value) => {
                #[allow(clippy::redundant_locals)]
                let $success_model = $model;
                $success_body
                $model.success_message = Some($msg.to_string());
            }
            Err(e) => {
                $model.set_error(e);
            }
        }
        crux_core::render::render()
    }};

    // Pattern 4: Only on_success without loading state (for HealthcheckResponse)
    ($model:expr, $result:expr, {
        on_success: |$success_model:ident, $value:tt| $success_body:block,
        no_loading: true $(,)?
    }) => {{
        $model.clear_error();
        match $result {
            Ok($value) => {
                #[allow(clippy::redundant_locals)]
                let $success_model = $model;
                $success_body
            }
            Err(e) => {
                $model.error_message = Some(e);
            }
        }
        crux_core::render::render()
    }};
}
