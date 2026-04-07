use crate::services::auth::{TokenManager, password::PasswordService};
use actix_session::SessionExt;
use actix_web::{
    Error, FromRequest, HttpMessage, HttpResponse,
    body::EitherBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready},
    web,
};
use actix_web_httpauth::extractors::{basic::BasicAuth, bearer::BearerAuth};
use anyhow::Result;
use log::error;
use std::{
    future::{Future, Ready, ready},
    pin::Pin,
    rc::Rc,
};

pub struct AuthMw;

impl<S, B> Transform<S, ServiceRequest> for AuthMw
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = AuthMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddleware {
            service: Rc::new(service),
        }))
    }
}

pub struct AuthMiddleware<S> {
    service: Rc<S>,
}

type LocalBoxFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

impl<S, B> Service<ServiceRequest> for AuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();

        Box::pin(async move {
            let token = match req.get_session().get::<String>("token") {
                Ok(token) => token.unwrap_or_default(),
                Err(e) => {
                    error!("failed to get session. {e:#}");
                    String::new()
                }
            };

            // Extract TokenManager from app data
            let Some(token_manager) = req.app_data::<web::Data<TokenManager>>().cloned() else {
                error!("failed to get TokenManager.");
                return Ok(unauthorized_error(req).map_into_right_body());
            };

            // 1. Check Session Cookie
            if token_manager.verify_token(&token) {
                let res = service.call(req).await?;
                return Ok(res.map_into_left_body());
            }

            let mut payload = req.take_payload().take();

            let is_authorized = match req
                .headers()
                .get(actix_web::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
            {
                // 2. Check Bearer Token
                Some(h) if h.starts_with("Bearer ") => {
                    BearerAuth::from_request(req.request(), &mut payload)
                        .await
                        .is_ok_and(|auth| token_manager.verify_token(auth.token()))
                }
                // 3. Check Basic Auth
                Some(h) if h.starts_with("Basic ") => {
                    BasicAuth::from_request(req.request(), &mut payload)
                        .await
                        .is_ok_and(verify_user)
                }
                _ => false,
            };

            if is_authorized {
                req.set_payload(payload);
                let res = service.call(req).await?;
                return Ok(res.map_into_left_body());
            }

            Ok(unauthorized_error(req).map_into_right_body())
        })
    }
}

#[allow(clippy::needless_pass_by_value)]
fn verify_user(auth: BasicAuth) -> bool {
    let Some(password) = auth.password() else {
        return false;
    };

    if let Err(e) = PasswordService::validate_password(password) {
        error!("verify_user() failed: {e:#}");
        return false;
    }

    true
}

fn unauthorized_error(req: ServiceRequest) -> ServiceResponse {
    let http_res = HttpResponse::Unauthorized().body("Invalid credentials");
    let (http_req, _) = req.into_parts();
    ServiceResponse::new(http_req, http_res)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const TOKEN_SUBJECT: &str = "omnect-ui";
    const TOKEN_EXPIRE_HOURS: u64 = 2;
    use actix_http::StatusCode;
    use actix_session::{
        SessionMiddleware,
        config::{BrowserSession, CookieContentSecurity},
        storage::{CookieSessionStore, SessionStore},
    };
    use actix_web::{
        App, HttpResponse, Responder,
        cookie::{Cookie, CookieJar, Key, SameSite},
        dev::ServiceResponse,
        http::header::ContentType,
        test, web,
    };
    use actix_web_httpauth::headers::authorization::Basic;
    use base64::prelude::*;
    use jsonwebtoken::{EncodingKey, Header, encode, get_current_timestamp};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    #[derive(Debug, Serialize, Deserialize)]
    struct TestClaims {
        #[serde(skip_serializing_if = "Option::is_none")]
        sub: Option<String>,
        iat: u64,
        exp: u64,
    }

    fn generate_valid_claim() -> TestClaims {
        let iat = get_current_timestamp();
        let exp = iat + TOKEN_EXPIRE_HOURS * 3600;

        TestClaims {
            iat,
            exp,
            sub: Some(TOKEN_SUBJECT.to_string()),
        }
    }

    fn generate_expired_claim() -> TestClaims {
        let now = get_current_timestamp();
        let iat = now - 2 * TOKEN_EXPIRE_HOURS * 3600;
        let exp = now - TOKEN_EXPIRE_HOURS * 3600;

        TestClaims {
            iat,
            exp,
            sub: Some(TOKEN_SUBJECT.to_string()),
        }
    }

    fn generate_invalid_subject_claim() -> TestClaims {
        let iat = get_current_timestamp();
        let exp = iat + TOKEN_EXPIRE_HOURS * 3600;

        TestClaims {
            iat,
            exp,
            sub: Some("some_unknown_subject".to_string()),
        }
    }

    fn generate_unset_subject_claim() -> TestClaims {
        let iat = get_current_timestamp();
        let exp = iat + TOKEN_EXPIRE_HOURS * 3600;

        TestClaims {
            iat,
            exp,
            sub: None,
        }
    }

    fn generate_token(claim: TestClaims) -> String {
        let key = EncodingKey::from_secret("test-secret-key!".as_bytes());
        encode(&Header::default(), &claim, &key).unwrap()
    }

    async fn index() -> impl Responder {
        HttpResponse::Ok().body("Success")
    }

    async fn echo_json(body: web::Json<serde_json::Value>) -> impl Responder {
        HttpResponse::Ok().json(body.into_inner())
    }

    const SESSION_SECRET: [u8; 64] = [
        0xb2, 0x64, 0x83, 0x0, 0xf5, 0xcb, 0xf6, 0x1d, 0x5c, 0x83, 0xc0, 0x90, 0x6b, 0xb2, 0xe4,
        0x26, 0x14, 0x9, 0x2b, 0xa1, 0xc4, 0xc5, 0x37, 0xe7, 0xc9, 0x20, 0x8e, 0xbc, 0xee, 0x2,
        0x3c, 0xa2, 0x32, 0x57, 0x96, 0xc9, 0x99, 0x62, 0x90, 0x4f, 0x24, 0xe5, 0x25, 0x6b, 0xe1,
        0x2b, 0x8a, 0x3, 0xa3, 0xc7, 0x1e, 0xb2, 0xb2, 0xbe, 0x29, 0x51, 0xc1, 0xe2, 0x1e, 0xb7,
        0x8, 0x15, 0xc9, 0xe0,
    ];

    async fn create_service() -> impl actix_service::Service<
        actix_http::Request,
        Response = ServiceResponse,
        Error = actix_web::Error,
    > {
        let key = Key::from(&SESSION_SECRET);
        let session_middleware = SessionMiddleware::builder(CookieSessionStore::default(), key)
            .cookie_name(String::from("omnect-ui-session"))
            .cookie_secure(true)
            .session_lifecycle(BrowserSession::default())
            .cookie_same_site(SameSite::Strict)
            .cookie_content_security(CookieContentSecurity::Private)
            .cookie_http_only(true)
            .build();

        let token_manager = TokenManager::new("test-secret-key!");

        test::init_service(
            App::new()
                .app_data(web::Data::new(token_manager))
                .wrap(session_middleware)
                .route("/", web::get().to(index).wrap(AuthMw))
                .route("/echo", web::post().to(echo_json).wrap(AuthMw)),
        )
        .await
    }

    async fn create_cookie_for_token(token: &str) -> Cookie<'_> {
        const SESSION_ID: &str = "omnect-ui-session";
        let token_name: String = "token".to_string();

        let key = Key::from(&SESSION_SECRET);
        let mut cookie_jar = CookieJar::new();
        let mut private_jar = cookie_jar.private_mut(&key);
        let session_store = CookieSessionStore::default();

        let ttl = get_current_timestamp() + 2 * 3600;
        let ttl = actix_web::cookie::time::Duration::seconds(ttl.try_into().unwrap());

        let session_value = session_store
            .save(
                HashMap::from([(token_name, format!("\"{}\"", token))]),
                &ttl,
            )
            .await
            .unwrap()
            .as_ref()
            .to_string();

        private_jar.add(Cookie::new(SESSION_ID, session_value));

        cookie_jar.get(SESSION_ID).unwrap().clone()
    }

    #[tokio::test]
    async fn middleware_correct_token_should_succeed() {
        let claim = generate_valid_claim();
        let token = generate_token(claim);

        let app = create_service().await;
        let cookie = create_cookie_for_token(&token).await;

        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .cookie(cookie)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert!(resp.status().is_success());
    }

    #[tokio::test]
    async fn middleware_expired_token_should_require_login() {
        let claim = generate_expired_claim();
        let token = generate_token(claim);

        let app = create_service().await;
        let cookie = create_cookie_for_token(&token).await;

        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .cookie(cookie)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn middleware_token_with_invalid_subject_should_require_login() {
        let claim = generate_invalid_subject_claim();
        let token = generate_token(claim);

        let app = create_service().await;
        let cookie = create_cookie_for_token(&token).await;

        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .cookie(cookie)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let claim = generate_unset_subject_claim();
        let token = generate_token(claim);

        let app = create_service().await;
        let cookie = create_cookie_for_token(&token).await;

        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .cookie(cookie)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn middleware_invalid_token_should_require_login() {
        let claim = generate_unset_subject_claim();
        let _ = generate_token(claim);
        let token = "someinvalidtestbytes".to_string();

        let app = create_service().await;
        let cookie = create_cookie_for_token(&token).await;

        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .cookie(cookie)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    fn setup_password_file(password: &str) {
        PasswordService::store_or_update_password(password)
            .expect("failed to setup password file for test");
    }

    #[tokio::test]
    #[expect(clippy::await_holding_lock)]
    async fn middleware_correct_user_credentials_should_succeed_and_return_valid_token() {
        let _lock = PasswordService::lock_for_test();

        let password = "some-password";
        setup_password_file(password);

        let app = create_service().await;

        let encoded_password = BASE64_STANDARD.encode(format!(":{password}"));

        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .insert_header(("Authorization", format!("Basic {encoded_password}")))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert!(resp.status().is_success());
    }

    #[tokio::test]
    #[expect(clippy::await_holding_lock)]
    async fn middleware_invalid_user_credentials_should_return_unauthorized_error() {
        let _lock = PasswordService::lock_for_test();

        let password = "some-password";
        setup_password_file(password);

        let app = create_service().await;

        let encoded_password = BASE64_STANDARD.encode(":some-other-password");

        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .insert_header(("Authorization", format!("Basic {encoded_password}")))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn middleware_correct_bearer_token_should_succeed() {
        let claim = generate_valid_claim();
        let token = generate_token(claim);

        let app = create_service().await;

        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .insert_header(("Authorization", format!("Bearer {token}")))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert!(resp.status().is_success());
    }

    #[tokio::test]
    async fn middleware_bearer_auth_preserves_request_body() {
        let claim = generate_valid_claim();
        let token = generate_token(claim);

        let app = create_service().await;

        let payload = serde_json::json!({"mode": 1, "preserve": ["certificates"]});

        let req = test::TestRequest::post()
            .uri("/echo")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", format!("Bearer {token}")))
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert!(resp.status().is_success());

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body, payload);
    }

    #[tokio::test]
    #[expect(clippy::await_holding_lock)]
    async fn middleware_basic_auth_preserves_request_body() {
        let _lock = PasswordService::lock_for_test();

        let password = "some-password";
        setup_password_file(password);

        let app = create_service().await;

        let payload = serde_json::json!({"mode": 1, "preserve": ["network"]});
        let encoded_password = BASE64_STANDARD.encode(format!(":{password}"));

        let req = test::TestRequest::post()
            .uri("/echo")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", format!("Basic {encoded_password}")))
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert!(resp.status().is_success());

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body, payload);
    }

    #[tokio::test]
    async fn verify_correct_token_should_succeed() {
        let claim = generate_valid_claim();
        let token = generate_token(claim);
        let token_manager = TokenManager::new("test-secret-key!");

        assert!(token_manager.verify_token(token.as_str()));
    }

    #[tokio::test]
    async fn verify_expired_token_should_fail() {
        let claim = generate_expired_claim();
        let token = generate_token(claim);
        let token_manager = TokenManager::new("test-secret-key!");

        assert!(!token_manager.verify_token(token.as_str()));
    }

    #[tokio::test]
    async fn verify_token_with_invalid_subject_should_fail() {
        let claim = generate_unset_subject_claim();
        let token = generate_token(claim);
        let token_manager = TokenManager::new("test-secret-key!");

        assert!(!token_manager.verify_token(token.as_str()));

        let claim = generate_invalid_subject_claim();
        let token = generate_token(claim);

        assert!(!token_manager.verify_token(token.as_str()));
    }

    #[tokio::test]
    async fn verify_token_with_invalid_token_should_fail() {
        let claim = generate_invalid_subject_claim();
        let _ = generate_token(claim);
        let token = "someinvalidtestbytes".to_string();
        let token_manager = TokenManager::new("test-secret-key!");

        assert!(!token_manager.verify_token(token.as_str()));
    }

    #[tokio::test]
    async fn verify_user_with_unset_password_should_fail() {
        let basic_auth = BasicAuth::from(Basic::new("some-user", None::<&str>));

        let expected = false;

        let result = verify_user(basic_auth);

        assert_eq!(expected, result);
    }
}
