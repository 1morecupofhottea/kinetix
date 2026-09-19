//! Kinetix: a multi-protocol LLM proxy. OpenAI Chat Completions and Anthropic
//! Messages in (streaming first), admin-configured upstreams out (Gemini,
//! OpenAI-compatible, Anthropic), with virtual keys, account pools with
//! automatic fallback routes, and cost tracking.
//!
//! This binary is a thin wrapper around the `kinetix` library crate (see
//! `src/lib.rs`), which holds all modules so that integration tests can drive
//! the wire encoders/decoders directly.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use kinetix::app::AppState;
use kinetix::config::Config;
use kinetix::crypto::Crypto;
use kinetix::logqueue::UsageLogQueue;
use kinetix::registry::Registry;

// Optional allocation accounting (NFR-1.8). Enabled with `--features alloc-stats`.
#[cfg(feature = "alloc-stats")]
#[global_allocator]
static GLOBAL_ALLOC: kinetix::alloc::CountingAllocator = kinetix::alloc::CountingAllocator;
use kinetix::{alerts, bootstrap, db, router};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Arc::new(Config::from_env()?);
    init_tracing(config.log_json);

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Kinetix");

    // Database + migrations (with a pre-migration backup, NFR-2.4).
    let pool = db::connect(&config.database_url).await?;
    db::backup_before_migration(&config.database_url, &config.data_dir);
    db::migrate(&pool).await?;

    let crypto = Arc::new(Crypto::new(&config.master_key));

    // Optional bootstrap seed (first run only).
    if let Some(path) = &config.bootstrap_file {
        if path.exists() {
            let boot = kinetix::config::load_bootstrap(path)?;
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

    // Registry + usage log queue. A reload failure at startup must not take
    // down serving: an empty snapshot still answers /healthz, and the background
    // reload loop (below) keeps retrying. Log loudly and continue (NFR-2.6/2.7).
    let registry = Arc::new(Registry::new());
    if let Err(e) = registry.reload(&pool).await {
        tracing::error!(
            error = %e,
            "initial registry reload failed; starting with an empty snapshot and retrying in the background"
        );
    }
    let log_queue = UsageLogQueue::new(pool.clone(), 4096);

    // Startup route-eligibility diagnostic (NFR-2.7 / Monitoring): warn once at
    // boot if any enabled Route currently has no eligible target, so an
    // operator sees a misconfiguration before traffic arrives. This never
    // affects serving (it only reads the just-loaded snapshot).
    warn_on_unroutable_routes(&registry);

    // HTTP client for upstreams: pooled, HTTP/2, bounded connect timeout.
    let http = reqwest::Client::builder()
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(10))
        .http2_adaptive_window(true)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("kinetix/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building HTTP client")?;

    // A separate client that follows redirects, used only for providers that
    // explicitly opt in (NFR-3.10). Redirect targets are revalidated by the
    // credential-host-binding check in the pipeline before any credential is
    // attached; this client is never used for credential-bearing calls to an
    // unauthorized host because the check runs first.
    let http_redirect = reqwest::Client::builder()
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!("kinetix/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building redirect HTTP client")?;

    let state = AppState::new(
        config.clone(),
        pool.clone(),
        registry.clone(),
        crypto,
        http,
        http_redirect,
        log_queue,
        config.ip_rate_limit_per_min,
    );

    // Background tasks.
    spawn_background_tasks(state.clone());

    let app = router::build(state.clone());
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;

    tracing::info!(addr = %config.bind, "Kinetix is listening");
    // Drain in-flight requests on shutdown, bounded by a configurable window
    // (NFR-2.3, default 30s). axum stops accepting new connections and waits
    // for existing responses to finish; the drain is capped so a stuck stream
    // cannot block termination forever.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(config.shutdown_grace_secs))
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
        registry
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        registry
            .with(tracing_subscriber::fmt::layer().compact())
            .init();
    }
}

fn spawn_background_tasks(state: AppState) {
    // Reload the registry frequently so time-based account recovery (cooldown
    // and quota windows) is reflected quickly, and so control-plane edits made
    // out of band are picked up. Reloading does no writes and only swaps an
    // immutable snapshot, so a short interval is cheap (NFR-2.8: health-state
    // changes visible within 1s; NFR-2.10: in-flight requests unaffected).
    let st = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(1000));
        loop {
            tick.tick().await;
            if let Err(e) = st.registry.reload(&st.pool).await {
                // Control-plane degradation must not fail serving (NFR-2.6/2.7).
                tracing::warn!(error = %e, "registry reload failed; continuing on last snapshot");
            }
            // Bound the prompt-cache-affinity map (FR-7.3).
            st.sticky_sweep(Duration::from_secs(30 * 60));
        }
    });

    // Scheduled consistent backup with retention (NFR-2.4). Best-effort; a
    // backup failure alerts via the log and never touches the data plane.
    {
        let st = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(6 * 3600));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tick.tick().await; // skip the immediate first tick
            loop {
                tick.tick().await;
                match db::scheduled_backup(
                    &st.pool,
                    &st.config.database_url,
                    &st.config.data_dir,
                    14,
                )
                .await
                {
                    Ok(Some(_)) => {
                        *st.last_backup_at.lock() = Some(db::now_iso());
                        st.last_backup_failed
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(None) => {} // in-memory database: nothing to back up
                    Err(e) => {
                        tracing::error!(error = %e, "scheduled backup failed");
                        st.last_backup_failed
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        });
    }

    // Webhook alerting (FR-6.6/FR-12.17): evaluates accounting and account
    // health on an interval and fires edge-triggered alerts. No-op when
    // KINETIX_ALERT_WEBHOOK_URL is unset.
    {
        let st = state.clone();
        let alerts = std::sync::Arc::new(alerts::AlertState::new());
        tokio::spawn(async move {
            alerts::run(st, alerts).await;
        });
    }

    // Purge expired body logs (FR-6.5 retention) and old route traces.
    let st = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match db::purge_expired_body_logs(&st.pool).await {
                Ok(n) if n > 0 => tracing::info!(purged = n, "purged expired body logs"),
                _ => {}
            }
            if let Ok(n) = db::purge_old_route_traces(&st.pool, 30).await {
                if n > 0 {
                    tracing::info!(purged = n, "purged old route traces");
                }
            }
        }
    });

    // Recover cooled-down / exhausted accounts whose windows have elapsed
    // (FR-12.9): recovery is bounded by the registry reload above, which flips
    // `effective_status` back to healthy automatically once the window passes.
}

/// Startup diagnostic: warn about enabled Routes with no eligible target so an
/// operator notices a misconfiguration at boot (NFR-2.7 / Monitoring). Reads the
/// snapshot only; never affects serving.
fn warn_on_unroutable_routes(registry: &Registry) {
    let names = registry.routes_with_no_targets();
    if !names.is_empty() {
        tracing::warn!(
            routes = ?names,
            "enabled route(s) have no eligible target at startup; requests to them will fail until configured"
        );
    }
}

async fn shutdown_signal(grace_secs: u64) {
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
    tracing::info!(
        grace_secs,
        "shutdown signal received; draining in-flight requests (bounded)"
    );
}
