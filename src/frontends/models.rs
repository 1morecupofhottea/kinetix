//! Model listing (`GET /v1/models`, FR-1.2, FR-10.10) and format-correct error
//! body encoding.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{json, Value};

use crate::frontends::FrontendFormat;
use crate::registry::Registry;
use crate::types::{ErrorKind, ProxyError};

impl axum::response::IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut builder = Response::builder().status(status);
        if let Some(retry) = self.retry_after_secs {
            builder = builder.header("retry-after", retry.to_string());
        }
        for (k, v) in &self.headers {
            builder = builder.header(k, v);
        }
        builder
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"error": {"message": self.message}}).to_string(),
            ))
            .unwrap_or_else(|_| Response::new(Body::from("internal error")))
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Build the model-list body in the calling frontend's format.
pub fn models_body(format: FrontendFormat, registry: &Registry, key_allowed: &[String]) -> Value {
    let snap = registry.snapshot();
    let created = chrono::Utc::now().timestamp();

    // Client-facing names: aliases and combos first, then bare upstream model IDs.
    let mut entries: Vec<(String, Option<i64>, Option<i64>)> = Vec::new();
    for alias in snap.aliases.values() {
        let (ctx, max) = match alias.target_type.as_str() {
            "combo" => snap
                .combos
                .get(&alias.target_id)
                .map(|_| (None, None))
                .unwrap_or((None, None)),
            _ => snap
                .models
                .get(&alias.target_id)
                .map(|m| (m.context_window, m.max_output_tokens))
                .unwrap_or((None, None)),
        };
        entries.push((alias.alias.clone(), ctx, max));
    }
    for combo in snap.combos.values() {
        if combo.enabled != 0 && !entries.iter().any(|(n, _, _)| n == &combo.name) {
            entries.push((combo.name.clone(), None, None));
        }
    }
    for m in snap.models.values() {
        if m.enabled == 0 {
            continue;
        }
        if !entries.iter().any(|(n, _, _)| n == &m.upstream_id) {
            entries.push((m.upstream_id.clone(), m.context_window, m.max_output_tokens));
        }
    }

    // Filter by the virtual key's allowed models (FR-10.10).
    entries.retain(|(name, _, _)| {
        if key_allowed.iter().any(|a| a == "*") {
            return true;
        }
        key_allowed.iter().any(|a| {
            if let Some(prefix) = a.strip_suffix('*') {
                name.starts_with(prefix)
            } else {
                a == name
            }
        })
    });

    match format {
        FrontendFormat::OpenAi => {
            let data: Vec<Value> = entries
                .iter()
                .map(|(name, ctx, max)| {
                    let mut obj = json!({
                        "id": name,
                        "object": "model",
                        "created": created,
                        "owned_by": "kinetix",
                    });
                    if let Some(c) = ctx {
                        obj["context_window"] = json!(c);
                    }
                    if let Some(m) = max {
                        obj["max_output_tokens"] = json!(m);
                    }
                    obj
                })
                .collect();
            json!({ "object": "list", "data": data })
        }
        FrontendFormat::Anthropic => {
            let data: Vec<Value> = entries
                .iter()
                .map(|(name, _, _)| {
                    json!({
                        "type": "model",
                        "id": name,
                        "display_name": name,
                        "created_at": chrono::Utc::now().to_rfc3339(),
                    })
                })
                .collect();
            let first = entries.first().map(|(n, _, _)| n.clone());
            let last = entries.last().map(|(n, _, _)| n.clone());
            json!({
                "data": data,
                "has_more": false,
                "first_id": first,
                "last_id": last,
            })
        }
    }
}

/// Encode an error body in the calling frontend's format (FR-2.8, NFR-6.2).
pub fn error_body(format: FrontendFormat, err: &ProxyError) -> Value {
    match format {
        FrontendFormat::OpenAi => {
            let etype = match err.kind {
                ErrorKind::BadRequest | ErrorKind::Unsupported => "invalid_request_error",
                ErrorKind::Unauthorized => "authentication_error",
                ErrorKind::Forbidden => "permission_error",
                ErrorKind::NotFound => "invalid_request_error",
                ErrorKind::RateLimited => "rate_limit_error",
                ErrorKind::BudgetExceeded => "budget_exceeded",
                ErrorKind::AllTargetsUnavailable => "upstream_unavailable",
                ErrorKind::Upstream => "upstream_error",
                ErrorKind::Internal => "internal_error",
            };
            json!({
                "error": {
                    "message": err.message,
                    "type": etype,
                    "param": null,
                    "code": etype
                }
            })
        }
        FrontendFormat::Anthropic => {
            let etype = match err.kind {
                ErrorKind::BadRequest | ErrorKind::Unsupported => "invalid_request_error",
                ErrorKind::Unauthorized => "authentication_error",
                ErrorKind::Forbidden => "permission_error",
                ErrorKind::NotFound => "not_found_error",
                ErrorKind::RateLimited => "rate_limit_error",
                ErrorKind::BudgetExceeded => "rate_limit_error",
                ErrorKind::AllTargetsUnavailable => "overloaded_error",
                ErrorKind::Upstream => "api_error",
                ErrorKind::Internal => "api_error",
            };
            json!({
                "type": "error",
                "error": { "type": etype, "message": err.message }
            })
        }
    }
}
