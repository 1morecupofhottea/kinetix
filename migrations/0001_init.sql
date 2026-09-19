-- Kinetix initial schema.
-- All configuration is admin-supplied; nothing about specific vendors is built in.

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- ---------------------------------------------------------------------------
-- Virtual keys (FR-3)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS virtual_keys (
    id              TEXT PRIMARY KEY,
    key_hash        TEXT NOT NULL UNIQUE,          -- SHA-256 hex of the full key
    name            TEXT NOT NULL,
    owner           TEXT NOT NULL,
    tag             TEXT NOT NULL DEFAULT '',
    allowed_models  TEXT NOT NULL DEFAULT '["*"]', -- JSON array of models/aliases/combos
    allowed_providers TEXT NOT NULL DEFAULT '[]',  -- JSON array; empty = any (FR-12.15)
    rpm_limit       INTEGER,
    tpm_limit       INTEGER,
    daily_budget    REAL,
    monthly_budget  REAL,
    expires_at      TEXT,
    status          TEXT NOT NULL DEFAULT 'active', -- active | disabled | revoked
    allowed_ips     TEXT NOT NULL DEFAULT '[]',
    body_logging    INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    revoked_at      TEXT
);

-- ---------------------------------------------------------------------------
-- Providers (FR-10.1)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS providers (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    base_url          TEXT NOT NULL,
    wire_format       TEXT NOT NULL,               -- openai | anthropic | gemini
    auth_scheme       TEXT NOT NULL,               -- bearer | custom_header | query_param
    custom_header_name TEXT,
    custom_param_name TEXT,
    extra_headers     TEXT NOT NULL DEFAULT '{}',  -- JSON object
    timeout_ms        INTEGER NOT NULL DEFAULT 120000,
    capability_mode   TEXT NOT NULL DEFAULT 'permissive', -- permissive | strict
    models_path       TEXT,                        -- discovery path override
    rate_limit_rules  TEXT NOT NULL DEFAULT '{}',  -- JSON classification rules (FR-12.4)
    enabled           INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT NOT NULL
);

-- ---------------------------------------------------------------------------
-- Accounts: one credential for one provider (FR-4, FR-12.1)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS accounts (
    id              TEXT PRIMARY KEY,
    provider_id     TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    label           TEXT NOT NULL,
    secret_enc      TEXT NOT NULL,                 -- AES-256-GCM(base64)
    key_mask        TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'healthy', -- healthy | cooldown | exhausted | disabled
    cooldown_until  TEXT,
    quota_reset_at  TEXT,
    quota_type      TEXT NOT NULL DEFAULT 'none',  -- none | daily | monthly | rolling
    quota_window_s  INTEGER,
    soft_quota_usd  REAL,
    priority        INTEGER NOT NULL DEFAULT 1,
    weight          INTEGER NOT NULL DEFAULT 1,
    last_error      TEXT,
    last_probe_at   TEXT,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_accounts_provider ON accounts(provider_id);

-- ---------------------------------------------------------------------------
-- Models (FR-10.2, FR-10.3)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS models (
    id                TEXT PRIMARY KEY,
    provider_id       TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    upstream_id       TEXT NOT NULL,
    display_name      TEXT NOT NULL,
    enabled           INTEGER NOT NULL DEFAULT 1,
    context_window    INTEGER,
    max_output_tokens INTEGER,
    capabilities      TEXT NOT NULL DEFAULT '{}',  -- JSON object
    prices            TEXT NOT NULL DEFAULT '{}',  -- JSON object (current)
    parameters        TEXT NOT NULL DEFAULT '{}',  -- JSON object (FR-10.6)
    thinking_map      TEXT NOT NULL DEFAULT '{}',  -- JSON object (FR-10.7)
    extra_request     TEXT NOT NULL DEFAULT '{}',  -- JSON object (FR-10.8)
    discovery         TEXT NOT NULL DEFAULT '{}',  -- suggested values + overrides
    created_at        TEXT NOT NULL,
    UNIQUE(provider_id, upstream_id)
);

CREATE INDEX IF NOT EXISTS idx_models_provider ON models(provider_id);

-- Versioned prices so past costs stay reproducible (FR-6.3)
CREATE TABLE IF NOT EXISTS price_versions (
    id            TEXT PRIMARY KEY,
    model_id      TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    input_per_1m  REAL,
    output_per_1m REAL,
    cached_per_1m REAL,
    thinking_per_1m REAL,
    created_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_price_versions_model ON price_versions(model_id);

-- ---------------------------------------------------------------------------
-- Aliases (FR-5.1)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS aliases (
    id           TEXT PRIMARY KEY,
    alias        TEXT NOT NULL UNIQUE,
    target_type  TEXT NOT NULL,                    -- model | combo
    target_id    TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL
);

-- ---------------------------------------------------------------------------
-- Combos (FR-12)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS combos (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    description       TEXT NOT NULL DEFAULT '',
    strategy          TEXT NOT NULL DEFAULT 'priority', -- priority | round-robin | weighted | least-used
    fallback_triggers TEXT NOT NULL DEFAULT '{}',  -- JSON object
    continuity_policy TEXT NOT NULL DEFAULT 'strip', -- strip | convert | error
    sticky_routing    INTEGER NOT NULL DEFAULT 0,
    max_attempts      INTEGER,
    enabled           INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS combo_targets (
    id          TEXT PRIMARY KEY,
    combo_id    TEXT NOT NULL REFERENCES combos(id) ON DELETE CASCADE,
    account_id  TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    model_id    TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    priority    INTEGER NOT NULL DEFAULT 1,
    weight      INTEGER NOT NULL DEFAULT 1,
    param_overrides TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_combo_targets_combo ON combo_targets(combo_id);

-- ---------------------------------------------------------------------------
-- Usage log (FR-6.1)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS usage_logs (
    id                 TEXT PRIMARY KEY,
    request_id         TEXT NOT NULL,
    ts                 TEXT NOT NULL,
    key_id             TEXT,
    key_name           TEXT,
    client_format      TEXT NOT NULL,              -- openai | anthropic
    requested_model    TEXT NOT NULL,
    effective_model    TEXT,
    combo_id           TEXT,
    combo_name         TEXT,
    fallback_hops      INTEGER NOT NULL DEFAULT 0,
    fallback_path      TEXT NOT NULL DEFAULT '[]',
    status             TEXT NOT NULL,              -- success | rate_limited | quota_exhausted | client_error | upstream_error | stream_error
    status_code        INTEGER NOT NULL,
    latency_ms         INTEGER,
    ttft_ms            INTEGER,
    input_tokens       INTEGER,
    output_tokens      INTEGER,
    cached_tokens      INTEGER,
    thinking_tokens    INTEGER,
    cost_usd           REAL,
    cost_known         INTEGER NOT NULL DEFAULT 0,
    price_version_id   TEXT,
    cache_status       TEXT NOT NULL DEFAULT 'bypass', -- hit | miss | bypass
    serving_account_id TEXT,
    serving_account    TEXT,
    serving_provider   TEXT,
    upstream_request_id TEXT,
    flagged            INTEGER NOT NULL DEFAULT 0,
    error_message      TEXT
);

CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_logs(ts);
CREATE INDEX IF NOT EXISTS idx_usage_key ON usage_logs(key_id, ts);
CREATE INDEX IF NOT EXISTS idx_usage_account ON usage_logs(serving_account_id, ts);

-- ---------------------------------------------------------------------------
-- Audit log (FR-6.7) -- append only
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS audit_logs (
    id          TEXT PRIMARY KEY,
    ts          TEXT NOT NULL,
    actor       TEXT NOT NULL,
    action      TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id   TEXT NOT NULL DEFAULT '',
    target_name TEXT NOT NULL DEFAULT '',
    details     TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_logs(ts);

-- ---------------------------------------------------------------------------
-- Optional body log (FR-6.5) -- opt-in per key, short retention
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS body_logs (
    id         TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    ts         TEXT NOT NULL,
    key_id     TEXT NOT NULL,
    direction  TEXT NOT NULL,                      -- request | response
    body       TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_body_logs_expires ON body_logs(expires_at);
