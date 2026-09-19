# syntax=docker/dockerfile:1

# --- Stage 1: build the dashboard bundle (embedded into the binary) ---------
FROM node:22-bookworm-slim AS dashboard
WORKDIR /build/dashboard
COPY dashboard/package.json dashboard/package-lock.json ./
RUN npm ci
COPY dashboard/ ./
RUN npm run build

# --- Stage 2: build the Rust binary ----------------------------------------
FROM rust:1-bookworm AS builder
WORKDIR /build
# The dashboard bundle must exist before the crate compiles: rust-embed reads
# dashboard/dist at build time.
COPY --from=dashboard /build/dashboard/dist ./dashboard/dist
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release --locked

# --- Stage 3: minimal runtime ----------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /home/kinetix --shell /usr/sbin/nologin kinetix

COPY --from=builder /build/target/release/kinetix /usr/local/bin/kinetix

# All persistent state lives under KINETIX_HOME so a single volume is enough:
#   /data/config  (config.toml, master.key, admin_password.hash)
#   /data/data    (kinetix.db + wal/shm, exports/, backups/)
#   /data/state   (logs)
ENV KINETIX_HOME=/data \
    KINETIX_BIND=0.0.0.0:8080 \
    KINETIX_LOG_JSON=true

RUN mkdir -p /data && chown -R kinetix:kinetix /data
VOLUME ["/data"]
USER kinetix

EXPOSE 8080

# The external probe distinguishes data-plane serviceability from degraded
# control-plane state (NFR-2.7); it stays 200 while inference still works.
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

# No subcommand defaults to help, so the server must be started explicitly.
ENTRYPOINT ["kinetix"]
CMD ["serve"]
