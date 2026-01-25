mod api;
mod config;
mod http_client;
mod keycloak_client;
mod middleware;
mod omnect_device_service_client;
mod services;

use crate::{
    api::Api,
    config::AppConfig,
    keycloak_client::KeycloakProvider,
    omnect_device_service_client::{DeviceServiceClient, OmnectDeviceServiceClient},
    services::{
        auth::TokenManager, certificate::CertificateService, network::NetworkConfigService,
    },
};
use actix_cors::Cors;
use actix_multipart::form::MultipartFormConfig;
use actix_server::ServerHandle;
use actix_session::{
    SessionMiddleware,
    config::{BrowserSession, CookieContentSecurity},
    storage::CookieSessionStore,
};
use actix_web::{
    App, HttpServer,
    cookie::{Key, SameSite},
    web::{self, Data},
};
use actix_web_static_files::ResourceFiles;
use anyhow::{Context, Result};
use env_logger::{Builder, Env, Target};
use log::{debug, error, info, warn};
use rustls::crypto::{CryptoProvider, ring::default_provider};
use std::io::Write;
use tokio::{
    process::{Child, Command},
    signal::unix::{SignalKind, signal},
    sync::broadcast,
};

const UPLOAD_LIMIT_BYTES: usize = 250 * 1024 * 1024;
const MEMORY_LIMIT_BYTES: usize = 10 * 1024 * 1024;

// Include the generated static files from build.rs
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

// Alias the generated function to a more descriptive name
#[inline(always)]
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
            ShutdownReason::Restart => write!(f, "restarting server"),
            ShutdownReason::Shutdown => write!(f, "shutting down"),
        }
    }
}

#[actix_web::main]
async fn main() {
    eprintln!("DEBUG: entering main");
    if let Err(e) = run().await {
        eprintln!("DEBUG: run() failed: {e:#}");
        error!("application error: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    eprintln!("DEBUG: entering run()");
    initialize()?;

    eprintln!("DEBUG: setting up restart receiver");
    let mut restart_rx = NetworkConfigService::setup_restart_receiver()
        .map_err(|_| anyhow::anyhow!("restart receiver already initialized"))?;

    eprintln!("DEBUG: setting up sigterm handler");
    let mut sigterm =
        signal(SignalKind::terminate()).context("failed to install SIGTERM handler")?;

    eprintln!("DEBUG: creating device service client");
    let mut service_client =
        OmnectDeviceServiceClient::new().context("failed to create device service client")?;

    while let ShutdownReason::Restart =
        run_until_shutdown(&mut service_client, &mut restart_rx, &mut sigterm).await?
    {}

    Ok(())
}

fn initialize() -> Result<()> {
    eprintln!("DEBUG: entering initialize()");
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

    info!(
        "module version: {} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_SHORT_REV")
    );

    eprintln!("DEBUG: installing crypto provider");
    CryptoProvider::install_default(default_provider())
        .map_err(|_| anyhow::anyhow!("crypto provider already installed"))?;

    eprintln!("DEBUG: creating frontend config file");
    KeycloakProvider::create_frontend_config_file()
        .context("failed to create frontend config file")?;

    if NetworkConfigService::rollback_exists() {
        warn!("unexpectedly started with pending network rollback");
    }

    Ok(())
}

async fn run_until_shutdown(
    service_client: &mut OmnectDeviceServiceClient,
    restart_rx: &mut broadcast::Receiver<()>,
    sigterm: &mut tokio::signal::unix::Signal,
) -> Result<ShutdownReason> {
    eprintln!("DEBUG: entering run_until_shutdown()");
    info!("starting server");

    // Get IPs to bind to
    eprintln!("DEBUG: getting online interface IPs");
    let mut bind_ips = get_online_interface_ips(service_client)
        .await
        .context("failed to get online IPs")?;

    // Always ensure localhost is bound
    if !bind_ips.contains(&"127.0.0.1".to_string()) {
        bind_ips.push("127.0.0.1".to_string());
    }

    // 1. Ensure all certificates are generated and cached
    eprintln!("DEBUG: ensuring certificates are updated for: {:?}", bind_ips);
    CertificateService::ensure_certificates_updated(service_client, &bind_ips)
        .await
        .context("failed to ensure certificates are updated")?;

    // 2. run centrifugo with valid cert
    eprintln!("DEBUG: starting centrifugo");
    let mut centrifugo = run_centrifugo().context("failed to start centrifugo")?;

    // 3. register publish endpoint with running centrifugo
    if !service_client.has_publish_endpoint {
        eprintln!("DEBUG: registering publish endpoint");
        service_client
            .register_publish_endpoint(AppConfig::get().centrifugo.publish_endpoint.clone())
            .await
            .context("failed to register publish endpoint")?;
    }

    eprintln!("DEBUG: starting server task");
    let (server_handle, server_task) = run_server(service_client.clone(), bind_ips).await?;

    if let Err(e) = NetworkConfigService::process_pending_rollback(service_client).await {
        error!("failed to process pending rollback: {e:#}");
    }

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
        result = server_task => {
            match result {
                Ok(Ok(())) => debug!("server stopped normally"),
                Ok(Err(e)) => error!("server stopped with error: {e}"),
                Err(e) => error!("server task panicked: {e}"),
            }
            ShutdownReason::Shutdown
        },
        _ = centrifugo.wait() => {
            error!("centrifugo stopped unexpectedly");
            ShutdownReason::Shutdown
        }
    };

    info!("{reason}");

    server_handle.stop(true).await;
    if let Err(e) = centrifugo.kill().await {
        error!("failed to kill centrifugo: {e:#}");
    }

    if matches!(reason, ShutdownReason::Shutdown) {
        if let Err(e) = service_client.shutdown().await {
            error!("failed to shutdown service client: {e:#}");
        }
        info!("shutdown complete");
    }

    Ok(reason)
}

async fn run_server(
    service_client: OmnectDeviceServiceClient,
    bind_ips: Vec<String>,
) -> Result<(
    ServerHandle,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
)> {
    let api = UiApi::new(service_client.clone(), Default::default())
        .await
        .context("failed to create api")?;

    let config = &AppConfig::get();
    let ui_port = config.ui.port;
    let session_key = Key::generate();
    let token_manager = TokenManager::new(&config.centrifugo.client_token);

    info!("binding web server to IPs: {:?}", bind_ips);

    let mut server = HttpServer::new(move || {
        App::new()
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .allow_any_header()
                    .allowed_methods(vec!["GET"])
                    .supports_credentials()
                    .max_age(3600),
            )
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), session_key.clone())
                    .cookie_name(String::from("omnect-ui-session"))
                    .cookie_secure(true)
                    .session_lifecycle(BrowserSession::default())
                    .cookie_same_site(SameSite::Strict)
                    .cookie_content_security(CookieContentSecurity::Private)
                    .cookie_http_only(true)
                    .build(),
            )
            .app_data(
                MultipartFormConfig::default()
                    .total_limit(UPLOAD_LIMIT_BYTES)
                    .memory_limit(MEMORY_LIMIT_BYTES),
            )
            .app_data(Data::new(token_manager.clone()))
            .app_data(Data::new(api.clone()))
            .app_data(Data::new(static_files()))
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
            .route("/network", web::post().to(UiApi::set_network_config))
            .route("/ack-rollback", web::post().to(UiApi::ack_rollback))
            .service(ResourceFiles::new("/static", static_files()))
            .default_service(web::route().to(UiApi::index))
    });

    let mut bound_listeners = Vec::new();
    for ip in bind_ips {
        match std::net::TcpListener::bind((ip.as_str(), ui_port)) {
            Ok(listener) => match build_tls_config_for_ip(&ip) {
                Ok(tls_config) => {
                    info!("successfully bound tcp listener to {ip}:{ui_port}");
                    bound_listeners.push((listener, tls_config));
                }
                Err(e) => {
                    warn!("failed to build tls config for {ip}: {e}");
                }
            },
            Err(e) => {
                warn!("failed to bind tcp listener to {ip}:{ui_port}: {e}");
            }
        }
    }

    if bound_listeners.is_empty() {
        anyhow::bail!("failed to bind to any IP addresses");
    }

    for (listener, tls_config) in bound_listeners {
        server = server
            .listen_rustls_0_23(listener, tls_config)
            .context("failed to attach tcp listener to server")?;
    }

    let server = server.disable_signals().run();

    Ok((server.handle(), tokio::spawn(server)))
}

fn run_centrifugo() -> Result<Child> {
    let config = &AppConfig::get().centrifugo;
    let certificate = &AppConfig::get().certificate;

    let centrifugo = Command::new(&config.binary_path)
        .arg("-c")
        .arg(&config.config_path)
        .envs(vec![
            (
                "CENTRIFUGO_HTTP_SERVER_TLS_CERT_PEM",
                certificate.cert_path.to_string_lossy().to_string(),
            ),
            (
                "CENTRIFUGO_HTTP_SERVER_TLS_KEY_PEM",
                certificate.key_path.to_string_lossy().to_string(),
            ),
            ("CENTRIFUGO_HTTP_SERVER_PORT", config.port.clone()),
            (
                "CENTRIFUGO_CLIENT_TOKEN_HMAC_SECRET_KEY",
                config.client_token.clone(),
            ),
            ("CENTRIFUGO_HTTP_API_KEY", config.api_key.clone()),
            ("CENTRIFUGO_LOG_LEVEL", config.log_level.clone()),
        ])
        .spawn()
        .context("failed to spawn centrifugo process")?;

    info!(
        "centrifugo pid: {}",
        centrifugo
            .id()
            .context("failed to get centrifugo process id")?
    );

    Ok(centrifugo)
}

fn build_tls_config_for_ip(ip: &str) -> Result<rustls::ServerConfig> {
    // Create the SNI resolver with the IP as fallback
    let resolver = CertificateService::create_sni_resolver(ip.to_string());

    // Build the TLS config with the SNI resolver
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);

    Ok(config)
}

/// Get all IP addresses from online network interfaces
async fn get_online_interface_ips(
    service_client: &OmnectDeviceServiceClient,
) -> Result<Vec<String>> {
    let status = service_client.status().await?;

    Ok(status
        .network_status
        .network_interfaces
        .iter()
        .filter(|iface| iface.online)
        .flat_map(|iface| iface.ipv4.addrs.iter().map(|addr| addr.addr.clone()))
        .collect())
}
