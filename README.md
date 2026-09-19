# Kinetix

**Kinetix** is a single self-hosted Rust service that speaks the OpenAI Chat Completions
and Anthropic Messages wire formats (streaming first) in front of admin-configured
upstream LLM APIs (Gemini first), and layers on virtual keys, account pools with
executable Routes and automatic fallback, cost tracking, and an embedded admin
dashboard. It is built for developers and small technical teams running AI coding
agents such as [Pi](https://pi.dev).

[![CI](https://github.com/LazyGreed/kinetix/actions/workflows/ci.yml/badge.svg)](https://github.com/LazyGreed/kinetix/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Wiki](https://img.shields.io/badge/docs-wiki-blueviolet)](https://github.com/LazyGreed/kinetix/wiki)

## Documentation

- [Wiki](https://github.com/LazyGreed/kinetix/wiki) — task-oriented documentation:
  getting started, CLI, configuration, architecture, routing, admin API, dashboard,
  authentication, providers, observability, deployment, Docker, security, testing,
  troubleshooting, and FAQ. The wiki sources live in
  [docs/wiki/](docs/wiki).
- [docs/DESIGN.md](docs/DESIGN.md) — the full product and technical design.
- [docs/compatibility.md](docs/compatibility.md) — client compatibility notes and
  documented deviations.
- [docs/pi-compatibility.md](docs/pi-compatibility.md) — Pi setup and acceptance
  notes.
- [docs/benchmarks.md](docs/benchmarks.md) — benchmark methodology and results.

## What Kinetix does

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
- **Cost tracking**: versioned per-model prices, cached/thinking-aware billing, and
  usage rows written to SQLite through a non-blocking queue.
- **Admin API** under `/admin/api/*` (session-cookie auth, optional Cloudflare
  Access JWT) and the embedded React dashboard at `/admin`.
- **Security**: credentials encrypted at rest (AES-256-GCM), SSRF guardrails on
  admin-supplied endpoints, and no request/response bodies stored by default.

## Quick start

Kinetix is a single binary that is both the proxy and its own admin CLI. The
recommended install (Linux) needs no `.env` and no configuration files —
everything is set with subcommands and stored under your XDG directories
(`~/.config/kinetix`, `~/.local/share/kinetix`, `~/.local/state/kinetix`).

```bash
# 1. Install (downloads a release binary if one is available, else builds from
#    source, and drops the binary in ~/.local/bin)
curl -fsSL https://raw.githubusercontent.com/LazyGreed/kinetix/main/install.sh | bash

# 2. Run the proxy
kinetix serve
```

`kinetix init` prints the generated dashboard admin password **once**; change it
later with `kinetix password set`. `kinetix --help` lists every subcommand
(`provider`, `model`, `account`, `route`, `alias`, `key`, `export`, `backup`,
`doctor`, `status`, `password`, `uninstall`). Add your own upstreams, models,
aliases, Routes, and virtual keys with those subcommands — Kinetix ships no
provider presets. The admin CLI writes the SQLite control plane directly, so it
works whether or not the server is running.

Manage everything from the dashboard at <http://127.0.0.1:8080/admin> (log in
with the admin password).

## Pointing a client at Kinetix

Configure your client (for example Pi) with Kinetix's base URL and a virtual key:

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

See [docs/pi-compatibility.md](docs/pi-compatibility.md) and
[docs/compatibility.md](docs/compatibility.md) for details.

## Deployment

See [deploy/README.md](deploy/README.md) for the full runbook (systemd unit with
auto-restart and graceful drain, Cloudflare Tunnel + Access setup, backup/restore,
upgrades). [deploy/kinetix.service](deploy/kinetix.service) is a ready systemd unit.

### Docker

A multi-stage `Dockerfile` and `docker-compose.yml` run Kinetix in a container with
a persistent `/data` volume and an optional `cloudflared` service:

```bash
cp .env.docker.example .env      # optional: set KINETIX_MASTER_KEY / KINETIX_ADMIN_TOKEN
docker compose up -d --build
docker compose logs kinetix | grep -i password   # first-run admin password
```

### Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/LazyGreed/kinetix/main/uninstall.sh | bash
# or, equivalently, with the installed binary:
kinetix uninstall [--yes] [--remove-binary] [--keep-data] [--dry-run]
```

## Dashboard

The embedded React dashboard at `/admin` covers the whole control plane: virtual
keys, Routes, upstream providers and their accounts, model aliases, usage/spend
(with a Today/24h/7d/30d window and per-day JSONL/CSV exports you can download or
delete), the Request Inspector (live in-flight view plus inline Route Trace and
flight-recorder diagnostics), the append-only audit log, and a **Settings &
Security** page to change the admin password. Destructive actions ask for
confirmation first, and there is a Light/Dark/System theme toggle.

## Admin authentication

- `kinetix init` (or the first `kinetix serve`) generates an admin password and
  prints it **once**; it is stored as a hash
  (`~/.config/kinetix/admin_password.hash` and the `admin_password_hash` DB
  setting). Change it with `kinetix password set` or from the dashboard.
- The dashboard logs in with that password and receives an httpOnly session cookie
  carrying an in-memory session token. Sessions live only in process memory with a
  TTL (`KINETIX_SESSION_TTL_MINUTES`, default 720), so **a server restart
  invalidates every session** and a browser must log in again.
- Changing the password invalidates all sessions immediately.
- CLI/tooling may instead send `x-kinetix-admin-token` carrying a live session
  token or the current raw admin password. A raw credential is never accepted from
  the cookie — credentials stay write-only.

## Command-line interface

The binary is also the administration tool. `kinetix` with no subcommand prints
help; `kinetix serve` runs the proxy. Every other subcommand opens the SQLite
control plane directly (deriving the master key from the config directory), so it
works with the server stopped and needs no admin password.

| Command | Purpose |
| --- | --- |
| `kinetix init` | Create XDG dirs; generate + print the admin password once |
| `kinetix serve` | Run the proxy (`--allow-private-upstreams`, `--allow-insecure-tls` for local dev) |
| `kinetix status` / `doctor` | Show resolved paths/counts; run health diagnostics |
| `kinetix password set/show` | Change or inspect the dashboard password |
| `kinetix key create/list/enable/disable/revoke` | Manage virtual keys (`revoke` hard-deletes the key and its usage) |
| `kinetix provider add/list/remove` | Manage upstreams (optionally creating the first account) |
| `kinetix model add/list/remove` | Manage models |
| `kinetix account add/list/reset/remove` | Manage credential pools |
| `kinetix route add/list/remove` | Manage routes |
| `kinetix alias add/list/remove` | Manage aliases |
| `kinetix export run/list/prune` | Export usage to JSONL + CSV under the data dir |
| `kinetix backup run/list` | Run/list database backups |
| `kinetix uninstall` | Remove config/data/state (and optionally the binary) |

Configuration precedence is **CLI flag > environment variable > config file >
default**. `--home <dir>` runs an isolated instance rooted at that directory
(config/data/state beneath it) and ignores any `.env`. In production, keep the
proxy on localhost behind cloudflared (see [deploy/README.md](deploy/README.md)).

## License

MIT — see [LICENSE](LICENSE).
