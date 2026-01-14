use base64::prelude::*;
use crux_core::Command;

use crate::auth_post;
use crate::auth_post_basic;
use crate::events::{AuthEvent, Event};
use crate::handle_response;
use crate::model::Model;
use crate::types::{AuthToken, SetPasswordRequest, UpdatePasswordRequest};
use crate::unauth_post;
use crate::Effect;

/// Handle authentication-related events
pub fn handle(event: AuthEvent, model: &mut Model) -> Command<Effect, Event> {
    match event {
        AuthEvent::Login { password } => {
            let encoded = BASE64_STANDARD.encode(format!(":{password}"));
            auth_post_basic!(Auth, AuthEvent, model, "/token/login", LoginResponse, "Login",
                credentials: encoded,
                map: |token| AuthToken { token })
        }

        AuthEvent::LoginResponse(result) => handle_response!(model, result, {
            on_success: |model, auth| {
                model.auth_token = Some(auth.token);
                model.is_authenticated = true;
            },
        }),

        AuthEvent::Logout => {
            auth_post!(Auth, AuthEvent, model, "/logout", LogoutResponse, "Logout")
        }

        AuthEvent::LogoutResponse(result) => handle_response!(model, result, {
            on_success: |model, _| {
                model.invalidate_session();
            },
        }),

        AuthEvent::SetPassword { password } => {
            let request = SetPasswordRequest { password };
            unauth_post!(Auth, AuthEvent, model, "/set-password", SetPasswordResponse, "Set password",
                body_json: &request
            )
        }

        AuthEvent::SetPasswordResponse(result) => handle_response!(model, result, {
            on_success: |model, _| {
                model.requires_password_set = false;
            },
            success_message: "Password set successfully",
        }),

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
            on_success: |model, _| {},
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, Event};
    use crux_core::testing::AppTester;

    mod login {
        use super::*;

        #[test]
        fn sets_loading_state() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let _ = app.update(
                Event::Auth(AuthEvent::Login {
                    password: "test_password".into(),
                }),
                &mut model,
            );

            assert!(model.is_loading);
            assert!(model.error_message.is_none());
        }

        #[test]
        fn success_sets_authenticated_and_stores_token() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::LoginResponse(Ok(AuthToken {
                    token: "test_token_123".into(),
                }))),
                &mut model,
            );

            assert!(model.is_authenticated);
            assert!(!model.is_loading);
            assert_eq!(model.auth_token, Some("test_token_123".into()));
            assert!(model.error_message.is_none());
        }

        #[test]
        fn failure_sets_error_and_not_authenticated() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::LoginResponse(Err("Invalid credentials".into()))),
                &mut model,
            );

            assert!(!model.is_authenticated);
            assert!(!model.is_loading);
            assert!(model.auth_token.is_none());
            assert_eq!(model.error_message, Some("Invalid credentials".into()));
        }

        #[test]
        fn clears_previous_error_on_new_attempt() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                error_message: Some("Previous error".into()),
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::Login {
                    password: "test".into(),
                }),
                &mut model,
            );

            assert!(model.error_message.is_none());
        }
    }

    mod logout {
        use super::*;

        #[test]
        fn sets_loading_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                ..Default::default()
            };

            let _ = app.update(Event::Auth(AuthEvent::Logout), &mut model);

            assert!(model.is_loading);
        }

        #[test]
        fn success_clears_session() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::LogoutResponse(Ok(()))),
                &mut model,
            );

            assert!(!model.is_authenticated);
            assert!(model.auth_token.is_none());
            assert!(!model.is_loading);
        }

        #[test]
        fn failure_sets_error_but_keeps_session() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::LogoutResponse(Err("Network error".into()))),
                &mut model,
            );

            // Session should remain intact on logout failure
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
            let app = AppTester::<App>::default();
            let mut model = Model {
                requires_password_set: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::SetPassword {
                    password: "new_password".into(),
                }),
                &mut model,
            );

            assert!(model.is_loading);
        }

        #[test]
        fn success_clears_requires_password_set() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                requires_password_set: true,
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::SetPasswordResponse(Ok(()))),
                &mut model,
            );

            assert!(!model.requires_password_set);
            assert!(!model.is_loading);
            assert_eq!(
                model.success_message,
                Some("Password set successfully".into())
            );
        }

        #[test]
        fn failure_keeps_requires_password_set() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                requires_password_set: true,
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::SetPasswordResponse(Err(
                    "Password too weak".into(),
                ))),
                &mut model,
            );

            assert!(model.requires_password_set);
            assert!(!model.is_loading);
            assert_eq!(model.error_message, Some("Password too weak".into()));
        }
    }

    mod update_password {
        use super::*;

        #[test]
        fn sets_loading_state() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::UpdatePassword {
                    current_password: "old_pass".into(),
                    password: "new_pass".into(),
                }),
                &mut model,
            );

            assert!(model.is_loading);
        }

        #[test]
        fn success_shows_success_message() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::UpdatePasswordResponse(Ok(()))),
                &mut model,
            );

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
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_authenticated: true,
                auth_token: Some("token".into()),
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::UpdatePasswordResponse(Err(
                    "Current password incorrect".into(),
                ))),
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

    mod check_requires_password_set {
        use super::*;

        #[test]
        fn sets_loading_state() {
            let app = AppTester::<App>::default();
            let mut model = Model::default();

            let _ = app.update(
                Event::Auth(AuthEvent::CheckRequiresPasswordSet),
                &mut model,
            );

            assert!(model.is_loading);
        }

        #[test]
        fn response_true_sets_requires_password_set() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                requires_password_set: false,
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::CheckRequiresPasswordSetResponse(Ok(true))),
                &mut model,
            );

            assert!(model.requires_password_set);
            assert!(!model.is_loading);
        }

        #[test]
        fn response_false_clears_requires_password_set() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                requires_password_set: true,
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::CheckRequiresPasswordSetResponse(Ok(false))),
                &mut model,
            );

            assert!(!model.requires_password_set);
            assert!(!model.is_loading);
        }

        #[test]
        fn failure_sets_error() {
            let app = AppTester::<App>::default();
            let mut model = Model {
                is_loading: true,
                ..Default::default()
            };

            let _ = app.update(
                Event::Auth(AuthEvent::CheckRequiresPasswordSetResponse(Err(
                    "Server error".into(),
                ))),
                &mut model,
            );

            assert!(!model.is_loading);
            assert_eq!(model.error_message, Some("Server error".into()));
        }
    }
}
