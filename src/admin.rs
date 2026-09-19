//! Admin API (FR-8.4). Every dashboard action is available here; the dashboard
//! is just a client. Protected by `AdminAuth`.

use std::sync::atomic::Ordering;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::adapters::UpstreamContext;
use crate::app::AppState;
use crate::auth::{self, AdminAuth, SESSION_COOKIE};
use crate::credentials::CredentialStrategy;
use crate::crypto;
use crate::db::{self, Pool};
use crate::frontends::FrontendFormat;
use crate::limits;
use crate::pipeline;
use crate::types::{AuthScheme, Capabilities, Prices, WireFormat};

type ApiResult = Result<Json<Value>, ApiError>;

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        ApiError(StatusCode::BAD_REQUEST, msg.into())
    }
    fn not_found(msg: impl Into<String>) -> Self {
        ApiError(StatusCode::NOT_FOUND, msg.into())
    }
    fn internal(e: impl std::fmt::Display) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

// ===========================================================================
// Auth / session
// ===========================================================================

#[derive(Deserialize)]
pub struct LoginBody {
    pub username: Option<String>,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<LoginBody>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    // Admin authentication is a control-plane action: if the store is
    // unavailable it must fail closed, not fall through (NFR-2.7).
    if !db_healthy(&state).await {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "admin authentication unavailable: control plane degraded".into(),
        ));
    }
    if !auth::verify_admin_password(&state, &body.password) {
        let _ = db::insert_audit(
            &state.pool,
            "unknown",
            "admin_login_failed",
            "system",
            "",
            "Admin Console",
            "Rejected an admin login with an incorrect password.",
        )
        .await;
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid admin password".into(),
        ));
    }
    let token = auth::session_token(&state);
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .build();
    let actor = body.username.unwrap_or_else(|| "admin".to_string());
    let _ = db::insert_audit(
        &state.pool,
        &actor,
        "admin_login",
        "system",
        "",
        "Admin Console",
        "Administrator session started.",
    )
    .await;
    Ok((jar.add(cookie), Json(json!({ "ok": true, "user": actor }))))
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> (CookieJar, Json<Value>) {
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "admin_logout",
        "system",
        "",
        "Admin Console",
        "Administrator session ended.",
    )
    .await;
    let cookie = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .build();
    (jar.add(cookie), Json(json!({ "ok": true })))
}

pub async fn me(_auth: AdminAuth) -> Json<Value> {
    Json(json!({ "authenticated": true, "user": "admin" }))
}

/// `POST /admin/api/test-stream` — run a real request through the pipeline for a
/// chosen virtual key + model and stream the encoded result back to the
/// browser. The raw virtual key never leaves the server (it is stored hashed).
pub async fn test_stream(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Json(body): Json<TestStreamBody>,
) -> Response {
    let key = match db::get_virtual_key_by_id(&state.pool, &body.key_id)
        .await
        .map_err(ApiError::internal)
    {
        Ok(Some(k)) => k,
        Ok(None) => return ApiError::not_found("key not found").into_response(),
        Err(e) => return e.into_response(),
    };

    let format = if body.format.as_deref() == Some("anthropic") {
        FrontendFormat::Anthropic
    } else {
        FrontendFormat::OpenAi
    };
    let stream = body.stream.unwrap_or(true);

    // Build a minimal internal request from the tester form.
    let mut messages = Vec::new();
    if let Some(system) = body.system.filter(|s| !s.trim().is_empty()) {
        messages.push(crate::types::Message {
            role: crate::types::Role::System,
            parts: vec![crate::types::Part::Text(system)],
        });
    }
    messages.push(crate::types::Message {
        role: crate::types::Role::User,
        parts: vec![crate::types::Part::Text(body.prompt)],
    });

    let req = crate::types::InternalRequest {
        requested_model: body.model.clone(),
        system: vec![],
        messages,
        tools: vec![],
        tool_choice: None,
        tool_choice_name: None,
        params: crate::types::SamplingParams {
            max_tokens: body.max_tokens,
            temperature: body.temperature,
            ..Default::default()
        },
        stream,
        thinking: None,
        extra: Default::default(),
        raw_body: None,
    };

    let request_id = format!("req_{}", uuid::Uuid::new_v4().simple());
    if let Err(e) = limits::enforce(&state.pool, &key, &req.requested_model).await {
        return crate::api::error_response(format, &request_id, e);
    }

    match pipeline::run(&state, format, Some(key), req, request_id.clone(), true, None).await {
        Ok(resp) => resp,
        Err(e) => crate::api::error_response(format, &request_id, e),
    }
}

#[derive(Deserialize)]
pub struct TestStreamBody {
    pub key_id: String,
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f64>,
}

// ===========================================================================
// Overview / metrics
// ===========================================================================

pub async fn overview(State(state): State<AppState>, _auth: AdminAuth) -> ApiResult {
    let summary = db::usage_summary(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let keys = db::list_virtual_keys(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let accounts = db::list_accounts(&state.pool)
        .await
        .map_err(ApiError::internal)?;

    let active_streams = state.log_queue.depth();
    let fallback_rate = {
        let reqs = summary["requests"].as_i64().unwrap_or(0);
        let hops = summary["fallback_hops"].as_i64().unwrap_or(0);
        if reqs > 0 {
            hops as f64 / reqs as f64
        } else {
            0.0
        }
    };

    Ok(Json(json!({
        "active_streams": active_streams,
        "total_requests": summary["requests"],
        "total_tokens": summary["input_tokens"].as_i64().unwrap_or(0) + summary["output_tokens"].as_i64().unwrap_or(0),
        "total_spend_usd": summary["cost_usd"],
        "cached_tokens": summary["cached_tokens"],
        "thinking_tokens": summary["thinking_tokens"],
        "fallback_rate": fallback_rate,
        "avg_latency_ms": summary["avg_latency_ms"],
        "keys_active": keys.iter().filter(|k| k.status == "active").count(),
        "keys_total": keys.len(),
        "accounts_healthy": accounts.iter().filter(|a| a.status == "healthy").count(),
        "accounts_total": accounts.len(),
        "log_queue_depth": state.log_queue.depth(),
        "log_queue_dropped": state.log_queue.dropped(),
        "uptime_secs": state.uptime_secs(),
        "tunnel_status": "connected",
    })))
}

// ===========================================================================
// Virtual keys
// ===========================================================================

pub async fn list_keys(State(state): State<AppState>, _auth: AdminAuth) -> ApiResult {
    let keys = db::list_virtual_keys(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let (by_key, _) = db::lifetime_totals(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let mut out = Vec::new();
    for k in keys {
        let (daily, monthly) = limits::spend_snapshot(&state.pool, &k).await;
        let (requests, tokens) = by_key.get(&k.id).copied().unwrap_or((0, 0));
        out.push(key_json(&k, daily, monthly, requests, tokens));
    }
    Ok(Json(json!({ "keys": out })))
}

fn key_json(
    k: &db::VirtualKeyRow,
    daily_spend: f64,
    monthly_spend: f64,
    total_requests: i64,
    total_tokens: i64,
) -> Value {
    json!({
        "id": k.id,
        "name": k.name,
        "owner": k.owner,
        "tag": k.tag,
        "allowed_models": k.allowed_models(),
        "allowed_providers": k.allowed_providers(),
        "rpm_limit": k.rpm_limit,
        "tpm_limit": k.tpm_limit,
        "daily_budget": k.daily_budget,
        "monthly_budget": k.monthly_budget,
        "current_daily_spend": daily_spend,
        "current_monthly_spend": monthly_spend,
        "expires_at": k.expires_at,
        "status": k.status,
        "allowed_ips": k.allowed_ips(),
        "body_logging": k.body_logging != 0,
        "created_at": k.created_at,
        "key_mask": mask_hash(&k.key_hash),
        "total_requests": total_requests,
        "total_tokens": total_tokens,
    })
}

fn mask_hash(_hash: &str) -> String {
    "sk-kinetix-•••• (hidden)".to_string()
}

#[derive(Deserialize)]
pub struct CreateKeyBody {
    pub name: String,
    pub owner: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub allowed_models: Vec<String>,
    #[serde(default)]
    pub allowed_providers: Vec<String>,
    pub rpm_limit: Option<i64>,
    pub tpm_limit: Option<i64>,
    pub daily_budget: Option<f64>,
    pub monthly_budget: Option<f64>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub body_logging: bool,
}

pub async fn create_key(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Json(body): Json<CreateKeyBody>,
) -> ApiResult {
    let full_key = crypto::generate_virtual_key();
    let hash = crypto::hash_virtual_key(&full_key);
    let allowed = if body.allowed_models.is_empty() {
        vec!["*".to_string()]
    } else {
        body.allowed_models.clone()
    };
    let row = db::VirtualKeyRow {
        id: format!("key_{}", uuid::Uuid::new_v4().simple()),
        key_hash: hash,
        name: body.name.clone(),
        owner: body.owner.clone(),
        tag: body.tag.clone(),
        allowed_models: serde_json::to_string(&allowed).unwrap(),
        allowed_providers: serde_json::to_string(&body.allowed_providers).unwrap(),
        rpm_limit: body.rpm_limit,
        tpm_limit: body.tpm_limit,
        daily_budget: body.daily_budget,
        monthly_budget: body.monthly_budget,
        expires_at: body.expires_at.clone(),
        status: "active".to_string(),
        allowed_ips: serde_json::to_string(&body.allowed_ips).unwrap(),
        body_logging: body.body_logging as i64,
        created_at: db::now_iso(),
        revoked_at: None,
    };
    db::insert_virtual_key(&state.pool, &row)
        .await
        .map_err(ApiError::internal)?;
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "key_created",
        "key",
        &row.id,
        &row.name,
        &format!("Issued virtual key for {} (owner {})", row.name, row.owner),
    )
    .await;
    // The full key is shown exactly once (FR-3.1).
    Ok(Json(json!({
        "key": key_json(&row, 0.0, 0.0, 0, 0),
        "full_key": full_key,
    })))
}

#[derive(Deserialize)]
pub struct UpdateKeyBody {
    pub status: Option<String>,
    pub name: Option<String>,
    pub owner: Option<String>,
    pub tag: Option<String>,
    pub allowed_models: Option<Vec<String>>,
    pub allowed_providers: Option<Vec<String>>,
    pub rpm_limit: Option<i64>,
    pub tpm_limit: Option<i64>,
    pub daily_budget: Option<f64>,
    pub monthly_budget: Option<f64>,
    pub expires_at: Option<String>,
    pub body_logging: Option<bool>,
}

pub async fn update_key(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<UpdateKeyBody>,
) -> ApiResult {
    if let Some(status) = &body.status {
        db::set_virtual_key_status(&state.pool, &id, status)
            .await
            .map_err(ApiError::internal)?;
        let _ = db::insert_audit(
            &state.pool,
            "admin",
            &format!("key_status_{status}"),
            "key",
            &id,
            "",
            &format!("Changed key status to {status}."),
        )
        .await;
    }
    // Field updates (limits/budgets/etc.) rebuild the row.
    let existing = db::list_virtual_keys(&state.pool)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .find(|k| k.id == id)
        .ok_or_else(|| ApiError::not_found("key not found"))?;

    let name = body.name.unwrap_or(existing.name.clone());
    let owner = body.owner.unwrap_or(existing.owner.clone());
    let tag = body.tag.unwrap_or(existing.tag.clone());
    let allowed_models = body
        .allowed_models
        .map(|v| serde_json::to_string(&v).unwrap())
        .unwrap_or(existing.allowed_models.clone());
    let allowed_providers = body
        .allowed_providers
        .map(|v| serde_json::to_string(&v).unwrap())
        .unwrap_or(existing.allowed_providers.clone());
    let body_logging = body.body_logging.map(|b| b as i64).unwrap_or(existing.body_logging);

    sqlx::query(
        "UPDATE virtual_keys SET name=?, owner=?, tag=?, allowed_models=?, allowed_providers=?,
         rpm_limit=?, tpm_limit=?, daily_budget=?, monthly_budget=?, expires_at=?, body_logging=? WHERE id=?",
    )
    .bind(name)
    .bind(owner)
    .bind(tag)
    .bind(allowed_models)
    .bind(allowed_providers)
    .bind(body.rpm_limit.or(existing.rpm_limit))
    .bind(body.tpm_limit.or(existing.tpm_limit))
    .bind(body.daily_budget.or(existing.daily_budget))
    .bind(body.monthly_budget.or(existing.monthly_budget))
    .bind(body.expires_at.or(existing.expires_at))
    .bind(body_logging)
    .bind(&id)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "key_updated",
        "key",
        &id,
        &existing.name,
        "Updated key limits/budgets.",
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_key(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
) -> ApiResult {
    db::delete_virtual_key(&state.pool, &id)
        .await
        .map_err(ApiError::internal)?;
    let _ = db::insert_audit(&state.pool, "admin", "key_deleted", "key", &id, "", "Deleted key.").await;
    Ok(Json(json!({ "ok": true })))
}

// ===========================================================================
// Providers
// ===========================================================================

pub async fn list_providers(State(state): State<AppState>, _auth: AdminAuth) -> ApiResult {
    let providers = db::list_providers(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let accounts = db::list_accounts(&state.pool).await.map_err(ApiError::internal)?;
    let models = db::list_models(&state.pool).await.map_err(ApiError::internal)?;
    let out: Vec<Value> = providers
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "base_url": p.base_url,
                "wire_format": p.wire_format,
                "auth_scheme": p.auth_scheme,
                "custom_header_name": p.custom_header_name,
                "custom_param_name": p.custom_param_name,
                "extra_headers": p.extra_headers_map(),
                "timeout_ms": p.timeout_ms,
                "capability_mode": p.capability_mode,
                "models_path": p.models_path,
                "enabled": p.enabled != 0,
                "accounts_count": accounts.iter().filter(|a| a.provider_id == p.id).count(),
                "models_count": models.iter().filter(|m| m.provider_id == p.id).count(),
                "healthy_accounts": accounts.iter().filter(|a| a.provider_id == p.id && a.status == "healthy").count(),
            })
        })
        .collect();
    Ok(Json(json!({ "providers": out })))
}

#[derive(Deserialize)]
pub struct ProviderBody {
    pub name: String,
    pub base_url: String,
    pub wire_format: String,
    #[serde(default = "default_bearer")]
    pub auth_scheme: String,
    pub custom_header_name: Option<String>,
    pub custom_param_name: Option<String>,
    #[serde(default)]
    pub extra_headers: serde_json::Map<String, Value>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: i64,
    #[serde(default = "default_permissive")]
    pub capability_mode: String,
    pub models_path: Option<String>,
    /// NFR-3.10: redirects are never followed unless explicitly enabled.
    #[serde(default)]
    pub follow_redirects: bool,
    /// NFR-3.11: comma-separated authorized hosts for the credential.
    #[serde(default)]
    pub credential_hosts: String,
    /// NFR-3.12: explicit dev-mode opt-out of TLS verification.
    #[serde(default)]
    pub allow_insecure_tls: bool,
    /// Optional initial credential.
    pub api_key: Option<String>,
    pub account_label: Option<String>,
}

fn default_bearer() -> String {
    "bearer".into()
}
fn default_timeout() -> i64 {
    120_000
}
fn default_permissive() -> String {
    "permissive".into()
}

pub async fn create_provider(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Json(body): Json<ProviderBody>,
) -> ApiResult {
    validate_outbound_url(&state, &body.base_url)?;
    let wire = WireFormat::parse(&body.wire_format).ok_or_else(|| ApiError::bad("invalid wire_format"))?;
    let auth = AuthScheme::parse(&body.auth_scheme).ok_or_else(|| ApiError::bad("invalid auth_scheme"))?;

    let id = db::insert_provider(
        &state.pool,
        &db::NewProvider {
            name: &body.name,
            base_url: &body.base_url,
            wire_format: wire,
            auth_scheme: auth,
            custom_header_name: body.custom_header_name.as_deref(),
            custom_param_name: body.custom_param_name.as_deref(),
            extra_headers: Value::Object(body.extra_headers),
            timeout_ms: body.timeout_ms,
            capability_mode: &body.capability_mode,
            models_path: body.models_path.as_deref(),
            rate_limit_rules: json!({}),
            follow_redirects: body.follow_redirects,
            credential_hosts: &body.credential_hosts,
            allow_insecure_tls: body.allow_insecure_tls,
        },
    )
    .await
    .map_err(ApiError::internal)?;

    if let Some(api_key) = body.api_key.filter(|k| !k.trim().is_empty()) {
        let enc = state.crypto.encrypt(&api_key).map_err(ApiError::internal)?;
        db::insert_account(
            &state.pool,
            &id,
            body.account_label.as_deref().unwrap_or("Default key"),
            &enc,
            &crypto::mask_secret(&api_key),
            1,
            1,
            None,
            "none",
        )
        .await
        .map_err(ApiError::internal)?;
    }

    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "provider_created",
        "provider",
        &id,
        &body.name,
        &format!("Added {} upstream ({} wire format at {}).", body.name, body.wire_format, body.base_url),
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_provider(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<ProviderBody>,
) -> ApiResult {
    validate_outbound_url(&state, &body.base_url)?;
    let wire = WireFormat::parse(&body.wire_format).ok_or_else(|| ApiError::bad("invalid wire_format"))?;
    let auth = AuthScheme::parse(&body.auth_scheme).ok_or_else(|| ApiError::bad("invalid auth_scheme"))?;
    db::update_provider(
        &state.pool,
        &id,
        &body.name,
        &body.base_url,
        wire,
        auth,
        body.custom_header_name.as_deref(),
        body.custom_param_name.as_deref(),
        Value::Object(body.extra_headers),
        body.timeout_ms,
        &body.capability_mode,
        body.models_path.as_deref(),
        body.follow_redirects,
        &body.credential_hosts,
        body.allow_insecure_tls,
    )
    .await
    .map_err(ApiError::internal)?;
    if let Some(api_key) = body.api_key.filter(|k| !k.trim().is_empty()) {
        let enc = state.crypto.encrypt(&api_key).map_err(ApiError::internal)?;
        db::insert_account(
            &state.pool,
            &id,
            body.account_label.as_deref().unwrap_or("Default key"),
            &enc,
            &crypto::mask_secret(&api_key),
            1,
            1,
            None,
            "none",
        )
        .await
        .map_err(ApiError::internal)?;
    }
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "provider_updated",
        "provider",
        &id,
        &body.name,
        "Updated provider configuration.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_provider(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
) -> ApiResult {
    db::delete_provider(&state.pool, &id)
        .await
        .map_err(ApiError::internal)?;
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "provider_deleted",
        "provider",
        &id,
        "",
        "Deleted provider and its models/accounts.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /admin/api/providers/:id/discover` — fetch the upstream model list
/// using the provider's credentials (FR-10.4).
pub async fn discover_models(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
) -> ApiResult {
    let provider = db::get_provider(&state.pool, &id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    let accounts = db::accounts_for_provider(&state.pool, &id)
        .await
        .map_err(ApiError::internal)?;
    let account = accounts
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::bad("provider has no credentials to discover with"))?;
    let credential = state
        .credentials
        .credential(&account)
        .await
        .map_err(ApiError::internal)?;

    let wire = provider.wire();
    let adapter = state.adapters.for_format(wire);
    let path = provider
        .models_path
        .clone()
        .unwrap_or_else(|| adapter.default_models_path().to_string());
    let base = provider.base_url.trim_end_matches('/');
    let url = if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    };

    // Build a context so auth can be applied.
    let dummy_model = db::ModelRow {
        id: "discovery".into(),
        provider_id: provider.id.clone(),
        upstream_id: "discovery".into(),
        display_name: "discovery".into(),
        enabled: 1,
        context_window: None,
        max_output_tokens: None,
        capabilities: "{}".into(),
        prices: "{}".into(),
        parameters: "{}".into(),
        thinking_map: "{}".into(),
        extra_request: "{}".into(),
        discovery: "{}".into(),
        created_at: db::now_iso(),
    };
    let ctx = UpstreamContext {
        provider: &provider,
        model: &dummy_model,
        credential,
    };
    let mut req = state.http.get(&url).timeout(std::time::Duration::from_millis(provider.timeout_ms as u64));
    req = adapter.apply_auth(&ctx, req);
    for (k, v) in provider.extra_headers_map() {
        req = req.header(k, v);
    }

    let resp = req.send().await.map_err(|e| ApiError::bad(format!("discovery request failed: {}", crate::crypto::redact(&e.to_string()))))?;
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::bad(format!(
            "upstream returned HTTP {}: {}",
            status.as_u16(),
            crate::crypto::redact(&truncate(&body_text, 400))
        )));
    }
    let parsed: Value = serde_json::from_str(&body_text)
        .map_err(|e| ApiError::bad(format!("invalid discovery response: {e}")))?;
    let discovered = adapter.parse_model_list(&parsed);

    // Mark which are already imported.
    let existing = db::models_for_provider(&state.pool, &id)
        .await
        .map_err(ApiError::internal)?;
    let out: Vec<Value> = discovered
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id,
                "display_name": m.display_name,
                "context_window": m.context_window,
                "max_output_tokens": m.max_output_tokens,
                "already_imported": existing.iter().any(|e| e.upstream_id == m.id),
            })
        })
        .collect();
    Ok(Json(json!({ "models": out })))
}

/// `POST /admin/api/providers/:id/test` — send a minimal probe (FR-10.11).
pub async fn test_provider(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<TestBody>,
) -> ApiResult {
    let provider = db::get_provider(&state.pool, &id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    let accounts = db::accounts_for_provider(&state.pool, &id)
        .await
        .map_err(ApiError::internal)?;
    let account = accounts
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::bad("provider has no credentials to test with"))?;
    let credential = state
        .credentials
        .credential(&account)
        .await
        .map_err(ApiError::internal)?;

    let upstream_id = body
        .model
        .clone()
        .ok_or_else(|| ApiError::bad("provide a model to test"))?;
    let model = db::find_model_by_upstream(&state.pool, &id, &upstream_id)
        .await
        .map_err(ApiError::internal)?
        .unwrap_or_else(|| db::ModelRow {
            id: "probe".into(),
            provider_id: id.clone(),
            upstream_id: upstream_id.clone(),
            display_name: upstream_id.clone(),
            enabled: 1,
            context_window: None,
            max_output_tokens: Some(64),
            capabilities: "{}".into(),
            prices: "{}".into(),
            parameters: "{}".into(),
            thinking_map: "{}".into(),
            extra_request: "{}".into(),
            discovery: "{}".into(),
            created_at: db::now_iso(),
        });

    let adapter = state.adapters.for_format(provider.wire());
    let ctx = UpstreamContext {
        provider: &provider,
        model: &model,
        credential,
    };
    let mut internal = crate::types::InternalRequest {
        requested_model: upstream_id.clone(),
        system: vec![],
        messages: vec![crate::types::Message {
            role: crate::types::Role::User,
            parts: vec![crate::types::Part::Text("Reply with the single word: ok".into())],
        }],
        tools: vec![],
        tool_choice: None,
        tool_choice_name: None,
        params: crate::types::SamplingParams {
            max_tokens: Some(16),
            ..Default::default()
        },
        stream: false,
        thinking: None,
        extra: Default::default(),
        raw_body: None,
    };
    internal.stream = false;

    let url = adapter.build_url(&ctx).map_err(|e| ApiError::bad(e.message))?;
    let outbound = adapter.build_body(&ctx, &internal);
    let mut req = state
        .http
        .post(&url)
        .header("content-type", "application/json")
        .timeout(std::time::Duration::from_millis(provider.timeout_ms as u64))
        .json(&outbound);
    req = adapter.apply_auth(&ctx, req);
    for (k, v) in provider.extra_headers_map() {
        req = req.header(k, v);
    }

    let started = std::time::Instant::now();
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let latency = started.elapsed().as_millis() as i64;
            if (200..300).contains(&status) {
                Ok(Json(json!({
                    "ok": true, "status": status, "latency_ms": latency,
                    "response_preview": truncate(&text, 400),
                })))
            } else {
                let failure = adapter.classify_error(status, &text, &axum::http::HeaderMap::new());
                Ok(Json(json!({
                    "ok": false, "status": status, "latency_ms": latency,
                    "error": failure.message,
                })))
            }
        }
        Err(e) => Ok(Json(json!({
            "ok": false, "status": 0,
            "error": crate::crypto::redact(&e.to_string()),
        }))),
    }
}

#[derive(Deserialize)]
pub struct TestBody {
    pub model: Option<String>,
}

// ===========================================================================
// Models
// ===========================================================================

pub async fn list_models(State(state): State<AppState>, _auth: AdminAuth) -> ApiResult {
    let models = db::list_models(&state.pool).await.map_err(ApiError::internal)?;
    let providers = db::list_providers(&state.pool).await.map_err(ApiError::internal)?;
    let out: Vec<Value> = models.iter().map(|m| model_json(m, &providers)).collect();
    Ok(Json(json!({ "models": out })))
}

fn model_json(m: &db::ModelRow, providers: &[db::ProviderRow]) -> Value {
    let provider_name = providers
        .iter()
        .find(|p| p.id == m.provider_id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    json!({
        "id": m.id,
        "provider_id": m.provider_id,
        "provider_name": provider_name,
        "upstream_id": m.upstream_id,
        "display_name": m.display_name,
        "enabled": m.enabled != 0,
        "context_window": m.context_window,
        "max_output_tokens": m.max_output_tokens,
        "capabilities": m.caps(),
        "prices": m.prices(),
        "parameters": m.params(),
        "thinking_map": m.thinking(),
        "extra_request": m.extra_request_value(),
    })
}

#[derive(Deserialize)]
pub struct ModelBody {
    pub upstream_id: String,
    pub display_name: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub prices: Value,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub thinking_map: Value,
    #[serde(default)]
    pub extra_request: Value,
}

fn default_true() -> bool {
    true
}

pub async fn create_model(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(provider_id): Path<String>,
    Json(body): Json<ModelBody>,
) -> ApiResult {
    let _ = db::get_provider(&state.pool, &provider_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    let caps: Capabilities = serde_json::from_value(body.capabilities.clone()).unwrap_or_default();
    let prices: Prices = serde_json::from_value(body.prices.clone()).unwrap_or_default();

    let id = db::insert_model(
        &state.pool,
        &db::NewModel {
            provider_id: &provider_id,
            upstream_id: &body.upstream_id,
            display_name: body.display_name.as_deref().unwrap_or(&body.upstream_id),
            enabled: body.enabled,
            context_window: body.context_window,
            max_output_tokens: body.max_output_tokens,
            capabilities: serde_json::to_value(&caps).unwrap(),
            prices: serde_json::to_value(&prices).unwrap(),
            parameters: body.parameters.clone(),
            thinking_map: body.thinking_map.clone(),
            extra_request: body.extra_request.clone(),
            discovery: json!({}),
        },
    )
    .await
    .map_err(ApiError::internal)?;
    if prices.is_configured() {
        let _ = db::insert_price_version(&state.pool, &id, &prices).await;
    }
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "model_configured",
        "model",
        &id,
        &body.upstream_id,
        "Configured upstream model.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_model(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<ModelBody>,
) -> ApiResult {
    let caps: Capabilities = serde_json::from_value(body.capabilities.clone()).unwrap_or_default();
    let prices: Prices = serde_json::from_value(body.prices.clone()).unwrap_or_default();
    db::update_model(
        &state.pool,
        &id,
        body.display_name.as_deref().unwrap_or(&body.upstream_id),
        body.enabled,
        body.context_window,
        body.max_output_tokens,
        serde_json::to_value(&caps).unwrap(),
        serde_json::to_value(&prices).unwrap(),
        body.parameters.clone(),
        body.thinking_map.clone(),
        body.extra_request.clone(),
    )
    .await
    .map_err(ApiError::internal)?;
    if prices.is_configured() {
        let _ = db::insert_price_version(&state.pool, &id, &prices).await;
    }
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "model_updated",
        "model",
        &id,
        &body.upstream_id,
        "Updated model configuration.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_model(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
) -> ApiResult {
    db::delete_model(&state.pool, &id).await.map_err(ApiError::internal)?;
    let _ = db::insert_audit(&state.pool, "admin", "model_deleted", "model", &id, "", "Removed model.").await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

// ===========================================================================
// Accounts
// ===========================================================================

pub async fn list_accounts(State(state): State<AppState>, _auth: AdminAuth) -> ApiResult {
    let accounts = db::list_accounts(&state.pool).await.map_err(ApiError::internal)?;
    let providers = db::list_providers(&state.pool).await.map_err(ApiError::internal)?;
    let (_, by_account) = db::lifetime_totals(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let out: Vec<Value> = accounts
        .iter()
        .map(|a| {
            let (requests, tokens) = by_account.get(&a.id).copied().unwrap_or((0, 0));
            account_json(a, &providers, requests, tokens)
        })
        .collect();
    Ok(Json(json!({ "accounts": out })))
}

fn account_json(
    a: &db::AccountRow,
    providers: &[db::ProviderRow],
    requests_count: i64,
    tokens_count: i64,
) -> Value {
    let provider_name = providers
        .iter()
        .find(|p| p.id == a.provider_id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    json!({
        "id": a.id,
        "provider_id": a.provider_id,
        "provider_name": provider_name,
        "label": a.label,
        "key_mask": a.key_mask,
        "status": a.status,
        "cooldown_until": a.cooldown_until,
        "quota_reset_at": a.quota_reset_at,
        "quota_type": a.quota_type,
        "soft_quota_usd": a.soft_quota_usd,
        "priority": a.priority,
        "weight": a.weight,
        "last_error": a.last_error,
        "created_at": a.created_at,
        "requests_count": requests_count,
        "tokens_count": tokens_count,
    })
}

#[derive(Deserialize)]
pub struct AccountBody {
    pub provider_id: String,
    pub label: String,
    pub api_key: Option<String>,
    #[serde(default = "one")]
    pub priority: i64,
    #[serde(default = "one")]
    pub weight: i64,
    pub soft_quota_usd: Option<f64>,
    #[serde(default = "default_quota_type")]
    pub quota_type: String,
    pub status: Option<String>,
}

fn one() -> i64 {
    1
}
fn default_quota_type() -> String {
    "none".into()
}

pub async fn create_account(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Json(body): Json<AccountBody>,
) -> ApiResult {
    let api_key = body
        .api_key
        .clone()
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| ApiError::bad("api_key is required"))?;
    let enc = state.crypto.encrypt(&api_key).map_err(ApiError::internal)?;
    let id = db::insert_account(
        &state.pool,
        &body.provider_id,
        &body.label,
        &enc,
        &crypto::mask_secret(&api_key),
        body.priority,
        body.weight,
        body.soft_quota_usd,
        &body.quota_type,
    )
    .await
    .map_err(ApiError::internal)?;
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "account_credential_added",
        "account",
        &id,
        &body.label,
        "Enrolled a new credential into the pool.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_account(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<AccountBody>,
) -> ApiResult {
    db::update_account(
        &state.pool,
        &id,
        &body.label,
        body.status.as_deref().unwrap_or("healthy"),
        body.priority,
        body.weight,
        body.soft_quota_usd,
        &body.quota_type,
    )
    .await
    .map_err(ApiError::internal)?;
    // Optionally rotate the credential.
    if let Some(api_key) = body.api_key.filter(|k| !k.trim().is_empty()) {
        let enc = state.crypto.encrypt(&api_key).map_err(ApiError::internal)?;
        sqlx::query("UPDATE accounts SET secret_enc=?, key_mask=? WHERE id=?")
            .bind(enc)
            .bind(crypto::mask_secret(&api_key))
            .bind(&id)
            .execute(&state.pool)
            .await
            .map_err(ApiError::internal)?;
    }
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "account_updated",
        "account",
        &id,
        &body.label,
        "Updated account configuration.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /admin/api/accounts/:id/reset` — clear cooldown/exhaustion.
pub async fn reset_account(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
) -> ApiResult {
    crate::pool::mark_healthy(&state.pool, &id)
        .await
        .map_err(ApiError::internal)?;
    let _ = crate::pool::clear_circuit(&state.pool, &id).await;
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "account_reset",
        "account",
        &id,
        "",
        "Cleared cooldown/exhaustion/circuit state.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_account(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
) -> ApiResult {
    db::delete_account(&state.pool, &id).await.map_err(ApiError::internal)?;
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "account_credential_removed",
        "account",
        &id,
        "",
        "Removed credential from the pool.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

// ===========================================================================
// Routes
// ===========================================================================

pub async fn list_routes(State(state): State<AppState>, _auth: AdminAuth) -> ApiResult {
    let routes = db::list_routes(&state.pool).await.map_err(ApiError::internal)?;
    let accounts = db::list_accounts(&state.pool).await.map_err(ApiError::internal)?;
    let models = db::list_models(&state.pool).await.map_err(ApiError::internal)?;
    let mut out = Vec::new();
    for c in &routes {
        let targets = db::route_targets(&state.pool, &c.id).await.map_err(ApiError::internal)?;
        let targets_json: Vec<Value> = targets
            .iter()
            .map(|t| {
                let model = models.iter().find(|m| m.id == t.model_id);
                let account = t.account_id.as_ref().and_then(|aid| accounts.iter().find(|a| a.id == *aid));
                json!({
                    "id": t.id,
                    "account_id": t.account_id,
                    "account_label": account.map(|a| a.label.clone()),
                    "model_id": t.model_id,
                    "model_display_name": model.map(|m| m.display_name.clone()),
                    "provider_id": model.map(|m| m.provider_id.clone()),
                    "priority": t.priority,
                    "weight": t.weight,
                    "predicate": serde_json::from_str::<Value>(&t.predicate).unwrap_or(json!({})),
                    "param_overrides": serde_json::from_str::<Value>(&t.param_overrides).unwrap_or(json!({})),
                })
            })
            .collect();
        out.push(json!({
            "id": c.id,
            "name": c.name,
            "description": c.description,
            "strategy": c.strategy,
            "fallback_triggers": serde_json::from_str::<Value>(&c.fallback_triggers).unwrap_or(json!({})),
            "continuity_policy": c.continuity_policy,
            "portability_policy": c.portability_policy,
            "sticky_routing": c.sticky_routing != 0,
            "cache_affinity": c.cache_affinity != 0,
            "max_attempts": c.max_attempts,
            "enabled": c.enabled != 0,
            "targets": targets_json,
        }));
    }
    Ok(Json(json!({ "routes": out })))
}

#[derive(Deserialize)]
pub struct RouteBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "priority_strategy")]
    pub strategy: String,
    #[serde(default)]
    pub fallback_triggers: Value,
    #[serde(default = "strip_policy")]
    pub continuity_policy: String,
    /// FR-2.11: reject | strip_with_warning.
    #[serde(default = "portability_default")]
    pub portability_policy: String,
    #[serde(default)]
    pub sticky_routing: bool,
    /// FR-7.3: cache-aware sticky routing.
    #[serde(default)]
    pub cache_affinity: bool,
    pub max_attempts: Option<i64>,
    #[serde(default)]
    pub targets: Vec<RouteTargetBody>,
}

fn priority_strategy() -> String {
    "priority".into()
}
fn strip_policy() -> String {
    "strip".into()
}
fn portability_default() -> String {
    "strip_with_warning".into()
}

#[derive(Deserialize)]
pub struct RouteTargetBody {
    pub account_id: Option<String>,
    pub model_id: String,
    #[serde(default = "one")]
    pub priority: i64,
    #[serde(default = "one")]
    pub weight: i64,
    /// Typed eligibility predicate (FR-12.3). Empty/absent = always eligible.
    #[serde(default)]
    pub predicate: Value,
    /// Per-target parameter overrides (FR-12.2).
    #[serde(default)]
    pub param_overrides: Value,
}

pub async fn create_route(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Json(body): Json<RouteBody>,
) -> ApiResult {
    let id = db::insert_route(
        &state.pool,
        &db::NewRoute {
            name: &body.name,
            description: &body.description,
            strategy: &body.strategy,
            fallback_triggers: if body.fallback_triggers.is_null() {
                json!({"on429": true, "onQuota": true, "on5xx": true, "onTimeout": true})
            } else {
                body.fallback_triggers.clone()
            },
            continuity_policy: &body.continuity_policy,
            portability_policy: &body.portability_policy,
            sticky_routing: body.sticky_routing,
            cache_affinity: body.cache_affinity,
            max_attempts: body.max_attempts,
        },
    )
    .await
    .map_err(ApiError::internal)?;
    write_route_targets(&state.pool, &id, &body.targets).await?;
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "route_created",
        "route",
        &id,
        &body.name,
        &format!("Created route with {} targets.", body.targets.len()),
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_route(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
    Json(body): Json<RouteBody>,
) -> ApiResult {
    db::update_route(
        &state.pool,
        &id,
        &body.description,
        &body.strategy,
        body.fallback_triggers.clone(),
        &body.continuity_policy,
        &body.portability_policy,
        body.sticky_routing,
        body.cache_affinity,
        body.max_attempts,
    )
    .await
    .map_err(ApiError::internal)?;
    db::clear_route_targets(&state.pool, &id).await.map_err(ApiError::internal)?;
    write_route_targets(&state.pool, &id, &body.targets).await?;
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "route_updated",
        "route",
        &id,
        &body.name,
        "Updated route configuration.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

async fn write_route_targets(pool: &Pool, route_id: &str, targets: &[RouteTargetBody]) -> Result<(), ApiError> {
    for t in targets {
        let predicate = if t.predicate.is_null() {
            "{}".to_string()
        } else {
            t.predicate.to_string()
        };
        let overrides = if t.param_overrides.is_null() {
            "{}".to_string()
        } else {
            t.param_overrides.to_string()
        };
        db::insert_route_target(
            pool,
            route_id,
            t.account_id.as_deref(),
            &t.model_id,
            t.priority,
            t.weight,
            &predicate,
            &overrides,
        )
        .await
        .map_err(ApiError::internal)?;
    }
    Ok(())
}

pub async fn delete_route(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
) -> ApiResult {
    db::delete_route(&state.pool, &id).await.map_err(ApiError::internal)?;
    let _ = db::insert_audit(&state.pool, "admin", "route_deleted", "route", &id, "", "Deleted route.").await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /admin/api/routes/dry-run` (FR-8.7): evaluate routing for a
/// representative request descriptor without mutating anything.
#[derive(Deserialize)]
pub struct DryRunBody {
    pub model: String,
    #[serde(flatten, default)]
    pub descriptor: pipeline::DryRunRequest,
}

pub async fn dry_run_route(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Json(body): Json<DryRunBody>,
) -> ApiResult {
    let out = pipeline::dry_run(&state, &body.model, &body.descriptor)
        .await
        .map_err(|e| ApiError::bad(e.message))?;
    Ok(Json(out))
}

/// `POST /admin/api/validate` (FR-8.6): validate a provider endpoint (schema,
/// TLS/SSRF, credential-host binding) and, optionally, connectivity + resolved
/// IP/ASN. Never mutates production state.
#[derive(Deserialize)]
pub struct ValidateBody {
    pub base_url: String,
    #[serde(default)]
    pub check_connectivity: bool,
}

pub async fn validate_endpoint(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Json(body): Json<ValidateBody>,
) -> ApiResult {
    validate_outbound_url(&state, &body.base_url)?;
    let parsed = url::Url::parse(&body.base_url).map_err(|e| ApiError::bad(format!("invalid URL: {e}")))?;
    let host = parsed.host_str().unwrap_or("").to_string();
    let mut resolved: Vec<String> = Vec::new();
    let mut asn: Value = Value::String("unknown".into());
    let mut reachable: Value = Value::String("not_checked".into());
    if body.check_connectivity {
        let port = parsed.port_or_known_default().unwrap_or(443);
        match tokio::net::lookup_host((host.as_str(), port)).await {
            Ok(addrs) => {
                for a in addrs {
                    resolved.push(a.ip().to_string());
                }
                reachable = Value::Bool(true);
            }
            Err(e) => {
                reachable = Value::String(format!("dns_error: {e}"));
            }
        }
        // ASN lookup is not implemented; the requirement says show unknown
        // rather than guess (FR-8.8).
        asn = Value::String("unknown".into());
    }
    Ok(Json(json!({
        "valid": true,
        "scheme": parsed.scheme(),
        "host": host,
        "resolved_ips": resolved,
        "asn": asn,
        "connectivity": reachable,
        "note": "ASN is reported as unknown when it cannot be established; Kinetix never guesses (FR-8.8)."
    })))
}

// ===========================================================================
// Aliases
// ===========================================================================

pub async fn list_aliases(State(state): State<AppState>, _auth: AdminAuth) -> ApiResult {
    let aliases = db::list_aliases(&state.pool).await.map_err(ApiError::internal)?;
    let models = db::list_models(&state.pool).await.map_err(ApiError::internal)?;
    let routes = db::list_routes(&state.pool).await.map_err(ApiError::internal)?;
    let out: Vec<Value> = aliases
        .iter()
        .map(|a| {
            let display = if a.target_type == "route" {
                routes
                    .iter()
                    .find(|c| c.id == a.target_id)
                    .map(|c| format!("Route: {}", c.name))
            } else {
                models
                    .iter()
                    .find(|m| m.id == a.target_id)
                    .map(|m| format!("Model: {}", m.display_name))
            };
            json!({
                "id": a.id,
                "alias": a.alias,
                "target_type": a.target_type,
                "target_id": a.target_id,
                "target_display_name": display,
                "description": a.description,
            })
        })
        .collect();
    Ok(Json(json!({ "aliases": out })))
}

#[derive(Deserialize)]
pub struct AliasBody {
    pub alias: String,
    pub target_type: String,
    pub target_id: String,
    #[serde(default)]
    pub description: String,
}

pub async fn create_alias(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Json(body): Json<AliasBody>,
) -> ApiResult {
    let id = db::upsert_alias(
        &state.pool,
        &body.alias,
        &body.target_type,
        &body.target_id,
        &body.description,
    )
    .await
    .map_err(ApiError::internal)?;
    let _ = db::insert_audit(
        &state.pool,
        "admin",
        "alias_upserted",
        "alias",
        &id,
        &body.alias,
        "Upserted model alias.",
    )
    .await;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "id": id })))
}

pub async fn delete_alias(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(id): Path<String>,
) -> ApiResult {
    db::delete_alias(&state.pool, &id).await.map_err(ApiError::internal)?;
    state.registry.reload(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

// ===========================================================================
// Usage / requests / audit
// ===========================================================================

#[derive(Deserialize)]
pub struct LimitQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    200
}

pub async fn usage(State(state): State<AppState>, _auth: AdminAuth, Query(q): Query<LimitQuery>) -> ApiResult {
    let rows = db::recent_usage(&state.pool, q.limit.min(2000))
        .await
        .map_err(ApiError::internal)?;
    let summary = db::usage_summary(&state.pool).await.map_err(ApiError::internal)?;
    let out: Vec<Value> = rows.iter().map(usage_json).collect();
    Ok(Json(json!({ "usage": out, "summary": summary })))
}

fn usage_json(u: &db::UsageLogRow) -> Value {
    json!({
        "id": u.id,
        "request_id": u.request_id,
        "timestamp": u.ts,
        "key_id": u.key_id,
        "key_name": u.key_name,
        "client_format": u.client_format,
        "requested_model": u.requested_model,
        "effective_model": u.effective_model,
        "route_id": u.route_id,
        "route_name": u.route_name,
        "fallback_hops": u.fallback_hops,
        "fallback_path": serde_json::from_str::<Value>(&u.fallback_path).unwrap_or(json!([])),
        "status": u.status,
        "status_code": u.status_code,
        "latency_ms": u.latency_ms,
        "ttft_ms": u.ttft_ms,
        "input_tokens": u.input_tokens,
        "output_tokens": u.output_tokens,
        "cached_tokens": u.cached_tokens,
        "thinking_tokens": u.thinking_tokens,
        "cost_usd": u.cost_usd,
        "cost_known": u.cost_known != 0,
        "cache_status": u.cache_status,
        "serving_account_id": u.serving_account_id,
        "serving_account": u.serving_account,
        "serving_provider": u.serving_provider,
        "flagged": u.flagged != 0,
        "error_message": u.error_message,
        "usage_confidence": u.usage_confidence,
        "commit_state": u.commit_state,
        "retry_count": u.retry_count,
        "opaque_route_id": u.opaque_route_id,
    })
}

/// `GET /admin/api/requests/{id}/route-trace` (FR-12.14, NFR-4.3).
pub async fn request_route_trace(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(request_id): Path<String>,
) -> ApiResult {
    let trace = db::get_route_trace_by_request(&state.pool, &request_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("no route trace for that request id"))?;
    Ok(Json(route_trace_json(&trace)))
}

/// `GET /admin/api/requests/{id}/diagnostics` (FR-13.4): correlate the Route
/// Trace with the flight-recorder events for one request.
pub async fn request_diagnostics(
    State(state): State<AppState>,
    _auth: AdminAuth,
    Path(request_id): Path<String>,
) -> ApiResult {
    let trace = db::get_route_trace_by_request(&state.pool, &request_id)
        .await
        .map_err(ApiError::internal)?;
    let flight = state.flight.events(&request_id);
    let usage = db::recent_usage(&state.pool, 2000)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .find(|u| u.request_id == request_id)
        .map(|u| usage_json(&u));
    Ok(Json(json!({
        "request_id": request_id,
        "route_trace": trace.as_ref().map(route_trace_json),
        "flight_events": flight,
        "usage": usage,
        "flight_recorder": {
            "tracked_requests": state.flight.request_count(),
            "dropped_requests": state.flight.dropped_requests(),
            "dropped_events": state.flight.dropped_events(),
        }
    })))
}

fn route_trace_json(t: &db::RouteTraceRow) -> Value {
    json!({
        "request_id": t.request_id,
        "opaque_route_id": t.opaque_route_id,
        "timestamp": t.ts,
        "requested_model": t.requested_model,
        "route_id": t.route_id,
        "route_name": t.route_name,
        "final_target": t.final_target,
        "commit_state": t.commit_state,
        "outcome": t.outcome,
        "steps": serde_json::from_str::<Value>(&t.steps).unwrap_or(json!([])),
        "warnings": serde_json::from_str::<Value>(&t.warnings).unwrap_or(json!([])),
    })
}

pub async fn audit(State(state): State<AppState>, _auth: AdminAuth, Query(q): Query<LimitQuery>) -> ApiResult {
    let rows = db::recent_audit(&state.pool, q.limit.min(2000))
        .await
        .map_err(ApiError::internal)?;
    let out: Vec<Value> = rows
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "timestamp": a.ts,
                "actor": a.actor,
                "action": a.action,
                "target_type": a.target_type,
                "target_id": a.target_id,
                "target_name": a.target_name,
                "details": a.details,
            })
        })
        .collect();
    Ok(Json(json!({ "audit": out })))
}

/// Prometheus-format metrics (NFR-4.2).
/// True when the control-plane database answers a trivial query.
pub async fn db_healthy(state: &AppState) -> bool {
    sqlx::query("SELECT 1").fetch_one(&state.pool).await.is_ok()
}

pub async fn metrics(State(state): State<AppState>, _auth: AdminAuth) -> Response {
    // Serve whatever is available from memory even when the store is down; the
    // control plane degrades, the data plane does not (NFR-2.6/2.7).
    let healthy = db_healthy(&state).await;
    let summary = if healthy {
        db::usage_summary(&state.pool).await.unwrap_or(json!({}))
    } else {
        json!({})
    };
    let accounts = if healthy {
        db::list_accounts(&state.pool).await.unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut body = String::new();
    body.push_str("# HELP kinetix_control_plane_degraded 1 when the control-plane store is unavailable\n");
    body.push_str("# TYPE kinetix_control_plane_degraded gauge\n");
    body.push_str(&format!(
        "kinetix_control_plane_degraded {}\n",
        if healthy { 0 } else { 1 }
    ));
    body.push_str("# HELP kinetix_requests_total Total proxied requests\n");
    body.push_str("# TYPE kinetix_requests_total counter\n");
    body.push_str(&format!(
        "kinetix_requests_total {}\n",
        summary["requests"].as_i64().unwrap_or(0)
    ));
    body.push_str("# HELP kinetix_cost_usd_total Total computed cost in USD\n");
    body.push_str("# TYPE kinetix_cost_usd_total counter\n");
    body.push_str(&format!(
        "kinetix_cost_usd_total {}\n",
        summary["cost_usd"].as_f64().unwrap_or(0.0)
    ));
    body.push_str("# HELP kinetix_log_queue_depth Pending usage-log rows\n");
    body.push_str("# TYPE kinetix_log_queue_depth gauge\n");
    body.push_str(&format!("kinetix_log_queue_depth {}\n", state.log_queue.depth()));
    body.push_str("# HELP kinetix_log_queue_dropped_total Dropped usage-log rows\n");
    body.push_str("# TYPE kinetix_log_queue_dropped_total counter\n");
    body.push_str(&format!(
        "kinetix_log_queue_dropped_total {}\n",
        state.log_queue.dropped()
    ));
    body.push_str("# HELP kinetix_account_status Account health by status\n");
    body.push_str("# TYPE kinetix_account_status gauge\n");
    for status in ["healthy", "cooldown", "exhausted", "disabled"] {
        let n = accounts.iter().filter(|a| a.status == status).count();
        body.push_str(&format!(
            "kinetix_account_status{{status=\"{status}\"}} {n}\n"
        ));
    }
    // Commit-point failure counters (FR-4.9, NFR-4.2).
    body.push_str("# HELP kinetix_failures_pre_commit_total Failures before the commit point\n");
    body.push_str("# TYPE kinetix_failures_pre_commit_total counter\n");
    body.push_str(&format!(
        "kinetix_failures_pre_commit_total {}\n",
        state.failures_pre_commit.load(Ordering::Relaxed)
    ));
    body.push_str("# HELP kinetix_failures_post_commit_total Failures after the commit point\n");
    body.push_str("# TYPE kinetix_failures_post_commit_total counter\n");
    body.push_str(&format!(
        "kinetix_failures_post_commit_total {}\n",
        state.failures_post_commit.load(Ordering::Relaxed)
    ));
    body.push_str("# HELP kinetix_cancellations_total Client-disconnect cancellations\n");
    body.push_str("# TYPE kinetix_cancellations_total counter\n");
    body.push_str(&format!(
        "kinetix_cancellations_total {}\n",
        state.cancellations.load(Ordering::Relaxed)
    ));
    let cancel_total = state.cancellations.load(Ordering::Relaxed);
    let cancel_ms = state.cancellation_latency_ms_total.load(Ordering::Relaxed);
    let avg_cancel = if cancel_total > 0 {
        cancel_ms as f64 / cancel_total as f64
    } else {
        0.0
    };
    body.push_str("# HELP kinetix_cancellation_latency_ms Average cancellation latency (ms)\n");
    body.push_str("# TYPE kinetix_cancellation_latency_ms gauge\n");
    body.push_str(&format!("kinetix_cancellation_latency_ms {avg_cancel}\n"));
    body.push_str("# HELP kinetix_cached_tokens_total Provider-reported cached prompt tokens\n");
    body.push_str("# TYPE kinetix_cached_tokens_total counter\n");
    body.push_str(&format!(
        "kinetix_cached_tokens_total {}\n",
        summary["cached_tokens"].as_i64().unwrap_or(0)
    ));
    body.push_str("# HELP kinetix_fallback_hops_total Total fallback hops across requests\n");
    body.push_str("# TYPE kinetix_fallback_hops_total counter\n");
    body.push_str(&format!(
        "kinetix_fallback_hops_total {}\n",
        summary["fallback_hops"].as_i64().unwrap_or(0)
    ));
    body.push_str("# HELP kinetix_flight_recorder_requests Requests tracked by the flight recorder\n");
    body.push_str("# TYPE kinetix_flight_recorder_requests gauge\n");
    body.push_str(&format!(
        "kinetix_flight_recorder_requests {}\n",
        state.flight.request_count()
    ));
    body.push_str("# HELP kinetix_flight_recorder_dropped_total Diagnostics dropped when saturated\n");
    body.push_str("# TYPE kinetix_flight_recorder_dropped_total counter\n");
    body.push_str(&format!(
        "kinetix_flight_recorder_dropped_total {}\n",
        state.flight.dropped_requests() + state.flight.dropped_events()
    ));
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response()
}

// ===========================================================================
// Helpers
// ===========================================================================

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Guardrail for admin-supplied endpoints (NFR-3.9): HTTPS by default, and
/// loopback/link-local/private/metadata ranges blocked unless explicitly allowed.
fn validate_outbound_url(state: &AppState, url: &str) -> Result<(), ApiError> {
    let parsed = url::Url::parse(url).map_err(|e| ApiError::bad(format!("invalid URL: {e}")))?;
    let host = parsed.host_str().ok_or_else(|| ApiError::bad("URL must have a host"))?;
    if state.config.allow_private_upstreams {
        return Ok(());
    }
    if parsed.scheme() != "https" {
        return Err(ApiError::bad(
            "endpoint must use https (set KINETIX_ALLOW_PRIVATE_UPSTREAMS=true to override for local development)",
        ));
    }
    if is_blocked_host(host) {
        return Err(ApiError::bad(format!(
            "host '{host}' resolves to a blocked private/metadata range; set KINETIX_ALLOW_PRIVATE_UPSTREAMS=true to allow"
        )));
    }
    Ok(())
}

fn is_blocked_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".internal") {
        return true;
    }
    if lower == "metadata.google.internal" {
        return true;
    }
    if let Ok(ip) = lower.parse::<std::net::IpAddr>() {
        return is_blocked_ip(ip);
    }
    false
}

/// Whether an IP literal falls in a blocked private/link-local/metadata range
/// (NFR-3.9). Shared with the connect-time DNS re-check in the pipeline.
pub fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.octets()[0] == 169 && v4.octets()[1] == 254
        }
        std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
    }
}
