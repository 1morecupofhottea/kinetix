# Architecture

Kinetix is a single Rust process with an embedded web dashboard. It is both the
data plane (proxying inference) and the control plane (configuration, usage,
audit). The two are deliberately separated so that control-plane trouble never
fails a serviceable request.

## Module map

| Layer | Location |
| --- | --- |
| HTTP router (public + admin + dashboard) | `src/router.rs` |
| Public API handlers (`/v1/*`, `/healthz`) | `src/api.rs` |
| Admin API handlers (`/admin/api/*`) | `src/admin.rs` |
| Request pipeline (resolve → retry/fallback → stream) | `src/pipeline.rs` |
| Inbound frontends (OpenAI / Anthropic decode + encode) | `src/frontends/` |
| Outbound adapters (Gemini / OpenAI / Anthropic) | `src/adapters/` |
| Provider-neutral internal model | `src/types.rs` |
| Config registry (immutable hot-swappable snapshot) | `src/registry.rs` |
| Account pool health / cooldown / quota / circuit breaker | `src/pool.rs` |
| Per-key limits, IP allowlist, client IP | `src/limits.rs` |
| Per-IP abuse limiter | `src/ratelimit.rs` |
| Auth (virtual keys, admin sessions, CF Access) | `src/auth.rs` |
| Route predicates (three-valued) | `src/predicate.rs` |
| Route Trace + flight recorder | `src/trace.rs` |
| Live in-flight registry | `src/live.rs` |
| Same-format passthrough | `src/passthrough.rs` |
| Byte-robust SSE framer | `src/sse.rs` |
| Validate / Dry Run | `src/validate.rs` |
| Cost, usage queue, crypto, credentials | `src/cost.rs`, `src/logqueue.rs`, `src/crypto.rs`, `src/credentials.rs` |
| CLI | `src/cli.rs` |
| Server startup | `src/server.rs` |
| XDG paths | `src/paths.rs` |
| Usage exports | `src/export.rs` |
| Alerts | `src/alerts.rs` |
| SQLite persistence + migrations | `src/db.rs`, `migrations/` |
| Embedded dashboard assets | `src/assets.rs`, `dashboard/` |

## The internal model

Every request is translated into a **provider-neutral** internal representation
(`src/types.rs`) before an outbound adapter re-encodes it:

- **Canonical state**: text, images, tool calls/results, sampling parameters,
  thinking level — the parts every provider understands.
- **Portable extensions**: things like an OpenAI `prompt_cache_key` that map onto
  more than one provider.
- **Opaque provider state**: vendor-specific payloads such as Gemini's
  `thoughtSignature`. These round-trip verbatim on the same provider but are not
  portable across providers.

Frontends decode inbound wire formats; adapters encode outbound ones. **Same-format
passthrough** (FR-2.7) forwards the client body byte-for-byte when the inbound and
outbound wire formats match, preserving unknown/vendor fields, while still
extracting usage.

## Request lifecycle

1. **Ingress** — per-IP abuse limit → virtual-key auth → per-key IP allowlist →
   per-key limits (RPM/TPM/budget/expiry/allowed models).
2. **Decode** — the frontend decodes the body into the internal model.
3. **Resolve** — the requested model name resolves to a single model or a Route
   (exact alias → route by name → `provider/model-id` → bare upstream id).
4. **Plan** — Route targets are ordered (strategy), filtered by Route predicates,
   key provider restrictions, capability compatibility, and context window.
5. **Attempt loop (before commit)** — for each target: check account health, soft
   quota, resolve the credential, send upstream. Key-level failures (429/quota/
   auth/5xx/timeout) update account state and fall back to the next target, with
   bounded backoff. Retries happen **only before the first client byte**.
6. **Commit** — the first response byte marks the commit point. After it, failures
   terminate the stream with a format-correct error; nothing is spliced.
7. **Stream** — the driver parses upstream SSE, encodes client events, sends
   keepalives, and measures TTFT.
8. **Finalize** — usage is enqueued (non-blocking), the Route Trace and a flight
   event are recorded, cost is computed, and (optionally) a redacted body log is
   written.

## Data plane vs control plane (NFR-2.6/2.7)

- Inference is served from an **immutable in-memory snapshot** captured at request
  start; config edits never affect in-flight work (NFR-2.10).
- `/healthz` stays **HTTP 200** while the data plane is serviceable and reports
  `control_plane: degraded` in the body instead of dropping out of rotation.
- Admin **mutations fail closed** (503) when the store is down; admin **reads**
  keep working.
- Request-path RPM/TPM/budget counters **fail open** on a DB error (log + serve)
  so a brief outage cannot fail a serviceable request.
- A failed initial config load does not abort startup; the 1-second reload loop
  retries.

## Background tasks

- Registry snapshot reload (every 1s) + sticky-map sweep.
- Scheduled backups (`VACUUM INTO`, every 6h, 14 kept).
- Alert evaluation.
- Hourly purge of expired body logs and old Route Traces.
- Hourly usage export.

See [Routing and Fallback](Routing-and-Fallback), [Observability](Observability),
and [Security](Security) for the details of each subsystem.
