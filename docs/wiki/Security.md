# Security

## Trust boundary

```
Internet → Cloudflare (Access) → cloudflared tunnel → localhost
        → Kinetix data/control surfaces → configured upstreams
```

Kinetix holds upstream credentials and enforces virtual-key limits and Route
policy. It listens on localhost only, with cloudflared as the sole ingress.

## Threat model

| Threat | Mitigation |
| --- | --- |
| Leaked virtual key | Keys are hashed, revocable, optionally IP-allowlisted, budgeted, and expiring. |
| Admin-surface exposure | Localhost-only; Cloudflare Access JWT + in-Kinetix password/session; mutations fail closed. |
| Upstream credential theft | Credentials are encrypted at rest (master key), never returned by the API (mask only), and host-bound. |
| Denial of wallet | Per-key RPM/TPM and daily/monthly budgets. |
| Header/client-IP spoofing | Client IP is taken only from trusted ingress headers (cloudflared is sole ingress). |
| Malformed payload / stream | Byte-robust SSE framing; protocol torture + fuzz tests. |
| SSRF / DNS rebinding / malicious redirect | HTTPS-only, blocked internal/metadata ranges, connect-time DNS re-check, zero-redirect default, credential host binding. |
| Topology leakage | Serving account/provider hidden from clients; opaque `X-Kinetix-Route-Id` only. |
| Control-plane failure | Data plane serves from an immutable snapshot; reads keep working; counters fail open. |

Out of scope: hostile insiders with shell/root, upstream-provider compromise, and
hard multi-tenant isolation.

## Hardening highlights (NFR-3)

- **TLS mandatory** for upstreams; plain HTTP requires an explicit dev flag
  (`KINETIX_ALLOW_INSECURE_TLS` or per-provider `allow_insecure_tls`) (NFR-3.12).
- **Zero-redirect default**; redirects must be enabled per provider and are
  revalidated (NFR-3.10).
- **Credential host binding** (NFR-3.11).
- **Connect-time DNS re-check** against policy (NFR-3.9), with a documented
  residual TOCTOU window.
- **Write-only admin credentials**: a raw admin password is accepted only from the
  header, never the cookie (NFR-3.14).
- **Per-IP abuse limit** before virtual-key auth (NFR-3.6).
- **No hidden defaults**: no auto-added headers (e.g. `anthropic-version`), no
  bundled provider presets, no guessed capabilities.

## Privacy (FR-6)

Coding-agent prompts/completions may contain proprietary source and secrets, so:

- Request/response **bodies are not stored by default**.
- Metadata (owner, model/Route, tokens, cost, timings, admin-only account
  attribution, routing decisions) is retained with configurable purge.
- Optional body logging is short-lived (7 days), admin-restricted, and redacted.
- The Route Trace and flight recorder are strictly metadata-only.

## Dependency & supply-chain hygiene

- `deny.toml` + `cargo-deny` gate licenses, advisories, wildcards, and sources in
  CI (`cargo deny check`); local builds use the same config.
- Dependabot advisories are addressed promptly (e.g. the `jsonwebtoken` bump).

## Legal / usage notes

- Confirm, per provider, that credential pooling/proxy use and account/quota
  patterns comply with their terms before team rollout.
- Multiple accounts are for legitimate resilience/cost ordering, not limit
  evasion.
- The core ships no consumer-account login integrations or plugin runtime.
