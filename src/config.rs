//! Application configuration, loaded from environment + optional TOML file.
//!
//! The master encryption key and admin token come from the environment (or a
//! file pointed to by an env var). The TOML file is only a *bootstrap* source:
//! it seeds providers/models/virtual keys on first run. After that the database
//! is authoritative (see `docs/` requirement FR-8.5 / open issue resolution).

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    /// Public base URL of the API hostname (used in docs/UI examples).
    pub public_base_url: String,
    pub database_url: String,
    /// 32-byte master key used to encrypt upstream credentials at rest.
    pub master_key: [u8; 32],
    /// Admin password/token for the admin API + dashboard session.
    pub admin_token: String,
    /// Optional Cloudflare Access audience. When set, admin requests must carry
    /// a valid `Cf-Access-Jwt-Assertion` (defense in depth, NFR-3.2).
    pub cf_access_aud: Option<String>,
    pub cf_access_team_domain: Option<String>,
    pub log_json: bool,
    /// Bootstrap config file (providers/models/keys), if present.
    pub bootstrap_file: Option<PathBuf>,
    /// Allow outbound requests to private/loopback ranges (NFR-3.9). Off by default.
    pub allow_private_upstreams: bool,
    /// Global dev override that permits plain-HTTP upstreams. This is a visibly
    /// marked development mode (NFR-3.12); TLS is mandatory otherwise.
    pub allow_insecure_tls: bool,
    /// Directory for the embedded dashboard override (dev mode) + db backups.
    pub data_dir: PathBuf,
    /// Graceful-shutdown drain window before in-flight streams are dropped
    /// (NFR-2.3). Default 30s.
    pub shutdown_grace_secs: u64,
    /// Optional webhook URL for alerts (FR-6.6). When set, alert-worthy events
    /// are POSTed as JSON. Unset means alerting is disabled.
    pub alert_webhook_url: Option<String>,
    /// Fallback-rate alert threshold as a fraction of requests (FR-12.17).
    pub alert_fallback_rate: f64,
    /// 5xx/error-rate alert threshold as a fraction of requests.
    pub alert_error_rate: f64,
    /// Minimum requests in the window before a rate alert can fire, so a
    /// quiet instance does not alert on one request.
    pub alert_min_requests: i64,
    /// Seconds between alert evaluations.
    pub alert_interval_secs: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let bind = env_or("KINETIX_BIND", "127.0.0.1:8080");
        let public_base_url = env_or("KINETIX_PUBLIC_BASE_URL", "http://127.0.0.1:8080");
        let database_url = env_or("KINETIX_DATABASE_URL", "sqlite://kinetix.db?mode=rwc");

        let master_key = load_master_key()?;
        let admin_token = std::env::var("KINETIX_ADMIN_TOKEN").unwrap_or_default();
        if admin_token.trim().len() < 8 {
            bail!(
                "KINETIX_ADMIN_TOKEN must be set to at least 8 characters. \
                 Generate one with `openssl rand -hex 24`."
            );
        }

        let log_json = env_or("KINETIX_LOG_JSON", "false") == "true";
        let allow_private_upstreams = env_or("KINETIX_ALLOW_PRIVATE_UPSTREAMS", "false") == "true";
        let allow_insecure_tls = env_or("KINETIX_ALLOW_INSECURE_TLS", "false") == "true";
        if allow_insecure_tls {
            tracing::warn!(
                "KINETIX_ALLOW_INSECURE_TLS=true: plain-HTTP upstreams are permitted. \
                 This is a development-only mode (NFR-3.12) and must not be used in production."
            );
        }

        let bootstrap_file = std::env::var("KINETIX_BOOTSTRAP_FILE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);

        let data_dir = PathBuf::from(env_or("KINETIX_DATA_DIR", "."));
        let shutdown_grace_secs = env_or("KINETIX_SHUTDOWN_GRACE_SECS", "30")
            .parse::<u64>()
            .unwrap_or(30);
        let alert_webhook_url = std::env::var("KINETIX_ALERT_WEBHOOK_URL")
            .ok()
            .filter(|u| !u.trim().is_empty());
        let alert_fallback_rate = env_or("KINETIX_ALERT_FALLBACK_RATE", "0.25")
            .parse::<f64>()
            .unwrap_or(0.25);
        let alert_error_rate = env_or("KINETIX_ALERT_ERROR_RATE", "0.10")
            .parse::<f64>()
            .unwrap_or(0.10);
        let alert_min_requests = env_or("KINETIX_ALERT_MIN_REQUESTS", "20")
            .parse::<i64>()
            .unwrap_or(20);
        let alert_interval_secs = env_or("KINETIX_ALERT_INTERVAL_SECS", "60")
            .parse::<u64>()
            .unwrap_or(60);

        Ok(Config {
            bind,
            public_base_url,
            database_url,
            master_key,
            admin_token,
            cf_access_aud: std::env::var("KINETIX_CF_ACCESS_AUD")
                .ok()
                .filter(|s| !s.is_empty()),
            cf_access_team_domain: std::env::var("KINETIX_CF_ACCESS_TEAM_DOMAIN")
                .ok()
                .filter(|s| !s.is_empty()),
            log_json,
            bootstrap_file,
            allow_private_upstreams,
            allow_insecure_tls,
            data_dir,
            shutdown_grace_secs,
            alert_webhook_url,
            alert_fallback_rate,
            alert_error_rate,
            alert_min_requests,
            alert_interval_secs,
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Load the 32-byte master key. Accepts `KINETIX_MASTER_KEY` (64 hex chars or
/// base64) or `KINETIX_MASTER_KEY_FILE` pointing to a file with the same.
fn load_master_key() -> Result<[u8; 32]> {
    let raw = if let Ok(v) = std::env::var("KINETIX_MASTER_KEY") {
        v
    } else if let Ok(path) = std::env::var("KINETIX_MASTER_KEY_FILE") {
        std::fs::read_to_string(&path)
            .with_context(|| format!("reading KINETIX_MASTER_KEY_FILE at {path}"))?
    } else {
        bail!(
            "KINETIX_MASTER_KEY (or KINETIX_MASTER_KEY_FILE) must be set. \
             Generate with `openssl rand -hex 32`."
        );
    };
    let raw = raw.trim();

    // Try hex first (64 chars), then base64.
    if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        let bytes = hex::decode(raw).context("master key is not valid hex")?;
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        return Ok(out);
    }

    use base64::Engine;
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(raw) {
        if bytes.len() == 32 {
            let mut out = [0u8; 32];
            out.copy_from_slice(&bytes);
            return Ok(out);
        }
    }

    // Last resort: derive from a passphrase with SHA-256 so a human-readable
    // secret still works (with a warning).
    if raw.len() >= 16 {
        use sha2::{Digest, Sha256};
        tracing::warn!("KINETIX_MASTER_KEY is not 32 bytes of hex/base64; deriving via SHA-256");
        let digest = Sha256::digest(raw.as_bytes());
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        return Ok(out);
    }

    bail!("KINETIX_MASTER_KEY must be 64 hex chars, base64 of 32 bytes, or >=16 chars")
}

// ---------------------------------------------------------------------------
// Bootstrap file schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapConfig {
    #[serde(default)]
    pub virtual_keys: Vec<BootstrapKey>,
    #[serde(default)]
    pub providers: Vec<BootstrapProvider>,
    #[serde(default)]
    pub aliases: Vec<BootstrapAlias>,
    #[serde(default)]
    pub routes: Vec<BootstrapRoute>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapKey {
    pub name: String,
    /// Full virtual key value. If omitted, one is generated and logged.
    pub key: Option<String>,
    pub owner: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default = "default_wildcard")]
    pub allowed_models: Vec<String>,
    #[serde(default)]
    pub rpm_limit: Option<u32>,
    #[serde(default)]
    pub tpm_limit: Option<u32>,
    #[serde(default)]
    pub daily_budget: Option<f64>,
    #[serde(default)]
    pub monthly_budget: Option<f64>,
}

fn default_wildcard() -> Vec<String> {
    vec!["*".to_string()]
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapProvider {
    pub name: String,
    pub base_url: String,
    /// `openai` | `anthropic` | `gemini`
    pub wire_format: String,
    /// `bearer` | `custom_header` | `query_param`
    #[serde(default = "default_bearer")]
    pub auth_scheme: String,
    #[serde(default)]
    pub custom_header_name: Option<String>,
    #[serde(default)]
    pub custom_param_name: Option<String>,
    #[serde(default)]
    pub extra_headers: std::collections::HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_permissive")]
    pub capability_mode: String,
    /// Upstream credentials (API keys) to seed into the pool.
    #[serde(default)]
    pub accounts: Vec<BootstrapAccount>,
    #[serde(default)]
    pub models: Vec<BootstrapModel>,
    /// Model-list endpoint path override for discovery.
    #[serde(default)]
    pub models_path: Option<String>,
}

fn default_bearer() -> String {
    "bearer".into()
}
fn default_timeout() -> u64 {
    120_000
}
fn default_permissive() -> String {
    "permissive".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapAccount {
    pub label: String,
    pub api_key: String,
    #[serde(default = "default_priority")]
    pub priority: i64,
    #[serde(default)]
    pub soft_quota_usd: Option<f64>,
    #[serde(default = "default_quota_type")]
    pub quota_type: String,
}

fn default_priority() -> i64 {
    1
}
fn default_quota_type() -> String {
    "none".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapModel {
    pub upstream_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub context_window: Option<i64>,
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
    #[serde(default)]
    pub prices: Option<BootstrapPrices>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BootstrapPrices {
    #[serde(default)]
    pub input_per_1m: Option<f64>,
    #[serde(default)]
    pub output_per_1m: Option<f64>,
    #[serde(default)]
    pub cached_per_1m: Option<f64>,
    #[serde(default)]
    pub thinking_per_1m: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapAlias {
    pub alias: String,
    /// `model` (provider/model-id) or `route` (route name)
    pub target_type: String,
    pub target: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapRoute {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_priority_strategy")]
    pub strategy: String,
    #[serde(default)]
    pub continuity_policy: String,
    /// FR-2.11: reject | strip_with_warning.
    #[serde(default)]
    pub portability_policy: String,
    /// FR-7.3: cache-aware sticky routing.
    #[serde(default)]
    pub cache_affinity: bool,
    #[serde(default)]
    pub sticky_routing: bool,
    #[serde(default)]
    pub max_attempts: Option<i64>,
    #[serde(default)]
    pub targets: Vec<BootstrapRouteTarget>,
}

fn default_priority_strategy() -> String {
    "priority".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapRouteTarget {
    /// Account label (must be unique across providers).
    pub account: String,
    /// `provider/model-id`
    pub model: String,
    #[serde(default = "default_priority")]
    pub priority: i64,
    #[serde(default)]
    pub weight: Option<i64>,
    /// Typed eligibility predicate (FR-12.3), as an inline TOML/JSON value.
    #[serde(default)]
    pub predicate: Option<toml::Value>,
}

pub fn load_bootstrap(path: &std::path::Path) -> Result<BootstrapConfig> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading bootstrap config {}", path.display()))?;
    let cfg: BootstrapConfig = toml::from_str(&text)
        .with_context(|| format!("parsing bootstrap config {}", path.display()))?;
    Ok(cfg)
}
