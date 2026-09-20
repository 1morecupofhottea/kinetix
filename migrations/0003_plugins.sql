-- Plugin architecture (docs/KINETIX-PLUGIN-ARCHITECTURE.md §10).
--
-- Host-managed plugin state. Plugins never get SQL access; the host owns every
-- row. Plugin KV is stored encrypted at rest with a key derived from the
-- Kinetix master key under a separate context label (§10).

CREATE TABLE IF NOT EXISTS plugins (
    id              TEXT PRIMARY KEY,
    version         TEXT NOT NULL,
    plugin_api_major INTEGER NOT NULL,
    package_sha256  TEXT NOT NULL,
    -- 0 = installed-disabled, 1 = enabled (enable is a separate operation).
    enabled         INTEGER NOT NULL DEFAULT 0,
    -- Ed25519 signature status: unsigned | verified | untrusted (Phase 2).
    signature       TEXT NOT NULL DEFAULT 'unsigned',
    manifest_json   TEXT NOT NULL,
    -- Compiled component bytes (the `.wasm` from the .kxp archive).
    component       BLOB NOT NULL,
    installed_at    TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

-- Approved permissions. Approval is all-or-nothing per install/upgrade (§20),
-- so this records the full approved set for the current version.
CREATE TABLE IF NOT EXISTS plugin_permissions (
    plugin_id   TEXT NOT NULL,
    permission  TEXT NOT NULL,
    value_json  TEXT NOT NULL,
    approved_at TEXT NOT NULL,
    PRIMARY KEY (plugin_id, permission),
    FOREIGN KEY (plugin_id) REFERENCES plugins(id) ON DELETE CASCADE
);

-- Private per-plugin KV namespace. `value` is encrypted (nonce||ciphertext,
-- base64) with the host plugin-KV key label. The plugin cannot read another
-- plugin's keys.
CREATE TABLE IF NOT EXISTS plugin_kv (
    plugin_id  TEXT NOT NULL,
    key        TEXT NOT NULL,
    value      BLOB NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (plugin_id, key),
    FOREIGN KEY (plugin_id) REFERENCES plugins(id) ON DELETE CASCADE
);

-- Per-plugin circuit breaker (§15).
CREATE TABLE IF NOT EXISTS plugin_runtime_state (
    plugin_id           TEXT PRIMARY KEY,
    circuit_state       TEXT NOT NULL DEFAULT 'closed',
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    circuit_open_until  TEXT,
    last_error_code     TEXT,
    last_error_at       TEXT,
    FOREIGN KEY (plugin_id) REFERENCES plugins(id) ON DELETE CASCADE
);

-- §27 AC#12: record the plugin id/version that produced each invocation's
-- output. Nullable for requests that used no plugin.
ALTER TABLE usage_logs ADD COLUMN plugin_id TEXT;
ALTER TABLE usage_logs ADD COLUMN plugin_version TEXT;

-- §6.0: bind a provider to plugin capabilities by namespaced reference
-- (`plugin:<id>/<capability>`). Empty means native behavior.
ALTER TABLE providers ADD COLUMN wire_plugin TEXT NOT NULL DEFAULT '';
ALTER TABLE providers ADD COLUMN credential_plugin TEXT NOT NULL DEFAULT '';
ALTER TABLE providers ADD COLUMN model_source_plugin TEXT NOT NULL DEFAULT '';
-- §6.3: opaque provider-state blobs are tagged with the producing plugin id
-- and version so a blob is never handed to a different adapter version.
ALTER TABLE models ADD COLUMN opaque_state_plugin TEXT NOT NULL DEFAULT '';
