//! Shared application state.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

use crate::adapters::AdapterRegistry;
use crate::config::Config;
use crate::credentials::StaticKeyStrategy;
use crate::crypto::Crypto;
use crate::db::Pool;
use crate::logqueue::UsageLogQueue;
use crate::registry::Registry;
use crate::trace::FlightRecorder;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: Pool,
    pub registry: Arc<Registry>,
    pub crypto: Arc<Crypto>,
    pub credentials: Arc<StaticKeyStrategy>,
    pub adapters: AdapterRegistry,
    pub http: reqwest::Client,
    /// HTTP client that follows redirects (only for opt-in providers, NFR-3.10).
    pub http_redirect: reqwest::Client,
    pub log_queue: UsageLogQueue,
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Diagnostic flight recorder (FR-13).
    pub flight: Arc<FlightRecorder>,
    /// Prompt-cache-affinity sticky routing (FR-7.3): session id -> route target
    /// key. Bounded in time by a periodic sweep.
    sticky: Arc<DashMap<String, StickyEntry>>,
    rr_counters: Arc<DashMap<String, Arc<AtomicU64>>>,
    /// Before-commit / after-commit failure counters (FR-4.9).
    pub failures_pre_commit: Arc<AtomicU64>,
    pub failures_post_commit: Arc<AtomicU64>,
    pub cancellations: Arc<AtomicU64>,
    pub cancellation_latency_ms_total: Arc<AtomicU64>,
    /// Requests rejected because the provider was at its RPM/TPM or the key hit
    /// a budget (dashboard counters).
    pub total_requests: Arc<AtomicU64>,
}

#[derive(Clone)]
pub struct StickyEntry {
    /// The selected route target key (route id + account id + model id).
    pub target_key: String,
    pub at: std::time::Instant,
}

impl AppState {
    pub fn new(
        config: Arc<Config>,
        pool: Pool,
        registry: Arc<Registry>,
        crypto: Arc<Crypto>,
        http: reqwest::Client,
        http_redirect: reqwest::Client,
        log_queue: UsageLogQueue,
    ) -> Self {
        let credentials = Arc::new(StaticKeyStrategy::new(crypto.clone()));
        AppState {
            config,
            pool,
            registry,
            crypto,
            credentials,
            adapters: AdapterRegistry::new(),
            http,
            http_redirect,
            log_queue,
            started_at: chrono::Utc::now(),
            flight: Arc::new(FlightRecorder::new(512, 128)),
            sticky: Arc::new(DashMap::new()),
            rr_counters: Arc::new(DashMap::new()),
            failures_pre_commit: Arc::new(AtomicU64::new(0)),
            failures_post_commit: Arc::new(AtomicU64::new(0)),
            cancellations: Arc::new(AtomicU64::new(0)),
            cancellation_latency_ms_total: Arc::new(AtomicU64::new(0)),
            total_requests: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Round-robin counter for a route.
    pub fn rr_counter(&self, route_id: &str) -> Arc<AtomicU64> {
        self.rr_counters
            .entry(route_id.to_string())
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .clone()
    }

    /// Remember the target a session was served by (FR-7.3).
    pub fn sticky_remember(&self, session: &str, target_key: String) {
        self.sticky.insert(
            session.to_string(),
            StickyEntry {
                target_key,
                at: std::time::Instant::now(),
            },
        );
    }

    /// Look up a session's previously-selected target, if still fresh.
    pub fn sticky_lookup(&self, session: &str, ttl: std::time::Duration) -> Option<String> {
        self.sticky.get(session).and_then(|e| {
            if e.at.elapsed() <= ttl {
                Some(e.target_key.clone())
            } else {
                None
            }
        })
    }

    /// Drop sticky entries older than `ttl` (bounded memory).
    pub fn sticky_sweep(&self, ttl: std::time::Duration) {
        self.sticky.retain(|_, e| e.at.elapsed() <= ttl);
    }

    pub fn uptime_secs(&self) -> i64 {
        (chrono::Utc::now() - self.started_at).num_seconds()
    }

    pub fn bump_counter(&self, _k: &str) {
        let _ = Ordering::Relaxed;
    }
}
