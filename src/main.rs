use actix_files::{Files, NamedFile};
use actix_web::{http::StatusCode, web, App, HttpResponse, HttpServer, Responder};
use actix_web_httpauth::extractors::{basic::BasicAuth, bearer::BearerAuth};
use anyhow::{Context, Result};
use env_logger::{Builder, Env, Target};
use http_body_util::{BodyExt, Empty};
use hyper::{
    Request,
    {body::Bytes, client::conn::http1},
};
use hyper_util::rt::TokioIo;
use jwt_simple::prelude::*;
use log::{debug, error, info};
use std::io::Write;
use tokio::{net::UnixStream, process::Command};

const TOKEN_EXPIRE_HOURES: u64 = 2;

#[actix_web::main]
async fn main() {
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

    let ui_port = std::env::var("UI_PORT")
        .expect("UI_PORT missing")
        .parse::<u64>()
        .expect("UI_PORT format");

    let device_cert_path = std::env::var("SSL_CERT_PATH").expect("SSL_CERT_PATH missing");
    let device_key_path = std::env::var("SSL_KEY_PATH").expect("SSL_KEY_PATH missing");

    info!("device cert file: {device_cert_path}");
    info!("device key file: {device_key_path}");

    let mut tls_certs =
        std::io::BufReader::new(std::fs::File::open(device_cert_path).expect("read certs_file"));
    let mut tls_key =
        std::io::BufReader::new(std::fs::File::open(device_key_path).expect("read key_file"));

    let tls_certs = rustls_pemfile::certs(&mut tls_certs)
        .collect::<Result<Vec<_>, _>>()
        .expect("failed to parse cert pem");

    // set up TLS config options
    let tls_config = match rustls_pemfile::read_one(&mut tls_key)
        .expect("cannot read key pem file")
        .expect("nothing found in key pem file")
    {
        rustls_pemfile::Item::Pkcs1Key(key) => rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(tls_certs, rustls::pki_types::PrivateKeyDer::Pkcs1(key))
            .expect("invalid tls config"),
        rustls_pemfile::Item::Pkcs8Key(key) => rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(tls_certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key))
            .expect("invalid tls config"),
        _ => panic!("unexpected item found in key pem file"),
    };

    let server = HttpServer::new(move || {
        App::new()
            .route("/", web::get().to(index))
            .route("/factory-reset", web::post().to(factory_reset))
            .route("/reboot", web::post().to(reboot))
            .route("/reload-network", web::post().to(reload_network))
            .route("/token/login", web::post().to(login_token))
            .route("/token/refresh", web::get().to(refresh_token))
            .service(
                Files::new(
                    "/static",
                    std::fs::canonicalize("static").expect("static folder not found"),
                )
            )
    })
    .bind_rustls_0_22(format!("0.0.0.0:{ui_port}"), tls_config)
    .expect("bind_rustls")
    .disable_signals()
    .run();

    let server_handle = server.handle();
    let server_task = tokio::spawn(server);

    let mut centrifugo =
        Command::new(std::fs::canonicalize("centrifugo").expect("centrifugo not found"))
            .spawn()
            .expect("Failed to spawn child process");

    debug!("centrifugo pid: {}", centrifugo.id().unwrap());

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            debug!("ctrl-c");
            server_handle.stop(true).await;
        },
        _ = server_task => {
            debug!("server stopped");
            centrifugo.kill().await.expect("kill centrifugo failed");
            debug!("centrifugo killed");
        },
        _ = centrifugo.wait() => {
            debug!("centrifugo stopped");
            server_handle.stop(true).await;
            debug!("server stopped");
        }
    }

    debug!("good bye");
}

async fn index() -> actix_web::Result<NamedFile> {
    debug!("index() called");

    // trigger omnect-device-service to republish
    match post("/republish/v1", None).await {
        Ok(response) => response,
        Err(e) => {
            error!("republish failed: {e:#}");
            return Err(actix_web::error::ErrorInternalServerError(
                "republish failed",
            ));
        }
    };

    Ok(NamedFile::open(
        std::fs::canonicalize("static/index.html").expect("static/index.html not found"),
    )?)
}

async fn login_token(auth: BasicAuth) -> impl Responder {
    debug!("login_token() called");

    match verify_user(auth) {
        Ok(true) => token(),
        Ok(false) => {
            error!("login_token verify false");
            HttpResponse::build(StatusCode::UNAUTHORIZED).finish()
        }
        Err(e) => {
            error!("login_token: {e:#}");
            HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).finish()
        }
    }
}

async fn refresh_token(auth: BearerAuth) -> impl Responder {
    debug!("refresh_token() called");

    match verify_token(auth) {
        Ok(true) => token(),
        Ok(false) => {
            error!("refresh_token verify false");
            HttpResponse::build(StatusCode::UNAUTHORIZED).finish()
        }
        Err(e) => {
            error!("refresh_token: {e:#}");
            HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).finish()
        }
    }
}

async fn factory_reset(auth: BearerAuth) -> impl Responder {
    debug!("factory_reset() called");

    match post("/factory-reset/v1", Some(auth)).await {
        Ok(response) => response,
        Err(e) => {
            error!("factory_reset failed: {e:#}");
            HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).finish()
        }
    }
}

async fn reboot(auth: BearerAuth) -> impl Responder {
    debug!("reboot() called");

    match post("/reboot/v1", Some(auth)).await {
        Ok(response) => response,
        Err(e) => {
            error!("reboot failed: {e:#}");
            HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).finish()
        }
    }
}

async fn reload_network(auth: BearerAuth) -> impl Responder {
    debug!("reload_network() called");

    match post("/reload-network/v1", Some(auth)).await {
        Ok(response) => response,
        Err(e) => {
            error!("reload-network failed: {e:#}");
            HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).finish()
        }
    }
}

async fn post(path: &str, auth: Option<BearerAuth>) -> Result<HttpResponse> {
    if let Some(auth) = auth {
        if !verify_token(auth)? {
            error!("post {path} verify false");
            return Ok(HttpResponse::build(StatusCode::UNAUTHORIZED).finish());
        }
    }

    let mut sender = match sender().await {
        Err(e) => {
            error!("error creating request sender: {e}. socket might be broken. exit application");
            std::process::exit(1)
        }
        Ok(sender) => sender,
    };

    let request = Request::builder()
        .uri(path)
        .method("POST")
        .header("Host", "localhost")
        .body(Empty::<Bytes>::new())
        .context("build request failed")?;

    let res = sender
        .send_request(request)
        .await
        .context("send request failed")?;

    let status_code =
        StatusCode::from_u16(res.status().as_u16()).context("get status code failed")?;

    let body = res
        .collect()
        .await
        .context("collect response body failed")?;

    let body = String::from_utf8(body.to_bytes().to_vec()).context("get response body failed")?;

    Ok(HttpResponse::build(status_code).body(body))
}

async fn sender() -> Result<http1::SendRequest<Empty<Bytes>>> {
    let stream = UnixStream::connect(std::env::var("SOCKET_PATH").expect("SOCKET_PATH missing"))
        .await
        .context("cannot create unix stream")?;

    let (mut sender, conn) = http1::handshake(TokioIo::new(stream))
        .await
        .context("unix stream handshake failed")?;

    actix_rt::spawn(async move {
        if let Err(err) = conn.await {
            error!("post connection failed: {:?}", err);
        }
    });

    sender
        .ready()
        .await
        .context("unix stream unexpectedly closed")?;

    Ok(sender)
}

fn token() -> HttpResponse {
    if let Ok(key) = std::env::var("CENTRIFUGO_TOKEN_HMAC_SECRET_KEY") {
        let key = HS256Key::from_bytes(key.as_bytes());
        let claims =
            Claims::create(Duration::from_hours(TOKEN_EXPIRE_HOURES)).with_subject("omnect-ui");

        if let Ok(token) = key.authenticate(claims) {
            return HttpResponse::Ok().body(token);
        } else {
            error!("token: cannot create token");
        };
    } else {
        error!("token: missing secret key");
    };

    HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).finish()
}

fn verify_token(auth: BearerAuth) -> Result<bool> {
    let key = std::env::var("CENTRIFUGO_TOKEN_HMAC_SECRET_KEY").context("missing jwt secret")?;
    let key = HS256Key::from_bytes(key.as_bytes());
    let options = VerificationOptions {
        accept_future: true,
        time_tolerance: Some(Duration::from_mins(15)),
        max_validity: Some(Duration::from_hours(TOKEN_EXPIRE_HOURES)),
        required_subject: Some("omnect-ui".to_string()),
        ..Default::default()
    };

    Ok(key
        .verify_token::<NoCustomClaims>(auth.token(), Some(options))
        .is_ok())
}

fn verify_user(auth: BasicAuth) -> Result<bool> {
    let user = std::env::var("LOGIN_USER").context("login_token: missing user")?;
    let password = std::env::var("LOGIN_PASSWORD").context("login_token: missing password")?;
    Ok(auth.user_id() == user && auth.password() == Some(&password))
}
