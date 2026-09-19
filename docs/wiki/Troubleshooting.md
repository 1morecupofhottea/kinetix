# Troubleshooting

## The server doesn't start

- **"Address already in use"** — another instance holds the port. Find it with
  `ss -ltnp | grep 8080` and stop it, or bind elsewhere (`--bind`/`KINETIX_BIND`).
- **"migration 1 was previously applied but has been modified"** — an
  already-applied migration file was edited in place (its checksum changed). In
  production, add a **new** migration. In dev, reset the database (see below).
- **Bare `kinetix` prints help** — that is expected; start the server with
  `kinetix serve`.

## Admin login fails

- The admin password is shown **once** at first run. If lost, reset it:
  `kinetix password set <new>` (works with the server stopped), or delete the DB
  in dev and re-init.
- A **server restart invalidates sessions** by design; log in again.
- If you supply `KINETIX_ADMIN_TOKEN` but login still fails, a stale
  `~/.config/kinetix/admin_password.hash` may exist. The provided credential is
  authoritative on the next start; or remove the stale file.

## Requests return 503 "TLS is mandatory"

Your provider uses a plain-HTTP URL. Use HTTPS, or — for local dev only — set
`KINETIX_ALLOW_INSECURE_TLS=true` (or the provider's `allow_insecure_tls`).

## Requests return 503 "all targets unavailable"

Every Route target failed. Likely causes:

- A single-account provider was marked exhausted/cooldown and the Route has no
  other account/provider to fall back to. Add a second account or a cross-provider
  target.
- A free-tier per-minute quota is exhausted — wait for the reset window, or reset
  the account (`kinetix account reset <id>`).
- The upstream endpoint is unreachable (check `X-Kinetix-Fallback` and the Route
  Trace).

## A disabled model is still served / a config change didn't apply

The registry snapshot reloads every **1 second** (NFR-2.8). Wait ~1–2s after a
config change. If you edited the DB out-of-band, the serving snapshot is refreshed
by the same loop.

## Reasoning models return empty content

A reasoning model can spend its whole token budget on thinking tokens before
emitting text (finish_reason `length`, empty content). Raise `max_tokens`.

## A client stays logged in after a restart

It shouldn't — sessions are in-memory and a restart invalidates them. If you see
this, ensure you're on a build that uses the in-memory session store (older
builds used a deterministic token). Clear cookies and log in again.

## Client disconnect isn't detected immediately

Post-commit disconnects are detected at once. A disconnect during the **pre-commit
connect window** is bounded only by the provider timeout, because there is no
response body to observe yet. This is expected.

## Test/throwaway instances write to my real config

Launch throwaway instances with `--home` (or `KINETIX_HOME`), which also ignores
any `.env`:

```bash
kinetix --home /tmp/kx1 init
kinetix --home /tmp/kx1 serve
```

## Reset a dev database

```bash
systemctl stop kinetix        # or stop your process
rm -f /var/lib/kinetix/data/kinetix.db /var/lib/kinetix/data/kinetix.db-wal /var/lib/kinetix/data/kinetix.db-shm
systemctl start kinetix       # re-seeds from the bootstrap file, if any
```

> Removing the DB changes the once-shown bootstrap virtual key.

## Where are the logs?

`$KINETIX_STATE_HOME/kinetix.log`, or journald/stdout. Set `KINETIX_LOG_JSON=true`
for structured logs.

## Still stuck?

- `kinetix doctor` checks dirs, the database, and server reachability.
- `GET /admin/api/requests/{id}/diagnostics` shows the flight recorder for a
  failing request.
- `GET /admin/api/overview` and `/admin/api/metrics` summarize health.
