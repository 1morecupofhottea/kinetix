-- Milestone 2 additions: executable Routes, Route Trace, flight recorder,
-- usage confidence, commit-point state, cache affinity, and outbound security.

-- ---------------------------------------------------------------------------
-- Routes: portability policy + prompt-cache affinity (FR-2.11, FR-7.3)
-- ---------------------------------------------------------------------------
ALTER TABLE routes ADD COLUMN portability_policy TEXT NOT NULL DEFAULT 'strip_with_warning';
ALTER TABLE routes ADD COLUMN cache_affinity INTEGER NOT NULL DEFAULT 0;

-- Route target eligibility predicate (FR-12.3/12.4): a typed expression tree.
ALTER TABLE route_targets ADD COLUMN predicate TEXT NOT NULL DEFAULT '{}';
-- Parameter overrides applied to this target (FR-12.2).
-- (param_overrides already exists.)

-- ---------------------------------------------------------------------------
-- Usage log: accounting truthfulness + commit semantics (FR-6.1/6.8, FR-4.5)
-- ---------------------------------------------------------------------------
-- provider_reported | estimated | unknown
ALTER TABLE usage_logs ADD COLUMN usage_confidence TEXT NOT NULL DEFAULT 'unknown';
-- pre_commit | post_commit (empty when the request never committed)
ALTER TABLE usage_logs ADD COLUMN commit_state TEXT NOT NULL DEFAULT '';
ALTER TABLE usage_logs ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_logs ADD COLUMN route_trace_id TEXT;
-- Opaque, admin-resolvable route id (FR-12.15). Never reveals topology.
ALTER TABLE usage_logs ADD COLUMN opaque_route_id TEXT;

CREATE INDEX IF NOT EXISTS idx_usage_request_id ON usage_logs(request_id);

-- ---------------------------------------------------------------------------
-- Route Trace (FR-12.14, NFR-4.3): one row per request, admin-only.
-- Metadata only -- never prompt/response content (Privacy section).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS route_traces (
    id          TEXT PRIMARY KEY,
    request_id  TEXT NOT NULL,
    opaque_route_id TEXT NOT NULL,
    ts          TEXT NOT NULL,
    requested_model TEXT NOT NULL,
    route_id    TEXT,
    route_name  TEXT,
    final_target TEXT,
    commit_state TEXT NOT NULL DEFAULT 'not_committed',
    outcome     TEXT NOT NULL,
    -- JSON array of trace steps: candidate enumeration, predicate results,
    -- skip reasons, attempt outcomes, fallback causes.
    steps       TEXT NOT NULL DEFAULT '[]',
    warnings    TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX IF NOT EXISTS idx_route_traces_request ON route_traces(request_id);
CREATE INDEX IF NOT EXISTS idx_route_traces_opaque ON route_traces(opaque_route_id);

-- ---------------------------------------------------------------------------
-- Flight recorder (FR-13): bounded metadata-only lifecycle diagnostics.
-- Kept in memory as the primary surface; this table is an optional durable
-- spill for failed/diagnostically-selected requests.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS flight_events (
    id          TEXT PRIMARY KEY,
    request_id  TEXT NOT NULL,
    ts          TEXT NOT NULL,
    seq         INTEGER NOT NULL,
    event       TEXT NOT NULL,
    detail      TEXT NOT NULL DEFAULT '',
    -- ms since request start
    elapsed_ms  INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_flight_events_request ON flight_events(request_id);

-- ---------------------------------------------------------------------------
-- Provider outbound security (NFR-3.10/3.11/3.12)
-- ---------------------------------------------------------------------------
ALTER TABLE providers ADD COLUMN follow_redirects INTEGER NOT NULL DEFAULT 0;
-- Comma-separated authorized hosts for credential host binding. Empty = the
-- provider's own base_url host only.
ALTER TABLE providers ADD COLUMN credential_hosts TEXT NOT NULL DEFAULT '';
-- Explicit dev-mode opt-in to skip TLS verification (visibly marked, NFR-3.12).
ALTER TABLE providers ADD COLUMN allow_insecure_tls INTEGER NOT NULL DEFAULT 0;

-- ---------------------------------------------------------------------------
-- Account circuit breaker (FR-4.7)
-- ---------------------------------------------------------------------------
ALTER TABLE accounts ADD COLUMN circuit_open_until TEXT;
ALTER TABLE accounts ADD COLUMN consecutive_failures INTEGER NOT NULL DEFAULT 0;

-- ---------------------------------------------------------------------------
-- Route-trace retention settings live in `settings`; body-log retention already
-- exists. Nothing further needed here.
-- ---------------------------------------------------------------------------
