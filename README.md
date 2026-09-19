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

- `cargo test` runs the unit + protocol-torture/fuzz suite (SSE framing across
  arbitrary chunk boundaries, UTF-8 splits, tool-argument fragmentation, usage
  only in the final event, malformed/unknown frames, bounded fuzzing).
- `scripts/bench.sh [concurrency-list] [requests-per-level]` runs the synthetic
  benchmark matrix (passthrough / translation / tools / large) against a
  deterministic local upstream, measuring added latency and TTFT versus NFR-1.
- `scripts/cancel_bench.py` measures client-disconnect cancellation latency
  (NFR-1.10).
- A local end-to-end harness exercises the public API, the admin API, streaming
  tool calls, and automatic fallback. CI will run the fixture suite from the
  requirements (FR-9).

## Status

Milestone 1 (walking skeleton) and Milestone 2 (dashboard, Routes, limits, cost,
audit) are implemented and verified end-to-end against live Gemini and
OpenAI-compatible upstreams. Revision 4 renamed the product to Kinetix (Prism is the
old name), renamed Combos to Routes, and promoted several behaviours to MUST.

**Implemented in this pass** (r4 M1 + M2):

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
- Data-plane/control-plane separation (NFR-2.6/2.7): `/healthz` distinguishes
  serviceability from degraded control-plane state; admin login fails closed
  when the store is down, while inference keeps serving from the in-memory
  snapshot and `/metrics` reports `kinetix_control_plane_degraded`.
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
  `scripts/cancel_bench.py`) covering passthrough, translation, tools, large
  bodies, and client-disconnect cancellation against NFR-1.

**Deferred** (documented, not silently dropped): local response caching (removed
from v1, FR-7.6), budget reservation (FR-6.9), and the M4 polish items
(config export/import FR-10.12, per-key IP allowlist FR-3.4, webhook alerts
FR-6.6/12.17, the live request view FR-8.3). See the r4 requirements document for
the full delta.
