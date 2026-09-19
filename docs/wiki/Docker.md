# Docker

Kinetix ships a multi-stage `Dockerfile` and a `docker-compose.yml`.

## Build and run

```bash
# With compose (recommended)
docker compose up -d --build

# Or plain Docker
docker build -t kinetix:local .
docker run -d --name kinetix \
  -e KINETIX_ADMIN_TOKEN=change-me \
  -p 127.0.0.1:8080:8080 \
  -v kinetix-data:/data \
  kinetix:local serve
```

The image:

- Builds the dashboard first (stage 1), then the Rust binary with the dashboard
  assets present (stage 2), then copies the binary into a slim runtime (stage 3).
- Runs as a non-root `kinetix` user.
- Sets `KINETIX_HOME=/data`, `KINETIX_BIND=0.0.0.0:8080`, `KINETIX_LOG_JSON=true`.
- Persists everything under the `/data` volume.
- Has a `curl /healthz` healthcheck.

## First run / the admin password

On first start with no `KINETIX_ADMIN_TOKEN`, Kinetix generates an admin password
and prints it once. Retrieve it from the logs:

```bash
docker compose logs kinetix | grep -i password
```

Or set it explicitly via `.env.docker.example`:

```bash
cp .env.docker.example .env
# edit KINETIX_MASTER_KEY / KINETIX_ADMIN_TOKEN, then
docker compose up -d
```

## Compose layout

- **kinetix** — the proxy, publishing `127.0.0.1:8080:8080` (so cloudflared or a
  reverse proxy sits in front), with the `kinetix-data` volume.
- **cloudflared** (optional, profile `tunnel`) — runs `cloudflared tunnel run`
  with `TUNNEL_TOKEN`. Enable with `docker compose --profile tunnel up -d`.

## Managing the container

```bash
docker compose exec kinetix kinetix status
docker compose exec kinetix kinetix provider add --name ... --base-url ... --api-key ...
docker compose exec kinetix kinetix key create --name pi --owner me
```

## Notes

- `KINETIX_ALLOW_PRIVATE_UPSTREAMS` / `KINETIX_ALLOW_INSECURE_TLS` are dev-only.
- `.dockerignore` keeps `target/`, `node_modules/`, `dashboard/dist/`, `*.db`,
  `.env`, and `config.toml` out of the build context.
- To persist the master key (so credentials survive recreation), set
  `KINETIX_MASTER_KEY` explicitly or keep the `/data` volume.
