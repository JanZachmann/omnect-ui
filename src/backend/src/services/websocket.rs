use crate::config::AppConfig;
use actix_web::{HttpResponse, Responder, web};
use actix_ws::Message;
use anyhow::Result;
use futures_util::StreamExt;
use log::{debug, error, warn};
use omnect_ui_core::types::WebSocketChannel;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

#[derive(Deserialize, Serialize, Debug)]
pub struct PublishPayload {
    pub channel: WebSocketChannel,
    pub data: Value,
}

#[allow(clippy::future_not_send, clippy::unused_async)]
pub async fn ws_route(
    req: actix_web::HttpRequest,
    stream: web::Payload,
    tx: web::Data<broadcast::Sender<String>>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    let peer = req.peer_addr();
    debug!("WebSocket connection attempt from {peer:?}");

    let (response, mut session, mut msg_stream) = actix_ws::handle(&req, stream)?;

    let mut rx = tx.subscribe();

    actix_web::rt::spawn(async move {
        debug!("WebSocket session started for {peer:?}");
        loop {
            tokio::select! {
                res = msg_stream.next() => {
                    match res {
                        Some(Ok(Message::Ping(bytes))) => {
                            if session.pong(&bytes).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(reason))) => {
                            debug!("WebSocket closed by client {peer:?}: {reason:?}");
                            let _ = session.close(reason).await;
                            break;
                        }
                        Some(Ok(_)) => {} // ignore text/binary
                        Some(Err(e)) => {
                            error!("WebSocket protocol error for {peer:?}: {e}");
                            break;
                        }
                        None => {
                            debug!("WebSocket stream ended for {peer:?}");
                            break;
                        }
                    }
                }

                res = rx.recv() => {
                    match res {
                        Ok(msg) => {
                            debug!("Forwarding broadcast message to {peer:?}: {msg}");
                            if session.text(msg).await.is_err() {
                                debug!("Failed to send message to {peer:?}, closing");
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("WebSocket receiver for {peer:?} lagged by {n} messages");
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            debug!("Broadcast channel closed, ending WebSocket for {peer:?}");
                            break;
                        }
                    }
                }
            }
        }
        debug!("WebSocket session ended for {peer:?}");
    });

    Ok(response)
}

#[allow(clippy::future_not_send, clippy::unused_async)]
pub async fn internal_publish(
    req: actix_web::HttpRequest,
    body: web::Bytes,
    tx: web::Data<broadcast::Sender<String>>,
) -> impl Responder {
    let config = AppConfig::get();
    let api_key = req.headers().get("X-API-Key").and_then(|h| h.to_str().ok());

    debug!("internal_publish called from {:?}", req.peer_addr());

    if api_key != Some(&config.publish.api_key) {
        warn!("Unauthorized publish attempt from {:?}", req.peer_addr());
        return HttpResponse::Unauthorized().finish();
    }

    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(e) => {
            error!("Invalid UTF-8 in publish body: {e}");
            return HttpResponse::BadRequest().finish();
        }
    };

    let json_value: Value = match serde_json::from_str(body_str) {
        Ok(v) => v,
        Err(e) => {
            error!("Invalid JSON in publish body: {e}. Body: {body_str}");
            return HttpResponse::BadRequest().finish();
        }
    };

    let payload = match serde_json::from_value::<PublishPayload>(json_value) {
        Ok(p) => p,
        Err(e) => {
            warn!("Invalid publish payload structure: {e}. Body: {body_str}");
            return HttpResponse::BadRequest().finish();
        }
    };

    if let Ok(json_str) = serde_json::to_string(&payload) {
        debug!("Broadcasting payload for channel: {:?}", payload.channel);
        let _ = tx.send(json_str);
    }

    HttpResponse::Ok().finish()
}
