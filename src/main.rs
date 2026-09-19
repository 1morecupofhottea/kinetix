//! Kinetix: a multi-protocol LLM proxy. OpenAI Chat Completions and Anthropic
//! Messages in (streaming first), admin-configured upstreams out (Gemini,
//! OpenAI-compatible, Anthropic), with virtual keys, account pools with
//! automatic fallback combos, and cost tracking.

mod adapters;
mod admin;
mod api;
mod app;
mod assets;
mod auth;
mod bootstrap;
mod config;
mod cost;
mod credentials;
mod crypto;
mod db;
mod frontends;
mod limits;
mod logqueue;
mod pipeline;
mod pool;
mod registry;
mod router;
mod types;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::app::AppState;
use crate::config::Config;
use crate::crypto::Crypto;
use crate::logqueue::UsageLogQueue;
use crate::registry::Registry;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Arc::new(Config::from_env()?);
    init_tracing(config.log_json);

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Kinetix");

    // Database + migrations.
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    let crypto = Arc::new(Crypto::new(&config.master_key));

    // Optional bootstrap seed (first run only).
    if let Some(path) = &config.bootstrap_file {
        if path.exists() {
            let boot = config::load_bootstrap(path)?;
            match bootstrap::seed_if_empty(&pool, &crypto, &boot).await {
                Ok(generated) => {
                    for (name, key) in generated {
                        tracing::warn!(key_name = %name, virtual_key = %key, "generated bootstrap virtual key (shown once)");
                    }
                }
                Err(e) => tracing::error!(error = %e, "bootstrap seeding failed"),
            }
        } else {
            tracing::warn!(path = %path.display(), "bootstrap file does not exist; skipping");
        }
    }

    // Registry + usage log queue.
    let registry = Arc::new(Registry::new());
    registry.reload(&pool).await?;
    let log_queue = UsageLogQueue::new(pool.clone(), 4096);

    // HTTP client for upstreams: pooled, HTTP/2, bounded connect timeout.
    let http = reqwest::Client::builder()
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(10))
        .http2_adaptive_window(true)
        .user_agent(concat!("kinetix/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building HTTP client")?;

    let state = AppState::new(config.clone(), pool.clone(), registry.clone(), crypto, http, log_queue);

    // Background tasks.
    spawn_background_tasks(state.clone());

    let app = router::build(state.clone());
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;

    tracing::info!(addr = %config.bind, "Kinetix is listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    tracing::info!("Kinetix shut down cleanly");
    Ok(())
}

fn init_tracing(json: bool) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("kinetix=info,tower_http=warn,sqlx=warn"));

    let registry = tracing_subscriber::registry().with(filter);
    if json {
        registry.with(tracing_subscriber::fmt::layer().json()).init();
    } else {
        registry.with(tracing_subscriber::fmt::layer().compact()).init();
    }
}

fn spawn_background_tasks(state: AppState) {
    // Periodically reload the registry so cooldown/quota expiry and any
    // out-of-band DB edits are reflected (state changes are visible within 1s
    // because failures reload eagerly; this catches time-based recovery).
    let st = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            if let Err(e) = st.registry.reload(&st.pool).await {
                tracing::warn!(error = %e, "registry reload failed");
            }
        }
    });

    // Purge expired body logs (FR-6.5 retention).
    let st = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match db::purge_expired_body_logs(&st.pool).await {
                Ok(n) if n > 0 => tracing::info!(purged = n, "purged expired body logs"),
                _ => {}
            }
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received; draining in-flight requests");
}
