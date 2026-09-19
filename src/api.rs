//! Public API surface (virtual-key auth): OpenAI Chat Completions, Anthropic
//! Messages, model listing, and health.

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;

use crate::app::AppState;
use crate::auth::{extract_virtual_key, authenticate_virtual_key};
use crate::frontends::{self, FrontendFormat};
use crate::limits;
use crate::pipeline;
use crate::types::ProxyError;

/// Generate a short, human-friendly request id.
fn new_request_id() -> String {
    format!("req_{}", uuid::Uuid::new_v4().simple())
}

/// Shared entry for both inbound frontends.
async fn handle(
    state: AppState,
    format: FrontendFormat,
    headers: HeaderMap,
    body: Value,
) -> Response {
    let request_id = new_request_id();

    // 1. Authenticate the virtual key.
    let presented = match extract_virtual_key(&headers) {
        Some(k) => k,
        None => {
            return error_response(format, &request_id, ProxyError::unauthorized(
                "missing API key. Provide it via 'Authorization: Bearer sk-kinetix-...' or 'x-api-key'.",
            ));
        }
    };
    let key = match authenticate_virtual_key(&state, &presented).await {
        Ok(k) => k,
        Err(e) => return error_response(format, &request_id, e),
    };

    // 2. Decode the request into the internal model.
    let req = match frontends::decode(format, body) {
        Ok(r) => r,
        Err(e) => return error_response(format, &request_id, e),
    };

    // 3. Enforce per-key limits and budgets.
    if let Err(e) = limits::enforce(&state.pool, &key, &req.requested_model).await {
        return error_response(format, &request_id, e);
    }

    // 4. Run the pipeline.
    match pipeline::run(&state, format, Some(key), req, request_id.clone(), true).await {
        Ok(resp) => resp,
        Err(e) => error_response(format, &request_id, e),
    }
}

pub async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let json: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                FrontendFormat::OpenAi,
                &new_request_id(),
                ProxyError::bad_request(format!("invalid JSON body: {e}")),
            )
        }
    };
    handle(state, FrontendFormat::OpenAi, headers, json).await
}

pub async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let json: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                FrontendFormat::Anthropic,
                &new_request_id(),
                ProxyError::bad_request(format!("invalid JSON body: {e}")),
            )
        }
    };
    handle(state, FrontendFormat::Anthropic, headers, json).await
}

/// `GET /v1/models`. The response shape is chosen by the client's auth style so
/// both OpenAI and Anthropic clients can discover models (FR-1.2, FR-10.10).
pub async fn list_models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let format = if headers.contains_key("x-api-key")
        || headers
            .get("anthropic-version")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    {
        FrontendFormat::Anthropic
    } else {
        FrontendFormat::OpenAi
    };

    // Authentication is required so the list can be scoped to the key.
    let presented = match extract_virtual_key(&headers) {
        Some(k) => k,
        None => {
            return error_response(
                format,
                &new_request_id(),
                ProxyError::unauthorized("missing API key"),
            )
        }
    };
    let key = match authenticate_virtual_key(&state, &presented).await {
        Ok(k) => k,
        Err(e) => return error_response(format, &new_request_id(), e),
    };

    let body = frontends::models::models_body(format, &state.registry, &key.allowed_models());
    Json(body).into_response()
}

pub async fn healthz(State(state): State<AppState>) -> Response {
    // Lightweight process + database check (Monitoring section).
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.pool).await.is_ok();
    let status = if db_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    let body = serde_json::json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "uptime_secs": state.uptime_secs(),
        "database": if db_ok { "ok" } else { "unavailable" },
    });
    (status, Json(body)).into_response()
}

/// Build a format-correct error response with the standard Prism headers.
pub fn error_response(format: FrontendFormat, request_id: &str, err: ProxyError) -> Response {
    let status = StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = frontends::models::error_body(format, &err);

    let mut builder = Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("x-request-id", request_id);
    if let Some(retry) = err.retry_after_secs {
        builder = builder.header("retry-after", retry.to_string());
    }
    for (k, v) in &err.headers {
        builder = builder.header(k, v);
    }
    builder
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(Body::from("internal error")))
}
