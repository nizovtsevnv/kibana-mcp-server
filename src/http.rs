use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use tracing::info;

use crate::kibana::{AuthMethod, KibanaClient};

struct Session {
    client: Arc<KibanaClient>,
}

struct AppState {
    shared_http_client: reqwest::Client,
    base_url: String,
    auth_token: Option<String>,
    sessions: Mutex<HashMap<String, Session>>,
}

pub async fn run_http_server(
    base_url: &str,
    insecure: bool,
    host: &str,
    port: u16,
    auth_token: Option<String>,
) {
    let shared_http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .danger_accept_invalid_certs(insecure)
        .build()
        .expect("failed to create shared HTTP client");

    let state = Arc::new(AppState {
        shared_http_client,
        base_url: base_url.to_string(),
        auth_token,
        sessions: Mutex::new(HashMap::new()),
    });

    let app = Router::new()
        .route("/mcp", post(handle_post).delete(handle_delete))
        .route("/health", get(handle_health))
        .with_state(state);

    let addr = format!("{host}:{port}");
    info!("HTTP server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind HTTP listener");

    axum::serve(listener, app).await.expect("HTTP server error");
}

fn check_auth(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = match &state.auth_token {
        Some(t) => t,
        None => return Ok(()),
    };

    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if let Some(token) = auth.strip_prefix("Bearer ") {
        if token == expected.as_str() {
            return Ok(());
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

fn get_session_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

async fn handle_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if let Err(status) = check_auth(&state, &headers) {
        return status.into_response();
    }

    // Check Content-Type
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.contains("application/json") {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/json",
        )
            .into_response();
    }

    // Peek at the method to determine if this is an initialize request
    let is_initialize = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("method").and_then(|m| m.as_str()).map(String::from))
        .as_deref()
        == Some("initialize");

    if is_initialize {
        // Extract per-client Kibana credentials from headers
        let auth = match AuthMethod::from_headers(&headers) {
            Ok(a) => a,
            Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
        };

        let client = Arc::new(KibanaClient::with_shared_client(
            state.shared_http_client.clone(),
            &state.base_url,
            auth,
        ));

        let result = crate::mcp::dispatch_request(&body, &client).await;

        match result {
            Some(response_json) => {
                let session_id = uuid::Uuid::new_v4().to_string();
                state
                    .sessions
                    .lock()
                    .expect("session lock poisoned")
                    .insert(session_id.clone(), Session { client });

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("mcp-session-id", &session_id)
                    .body(Body::from(response_json))
                    .expect("failed to build HTTP response")
            }
            None => (StatusCode::ACCEPTED, "").into_response(),
        }
    } else {
        // Require valid session ID for non-initialize requests
        let session_id = match get_session_id(&headers) {
            Some(id) => id,
            None => {
                return (StatusCode::BAD_REQUEST, "Missing Mcp-Session-Id header").into_response()
            }
        };

        let client = {
            let sessions = state.sessions.lock().expect("session lock poisoned");
            match sessions.get(&session_id) {
                Some(session) => Arc::clone(&session.client),
                None => return (StatusCode::BAD_REQUEST, "Invalid session ID").into_response(),
            }
        };

        let result = crate::mcp::dispatch_request(&body, &client).await;

        match result {
            Some(response_json) => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(response_json))
                .expect("failed to build HTTP response"),
            None => (StatusCode::ACCEPTED, "").into_response(),
        }
    }
}

async fn handle_health() -> impl IntoResponse {
    axum::Json(serde_json::json!({"status": "ok"}))
}

async fn handle_delete(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(status) = check_auth(&state, &headers) {
        return status.into_response();
    }

    let session_id = match get_session_id(&headers) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Missing Mcp-Session-Id header").into_response(),
    };

    let mut sessions = state.sessions.lock().expect("session lock poisoned");
    if sessions.remove(&session_id).is_some() {
        (StatusCode::OK, "Session terminated").into_response()
    } else {
        (StatusCode::NOT_FOUND, "Session not found").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_auth_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer secret123".parse().unwrap());

        let result = check_auth_standalone(Some("secret123"), &headers);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_auth_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());

        let result = check_auth_standalone(Some("secret123"), &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_auth_missing() {
        let headers = HeaderMap::new();

        let result = check_auth_standalone(Some("secret123"), &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_no_token_configured() {
        let headers = HeaderMap::new();

        let result = check_auth_standalone(None, &headers);
        assert!(result.is_ok());
    }

    #[test]
    fn test_session_lifecycle() {
        let sessions: Mutex<HashMap<String, Session>> = Mutex::new(HashMap::new());

        let session_id = uuid::Uuid::new_v4().to_string();
        let client = Arc::new(KibanaClient::new(
            "http://localhost:9200",
            None,
            None,
            None,
            false,
        ));
        sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), Session { client });
        assert!(sessions.lock().unwrap().contains_key(&session_id));

        assert!(sessions.lock().unwrap().remove(&session_id).is_some());
        assert!(!sessions.lock().unwrap().contains_key(&session_id));
    }

    #[test]
    fn test_missing_session_id() {
        let headers = HeaderMap::new();
        assert!(get_session_id(&headers).is_none());
    }

    #[test]
    fn test_get_session_id_present() {
        let mut headers = HeaderMap::new();
        headers.insert("mcp-session-id", "test-id-123".parse().unwrap());
        assert_eq!(get_session_id(&headers), Some("test-id-123".to_string()));
    }

    #[tokio::test]
    async fn test_health_returns_ok() {
        let resp = handle_health().await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    fn check_auth_standalone(token: Option<&str>, headers: &HeaderMap) -> Result<(), StatusCode> {
        let expected = match token {
            Some(t) => t,
            None => return Ok(()),
        };

        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if let Some(bearer_token) = auth.strip_prefix("Bearer ") {
            if bearer_token == expected {
                return Ok(());
            }
        }

        Err(StatusCode::UNAUTHORIZED)
    }
}
