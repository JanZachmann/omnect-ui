mod api;
mod config;
mod http_client;
mod keycloak_client;
mod middleware;
mod omnect_device_service_client;
mod services;
mod wifi_commissioning_client;

use crate::{
    api::Api,
    config::AppConfig,
    keycloak_client::KeycloakProvider,
    omnect_device_service_client::{DeviceServiceClient, OmnectDeviceServiceClient},
    services::{
        auth::{SessionKeyService, TokenManager},
        certificate::{CertificateService, CreateCertPayload},
        network::NetworkConfigService,
    },
    wifi_commissioning_client::{WifiAvailability, WifiCommissioningServiceClient},
};
use actix_multipart::form::MultipartFormConfig;
use actix_server::ServerHandle;
use actix_session::{
    SessionMiddleware,
    config::{BrowserSession, CookieContentSecurity},
    storage::CookieSessionStore,
};
use actix_web::{
    App, HttpServer,
    cookie::SameSite,
    web::{self, Data},
};
use actix_web_static_files::ResourceFiles;
use anyhow::{Context, Result};
use env_logger::{Builder, Env, Target};
use log::{debug, error, info, warn};
use rustls::crypto::{CryptoProvider, ring::default_provider};
use std::{io::Write, sync::Mutex};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::broadcast,
};
use uuid::Uuid;

const UPLOAD_LIMIT_BYTES: usize = 1024 * 1024 * 1024;
const MULTIPART_CHUNK_SIZE_BYTES: usize = 512 * 1024;

// Cached common name (IP address) used for the current certificate
static CACHED_COMMON_NAME: Mutex<Option<String>> = Mutex::new(None);

// Include the generated static files from build.rs
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

// Alias the generated function to a more descriptive name
#[inline]
fn static_files() -> std::collections::HashMap<&'static str, static_files::Resource> {
    generate()
}

type UiApi = Api<OmnectDeviceServiceClient, KeycloakProvider>;

enum ShutdownReason {
    Restart,
    Shutdown,
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Restart => write!(f, "restarting server"),
            Self::Shutdown => write!(f, "shutting down"),
        }
    }
}

#[actix_web::main]
async fn main() {
    if let Err(e) = run().await {
        error!("application error: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    initialize()?;

    #[allow(clippy::map_err_ignore)] // original error is () — no info to preserve
    let mut restart_rx = NetworkConfigService::setup_restart_receiver()
        .map_err(|_| anyhow::anyhow!("restart receiver already initialized"))?;

    let mut sigterm =
        signal(SignalKind::terminate()).context("failed to install SIGTERM handler")?;

    let mut service_client =
        OmnectDeviceServiceClient::new().context("failed to create device service client")?;

    while matches!(
        run_until_shutdown(&mut service_client, &mut restart_rx, &mut sigterm).await?,
        ShutdownReason::Restart
    ) {}

    Ok(())
}

fn initialize() -> Result<()> {
    log_panics::init();

    let mut builder = if cfg!(debug_assertions) {
        Builder::from_env(Env::default().default_filter_or("debug"))
    } else {
        Builder::from_env(Env::default().default_filter_or("info"))
    };

    builder.format(|f, record| match record.level() {
        log::Level::Error => {
            eprintln!("{}", record.args());
            Ok(())
        }
        _ => {
            writeln!(f, "{}", record.args())
        }
    });

    builder.target(Target::Stdout).init();

    info!("module version: {}", env!("CARGO_PKG_VERSION"));

    #[allow(clippy::map_err_ignore)]
    // original error is the rejected CryptoProvider value — not useful
    CryptoProvider::install_default(default_provider())
        .map_err(|_| anyhow::anyhow!("crypto provider already installed"))?;

    KeycloakProvider::create_frontend_config_file()
        .context("failed to create frontend config file")?;

    if NetworkConfigService::rollback_exists() {
        warn!("unexpectedly started with pending network rollback");
    }

    Ok(())
}

async fn needs_certificate_recreation(
    service_client: &OmnectDeviceServiceClient,
) -> Result<Option<String>> {
    // Check if we have a cached common name
    let cached = CACHED_COMMON_NAME.lock().unwrap().clone();

    // Get all current IP addresses from network interfaces
    let status = service_client.status().await?;
    let all_ips: Vec<String> = status
        .network_status
        .network_interfaces
        .iter()
        .filter(|iface| iface.online)
        .flat_map(|iface| iface.ipv4.addrs.iter().map(|addr| addr.addr.clone()))
        .collect();

    if let Some(cached_ip) = cached {
        // Certificate needs recreation if cached IP is not in current IP list
        if all_ips.contains(&cached_ip) {
            // Certificate still valid
            Ok(None)
        } else {
            // Return the first IP as the new common name
            Ok(Some(
                all_ips
                    .first()
                    .cloned()
                    .context("failed to get IP address from status")?,
            ))
        }
    } else {
        // No cached IP, need to create certificate
        Ok(Some(
            all_ips
                .first()
                .cloned()
                .context("failed to get IP address from status")?,
        ))
    }
}

async fn run_until_shutdown(
    service_client: &mut OmnectDeviceServiceClient,
    restart_rx: &mut broadcast::Receiver<()>,
    sigterm: &mut tokio::signal::unix::Signal,
) -> Result<ShutdownReason> {
    info!("starting server");

    // 1. create the cert with the ip in CommonName (only if IP changed)
    if let Some(current_ip) = needs_certificate_recreation(service_client).await? {
        info!("creating new certificate for IP: {current_ip}");
        CertificateService::create_module_certificate(CreateCertPayload {
            common_name: current_ip.clone(),
        })
        .await
        .context("failed to create certificate")?;

        // Update cached common name
        *CACHED_COMMON_NAME.lock().unwrap() = Some(current_ip);
    } else {
        info!("certificate still valid, skipping recreation");
    }

    let (ws_tx, _) = broadcast::channel(100);

    // 2. start both servers (HTTPS for UI, HTTP for internal ODS publish)
    let (ui_handle, internal_handle, ui_task, internal_task) =
        run_server(service_client.clone(), ws_tx).await?;

    // 3. register publish endpoint with ods (after server is listening)
    if !service_client.has_publish_endpoint {
        // Wait a tiny bit for the sockets to be fully ready
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        service_client
            .register_publish_endpoint(AppConfig::get().publish.endpoint.clone())
            .await
            .context("failed to register publish endpoint")?;
    }

    let service_client_clone = service_client.clone();
    let rollback_task = tokio::spawn(async move {
        if let Err(e) = NetworkConfigService::process_pending_rollback(service_client_clone).await {
            error!("failed to process pending rollback: {e:#}");
        }
    });

    let reason = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            debug!("ctrl-c received");
            ShutdownReason::Shutdown
        },
        _ = sigterm.recv() => {
            debug!("SIGTERM received");
            ShutdownReason::Shutdown
        },
        _ = restart_rx.recv() => {
            debug!("server restart requested");
            ShutdownReason::Restart
        },
        result = ui_task => {
            match result {
                Ok(Ok(())) => debug!("UI server stopped normally"),
                Ok(Err(e)) => error!("UI server stopped with error: {e}"),
                Err(e) => error!("UI server task panicked: {e}"),
            }
            ShutdownReason::Shutdown
        },
        result = internal_task => {
            match result {
                Ok(Ok(())) => debug!("internal server stopped normally"),
                Ok(Err(e)) => error!("internal server stopped with error: {e}"),
                Err(e) => error!("internal server task panicked: {e}"),
            }
            ShutdownReason::Shutdown
        }
    };

    rollback_task.abort();
    info!("{reason}");

    ui_handle.stop(true).await;
    internal_handle.stop(true).await;

    if matches!(reason, ShutdownReason::Shutdown) {
        if let Err(e) = service_client.shutdown().await {
            error!("failed to shutdown service client: {e:#}");
        }
        info!("shutdown complete");
    }

    Ok(reason)
}

fn optimal_worker_count() -> usize {
    const MIN_WORKERS: usize = 2;
    const MAX_WORKERS: usize = 4;

    let cpu_count = std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(2);

    // For I/O-bound workloads, use fewer workers than CPUs
    let workers = (cpu_count / 2).clamp(MIN_WORKERS, MAX_WORKERS);

    info!("configuring {workers} worker threads (detected {cpu_count} CPUs)");
    workers
}

async fn initialize_wifi_client() -> (Option<WifiCommissioningServiceClient>, WifiAvailability) {
    let unavailable = WifiAvailability::Unavailable {
        socket_present: false,
        version: None,
        min_required_version: WifiCommissioningServiceClient::MIN_REQUIRED_VERSION.to_string(),
    };

    let config = &AppConfig::get().wifi;

    let Some(client) = WifiCommissioningServiceClient::try_new(&config.socket_path) else {
        return (None, unavailable);
    };

    let availability = client.check_availability().await;

    if matches!(availability, WifiAvailability::Available { .. }) {
        (Some(client), availability)
    } else {
        (None, availability)
    }
}

async fn run_server(
    service_client: OmnectDeviceServiceClient,
    ws_tx: broadcast::Sender<String>,
) -> Result<(
    ServerHandle,
    ServerHandle,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
)> {
    let api = UiApi::new(service_client.clone(), KeycloakProvider)
        .await
        .context("failed to create api")?;

    let (wifi_client, wifi_availability) = initialize_wifi_client().await;
    let wifi_data: Data<Option<WifiCommissioningServiceClient>> = Data::new(wifi_client);
    let wifi_availability_data = Data::new(wifi_availability);

    let tls_config = load_tls_config().context("failed to load tls config")?;
    let config = &AppConfig::get();
    let ui_port = config.ui.port;
    let publish_url =
        url::Url::parse(&config.publish.endpoint.url).context("failed to parse publish url")?;
    let publish_port = publish_url
        .port()
        .context("failed to get port from publish url")?;
    let session_key = SessionKeyService::load_or_generate(&config.paths.session_key_path);
    // TokenManager generates the JWT tokens for the UI to use in API requests (via Bearer token or session).
    let token_manager = TokenManager::new(&Uuid::new_v4().to_string());

    let ws_tx_data = Data::new(ws_tx);

    // Internal HTTP server — serves only the API-key-protected ODS publish endpoint.
    // Isolated from the HTTPS server to prevent exposing UI routes on unencrypted HTTP.
    let internal_ws_tx = ws_tx_data.clone();
    let internal_server = HttpServer::new(move || {
        App::new().app_data(internal_ws_tx.clone()).route(
            "/api/internal/publish",
            web::post().to(services::websocket::internal_publish),
        )
    })
    .workers(1)
    .bind(format!("[::]:{publish_port}"))
    .context("failed to bind internal HTTP server")?
    .disable_signals()
    .run();

    let internal_handle = internal_server.handle();
    let internal_task = tokio::spawn(internal_server);

    // Main HTTPS server — serves UI, API, and WebSocket routes
    let ui_server = HttpServer::new(move || {
        App::new()
            .app_data(
                MultipartFormConfig::default()
                    .total_limit(UPLOAD_LIMIT_BYTES)
                    .memory_limit(MULTIPART_CHUNK_SIZE_BYTES),
            )
            .app_data(web::PayloadConfig::new(UPLOAD_LIMIT_BYTES))
            .app_data(web::JsonConfig::default().limit(UPLOAD_LIMIT_BYTES))
            .app_data(Data::new(token_manager.clone()))
            .app_data(ws_tx_data.clone())
            .app_data(Data::new(api.clone()))
            .app_data(Data::new(static_files()))
            .app_data(wifi_data.clone())
            .app_data(wifi_availability_data.clone())
            .service(ResourceFiles::new("/static", static_files()))
            .service(
                web::scope("")
                    .wrap(
                        SessionMiddleware::builder(
                            CookieSessionStore::default(),
                            session_key.clone(),
                        )
                        .cookie_name(String::from("omnect-ui-session"))
                        .cookie_secure(true)
                        .session_lifecycle(BrowserSession::default())
                        .cookie_same_site(SameSite::Strict)
                        .cookie_content_security(CookieContentSecurity::Private)
                        .cookie_http_only(true)
                        .build(),
                    )
                    .route(
                        "/ws",
                        web::get()
                            .to(services::websocket::ws_route)
                            .wrap(middleware::AuthMw),
                    )
                    .route("/", web::get().to(UiApi::index))
                    .route("/config.js", web::get().to(UiApi::config))
                    .route(
                        "/factory-reset",
                        web::post()
                            .to(UiApi::factory_reset)
                            .wrap(middleware::AuthMw),
                    )
                    .route(
                        "/reboot",
                        web::post().to(UiApi::reboot).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/update/file",
                        web::post()
                            .to(UiApi::upload_firmware_file)
                            .wrap(middleware::AuthMw),
                    )
                    .route(
                        "/update/load",
                        web::post().to(UiApi::load_update).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/update/run",
                        web::post().to(UiApi::run_update).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/token/login",
                        web::post().to(UiApi::token).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/token/refresh",
                        web::get().to(UiApi::token).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/token/validate",
                        web::post().to(UiApi::validate_portal_token),
                    )
                    .route(
                        "/require-set-password",
                        web::get().to(UiApi::require_set_password),
                    )
                    .route("/set-password", web::post().to(UiApi::set_password))
                    .route("/update-password", web::post().to(UiApi::update_password))
                    .route("/version", web::get().to(UiApi::version))
                    .route("/logout", web::post().to(UiApi::logout))
                    .route("/healthcheck", web::get().to(UiApi::healthcheck))
                    .route(
                        "/republish",
                        web::post().to(UiApi::republish).wrap(middleware::AuthMw),
                    )
                    .route("/api/settings", web::get().to(UiApi::get_settings))
                    .route(
                        "/api/settings",
                        web::post()
                            .to(UiApi::update_settings)
                            .wrap(middleware::AuthMw),
                    )
                    .route(
                        "/network",
                        web::post()
                            .to(UiApi::set_network_config)
                            .wrap(middleware::AuthMw),
                    )
                    .route(
                        "/ack-rollback",
                        web::post().to(UiApi::ack_rollback).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/ack-factory-reset-result",
                        web::post()
                            .to(UiApi::ack_factory_reset_result)
                            .wrap(middleware::AuthMw),
                    )
                    .route(
                        "/ack-update-validation",
                        web::post()
                            .to(UiApi::ack_update_validation)
                            .wrap(middleware::AuthMw),
                    )
                    // WiFi management routes
                    .route("/wifi/available", web::get().to(api::wifi_available))
                    .route(
                        "/wifi/scan",
                        web::post().to(api::wifi_scan).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/wifi/scan/results",
                        web::get()
                            .to(api::wifi_scan_results)
                            .wrap(middleware::AuthMw),
                    )
                    .route(
                        "/wifi/connect",
                        web::post().to(api::wifi_connect).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/wifi/disconnect",
                        web::post()
                            .to(api::wifi_disconnect)
                            .wrap(middleware::AuthMw),
                    )
                    .route(
                        "/wifi/status",
                        web::get().to(api::wifi_status).wrap(middleware::AuthMw),
                    )
                    .route(
                        "/wifi/networks",
                        web::get()
                            .to(api::wifi_saved_networks)
                            .wrap(middleware::AuthMw),
                    )
                    .route(
                        "/wifi/networks/forget",
                        web::post()
                            .to(api::wifi_forget_network)
                            .wrap(middleware::AuthMw),
                    ),
            )
            .default_service(web::route().to(UiApi::index))
    })
    .workers(optimal_worker_count())
    .bind_rustls_0_23(format!("0.0.0.0:{ui_port}"), tls_config)
    .context("failed to bind HTTPS server")?
    .disable_signals()
    .run();

    let ui_handle = ui_server.handle();
    let ui_task = tokio::spawn(ui_server);

    Ok((ui_handle, internal_handle, ui_task, internal_task))
}

fn load_tls_config() -> Result<rustls::ServerConfig> {
    let paths = &AppConfig::get().certificate;

    let mut tls_certs = std::io::BufReader::new(
        std::fs::File::open(&paths.cert_path).context("failed to open certificate file")?,
    );

    let mut tls_key = std::io::BufReader::new(
        std::fs::File::open(&paths.key_path).context("failed to open key file")?,
    );

    let tls_certs = rustls_pemfile::certs(&mut tls_certs)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse certificate pem")?;

    let key_item = rustls_pemfile::read_one(&mut tls_key)
        .context("failed to read key pem file")?
        .context("no valid key found in pem file")?;

    let config = match key_item {
        rustls_pemfile::Item::Pkcs1Key(key) => rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(tls_certs, rustls::pki_types::PrivateKeyDer::Pkcs1(key))
            .context("failed to create tls config with pkcs1 key")?,
        rustls_pemfile::Item::Pkcs8Key(key) => rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(tls_certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key))
            .context("failed to create tls config with pkcs8 key")?,
        _ => anyhow::bail!("unexpected key type in pem file"),
    };

    Ok(config)
}

#[cfg(test)]
mod tests {
    use crate::{middleware::AuthMw, services::auth::TokenManager};
    use actix_http::StatusCode;
    use actix_session::{
        SessionMiddleware,
        config::{BrowserSession, CookieContentSecurity},
        storage::CookieSessionStore,
    };
    use actix_web::{App, HttpResponse, cookie::SameSite, dev::ServiceResponse, test, web};

    const SESSION_SECRET: [u8; 64] = [
        0xb2, 0x64, 0x83, 0x0, 0xf5, 0xcb, 0xf6, 0x1d, 0x5c, 0x83, 0xc0, 0x90, 0x6b, 0xb2, 0xe4,
        0x26, 0x14, 0x9, 0x2b, 0xa1, 0xc4, 0xc5, 0x37, 0xe7, 0xc9, 0x20, 0x8e, 0xbc, 0xee, 0x2,
        0x3c, 0xa2, 0x32, 0x57, 0x96, 0xc9, 0x99, 0x62, 0x90, 0x4f, 0x24, 0xe5, 0x25, 0x6b, 0xe1,
        0x2b, 0x8a, 0x3, 0xa3, 0xc7, 0x1e, 0xb2, 0xb2, 0xbe, 0x29, 0x51, 0xc1, 0xe2, 0x1e, 0xb7,
        0x8, 0x15, 0xc9, 0xe0,
    ];

    const TOKEN_SECRET: &str = "test-secret-key!";

    async fn ok_handler() -> HttpResponse {
        HttpResponse::Ok().finish()
    }

    /// Build a test app mirroring production route + middleware layout.
    /// Uses stub handlers — we're testing auth enforcement, not handler logic.
    async fn create_route_auth_service() -> impl actix_service::Service<
        actix_http::Request,
        Response = ServiceResponse,
        Error = actix_web::Error,
    > {
        let key = actix_web::cookie::Key::from(&SESSION_SECRET);
        let session_mw = SessionMiddleware::builder(CookieSessionStore::default(), key)
            .cookie_name(String::from("omnect-ui-session"))
            .cookie_secure(true)
            .session_lifecycle(BrowserSession::default())
            .cookie_same_site(SameSite::Strict)
            .cookie_content_security(CookieContentSecurity::Private)
            .cookie_http_only(true)
            .build();

        let token_manager = TokenManager::new(TOKEN_SECRET);

        test::init_service(
            App::new().app_data(web::Data::new(token_manager)).service(
                web::scope("")
                    .wrap(session_mw)
                    // Protected routes
                    .route("/ws", web::get().to(ok_handler).wrap(AuthMw))
                    .route("/network", web::post().to(ok_handler).wrap(AuthMw))
                    .route("/republish", web::post().to(ok_handler).wrap(AuthMw))
                    .route("/ack-rollback", web::post().to(ok_handler).wrap(AuthMw))
                    .route(
                        "/ack-factory-reset-result",
                        web::post().to(ok_handler).wrap(AuthMw),
                    )
                    .route(
                        "/ack-update-validation",
                        web::post().to(ok_handler).wrap(AuthMw),
                    )
                    .route("/api/settings", web::get().to(ok_handler))
                    .route("/api/settings", web::post().to(ok_handler).wrap(AuthMw))
                    .route("/factory-reset", web::post().to(ok_handler).wrap(AuthMw))
                    .route("/reboot", web::post().to(ok_handler).wrap(AuthMw))
                    // Public routes
                    .route("/version", web::get().to(ok_handler))
                    .route("/healthcheck", web::get().to(ok_handler))
                    .route("/require-set-password", web::get().to(ok_handler)),
            ),
        )
        .await
    }

    #[tokio::test]
    async fn protected_routes_require_auth() {
        let app = create_route_auth_service().await;

        let protected = vec![
            ("GET", "/ws"),
            ("POST", "/network"),
            ("POST", "/republish"),
            ("POST", "/ack-rollback"),
            ("POST", "/ack-factory-reset-result"),
            ("POST", "/ack-update-validation"),
            ("POST", "/api/settings"),
            ("POST", "/factory-reset"),
            ("POST", "/reboot"),
        ];

        for (method, path) in protected {
            let req = match method {
                "GET" => test::TestRequest::get().uri(path).to_request(),
                "POST" => test::TestRequest::post().uri(path).to_request(),
                _ => unreachable!(),
            };
            let resp = test::call_service(&app, req).await;
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {path} should require authentication"
            );
        }
    }

    #[tokio::test]
    async fn public_routes_dont_require_auth() {
        let app = create_route_auth_service().await;

        let public = vec![
            ("GET", "/version"),
            ("GET", "/healthcheck"),
            ("GET", "/require-set-password"),
            ("GET", "/api/settings"),
        ];

        for (method, path) in public {
            let req = match method {
                "GET" => test::TestRequest::get().uri(path).to_request(),
                "POST" => test::TestRequest::post().uri(path).to_request(),
                _ => unreachable!(),
            };
            let resp = test::call_service(&app, req).await;
            assert!(
                resp.status().is_success(),
                "{method} {path} should be publicly accessible, got {}",
                resp.status()
            );
        }
    }

    #[tokio::test]
    async fn protected_routes_succeed_with_valid_bearer() {
        let app = create_route_auth_service().await;
        let token_manager = TokenManager::new(TOKEN_SECRET);
        let token = token_manager.create_token().unwrap();

        let protected = vec![("GET", "/ws"), ("POST", "/network"), ("POST", "/republish")];

        for (method, path) in protected {
            let req_builder = match method {
                "GET" => test::TestRequest::get().uri(path),
                "POST" => test::TestRequest::post().uri(path),
                _ => unreachable!(),
            };
            let req = req_builder
                .insert_header(("Authorization", format!("Bearer {token}")))
                .to_request();
            let resp = test::call_service(&app, req).await;
            assert!(
                resp.status().is_success(),
                "{method} {path} should succeed with valid bearer, got {}",
                resp.status()
            );
        }
    }
}
