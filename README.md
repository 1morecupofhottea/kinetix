# Kinetix

**Kinetix** is a single self-hosted Rust service that speaks the OpenAI Chat Completions
and Anthropic Messages wire formats (streaming first) in front of admin-configured
upstream LLM APIs (Gemini first), and layers on virtual keys, account pools with
executable Routes and automatic fallback, cost tracking, and an embedded admin
dashboard. It is built for developers and small technical teams running AI coding
agents such as [Pi](https://pi.dev).

See `docs/kinetix-llm-proxy-requirements-r4.md` for the current (revision 4)
requirements; `docs/prism-llm-proxy-requirements.md` is the earlier draft.

## What works today (Milestone 1 + 2)

- **Inbound wire formats**: OpenAI `POST /v1/chat/completions` and Anthropic
  `POST /v1/messages`, both streaming and non-streaming, plus `GET /v1/models`
  (content-negotiated between OpenAI and Anthropic shapes) and `GET /healthz`.
- **Outbound adapters**: Gemini (`generateContent` / `streamGenerateContent`),
  OpenAI-compatible (`/chat/completions`), and Anthropic (`/messages`). All three are
  selected by the provider's configured `wire_format`, not by vendor name.
- **Virtual keys** (`sk-kinetix-...`): shown once, stored only as a SHA-256 hash,
  with per-key allowed models/aliases/routes, RPM/TPM limits, daily/monthly USD
  budgets, expiry, allowed IPs, and optional body logging.
- **Account pools & health**: per-account cooldown on 429 (honoring `Retry-After`),
  exhaustion on quota, disable on auth errors, soft spend quotas, and automatic
  failover to another account before any bytes reach the client.
- **Routes**: named `(account, model)` target lists with priority / round-robin /
  weighted / least-used selection, configurable fallback triggers, a cross-provider
  continuity policy (strip vendor thinking signatures), and sticky routing.
  (Renamed from "combos" in r4.)
- **Cost tracking**: versioned per-model prices, cached/thinking-aware billing, and
  usage rows written to SQLite through a non-blocking queue.
- **Admin API** under `/admin/api/*` (session-cookie auth, optional Cloudflare
  Access JWT) and the embedded React dashboard at `/admin`.
- **Security**: credentials encrypted at rest (AES-256-GCM), SSRF guardrails on
  admin-supplied endpoints, and no request/response bodies stored by default.

## Quick start

```bash
# 1. Configure
cp .env.example .env            # set KINETIX_MASTER_KEY and KINETIX_ADMIN_TOKEN
cp config.toml.example config.toml   # set your upstream api_key

# 2. Build (dashboard first, then the binary that embeds it)
./scripts/build-dashboard.sh

# 3. Run
./target/release/kinetix        # or: cargo run
```

On first start Kinetix seeds the database from `config.toml` and prints the generated
virtual key **once** to the logs. Afterwards the database is authoritative: manage
everything from the dashboard at <http://127.0.0.1:8080/admin>.

Point a client (e.g. Pi) at the proxy:

```json
{
  "providers": {
    "kinetix": {
      "baseUrl": "http://127.0.0.1:8080/v1",
      "apiKey": "sk-kinetix-...",
      "api": "openai-completions"
    }
  }
}
```

## Architecture

Single Rust binary, one process:

| Layer | Location |
| --- | --- |
| HTTP router (public + admin + dashboard) | `src/router.rs` |
| Public API handlers | `src/api.rs` |
| Admin API handlers | `src/admin.rs` |
| Request pipeline (resolve → retry/fallback → stream) | `src/pipeline.rs` |
| Inbound frontends (OpenAI / Anthropic decode + encode) | `src/frontends/` |
| Outbound adapters (Gemini / OpenAI / Anthropic) | `src/adapters/` |
| Provider-neutral internal model | `src/types.rs` |
| Config registry (hot-swappable snapshot) | `src/registry.rs` |
| Account pool health / cooldown / quota | `src/pool.rs` |
| Per-key limit enforcement | `src/limits.rs` |
| Auth (virtual keys, admin session, CF Access) | `src/auth.rs` |
| SQLite persistence + migrations | `src/db.rs`, `migrations/` |
| Cost, usage log queue, crypto, credentials | `src/cost.rs`, `src/logqueue.rs`, `src/crypto.rs`, `src/credentials.rs` |
| Embedded dashboard (React 19; r4 specifies SvelteKit — documented deviation) | `src/assets.rs`, `dashboard/` |

Tech: Rust + Tokio, Axum 0.8, reqwest (rustls, HTTP/2), SQLite in WAL mode via sqlx,
`rust-embed` for the dashboard, Cloudflare Tunnel + systemd for deployment.

## Testing

- `scripts/ci.sh` runs the full gate (fmt, clippy, tests, release build,
  `cargo deny` when installed). `.github/workflows/ci.yml` runs the same gate in
  CI plus the dashboard typecheck/build and `cargo-deny` (advisories, licenses,
  bans — NFR-3.7).
- `cargo test` runs the unit + protocol-torture/fuzz suite (SSE framing across
  arbitrary chunk boundaries, UTF-8 splits, tool-argument fragmentation,
  interleaved parallel tool calls, reasoning/text interleaving, usage only in
  the final event, zero-token responses, large tool calls, malformed/unknown
  frames, bounded fuzzing, and a >120s silent-thinking keepalive stream).
- `scripts/bench.sh [concurrency-list] [requests-per-level]` runs the synthetic
  benchmark matrix (passthrough / translation / tools / large /
  tools-large-fragments) against a deterministic local upstream, measuring added
  latency and TTFT versus NFR-1.
- `scripts/bench-rust.sh [concurrency-list] [requests-per-level]` is a
  std-only-Rust capacity rig that reaches the NFR-1.4 reference load (200
  concurrent streams) without the Python GIL saturating first. Set `ALLOC_STATS=1`
  to build with the `alloc-stats` feature and report allocations/bytes per
  request (NFR-1.8). Measured results and methodology are in
  [`docs/benchmarks.md`](docs/benchmarks.md).
- `scripts/cancel_bench.py` measures client-disconnect cancellation latency
  (NFR-1.10).
- `scripts/smoke.sh` is an end-to-end smoke test (synthetic upstream + a fresh
  instance) covering the public API, passthrough, an OpenAI tool call, the
  Anthropic format, the admin API, the Route Trace/diagnostics, and metrics.
  CI runs it after the release build.
- A local end-to-end harness exercises the public API, the admin API, streaming
  tool calls, and automatic fallback. CI will run the fixture suite from the
  requirements (FR-9).

## Compatibility notes

Client-specific behaviour, the Pi acceptance configuration, known fidelity
notes, and the documented deviations from r4 live in
[`docs/compatibility.md`](docs/compatibility.md) (FR-9.3).

Pi (the primary target client) was exercised end to end with the real `pi`
binary — plain streaming, a tool-calling turn, and a multi-turn session — and
the observed wire behavior is recorded in
[`docs/pi-compatibility.md`](docs/pi-compatibility.md) (FR-9.2/9.3, NFR-7.2).

## Deployment

See `deploy/README.md` for the full runbook (systemd unit with auto-restart and
graceful drain, Cloudflare Tunnel + Access setup, backup/restore, upgrades).
`deploy/kinetix.service` is a ready systemd unit (NFR-2.2/2.3).

## Admin authentication

- The dashboard logs in with the configured admin token and receives an
  httpOnly session cookie carrying a derived session token (rotating the admin
  token invalidates all sessions).
- CLI/tooling may instead send `x-kinetix-admin-token`: this accepts either the
  derived session token or the **raw** admin token. A raw token is never
  accepted from the cookie (NFR-3.14 — credentials stay write-only and are not
  placed in a readable cookie).

## Status

All four r4 milestones (M1 Correct streaming skeleton, M2 Operable routing
service, M3 Anthropic + cross-provider fidelity, M4 Operational polish) are
implemented and verified end-to-end against live Gemini and OpenAI-compatible
upstreams, plus real Pi acceptance sessions. Revision 4 renamed the product to
Kinetix (Prism is the old name), renamed Combos to Routes, and promoted several
behaviours to MUST.

**Implemented** (r4 M1–M4):

- Same-format passthrough (FR-2.7/2.10) for OpenAI→OpenAI-compatible and
  Anthropic→Anthropic, preserving unknown/provider-specific fields verbatim while
  extracting usage.
- Executable, three-valued Route predicates with an explanation and Dry Run
  (FR-12.3/12.4), plus the Route Trace (FR-12.14) and a bounded metadata-only
  flight recorder (FR-13), retrievable at
  `/admin/api/requests/{id}/route-trace` and `.../diagnostics`.
- Explicit commit-point state machine (FR-4.5): retry/fallback only before the
  first client byte; post-commit failures terminate the stream with a
  format-correct error (never spliced), with separate before-/after-commit and
  cancellation metrics.
- `strip_with_warning` portability policy (FR-2.11): non-portable reasoning/
  thinking signatures are removed on cross-provider fallback with a recorded
  trace warning and an `X-Kinetix-Warning` response header; `reject` is the
  alternative.
- Topology hiding (FR-12.15): clients see `X-Kinetix-Route-Id` (opaque
  `krt_…`), not the serving account/provider.
- Usage confidence states (FR-6.2/6.8): provider-reported vs unknown tokens; cost
  is unknown (not zero) when prices are missing.
- Validate/Dry Run (FR-8.6/8.7): `/admin/api/validate`,
  `/admin/api/routes/dry-run` (ASN shown as unknown when it cannot be resolved).
- Outbound security (NFR-3): zero-redirect default, credential host binding,
  connect-time DNS re-check, and an explicit insecure-TLS dev mode.
- Cache-aware sticky routing (FR-7.3) keyed on an explicit session header.
- Protocol torture + fuzz tests (FR-9.4/9.5) over a byte-robust SSE framer.
- Data-plane/control-plane separation (NFR-2.6/2.7): `/healthz` stays HTTP 200
  while the data plane is serviceable (it reports `control_plane: degraded` in
  the body rather than dropping out of rotation); admin login fails closed when
  the store is down, while inference keeps serving from the in-memory snapshot
  and `/metrics` reports `kinetix_control_plane_degraded`. A failed initial
  config load no longer aborts startup (the background reload loop retries).
- Accounting truthfulness (FR-6.2/6.8): unknown token counts are never coerced
  to zero, and `/metrics` exposes `kinetix_usage_unknown_total`,
  `kinetix_usage_estimated_total`, and `kinetix_usage_unknown_cost_total` so
  non-provider-reported usage/cost is never read as exact. `/metrics` also
  reports request/error rate (`kinetix_error_rate`), average latency/TTFT
  (`kinetix_avg_latency_ms`, `kinetix_avg_ttft_ms`), and
  `kinetix_credential_failures_total` (NFR-4.2).
- Dashboard request inspector renders the Route Trace (candidate/skip/attempt/
  commit/result steps with timings) and the flight-recorder diagnostics inline
  (NFR-4.3/4.4); the Routes editor exposes the opaque-state portability policy
  (FR-2.11) and cache-affinity toggle (FR-7.3).
- Validate / Dry Run before Apply (FR-8.6/NFR-6.4): `POST
  /admin/api/validate/{provider,model,account}` checks schema, wire-format/auth
  companions, outbound security, and reports missing price/capability data as
  `unknown` warnings (never assumed); the Routes editor has a **Dry Run** action
  (`POST /admin/api/routes/dry-run`) that returns candidate ordering, predicate
  outcomes, eligibility, and the would-be selection without touching production.
- The credential-strategy seam (FR-11.2) represents refresh/expiry/rotation and
  health reporting (`resolve`/`health`/`rotate`, `CredentialHealth`,
  `ResolvedCredential`) even though v1 ships only the static-key strategy.
- The opaque `X-Kinetix-Route-Id` a client receives resolves back to its Route
  Trace at `GET /admin/api/route-traces/{opaque_id}` (admin-only), so serving
  topology stays hidden from clients while remaining explainable to operators
  (FR-12.15). The Live Tester can resolve the id it received.
- Alerting (FR-6.6/FR-12.17) also covers the Monitoring section: high fallback
  and error rates, unhealthy accounts, routes with no healthy target, virtual
  keys crossing 80% of their monthly budget, usage-log queue saturation/drops,
  scheduled-backup failure, p95 added proxy latency above
  `KINETIX_ALERT_P95_LATENCY_MS` sustained for 10 minutes, and repeated
  credential-strategy failures (`kinetix_credential_failures_total`).
- Generation-parameter metadata (FR-10.6): a configured `default` is applied
  when the client omits the field (a client value always wins), `supported`
  fields are dropped, and an out-of-range value is clamped or rejected per the
  configured `policy`. No parameter defaults are ever invented for a model with
  no configuration.
- A post-commit stream failure is marked with an explicit `finish_reason`
  (OpenAI `error`) before `[DONE]`, so a truncated stream is never mistaken for
  a clean completion (FR-4.6/NFR-2.9).
- Per-key IP allowlists (FR-3.4) are editable from the Keys view (an explicit
  empty list clears the restriction) and enforced on both `/v1/chat/completions`
  and `/v1/models`.
- Discovery never overwrites admin edits (FR-10.5): `POST
  /admin/api/providers/:id/discover` records a per-model `discovery` observation
  and returns a `disappeared` list flagging models no longer advertised
  upstream, instead of silently deleting them.
- Scheduled consistent backups (NFR-2.4): `VACUUM INTO` snapshots every 6h with
  14-file retention in `$KINETIX_DATA_DIR/backups`, alongside the pre-migration
  backup. Best-effort and off the data path.
- Admin mutations fail closed when the control-plane store is degraded
  (NFR-2.7): a middleware rejects non-GET admin requests with 503 while reads
  and inference keep working; the request-path RPM/TPM/budget counters also fail
  open (log + serve) on a DB error so a brief outage cannot fail a serviceable
  request.
- Immutable per-request config snapshots (NFR-2.10/FR-10.13): every request
  captures one `Arc<Snapshot>` for its whole lifetime, so config edits never
  affect in-flight work; the snapshot is reloaded every second so time-based
  account recovery and out-of-band edits are visible within 1s (NFR-2.8).
- Bounded half-open circuit-breaker recovery probing (FR-4.7): an account whose
  circuit has opened is retried at most once every few seconds; a successful
  probe clears the breaker.
- Cancellation latency (NFR-1.10): a response-body drop-guard signals the driver
  the instant the client goes away, so upstream cancellation is immediate
  (measured ~0ms signal latency in the cancellation benchmark).
- A synthetic-upstream benchmark harness (`scripts/bench.sh`,
  `scripts/synthetic_upstream.py`, `scripts/bench_client.py`,
  `scripts/cancel_bench.py`, plus the Rust `scripts/bench-rust.sh` capacity rig)
  covering passthrough, translation, tools, large bodies, client-disconnect
  cancellation, and the 200-concurrent-stream capacity gate against NFR-1; see
  [`docs/benchmarks.md`](docs/benchmarks.md).
- TLS is mandatory except in an explicit, visibly-marked dev mode
  (`KINETIX_ALLOW_INSECURE_TLS`, NFR-3.12); plain-HTTP upstreams are refused on
  the request path, not just at config time.
- Bounded graceful-shutdown drain (`KINETIX_SHUTDOWN_GRACE_SECS`, default 30s,
  NFR-2.3) and a best-effort pre-migration database backup written to
  `$KINETIX_DATA_DIR/backups` (NFR-2.4).
- `deny.toml` + `scripts/ci.sh`: fmt/clippy/test/release gate plus dependency
  license/advisory/ban checks (`cargo deny`, NFR-3.7) that explicitly ban a
  local response-cache crate (FR-7.6).
- Live in-flight request view (FR-8.3): a bounded, metadata-only in-memory
  registry (`src/live.rs`) exposed at `GET /admin/api/requests/live`, surfaced in
  the dashboard's Request Inspector, and reflected in the overview's
  `active_streams` and the `kinetix_active_streams` metric. It shows phase
  (selecting → committed → done), commit state, fallback hops, TTFT, and token
  counts without storing any bodies.
- Flight-recorder coverage (FR-13.1) extended to `upstream_first_frame`,
  `reasoning_event`, `tool_call_event`, and `cancellation_issued`.

- Per-key IP allowlist (FR-3.4): exact addresses or CIDR ranges evaluated
  against trusted ingress headers (`CF-Connecting-IP`, then the first
  `X-Forwarded-For` hop, then `X-Real-IP`); fails closed when an allowlist is set
  but no client IP can be determined.
- Per-IP abuse rate limit (NFR-3.6): a bounded in-memory fixed-window limiter
  (`KINETIX_IP_RATE_LIMIT_PER_MIN`, default 600, 0 disables) applied before
  virtual-key auth on `/v1`, so an unauthenticated flood is rejected cheaply
  with a format-correct 429. It uses the same trusted client IP as the allowlist
  and fails open when no client IP can be determined. Exposed as the
  `kinetix_ip_rate_limited_total` metric.
- User-authored configuration export/import (FR-10.12):
  `GET /admin/api/config/export` (secret-free by default; `?include_secrets=true`
  carries encrypted blobs) and `POST /admin/api/config/import` with a two-phase
  `apply:false` Validate/Dry Run (schema + outbound-security checks, no writes)
  then `apply:true` (upsert by name, never deletes, never overwrites an existing
  credential).

- Optional per-key body logging (FR-6.5): off unless a key sets `body_logging`;
  stored rows are redacted (`crypto::redact`) with short (7-day) retention and
  purged hourly. Request bodies are captured for every path; non-streaming
  responses are captured too. Streaming responses are deliberately not buffered
  (NFR-1.3), so only their request is retained.
- Webhook alerting (FR-6.6/FR-12.17): a background loop watches recent usage and
  account/route health and POSTs edge-triggered JSON alerts (high fallback rate,
  high error rate, an account exhausted/circuit-open, a route with no healthy
  target). Disabled unless `KINETIX_ALERT_WEBHOOK_URL` is set; delivery is
  best-effort and never touches the data plane.

**Deferred** (documented, not silently dropped): local response caching (removed
from v1, FR-7.6) and budget reservation (FR-6.9). See the r4 requirements
document for the full delta.
