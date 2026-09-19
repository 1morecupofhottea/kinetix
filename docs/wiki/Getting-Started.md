# Getting Started

## Requirements

- Linux (x86_64 or aarch64) — the installer and release binaries target Linux.
  Other platforms work from source.
- A shell with `curl`. Building from source additionally needs Rust and git.

## Install

The recommended install needs **no `.env` and no config files**: everything is
set with subcommands and stored under your XDG directories.

```bash
curl -fsSL https://raw.githubusercontent.com/LazyGreed/kinetix/main/install.sh | bash
```

The installer:

1. Downloads a prebuilt release binary when you pass a release tag
   (`KINETIX_VERSION=vX.Y.Z`), otherwise clones and builds from source.
2. Installs `kinetix` to `~/.local/bin` (override with `KINETIX_PREFIX`).
3. Ensures the bin directory is on your `PATH`.
4. Runs `kinetix init`, which creates the XDG directories and prints a generated
   **admin password once**.

> Uninstall is the mirror image:
> `curl -fsSL https://raw.githubusercontent.com/LazyGreed/kinetix/main/uninstall.sh | bash`
> or `kinetix uninstall`. See [Configuration](Configuration).

## First run

```bash
# 1. Add an upstream and its credential (creates the first account too)
kinetix provider add --name "My Provider" --base-url https://api.example.com/v1 \
  --wire-format openai --auth-scheme bearer --api-key sk-... --account-label primary

# 2. Add a model it serves
kinetix model add --provider "My Provider" --upstream-id gpt-4o-mini --display-name "GPT-4o mini"

# 3. Issue a virtual key for your client (printed once)
kinetix key create --name "pi" --owner me

# 4. Run the proxy
kinetix serve
```

Then open the dashboard at <http://127.0.0.1:8080/admin> and log in with the
admin password.

## Point a client at it

Any OpenAI-compatible client works. Example (Pi):

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

Anthropic-format clients point at `http://127.0.0.1:8080` (the `/v1/messages`
endpoint). See [Providers](Providers) and
[`docs/pi-compatibility.md`](https://github.com/LazyGreed/kinetix/blob/main/docs/pi-compatibility.md).

## Quick health check

```bash
curl -s http://127.0.0.1:8080/healthz
# {"status":"ok","uptime_secs":3,"database":"ok","data_plane":"serving","control_plane":"ok"}
```

`status: ok` means the data plane is serviceable. `control_plane: degraded`
means the database is unavailable but inference still works from the in-memory
snapshot — the endpoint intentionally stays HTTP 200 so the instance is not
dropped from a load balancer. See [Observability](Observability).

## Next steps

- [CLI Reference](CLI-Reference) — the full command surface.
- [Routing and Fallback](Routing-and-Fallback) — build multi-target Routes.
- [Deployment](Deployment) — run it as a service behind Cloudflare Tunnel.
