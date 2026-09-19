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

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: Pool,
    pub registry: Arc<Registry>,
    pub crypto: Arc<Crypto>,
    pub credentials: Arc<StaticKeyStrategy>,
    pub adapters: AdapterRegistry,
    pub http: reqwest::Client,
    pub log_queue: UsageLogQueue,
    pub started_at: chrono::DateTime<chrono::Utc>,
    rr_counters: Arc<DashMap<String, Arc<AtomicU64>>>,
}

impl AppState {
    pub fn new(
        config: Arc<Config>,
        pool: Pool,
        registry: Arc<Registry>,
        crypto: Arc<Crypto>,
        http: reqwest::Client,
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
            log_queue,
            started_at: chrono::Utc::now(),
            rr_counters: Arc::new(DashMap::new()),
        }
    }

    /// Round-robin counter for a combo.
    pub fn rr_counter(&self, combo_id: &str) -> Arc<AtomicU64> {
        self.rr_counters
            .entry(combo_id.to_string())
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .clone()
    }

    pub fn uptime_secs(&self) -> i64 {
        (chrono::Utc::now() - self.started_at).num_seconds()
    }

    pub fn bump_counter(&self, _k: &str) {
        let _ = Ordering::Relaxed;
    }
}
