// Actix handler signatures require async and use !Send futures by design
#![allow(
    clippy::future_not_send,
    clippy::needless_pass_by_value,
    clippy::unused_async
)]

use crate::{
    config::AppConfig,
    http_client::handle_service_result,
    keycloak_client::SingleSignOnProvider,
    omnect_device_service_client::{DeviceServiceClient, FactoryReset, RunUpdate},
    services::{
        auth::{AuthorizationService, PasswordService, TokenManager},
        firmware::FirmwareService,
        marker,
        network::{NetworkConfigRequest, NetworkConfigService},
        settings::SettingsService,
    },
    wifi_commissioning_client::{
        WifiAvailability, WifiCommissioningClient, WifiConnectRequest, WifiForgetRequest,
    },
};
use actix_files::NamedFile;
use actix_multipart::Multipart;
use actix_session::Session;
use actix_web::{HttpResponse, Responder, web};
use anyhow::Result;
use futures_util::StreamExt;
use log::{debug, error};
pub use omnect_ui_core::types::{SetPasswordRequest, UpdatePasswordRequest};
use std::collections::HashMap;

pub type StaticResources = HashMap<&'static str, static_files::Resource>;

#[derive(Clone)]
pub struct Api<ServiceClient, SingleSignOn>
where
    ServiceClient: DeviceServiceClient,
    SingleSignOn: SingleSignOnProvider,
{
    pub service_client: ServiceClient,
    pub single_sign_on: SingleSignOn,
}

impl<ServiceClient, SingleSignOn> Api<ServiceClient, SingleSignOn>
where
    ServiceClient: DeviceServiceClient,
    SingleSignOn: SingleSignOnProvider,
{
    pub async fn new(service_client: ServiceClient, single_sign_on: SingleSignOn) -> Result<Self> {
        Ok(Self {
            service_client,
            single_sign_on,
        })
    }

    pub async fn index(
        _api: web::Data<Self>,
        static_resources: web::Data<StaticResources>,
    ) -> actix_web::Result<HttpResponse> {
        debug!("index() called");

        let Some(index_html) = static_resources.get("index.html") else {
            return Err(actix_web::error::ErrorNotFound(
                "index.html not found in embedded resources",
            ));
        };

        Ok(HttpResponse::Ok()
            .content_type(index_html.mime_type)
            .body(index_html.data.to_vec()))
    }

    pub async fn config() -> actix_web::Result<NamedFile> {
        Ok(NamedFile::open(&AppConfig::get().paths.app_config_path)?)
    }

    pub async fn healthcheck(api: web::Data<Self>) -> impl Responder {
        debug!("healthcheck() called");

        match api.service_client.healthcheck_info().await {
            Ok(info) if info.version_info.mismatch => HttpResponse::ServiceUnavailable()
                .insert_header(("Access-Control-Allow-Origin", "*"))
                .json(&info),
            Ok(info) => HttpResponse::Ok()
                .insert_header(("Access-Control-Allow-Origin", "*"))
                .json(&info),
            Err(e) => {
                error!("healthcheck failed: {e:#}");
                HttpResponse::InternalServerError()
                    .insert_header(("Access-Control-Allow-Origin", "*"))
                    .body(e.to_string())
            }
        }
    }

    pub async fn republish(api: web::Data<Self>) -> impl Responder {
        debug!("republish() called");
        handle_service_result(api.service_client.republish().await, "republish")
    }

    pub async fn factory_reset(
        body: web::Json<FactoryReset>,
        api: web::Data<Self>,
        session: Session,
    ) -> impl Responder {
        debug!("factory_reset() called: {body:?}");

        marker::FACTORY_RESET_RESULT_ACKED.clear();
        let result = api.service_client.factory_reset(body.into_inner()).await;

        if result.is_ok() {
            session.purge();
        }

        handle_service_result(result, "factory_reset")
    }

    pub async fn reboot(api: web::Data<Self>) -> impl Responder {
        debug!("reboot() called");
        handle_service_result(api.service_client.reboot().await, "reboot")
    }

    pub async fn token(session: Session, token_manager: web::Data<TokenManager>) -> impl Responder {
        debug!("token() called");

        NetworkConfigService::cancel_rollback();
        Self::session_token(session, token_manager)
    }

    pub async fn logout(session: Session) -> impl Responder {
        debug!("logout() called");
        session.purge();
        HttpResponse::Ok().finish()
    }

    pub async fn version() -> impl Responder {
        HttpResponse::Ok().body(env!("CARGO_PKG_VERSION"))
    }

    pub async fn upload_firmware_file(mut payload: Multipart) -> impl Responder {
        debug!("upload_firmware_file() called");

        while let Some(item) = payload.next().await {
            let field = match item {
                Ok(field) => field,
                Err(e) => return HttpResponse::BadRequest().body(e.to_string()),
            };

            if field.name() == Some("file") {
                return handle_service_result(
                    FirmwareService::receive_firmware(field).await,
                    "upload_firmware_file",
                );
            }
        }

        HttpResponse::BadRequest().body("Missing file field")
    }

    pub async fn load_update(api: web::Data<Self>) -> impl Responder {
        debug!("load_update() called");

        handle_service_result(
            FirmwareService::load_update(&api.service_client).await,
            "load_update",
        )
    }

    pub async fn run_update(body: web::Json<RunUpdate>, api: web::Data<Self>) -> impl Responder {
        debug!("run_update() called with validate_iothub_connection: {body:?}");
        marker::UPDATE_VALIDATION_ACKED.clear();
        handle_service_result(
            FirmwareService::run_update(&api.service_client, body.into_inner()).await,
            "run_update",
        )
    }

    pub async fn set_password(
        body: web::Json<SetPasswordRequest>,
        session: Session,
        token_manager: web::Data<TokenManager>,
    ) -> impl Responder {
        debug!("set_password() called");

        if PasswordService::password_exists() {
            return HttpResponse::Found()
                .append_header(("Location", "/login"))
                .finish();
        }

        let portal_validated = session
            .get::<bool>("portal_validated")
            .unwrap_or(None)
            .unwrap_or(false);
        if !portal_validated {
            return HttpResponse::Unauthorized().body("portal authentication required");
        }

        if let Err(e) = PasswordService::store_or_update_password(&body.password) {
            error!("set_password failed: {e:#}");
            return HttpResponse::InternalServerError().body(e.to_string());
        }

        Self::session_token(session, token_manager)
    }

    pub async fn update_password(
        body: web::Json<UpdatePasswordRequest>,
        session: Session,
    ) -> impl Responder {
        debug!("update_password() called");

        if let Err(e) = PasswordService::validate_password(&body.current_password) {
            error!("validate_password failed: {e:#}");
            return HttpResponse::BadRequest().body("current password is not correct");
        }

        let result = PasswordService::store_or_update_password(&body.password);

        if result.is_ok() {
            session.purge();
        }

        handle_service_result(result, "update_password")
    }

    pub async fn require_set_password() -> impl Responder {
        debug!("require_set_password() called");

        let password_exists = PasswordService::password_exists();
        HttpResponse::Ok().json(!password_exists)
    }

    pub async fn validate_portal_token(
        body: String,
        api: web::Data<Self>,
        session: Session,
    ) -> impl Responder {
        debug!("validate_portal_token() called");

        if let Err(e) = AuthorizationService::validate_token_and_claims(
            &api.single_sign_on,
            &api.service_client,
            &body,
        )
        .await
        {
            error!("validate_portal_token failed: {e:#}");
            return HttpResponse::Unauthorized().finish();
        }

        if let Err(e) = session.insert("portal_validated", true) {
            error!("failed to insert portal_validated into session: {e}");
        }
        HttpResponse::Ok().finish()
    }

    pub async fn set_network_config(
        network_config: web::Json<NetworkConfigRequest>,
        api: web::Data<Self>,
    ) -> impl Responder {
        debug!("set_network_config() called");

        handle_service_result(
            NetworkConfigService::set_network_config(&api.service_client, &network_config).await,
            "set_network_config",
        )
    }

    pub async fn get_settings() -> impl Responder {
        debug!("get_settings() called");
        HttpResponse::Ok().json(SettingsService::get())
    }

    pub async fn update_settings(
        body: web::Json<omnect_ui_core::types::TimeoutSettings>,
    ) -> impl Responder {
        debug!("update_settings() called: {body:?}");
        handle_service_result(SettingsService::save(&body), "update_settings")
    }

    pub async fn ack_rollback() -> impl Responder {
        debug!("ack_rollback() called");
        marker::NETWORK_ROLLBACK_OCCURRED.clear();
        HttpResponse::Ok().finish()
    }

    pub async fn ack_factory_reset_result() -> impl Responder {
        debug!("ack_factory_reset_result() called");
        marker::FACTORY_RESET_RESULT_ACKED.set_or_log();
        HttpResponse::Ok().finish()
    }

    pub async fn ack_update_validation() -> impl Responder {
        debug!("ack_update_validation() called");
        marker::UPDATE_VALIDATION_ACKED.set_or_log();
        HttpResponse::Ok().finish()
    }

    fn session_token(session: Session, token_manager: web::Data<TokenManager>) -> HttpResponse {
        let token = match token_manager.create_token() {
            Ok(token) => token,
            Err(e) => {
                error!("failed to create token: {e:#}");
                return HttpResponse::InternalServerError().body("failed to create token");
            }
        };

        if session.insert("token", &token).is_err() {
            error!("failed to insert token into session");
            return HttpResponse::InternalServerError().body("failed to insert token into session");
        }

        HttpResponse::Ok()
            .content_type("text/plain; charset=utf-8")
            .body(token)
    }
}

// --- WiFi API handlers ---
// Stored as separate web::Data resources to avoid changing the Api generic structure.

use crate::wifi_commissioning_client::WifiCommissioningServiceClient;

type WifiClient = Option<WifiCommissioningServiceClient>;

pub async fn wifi_available(availability: web::Data<WifiAvailability>) -> impl Responder {
    debug!("wifi_available() called");
    HttpResponse::Ok().json(availability.as_ref())
}

macro_rules! wifi_client_or_404 {
    ($wifi:expr) => {
        match $wifi.as_ref().as_ref() {
            Some(client) => client,
            None => return HttpResponse::NotFound().body("WiFi service unavailable"),
        }
    };
}

pub async fn wifi_scan(wifi: web::Data<WifiClient>) -> impl Responder {
    debug!("wifi_scan() called");
    let client = wifi_client_or_404!(wifi);
    handle_service_result(client.scan().await.map(|_| ()), "wifi_scan")
}

pub async fn wifi_scan_results(wifi: web::Data<WifiClient>) -> impl Responder {
    debug!("wifi_scan_results() called");
    let client = wifi_client_or_404!(wifi);
    match client.scan_results().await {
        Ok(results) => HttpResponse::Ok().json(&results),
        Err(e) => {
            error!("wifi_scan_results failed: {e:#}");
            HttpResponse::InternalServerError().body(e.to_string())
        }
    }
}

pub async fn wifi_connect(
    body: web::Json<WifiConnectRequest>,
    wifi: web::Data<WifiClient>,
) -> impl Responder {
    debug!("wifi_connect() called");
    let client = wifi_client_or_404!(wifi);
    handle_service_result(
        client.connect(body.into_inner()).await.map(|_| ()),
        "wifi_connect",
    )
}

pub async fn wifi_disconnect(wifi: web::Data<WifiClient>) -> impl Responder {
    debug!("wifi_disconnect() called");
    let client = wifi_client_or_404!(wifi);
    handle_service_result(client.disconnect().await.map(|_| ()), "wifi_disconnect")
}

pub async fn wifi_status(wifi: web::Data<WifiClient>) -> impl Responder {
    debug!("wifi_status() called");
    let client = wifi_client_or_404!(wifi);
    match client.status().await {
        Ok(status) => HttpResponse::Ok().json(&status),
        Err(e) => {
            error!("wifi_status failed: {e:#}");
            HttpResponse::InternalServerError().body(e.to_string())
        }
    }
}

pub async fn wifi_saved_networks(wifi: web::Data<WifiClient>) -> impl Responder {
    debug!("wifi_saved_networks() called");
    let client = wifi_client_or_404!(wifi);
    match client.saved_networks().await {
        Ok(networks) => HttpResponse::Ok().json(&networks),
        Err(e) => {
            error!("wifi_saved_networks failed: {e:#}");
            HttpResponse::InternalServerError().body(e.to_string())
        }
    }
}

pub async fn wifi_forget_network(
    body: web::Json<WifiForgetRequest>,
    wifi: web::Data<WifiClient>,
) -> impl Responder {
    debug!("wifi_forget_network() called");
    let client = wifi_client_or_404!(wifi);
    handle_service_result(
        client.forget_network(body.into_inner()).await.map(|_| ()),
        "wifi_forget_network",
    )
}
