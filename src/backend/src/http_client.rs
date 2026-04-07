use actix_web::HttpResponse;
use anyhow::{Context, Result, ensure};
use log::error;
use reqwest::{Client, Response};
use std::path::Path;

/// Create a Unix socket client for local service communication
///
/// Accepts either a raw path or a URI with `unix://` scheme.
///
/// # Arguments
/// * `socket_path` - Path to the Unix socket (with or without `unix://` prefix)
///
/// # Examples
/// ```no_run
/// use omnect_ui::http_client::unix_socket_client;
///
/// // Raw path
/// let client = unix_socket_client("/socket/api.sock")
///     .expect("failed to create client");
///
/// // URI with unix:// scheme
/// let client = unix_socket_client("unix:///socket/api.sock")
///     .expect("failed to create client");
/// ```
pub fn unix_socket_client(socket_path: &str) -> Result<Client> {
    let socket_path = Path::new(socket_path.strip_prefix("unix://").unwrap_or(socket_path));

    // Verify the socket path exists
    ensure!(
        socket_path
            .try_exists()
            .context("failed to check if socket path exists")?,
        "failed since socket path does not exist: {}",
        socket_path.display()
    );

    Client::builder()
        .unix_socket(socket_path)
        .build()
        .context("failed to create Unix socket HTTP client")
}

/// Trait for converting service results into HTTP responses
pub trait ServiceResultResponse {
    fn into_response(self) -> HttpResponse;
}

impl ServiceResultResponse for () {
    fn into_response(self) -> HttpResponse {
        HttpResponse::Ok().finish()
    }
}

impl ServiceResultResponse for String {
    fn into_response(self) -> HttpResponse {
        HttpResponse::Ok().body(self)
    }
}

impl ServiceResultResponse for crate::services::network::SetNetworkConfigResponse {
    fn into_response(self) -> HttpResponse {
        match serde_json::to_string(&self) {
            Ok(json) => HttpResponse::Ok()
                .content_type("application/json")
                .body(json),
            Err(e) => {
                error!("failed to serialize SetNetworkConfigResponse: {e:#}");
                HttpResponse::InternalServerError().body("failed to serialize response")
            }
        }
    }
}

/// Handle Result and extracting convert data to Response
///
/// This is a common utility for processing Results and transform to HTTP responses.
/// It ensures the Result status is successful and and puts data or the error in a corresponding Response.
///
/// # Arguments
/// * `result` - The Result to handle
/// * `operation` - Context message describing the operation
///
/// # Returns
/// * `HttpResponse` - The `ServiceResultResponse` (`HttpResponse::Ok` or `HttpResponse::InternalServerError`)
pub fn handle_service_result<T>(result: Result<T>, operation: &str) -> HttpResponse
where
    T: ServiceResultResponse,
{
    match result {
        Ok(data) => data.into_response(),
        Err(e) => {
            error!("{operation} failed: {e:#}");
            HttpResponse::InternalServerError().body(e.to_string())
        }
    }
}

/// Handle HTTP response by checking status and extracting body
///
/// This is a common utility for processing HTTP responses.
/// It ensures the response status is successful and extracts the body text.
///
/// # Arguments
/// * `res` - The HTTP response to handle
/// * `context_msg` - Context message describing the request (e.g., "certificate request")
///
/// # Returns
/// * `Ok(String)` - The response body if the status is successful
/// * `Err` - If the status is not successful or reading the body fails
pub async fn handle_http_response(res: Response, context_msg: &str) -> Result<String> {
    let status = res.status();
    let body = res.text().await.context("failed to read response body")?;

    ensure!(
        status.is_success(),
        "{context_msg} failed with status {status} and body: {body}"
    );

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_socket_client_rejects_nonexistent_path() {
        let socket_path = "/tmp/nonexistent-test.sock";
        let result = unix_socket_client(socket_path);
        // Should fail because the socket doesn't exist
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("socket path does not exist")
        );
    }

    #[test]
    fn test_unix_socket_client_rejects_nonexistent_unix_uri() {
        let socket_path = "unix:///tmp/nonexistent-workload.sock";
        let result = unix_socket_client(socket_path);
        // Should strip unix:// prefix and then fail because socket doesn't exist
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("socket path does not exist")
        );
    }
}
