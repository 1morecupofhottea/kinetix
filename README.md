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

A local end-to-end harness lives at `/tmp/ktest.sh` (see `scripts/`), exercising the
public API, the admin API, streaming tool calls, and automatic fallback. CI will run
the fixture suite from the requirements (FR-9).

## Status

Milestone 1 (walking skeleton) and Milestone 2 (dashboard, Routes, limits, cost,
audit) are implemented and verified end-to-end against live Gemini and
OpenAI-compatible upstreams. Revision 4 renamed the product to Kinetix (Prism is the
old name), renamed Combos to Routes, and promoted several behaviours to MUST that are
**not yet implemented**: same-format passthrough (FR-2.7), executable Route predicates
and the Route Trace (FR-12.3/12.14), the diagnostic flight recorder (FR-13),
cache-aware sticky routing (FR-7.3), the explicit non-portable-state policy (FR-2.11),
topology hiding via `X-Kinetix-Route-Id` (FR-12.15), usage confidence states (FR-6.8),
Validate/Dry Run (FR-8.6/8.7), and the protocol torture/fuzz harness (FR-9.4/9.5).
See the r4 requirements document for the full delta.
