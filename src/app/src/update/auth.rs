use base64::prelude::*;
use crux_core::{Command, render::render};

use crate::{
    Effect, auth_post, auth_post_basic,
    events::{AuthEvent, Event, WifiEvent},
    handle_response,
    model::Model,
    types::{AuthToken, SetPasswordRequest, UpdatePasswordRequest, WifiState},
    unauth_post,
};

/// Handle authentication-related events
pub fn handle(event: AuthEvent, model: &mut Model) -> Command<Effect, Event> {
    match event {
        AuthEvent::Login { password } => {
            let encoded = BASE64_STANDARD.encode(format!(":{password}"));
            auth_post_basic!(Auth, AuthEvent, model, "/token/login", LoginResponse, "Login",
                credentials: encoded,
                map: |token| AuthToken { token })
        }

        AuthEvent::LoginResponse(result) => {
            model.stop_loading();
            match result {
                Ok(auth) => {
                    model.auth_token = Some(auth.token);
                    model.is_authenticated = true;
                    post_auth_commands(model)
                }
                Err(e) => {
                    model.set_error(e);
                    render()
                }
            }
        }

        AuthEvent::Logout => {
            auth_post!(Auth, AuthEvent, model, "/logout", LogoutResponse, "Logout")
        }

        AuthEvent::LogoutResponse(result) => handle_response!(model, result, {
            on_success: |model, ()| {
                model.invalidate_session();
            },
        }),

        AuthEvent::SetPassword { password } => {
            let request = SetPasswordRequest { password };
            unauth_post!(Auth, AuthEvent, model, "/set-password", SetPasswordResponse, "Set password",
                body_json: &request,
                map: |token| AuthToken { token })
        }

        AuthEvent::SetPasswordResponse(result) => {
            model.stop_loading();
            match result {
                Ok(auth) => {
                    model.requires_password_set = false;
                    model.auth_token = Some(auth.token);
                    model.is_authenticated = true;
                    post_auth_commands(model)
                }
                Err(e) => {
                    model.set_error(e);
                    render()
                }
            }
        }

        AuthEvent::UpdatePassword {
            current_password,
            password,
        } => {
            let request = UpdatePasswordRequest {
                current_password,
                password,
            };
            auth_post!(Auth, AuthEvent, model, "/update-password", UpdatePasswordResponse, "Update password",
                body_json: &request
            )
        }

        AuthEvent::UpdatePasswordResponse(result) => handle_response!(model, result, {
            on_success: |model, ()| {},
            success_message: "Password updated successfully",
        }),

        AuthEvent::CheckRequiresPasswordSet => {
            unauth_post!(Auth, AuthEvent, model, "/require-set-password", CheckRequiresPasswordSetResponse, "Check password",
                method: get,
                expect_json: bool
            )
        }

        AuthEvent::CheckRequiresPasswordSetResponse(result) => handle_response!(model, result, {
            on_success: |model, requires| {
                model.requires_password_set = requires;
            },
        }),

        AuthEvent::RestoreSession(token) => {
            model.auth_token = Some(token);
            model.is_authenticated = true;
            post_auth_commands(model)
        }
    }
}

/// After successful authentication, fetch `WiFi` data if `WiFi` is available
fn post_auth_commands(model: &mut Model) -> Command<Effect, Event> {
    if matches!(model.wifi_state, WifiState::Ready { .. }) {
        Command::all([
            render(),
            super::wifi::handle(WifiEvent::GetStatus, model),
            super::wifi::handle(WifiEvent::GetSavedNetworks, model),
        ])
    } else {
        render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod login {
        use super::*;

        #[test]
        fn posts_to_login_endpoint() {
            let mut model = Model::default();
            let mut cmd = handle(
                AuthEvent::Login {
                    password: "test_password".into(),
                },
                &mut model,
            );

            // Login produces render + http effects
            let effects = [cmd.expect_effect(), cmd.expect_effect()];
            let http_req = effects
                .into_iter()
                .find_map(|e| match e {
                    Effect::Http(req) => Some(req),
                    _ => None,
                })
                .expect("Expected Http effect");
            let (http_request, _) = http_req.split();

            assert_eq!(http_request.url, "https://relative/token/login");
            assert_eq!(http_request.method, "POST");
            assert!(
                http_request
                    .headers
                    .iter()
                    .any(|h| h.name.eq_ignore_ascii_case("authorization")
                        && h.value.starts_with("Basic "))
            );
        }

        #[test]
        fn sets_loading_state() {
            let mut model = Model::default();

            let _ = handle(
                AuthEvent::Login {
                    password: "test_password".into(),
                },
                &mut model,
            );

            assert!(model.is_loading);
            assert!(model.error_message.is_none());
        }

        #[test]
        fn success_sets_authenticated_and_stores_token() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::LoginResponse(Ok(AuthToken {
                    token: "test_token_123".into(),
                })),
                &mut model,
            );

            assert!(model.is_authenticated);
            assert!(!model.is_loading);
            assert_eq!(model.auth_token, Some("test_token_123".into()));
            assert!(model.error_message.is_none());
        }

        #[test]
        fn failure_sets_error_and_not_authenticated() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::LoginResponse(Err("Invalid credentials".into())),
                &mut model,
            );

            assert!(!model.is_authenticated);
            assert!(!model.is_loading);
            assert!(model.auth_token.is_none());
            assert_eq!(model.error_message, Some("Invalid credentials".into()));
        }

        #[test]
        fn clears_previous_error_on_new_attempt() {
            let mut model = Model {
                error_message: Some("Previous error".into()),
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::Login {
                    password: "test".into(),
                },
                &mut model,
            );

            assert!(model.error_message.is_none());
        }
    }

    mod logout {
        use super::*;

        #[test]
        fn posts_to_logout_endpoint() {
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("bearer-token".into()),
                ..Default::default()
            };
            let mut cmd = handle(AuthEvent::Logout, &mut model);

            let effects = [cmd.expect_effect(), cmd.expect_effect()];
            let http_req = effects
                .into_iter()
                .find_map(|e| match e {
                    Effect::Http(req) => Some(req),
                    _ => None,
                })
                .expect("Expected Http effect");
            let (http_request, _) = http_req.split();

            assert_eq!(http_request.url, "https://relative/logout");
            assert_eq!(http_request.method, "POST");
            assert!(
                http_request
                    .headers
                    .iter()
                    .any(|h| h.name.eq_ignore_ascii_case("authorization")
                        && h.value.starts_with("Bearer "))
            );
        }

        #[test]
        fn sets_loading_state() {
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                ..Default::default()
            };

            let _ = handle(AuthEvent::Logout, &mut model);

            assert!(model.is_loading);
        }

        #[test]
        fn success_clears_session() {
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(AuthEvent::LogoutResponse(Ok(())), &mut model);

            assert!(!model.is_authenticated);
            assert!(model.auth_token.is_none());
            assert!(!model.is_loading);
        }

        #[test]
        fn failure_sets_error_but_keeps_session() {
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::LogoutResponse(Err("Network error".into())),
                &mut model,
            );

            // Session remains intact on logout failure
            assert!(model.is_authenticated);
            assert!(model.auth_token.is_some());
            assert!(!model.is_loading);
            assert_eq!(model.error_message, Some("Network error".into()));
        }
    }

    mod set_password {
        use super::*;

        #[test]
        fn sets_loading_state() {
            let mut model = Model {
                requires_password_set: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::SetPassword {
                    password: "new_password".into(),
                },
                &mut model,
            );

            assert!(model.is_loading);
        }

        #[test]
        fn success_authenticates_and_clears_requires_password_set() {
            let mut model = Model {
                requires_password_set: true,
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::SetPasswordResponse(Ok(AuthToken {
                    token: "set_password_token".into(),
                })),
                &mut model,
            );

            assert!(!model.requires_password_set);
            assert!(!model.is_loading);
            assert!(model.is_authenticated);
            assert_eq!(model.auth_token, Some("set_password_token".into()));
        }

        #[test]
        fn failure_keeps_requires_password_set_and_not_authenticated() {
            let mut model = Model {
                requires_password_set: true,
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::SetPasswordResponse(Err("Password too weak".into())),
                &mut model,
            );

            assert!(model.requires_password_set);
            assert!(!model.is_loading);
            assert!(!model.is_authenticated);
            assert!(model.auth_token.is_none());
            assert_eq!(model.error_message, Some("Password too weak".into()));
        }
    }

    mod update_password {
        use super::*;

        #[test]
        fn sets_loading_state() {
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::UpdatePassword {
                    current_password: "old_pass".into(),
                    password: "new_pass".into(),
                },
                &mut model,
            );

            assert!(model.is_loading);
        }

        #[test]
        fn success_shows_success_message() {
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(AuthEvent::UpdatePasswordResponse(Ok(())), &mut model);

            assert!(!model.is_loading);
            assert_eq!(
                model.success_message,
                Some("Password updated successfully".into())
            );
            // Session should remain valid
            assert!(model.is_authenticated);
            assert!(model.auth_token.is_some());
        }

        #[test]
        fn failure_shows_error() {
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::UpdatePasswordResponse(Err("Current password incorrect".into())),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(
                model.error_message,
                Some("Current password incorrect".into())
            );
            // Session should remain valid even on password update failure
            assert!(model.is_authenticated);
        }
    }

    mod restore_session {
        use super::*;

        #[test]
        fn sets_authenticated_and_stores_token() {
            let mut model = Model::default();

            let _ = handle(
                AuthEvent::RestoreSession("restored-token-123".into()),
                &mut model,
            );

            assert!(model.is_authenticated);
            assert_eq!(model.auth_token, Some("restored-token-123".into()));
            assert!(!model.is_loading);
            assert!(model.error_message.is_none());
        }
    }

    mod check_requires_password_set {
        use super::*;

        #[test]
        fn sends_get_to_check_endpoint() {
            let mut model = Model::default();
            let mut cmd = handle(AuthEvent::CheckRequiresPasswordSet, &mut model);

            let effects = [cmd.expect_effect(), cmd.expect_effect()];
            let http_req = effects
                .into_iter()
                .find_map(|e| match e {
                    Effect::Http(req) => Some(req),
                    _ => None,
                })
                .expect("Expected Http effect");
            let (http_request, _) = http_req.split();

            assert_eq!(http_request.url, "https://relative/require-set-password");
            assert_eq!(http_request.method, "GET");
        }

        #[test]
        fn sets_loading_state() {
            let mut model = Model::default();

            let _ = handle(AuthEvent::CheckRequiresPasswordSet, &mut model);

            assert!(model.is_loading);
        }

        #[test]
        fn response_true_sets_requires_password_set() {
            let mut model = Model {
                requires_password_set: false,
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::CheckRequiresPasswordSetResponse(Ok(true)),
                &mut model,
            );

            assert!(model.requires_password_set);
            assert!(!model.is_loading);
        }

        #[test]
        fn response_false_clears_requires_password_set() {
            let mut model = Model {
                requires_password_set: true,
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::CheckRequiresPasswordSetResponse(Ok(false)),
                &mut model,
            );

            assert!(!model.requires_password_set);
            assert!(!model.is_loading);
        }

        #[test]
        fn failure_sets_error() {
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = handle(
                AuthEvent::CheckRequiresPasswordSetResponse(Err("Server error".into())),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(model.error_message, Some("Server error".into()));
        }
    }
}
