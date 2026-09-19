//! Database layer: connection pool, migrations, and typed access to every
//! configuration and logging table. SQLite in WAL mode via `sqlx`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, Row, SqlitePool};
use std::collections::HashMap;
use std::str::FromStr;

use crate::types::{AuthScheme, Capabilities, ParamSpec, Prices, ThinkingMap, WireFormat};

pub type Pool = SqlitePool;

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

pub async fn connect(database_url: &str) -> Result<Pool> {
    let opts = SqliteConnectOptions::from_str(database_url)
        .with_context(|| format!("invalid database url {database_url}"))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(10))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(16)
        .connect_with(opts)
        .await
        .context("connecting to sqlite")?;
    Ok(pool)
}

pub async fn migrate(pool: &Pool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("running migrations")?;
    Ok(())
}

// ===========================================================================
// Virtual keys
// ===========================================================================

#[derive(Debug, Clone, FromRow)]
pub struct VirtualKeyRow {
    pub id: String,
    pub key_hash: String,
    pub name: String,
    pub owner: String,
    pub tag: String,
    pub allowed_models: String,
    pub allowed_providers: String,
    pub rpm_limit: Option<i64>,
    pub tpm_limit: Option<i64>,
    pub daily_budget: Option<f64>,
    pub monthly_budget: Option<f64>,
    pub expires_at: Option<String>,
    pub status: String,
    pub allowed_ips: String,
    pub body_logging: i64,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

impl VirtualKeyRow {
    pub fn allowed_models(&self) -> Vec<String> {
        serde_json::from_str(&self.allowed_models).unwrap_or_else(|_| vec!["*".into()])
    }
    pub fn allowed_providers(&self) -> Vec<String> {
        serde_json::from_str(&self.allowed_providers).unwrap_or_default()
    }
    pub fn allowed_ips(&self) -> Vec<String> {
        serde_json::from_str(&self.allowed_ips).unwrap_or_default()
    }
    /// Whether a requested model name (alias/route/model) is permitted.
    pub fn permits_model(&self, model: &str) -> bool {
        let allowed = self.allowed_models();
        if allowed.iter().any(|m| m == "*") {
            return true;
        }
        allowed.iter().any(|m| {
            if let Some(prefix) = m.strip_suffix('*') {
                model.starts_with(prefix)
            } else {
                m == model
            }
        })
    }
}

pub async fn insert_virtual_key(pool: &Pool, k: &VirtualKeyRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO virtual_keys
         (id, key_hash, name, owner, tag, allowed_models, allowed_providers, rpm_limit, tpm_limit,
          daily_budget, monthly_budget, expires_at, status, allowed_ips, body_logging, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&k.id)
    .bind(&k.key_hash)
    .bind(&k.name)
    .bind(&k.owner)
    .bind(&k.tag)
    .bind(&k.allowed_models)
    .bind(&k.allowed_providers)
    .bind(k.rpm_limit)
    .bind(k.tpm_limit)
    .bind(k.daily_budget)
    .bind(k.monthly_budget)
    .bind(&k.expires_at)
    .bind(&k.status)
    .bind(&k.allowed_ips)
    .bind(k.body_logging)
    .bind(&k.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_virtual_keys(pool: &Pool) -> Result<Vec<VirtualKeyRow>> {
    Ok(sqlx::query_as::<_, VirtualKeyRow>(
        "SELECT * FROM virtual_keys ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get_virtual_key_by_hash(pool: &Pool, hash: &str) -> Result<Option<VirtualKeyRow>> {
    Ok(
        sqlx::query_as::<_, VirtualKeyRow>("SELECT * FROM virtual_keys WHERE key_hash = ?")
            .bind(hash)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn get_virtual_key_by_id(pool: &Pool, id: &str) -> Result<Option<VirtualKeyRow>> {
    Ok(
        sqlx::query_as::<_, VirtualKeyRow>("SELECT * FROM virtual_keys WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn set_virtual_key_status(pool: &Pool, id: &str, status: &str) -> Result<()> {
    let revoked_at = if status == "revoked" {
        Some(now_iso())
    } else {
        None
    };
    sqlx::query("UPDATE virtual_keys SET status = ?, revoked_at = COALESCE(?, revoked_at) WHERE id = ?")
        .bind(status)
        .bind(revoked_at)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_virtual_key(pool: &Pool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM virtual_keys WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ===========================================================================
// Providers
// ===========================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ProviderRow {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub wire_format: String,
    pub auth_scheme: String,
    pub custom_header_name: Option<String>,
    pub custom_param_name: Option<String>,
    pub extra_headers: String,
    pub timeout_ms: i64,
    pub capability_mode: String,
    pub models_path: Option<String>,
    pub rate_limit_rules: String,
    pub enabled: i64,
    pub created_at: String,
}

impl ProviderRow {
    pub fn wire(&self) -> WireFormat {
        WireFormat::parse(&self.wire_format).unwrap_or(WireFormat::Openai)
    }
    pub fn auth(&self) -> AuthScheme {
        AuthScheme::parse(&self.auth_scheme).unwrap_or(AuthScheme::Bearer)
    }
    pub fn extra_headers_map(&self) -> HashMap<String, String> {
        serde_json::from_str(&self.extra_headers).unwrap_or_default()
    }
    pub fn strict(&self) -> bool {
        self.capability_mode == "strict"
    }
}

pub async fn list_providers(pool: &Pool) -> Result<Vec<ProviderRow>> {
    Ok(sqlx::query_as::<_, ProviderRow>("SELECT * FROM providers ORDER BY created_at")
        .fetch_all(pool)
        .await?)
}

pub async fn get_provider(pool: &Pool, id: &str) -> Result<Option<ProviderRow>> {
    Ok(
        sqlx::query_as::<_, ProviderRow>("SELECT * FROM providers WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub struct NewProvider<'a> {
    pub name: &'a str,
    pub base_url: &'a str,
    pub wire_format: WireFormat,
    pub auth_scheme: AuthScheme,
    pub custom_header_name: Option<&'a str>,
    pub custom_param_name: Option<&'a str>,
    pub extra_headers: Value,
    pub timeout_ms: i64,
    pub capability_mode: &'a str,
    pub models_path: Option<&'a str>,
    pub rate_limit_rules: Value,
}

pub async fn insert_provider(pool: &Pool, p: &NewProvider<'_>) -> Result<String> {
    let id = format!("prov_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO providers
         (id, name, base_url, wire_format, auth_scheme, custom_header_name, custom_param_name,
          extra_headers, timeout_ms, capability_mode, models_path, rate_limit_rules, enabled, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,1,?)",
    )
    .bind(&id)
    .bind(p.name)
    .bind(p.base_url)
    .bind(p.wire_format.as_str())
    .bind(auth_scheme_str(p.auth_scheme))
    .bind(p.custom_header_name)
    .bind(p.custom_param_name)
    .bind(p.extra_headers.to_string())
    .bind(p.timeout_ms)
    .bind(p.capability_mode)
    .bind(p.models_path)
    .bind(p.rate_limit_rules.to_string())
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok(id)
}

pub fn auth_scheme_str(s: AuthScheme) -> &'static str {
    match s {
        AuthScheme::Bearer => "bearer",
        AuthScheme::CustomHeader => "custom_header",
        AuthScheme::QueryParam => "query_param",
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn update_provider(
    pool: &Pool,
    id: &str,
    name: &str,
    base_url: &str,
    wire_format: WireFormat,
    auth_scheme: AuthScheme,
    custom_header_name: Option<&str>,
    custom_param_name: Option<&str>,
    extra_headers: Value,
    timeout_ms: i64,
    capability_mode: &str,
    models_path: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE providers SET name=?, base_url=?, wire_format=?, auth_scheme=?, custom_header_name=?,
         custom_param_name=?, extra_headers=?, timeout_ms=?, capability_mode=?, models_path=? WHERE id=?",
    )
    .bind(name)
    .bind(base_url)
    .bind(wire_format.as_str())
    .bind(auth_scheme_str(auth_scheme))
    .bind(custom_header_name)
    .bind(custom_param_name)
    .bind(extra_headers.to_string())
    .bind(timeout_ms)
    .bind(capability_mode)
    .bind(models_path)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_provider(pool: &Pool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM providers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ===========================================================================
// Accounts
// ===========================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AccountRow {
    pub id: String,
    pub provider_id: String,
    pub label: String,
    #[serde(skip)]
    pub secret_enc: String,
    pub key_mask: String,
    pub status: String,
    pub cooldown_until: Option<String>,
    pub quota_reset_at: Option<String>,
    pub quota_type: String,
    pub quota_window_s: Option<i64>,
    pub soft_quota_usd: Option<f64>,
    pub priority: i64,
    pub weight: i64,
    pub last_error: Option<String>,
    pub last_probe_at: Option<String>,
    pub created_at: String,
}

pub async fn list_accounts(pool: &Pool) -> Result<Vec<AccountRow>> {
    Ok(sqlx::query_as::<_, AccountRow>(
        "SELECT * FROM accounts ORDER BY priority, created_at",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn accounts_for_provider(pool: &Pool, provider_id: &str) -> Result<Vec<AccountRow>> {
    Ok(sqlx::query_as::<_, AccountRow>(
        "SELECT * FROM accounts WHERE provider_id = ? AND status != 'disabled' ORDER BY priority",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_account(pool: &Pool, id: &str) -> Result<Option<AccountRow>> {
    Ok(
        sqlx::query_as::<_, AccountRow>("SELECT * FROM accounts WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn insert_account(
    pool: &Pool,
    provider_id: &str,
    label: &str,
    secret_enc: &str,
    key_mask: &str,
    priority: i64,
    weight: i64,
    soft_quota_usd: Option<f64>,
    quota_type: &str,
) -> Result<String> {
    let id = format!("acc_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO accounts
         (id, provider_id, label, secret_enc, key_mask, status, quota_type, soft_quota_usd, priority, weight, created_at)
         VALUES (?,?,?,?,?,'healthy',?,?,?,?,?)",
    )
    .bind(&id)
    .bind(provider_id)
    .bind(label)
    .bind(secret_enc)
    .bind(key_mask)
    .bind(quota_type)
    .bind(soft_quota_usd)
    .bind(priority)
    .bind(weight)
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn update_account(
    pool: &Pool,
    id: &str,
    label: &str,
    status: &str,
    priority: i64,
    weight: i64,
    soft_quota_usd: Option<f64>,
    quota_type: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE accounts SET label=?, status=?, priority=?, weight=?, soft_quota_usd=?, quota_type=? WHERE id=?",
    )
    .bind(label)
    .bind(status)
    .bind(priority)
    .bind(weight)
    .bind(soft_quota_usd)
    .bind(quota_type)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_account_status(
    pool: &Pool,
    id: &str,
    status: &str,
    cooldown_until: Option<&str>,
    quota_reset_at: Option<&str>,
    last_error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE accounts SET status=?, cooldown_until=?, quota_reset_at=?, last_error=? WHERE id=?",
    )
    .bind(status)
    .bind(cooldown_until)
    .bind(quota_reset_at)
    .bind(last_error)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_account(pool: &Pool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ===========================================================================
// Models
// ===========================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ModelRow {
    pub id: String,
    pub provider_id: String,
    pub upstream_id: String,
    pub display_name: String,
    pub enabled: i64,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub capabilities: String,
    pub prices: String,
    pub parameters: String,
    pub thinking_map: String,
    pub extra_request: String,
    pub discovery: String,
    pub created_at: String,
}

impl ModelRow {
    pub fn caps(&self) -> Capabilities {
        serde_json::from_str(&self.capabilities).unwrap_or_default()
    }
    pub fn prices(&self) -> Prices {
        serde_json::from_str(&self.prices).unwrap_or_default()
    }
    pub fn params(&self) -> HashMap<String, ParamSpec> {
        serde_json::from_str(&self.parameters).unwrap_or_default()
    }
    pub fn thinking(&self) -> ThinkingMap {
        serde_json::from_str(&self.thinking_map).unwrap_or_default()
    }
    pub fn extra_request_value(&self) -> Value {
        serde_json::from_str(&self.extra_request).unwrap_or(Value::Null)
    }
}

pub async fn list_models(pool: &Pool) -> Result<Vec<ModelRow>> {
    Ok(sqlx::query_as::<_, ModelRow>("SELECT * FROM models ORDER BY created_at")
        .fetch_all(pool)
        .await?)
}

pub async fn models_for_provider(pool: &Pool, provider_id: &str) -> Result<Vec<ModelRow>> {
    Ok(
        sqlx::query_as::<_, ModelRow>("SELECT * FROM models WHERE provider_id = ?")
            .bind(provider_id)
            .fetch_all(pool)
            .await?,
    )
}

pub async fn get_model(pool: &Pool, id: &str) -> Result<Option<ModelRow>> {
    Ok(sqlx::query_as::<_, ModelRow>("SELECT * FROM models WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub async fn find_model_by_upstream(
    pool: &Pool,
    provider_id: &str,
    upstream_id: &str,
) -> Result<Option<ModelRow>> {
    Ok(sqlx::query_as::<_, ModelRow>(
        "SELECT * FROM models WHERE provider_id = ? AND upstream_id = ?",
    )
    .bind(provider_id)
    .bind(upstream_id)
    .fetch_optional(pool)
    .await?)
}

pub struct NewModel<'a> {
    pub provider_id: &'a str,
    pub upstream_id: &'a str,
    pub display_name: &'a str,
    pub enabled: bool,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub capabilities: Value,
    pub prices: Value,
    pub parameters: Value,
    pub thinking_map: Value,
    pub extra_request: Value,
    pub discovery: Value,
}

pub async fn insert_model(pool: &Pool, m: &NewModel<'_>) -> Result<String> {
    let id = format!("model_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO models
         (id, provider_id, upstream_id, display_name, enabled, context_window, max_output_tokens,
          capabilities, prices, parameters, thinking_map, extra_request, discovery, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(m.provider_id)
    .bind(m.upstream_id)
    .bind(m.display_name)
    .bind(m.enabled as i64)
    .bind(m.context_window)
    .bind(m.max_output_tokens)
    .bind(m.capabilities.to_string())
    .bind(m.prices.to_string())
    .bind(m.parameters.to_string())
    .bind(m.thinking_map.to_string())
    .bind(m.extra_request.to_string())
    .bind(m.discovery.to_string())
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_model(
    pool: &Pool,
    id: &str,
    display_name: &str,
    enabled: bool,
    context_window: Option<i64>,
    max_output_tokens: Option<i64>,
    capabilities: Value,
    prices: Value,
    parameters: Value,
    thinking_map: Value,
    extra_request: Value,
) -> Result<()> {
    sqlx::query(
        "UPDATE models SET display_name=?, enabled=?, context_window=?, max_output_tokens=?,
         capabilities=?, prices=?, parameters=?, thinking_map=?, extra_request=? WHERE id=?",
    )
    .bind(display_name)
    .bind(enabled as i64)
    .bind(context_window)
    .bind(max_output_tokens)
    .bind(capabilities.to_string())
    .bind(prices.to_string())
    .bind(parameters.to_string())
    .bind(thinking_map.to_string())
    .bind(extra_request.to_string())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_model(pool: &Pool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM models WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Record a price version snapshot (FR-6.3).
pub async fn insert_price_version(pool: &Pool, model_id: &str, p: &Prices) -> Result<String> {
    let id = format!("price_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO price_versions (id, model_id, input_per_1m, output_per_1m, cached_per_1m, thinking_per_1m, created_at)
         VALUES (?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(model_id)
    .bind(p.input_per_1m)
    .bind(p.output_per_1m)
    .bind(p.cached_per_1m)
    .bind(p.thinking_per_1m)
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok(id)
}

// ===========================================================================
// Aliases
// ===========================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AliasRow {
    pub id: String,
    pub alias: String,
    pub target_type: String,
    pub target_id: String,
    pub description: String,
    pub created_at: String,
}

pub async fn list_aliases(pool: &Pool) -> Result<Vec<AliasRow>> {
    Ok(sqlx::query_as::<_, AliasRow>("SELECT * FROM aliases ORDER BY alias")
        .fetch_all(pool)
        .await?)
}

pub async fn get_alias(pool: &Pool, alias: &str) -> Result<Option<AliasRow>> {
    Ok(
        sqlx::query_as::<_, AliasRow>("SELECT * FROM aliases WHERE alias = ?")
            .bind(alias)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn upsert_alias(
    pool: &Pool,
    alias: &str,
    target_type: &str,
    target_id: &str,
    description: &str,
) -> Result<String> {
    if let Some(existing) = get_alias(pool, alias).await? {
        sqlx::query("UPDATE aliases SET target_type=?, target_id=?, description=? WHERE id=?")
            .bind(target_type)
            .bind(target_id)
            .bind(description)
            .bind(&existing.id)
            .execute(pool)
            .await?;
        return Ok(existing.id);
    }
    let id = format!("alias_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO aliases (id, alias, target_type, target_id, description, created_at) VALUES (?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(alias)
    .bind(target_type)
    .bind(target_id)
    .bind(description)
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn delete_alias(pool: &Pool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM aliases WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ===========================================================================
// Routes
// ===========================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RouteRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub strategy: String,
    pub fallback_triggers: String,
    pub continuity_policy: String,
    pub sticky_routing: i64,
    pub max_attempts: Option<i64>,
    pub enabled: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RouteTargetRow {
    pub id: String,
    pub route_id: String,
    pub account_id: Option<String>,
    pub model_id: String,
    pub priority: i64,
    pub weight: i64,
    pub param_overrides: String,
}

pub async fn list_routes(pool: &Pool) -> Result<Vec<RouteRow>> {
    Ok(sqlx::query_as::<_, RouteRow>("SELECT * FROM routes ORDER BY created_at")
        .fetch_all(pool)
        .await?)
}

pub async fn get_route(pool: &Pool, id: &str) -> Result<Option<RouteRow>> {
    Ok(sqlx::query_as::<_, RouteRow>("SELECT * FROM routes WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub async fn get_route_by_name(pool: &Pool, name: &str) -> Result<Option<RouteRow>> {
    Ok(
        sqlx::query_as::<_, RouteRow>("SELECT * FROM routes WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn route_targets(pool: &Pool, route_id: &str) -> Result<Vec<RouteTargetRow>> {
    Ok(sqlx::query_as::<_, RouteTargetRow>(
        "SELECT * FROM route_targets WHERE route_id = ? ORDER BY priority, weight DESC",
    )
    .bind(route_id)
    .fetch_all(pool)
    .await?)
}

pub struct NewRoute<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub strategy: &'a str,
    pub fallback_triggers: Value,
    pub continuity_policy: &'a str,
    pub sticky_routing: bool,
    pub max_attempts: Option<i64>,
}

pub async fn insert_route(pool: &Pool, c: &NewRoute<'_>) -> Result<String> {
    let id = format!("route_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO routes (id, name, description, strategy, fallback_triggers, continuity_policy, sticky_routing, max_attempts, enabled, created_at)
         VALUES (?,?,?,?,?,?,?,?,1,?)",
    )
    .bind(&id)
    .bind(c.name)
    .bind(c.description)
    .bind(c.strategy)
    .bind(c.fallback_triggers.to_string())
    .bind(c.continuity_policy)
    .bind(c.sticky_routing as i64)
    .bind(c.max_attempts)
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn update_route(
    pool: &Pool,
    id: &str,
    description: &str,
    strategy: &str,
    fallback_triggers: Value,
    continuity_policy: &str,
    sticky_routing: bool,
    max_attempts: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "UPDATE routes SET description=?, strategy=?, fallback_triggers=?, continuity_policy=?, sticky_routing=?, max_attempts=? WHERE id=?",
    )
    .bind(description)
    .bind(strategy)
    .bind(fallback_triggers.to_string())
    .bind(continuity_policy)
    .bind(sticky_routing as i64)
    .bind(max_attempts)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_route_targets(pool: &Pool, route_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM route_targets WHERE route_id = ?")
        .bind(route_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn insert_route_target(
    pool: &Pool,
    route_id: &str,
    account_id: Option<&str>,
    model_id: &str,
    priority: i64,
    weight: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO route_targets (id, route_id, account_id, model_id, priority, weight, param_overrides)
         VALUES (?,?,?,?,?,?,'{}')",
    )
    .bind(format!("tgt_{}", uuid::Uuid::new_v4().simple()))
    .bind(route_id)
    .bind(account_id)
    .bind(model_id)
    .bind(priority)
    .bind(weight)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_route(pool: &Pool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM routes WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ===========================================================================
// Usage logs
// ===========================================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct UsageLogRow {
    pub id: String,
    pub request_id: String,
    pub ts: String,
    pub key_id: Option<String>,
    pub key_name: Option<String>,
    pub client_format: String,
    pub requested_model: String,
    pub effective_model: Option<String>,
    pub route_id: Option<String>,
    pub route_name: Option<String>,
    pub fallback_hops: i64,
    pub fallback_path: String,
    pub status: String,
    pub status_code: i64,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub thinking_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
    pub cost_known: i64,
    pub price_version_id: Option<String>,
    pub cache_status: String,
    pub serving_account_id: Option<String>,
    pub serving_account: Option<String>,
    pub serving_provider: Option<String>,
    pub upstream_request_id: Option<String>,
    pub flagged: i64,
    pub error_message: Option<String>,
}

pub async fn insert_usage_log(pool: &Pool, u: &UsageLogRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO usage_logs
        (id, request_id, ts, key_id, key_name, client_format, requested_model, effective_model, route_id,
         route_name, fallback_hops, fallback_path, status, status_code, latency_ms, ttft_ms, input_tokens,
         output_tokens, cached_tokens, thinking_tokens, cost_usd, cost_known, price_version_id, cache_status,
         serving_account_id, serving_account, serving_provider, upstream_request_id, flagged, error_message)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&u.id)
    .bind(&u.request_id)
    .bind(&u.ts)
    .bind(&u.key_id)
    .bind(&u.key_name)
    .bind(&u.client_format)
    .bind(&u.requested_model)
    .bind(&u.effective_model)
    .bind(&u.route_id)
    .bind(&u.route_name)
    .bind(u.fallback_hops)
    .bind(&u.fallback_path)
    .bind(&u.status)
    .bind(u.status_code)
    .bind(u.latency_ms)
    .bind(u.ttft_ms)
    .bind(u.input_tokens)
    .bind(u.output_tokens)
    .bind(u.cached_tokens)
    .bind(u.thinking_tokens)
    .bind(u.cost_usd)
    .bind(u.cost_known)
    .bind(&u.price_version_id)
    .bind(&u.cache_status)
    .bind(&u.serving_account_id)
    .bind(&u.serving_account)
    .bind(&u.serving_provider)
    .bind(&u.upstream_request_id)
    .bind(u.flagged)
    .bind(&u.error_message)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn recent_usage(pool: &Pool, limit: i64) -> Result<Vec<UsageLogRow>> {
    Ok(sqlx::query_as::<_, UsageLogRow>(
        "SELECT * FROM usage_logs ORDER BY ts DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn usage_summary(pool: &Pool) -> Result<Value> {
    let row = sqlx::query(
        "SELECT
            COUNT(*) as requests,
            COALESCE(SUM(input_tokens),0) as input_tokens,
            COALESCE(SUM(output_tokens),0) as output_tokens,
            COALESCE(SUM(cached_tokens),0) as cached_tokens,
            COALESCE(SUM(thinking_tokens),0) as thinking_tokens,
            COALESCE(SUM(cost_usd),0.0) as cost_usd,
            COALESCE(SUM(fallback_hops),0) as fallback_hops,
            COALESCE(AVG(latency_ms),0.0) as avg_latency
         FROM usage_logs",
    )
    .fetch_one(pool)
    .await?;
    Ok(serde_json::json!({
        "requests": row.get::<i64, _>("requests"),
        "input_tokens": row.get::<i64, _>("input_tokens"),
        "output_tokens": row.get::<i64, _>("output_tokens"),
        "cached_tokens": row.get::<i64, _>("cached_tokens"),
        "thinking_tokens": row.get::<i64, _>("thinking_tokens"),
        "cost_usd": row.get::<f64, _>("cost_usd"),
        "fallback_hops": row.get::<i64, _>("fallback_hops"),
        "avg_latency_ms": row.get::<f64, _>("avg_latency"),
    }))
}

/// Sum of cost for a key within a time window (ISO timestamp lower bound).
pub async fn key_spend_since(pool: &Pool, key_id: &str, since_iso: &str) -> Result<f64> {
    let row = sqlx::query(
        "SELECT COALESCE(SUM(cost_usd),0.0) as total FROM usage_logs WHERE key_id = ? AND ts >= ?",
    )
    .bind(key_id)
    .bind(since_iso)
    .fetch_one(pool)
    .await?;
    Ok(row.get::<f64, _>("total"))
}

/// Sum of cost for an account within a time window (for soft quotas).
pub async fn account_spend_since(
    pool: &Pool,
    account_id: &str,
    since_iso: &str,
) -> Result<f64> {
    let row = sqlx::query(
        "SELECT COALESCE(SUM(cost_usd),0.0) as total FROM usage_logs WHERE serving_account_id = ? AND ts >= ?",
    )
    .bind(account_id)
    .bind(since_iso)
    .fetch_one(pool)
    .await?;
    Ok(row.get::<f64, _>("total"))
}

/// Count of requests for a key since a timestamp (for RPM/TPM windows).
pub async fn key_usage_since(
    pool: &Pool,
    key_id: &str,
    since_iso: &str,
) -> Result<(i64, i64)> {
    let row = sqlx::query(
        "SELECT COUNT(*) as n, COALESCE(SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)),0) as t
         FROM usage_logs WHERE key_id = ? AND ts >= ?",
    )
    .bind(key_id)
    .bind(since_iso)
    .fetch_one(pool)
    .await?;
    Ok((row.get::<i64, _>("n"), row.get::<i64, _>("t")))
}

// ===========================================================================
// Lifetime totals (for the dashboard)
// ===========================================================================

/// (requests, total tokens) grouped by key_id and by serving_account_id.
pub async fn lifetime_totals(
    pool: &Pool,
) -> Result<(HashMap<String, (i64, i64)>, HashMap<String, (i64, i64)>)> {
    let mut by_key: HashMap<String, (i64, i64)> = HashMap::new();
    let rows = sqlx::query(
        "SELECT key_id, COUNT(*) as n, COALESCE(SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)),0) as t
         FROM usage_logs WHERE key_id IS NOT NULL GROUP BY key_id",
    )
    .fetch_all(pool)
    .await?;
    for r in rows {
        by_key.insert(
            r.get::<String, _>("key_id"),
            (r.get::<i64, _>("n"), r.get::<i64, _>("t")),
        );
    }

    let mut by_account: HashMap<String, (i64, i64)> = HashMap::new();
    let rows = sqlx::query(
        "SELECT serving_account_id, COUNT(*) as n, COALESCE(SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)),0) as t
         FROM usage_logs WHERE serving_account_id IS NOT NULL GROUP BY serving_account_id",
    )
    .fetch_all(pool)
    .await?;
    for r in rows {
        by_account.insert(
            r.get::<String, _>("serving_account_id"),
            (r.get::<i64, _>("n"), r.get::<i64, _>("t")),
        );
    }

    Ok((by_key, by_account))
}

/// (requests, total tokens) grouped by serving_account_id and by key_id.
pub async fn request_counts_by_key(
    pool: &Pool,
) -> Result<HashMap<String, i64>> {
    let mut out: HashMap<String, i64> = HashMap::new();
    let rows = sqlx::query(
        "SELECT key_id, COUNT(*) as n FROM usage_logs WHERE key_id IS NOT NULL GROUP BY key_id",
    )
    .fetch_all(pool)
    .await?;
    for r in rows {
        out.insert(r.get::<String, _>("key_id"), r.get::<i64, _>("n"));
    }
    Ok(out)
}

// ===========================================================================
// Audit logs
// ===========================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AuditLogRow {
    pub id: String,
    pub ts: String,
    pub actor: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub target_name: String,
    pub details: String,
}

pub async fn insert_audit(
    pool: &Pool,
    actor: &str,
    action: &str,
    target_type: &str,
    target_id: &str,
    target_name: &str,
    details: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO audit_logs (id, ts, actor, action, target_type, target_id, target_name, details)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(format!("audit_{}", uuid::Uuid::new_v4().simple()))
    .bind(now_iso())
    .bind(actor)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(target_name)
    .bind(details)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn recent_audit(pool: &Pool, limit: i64) -> Result<Vec<AuditLogRow>> {
    Ok(sqlx::query_as::<_, AuditLogRow>(
        "SELECT * FROM audit_logs ORDER BY ts DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

// ===========================================================================
// Body logs
// ===========================================================================

pub async fn insert_body_log(
    pool: &Pool,
    request_id: &str,
    key_id: &str,
    direction: &str,
    body: &str,
    retention_days: i64,
) -> Result<()> {
    let expires = Utc::now() + chrono::Duration::days(retention_days);
    sqlx::query(
        "INSERT INTO body_logs (id, request_id, ts, key_id, direction, body, expires_at) VALUES (?,?,?,?,?,?,?)",
    )
    .bind(format!("body_{}", uuid::Uuid::new_v4().simple()))
    .bind(request_id)
    .bind(now_iso())
    .bind(key_id)
    .bind(direction)
    .bind(body)
    .bind(expires.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn purge_expired_body_logs(pool: &Pool) -> Result<u64> {
    let res = sqlx::query("DELETE FROM body_logs WHERE expires_at < ?")
        .bind(now_iso())
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

// ===========================================================================
// Settings
// ===========================================================================

pub async fn get_setting(pool: &Pool, key: &str) -> Result<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<String, _>("value")))
}

pub async fn set_setting(pool: &Pool, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}
