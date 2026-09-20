//! Plugin lifecycle and invocation (§11–§16).
//!
//! The manager owns install/enable/disable/remove, the per-plugin circuit
//! breaker, and every host→guest invocation. It is the only place that talks to
//! Wasmtime for request-path work, and it always maps guest results into typed
//! evidence that core policy consumes.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use tokio::sync::Semaphore;

use crate::crypto::Crypto;
use crate::db::Pool;

use super::manifest::{self, HostPolicy};
use super::package::{self, Package, SignatureStatus};
use super::runtime::{
    bindings, wit, DeadlineGuard, HostBacking, HostCtx, PluginFault, PluginRuntime,
};
use super::store::{self, PermissionGrant, PluginRow};
use super::types::{Capability, CircuitState, Limits, Manifest, Provided};

/// Bounds concurrent guest invocations so a flood of one plugin cannot exhaust
/// host threads or memory (§14).
const MAX_CONCURRENT_INVOCATIONS: usize = 16;
/// Consecutive runtime faults before a plugin's circuit opens (§15).
const CIRCUIT_FAULT_THRESHOLD: i64 = 5;
/// Cooldown before a half-open probe (§15).
const CIRCUIT_OPEN_SECS: i64 = 60;

/// Backing implementation for host effects (storage, logs, credentials).
pub struct Backing {
    pool: Pool,
    crypto: Arc<Crypto>,
}

#[async_trait::async_trait]
impl HostBacking for Backing {
    async fn kv_get(&self, plugin_id: &str, key: &str) -> Result<Option<Vec<u8>>> {
        store::kv_get(&self.pool, &self.crypto, plugin_id, key).await
    }
    async fn kv_put(&self, plugin_id: &str, key: &str, value: &[u8]) -> Result<()> {
        store::kv_put(&self.pool, &self.crypto, plugin_id, key, value).await
    }
    async fn kv_delete(&self, plugin_id: &str, key: &str) -> Result<()> {
        store::kv_delete(&self.pool, plugin_id, key).await
    }
    fn log(&self, plugin_id: &str, level: &str, message: &str) {
        // §18: plugin logs are namespaced and redacted like core logs.
        match level {
            "error" => tracing::error!(plugin = %plugin_id, "plugin: {message}"),
            "warn" => tracing::warn!(plugin = %plugin_id, "plugin: {message}"),
            "info" => tracing::info!(plugin = %plugin_id, "plugin: {message}"),
            "debug" => tracing::debug!(plugin = %plugin_id, "plugin: {message}"),
            _ => tracing::trace!(plugin = %plugin_id, "plugin: {message}"),
        }
    }
    async fn resolve_secret(
        &self,
        _plugin_id: &str,
        _provider_id: &str,
        account_id: &str,
    ) -> Result<String> {
        let account = crate::db::get_account(&self.pool, account_id)
            .await?
            .ok_or_else(|| anyhow!("account not found"))?;
        self.crypto.decrypt(&account.secret_enc)
    }
}

/// The plugin manager. Cheap to clone (Arc inside).
#[derive(Clone)]
pub struct PluginManager {
    inner: Arc<Inner>,
}

struct Inner {
    runtime: PluginRuntime,
    pool: Pool,
    crypto: Arc<Crypto>,
    backing: Arc<Backing>,
    http: reqwest::Client,
    policy: HostPolicy,
    semaphore: Semaphore,
    /// Simple counters for the admin metrics surface (§18).
    invocations: std::sync::atomic::AtomicU64,
    faults: std::sync::atomic::AtomicU64,
    timeouts: std::sync::atomic::AtomicU64,
    cancellations: std::sync::atomic::AtomicU64,
    http_requests: std::sync::atomic::AtomicU64,
}

impl PluginManager {
    pub fn new(
        pool: Pool,
        crypto: Arc<Crypto>,
        http: reqwest::Client,
        policy: HostPolicy,
    ) -> Result<Self> {
        let runtime = PluginRuntime::new()?;
        let backing = Arc::new(Backing {
            pool: pool.clone(),
            crypto: crypto.clone(),
        });
        Ok(PluginManager {
            inner: Arc::new(Inner {
                runtime,
                pool,
                crypto,
                backing,
                http,
                policy,
                semaphore: Semaphore::new(MAX_CONCURRENT_INVOCATIONS),
                invocations: Default::default(),
                faults: Default::default(),
                timeouts: Default::default(),
                cancellations: Default::default(),
                http_requests: Default::default(),
            }),
        })
    }

    pub fn policy(&self) -> HostPolicy {
        self.inner.policy
    }

    pub fn counters(&self) -> PluginCounters {
        use std::sync::atomic::Ordering::Relaxed;
        PluginCounters {
            invocations: self.inner.invocations.load(Relaxed),
            faults: self.inner.faults.load(Relaxed),
            timeouts: self.inner.timeouts.load(Relaxed),
            cancellations: self.inner.cancellations.load(Relaxed),
            http_requests: self.inner.http_requests.load(Relaxed),
        }
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Install (or upgrade) a plugin from package bytes. Verifies hash, parses
    /// and validates the manifest, checks signature, compiles the component,
    /// and stores it **installed-disabled** (§11).
    pub async fn install(
        &self,
        bytes: &[u8],
        expected_sha256: Option<&str>,
        trusted_keys: &[[u8; 32]],
        allow_untrusted_signature: bool,
    ) -> Result<InstallOutcome> {
        let pkg = package::read_package(bytes)?;
        if let Some(expected) = expected_sha256 {
            if !expected.eq_ignore_ascii_case(&pkg.package_sha256) {
                bail!(
                    "package hash mismatch: expected {expected}, computed {}",
                    pkg.package_sha256
                );
            }
        }
        let validated = package::validate_manifest(&pkg, self.inner.policy)?;
        let sig = package::verify_signature(&pkg, trusted_keys)?;
        if sig == SignatureStatus::Untrusted && !allow_untrusted_signature {
            bail!("package signature is present but not from a trusted publisher key");
        }
        // Compile now so a broken component is rejected before it is stored.
        self.inner
            .runtime
            .compile(&pkg.component)
            .map_err(|e| anyhow!("{e}"))?;

        let grants = permission_grants(&validated.manifest);
        store::upsert_plugin(
            &self.inner.pool,
            &validated,
            &pkg.package_sha256,
            &pkg.component,
            sig.as_str(),
            &grants,
        )
        .await?;

        Ok(InstallOutcome {
            id: validated.manifest.id.clone(),
            version: validated.manifest.version.clone(),
            signature: sig,
            provides: validated.manifest.provides.provided(),
        })
    }

    /// Install from a local file path. The computed SHA-256 is recorded (§11).
    pub async fn install_file(
        &self,
        path: &std::path::Path,
        trusted_keys: &[[u8; 32]],
        allow_untrusted_signature: bool,
    ) -> Result<InstallOutcome> {
        let bytes = std::fs::read(path).map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
        self.install(&bytes, None, trusted_keys, allow_untrusted_signature)
            .await
    }

    /// Enable an installed plugin, instantiating it once to prove it loads.
    pub async fn enable(&self, id: &str) -> Result<()> {
        let row = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("plugin '{id}' is not installed"))?;
        let manifest = row
            .manifest()
            .ok_or_else(|| anyhow!("plugin '{id}' has an unreadable manifest"))?;
        if !manifest.compatible() {
            bail!("plugin '{id}' is not API-compatible with this host");
        }
        let limits = manifest::effective_limits(&manifest, self.inner.policy)?;
        // Instantiate to prove the component links against our host API.
        let mut store = self.new_store(&row, &limits, false);
        let component = self.inner.runtime.compile(&row.component)?;
        let linker = self.inner.runtime.linker()?;
        let _ = self
            .inner
            .runtime
            .instantiate(&linker, &mut store, &component)
            .await?;
        store::set_enabled(&self.inner.pool, id, true).await?;
        store::clear_plugin_failures(&self.inner.pool, id).await?;
        Ok(())
    }

    pub async fn disable(&self, id: &str) -> Result<()> {
        store::set_enabled(&self.inner.pool, id, false).await?;
        Ok(())
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        store::delete_plugin(&self.inner.pool, id).await?;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<PluginRow>> {
        store::list_plugins(&self.inner.pool).await
    }

    pub async fn get(&self, id: &str) -> Result<Option<PluginRow>> {
        store::get_plugin(&self.inner.pool, id).await
    }

    /// Read the plugin's host-stamped cached routing facts (§6.4) for the
    /// request path. Returns `(name, value_json, observed_at, max_age_ms)`.
    /// This never invokes the guest, so a `cached` fact provider cannot add
    /// network or wall-time cost to routing.
    pub async fn cached_facts(
        &self,
        id: &str,
    ) -> Result<Vec<(String, serde_json::Value, Option<String>, Option<u64>)>> {
        let entries = store::kv_list_prefix(
            &self.inner.pool,
            &self.inner.crypto,
            id,
            super::runtime::CACHE_PREFIX,
        )
        .await?;
        let mut out = Vec::new();
        for (key, bytes) in entries {
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let name = key
                .strip_prefix(super::runtime::CACHE_PREFIX)
                .unwrap_or(&key)
                .to_string();
            let value = env.get("value").cloned().unwrap_or(serde_json::Value::Null);
            let observed_at = env
                .get("observed_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let max_age_ms = env.get("max_age_ms").and_then(|v| v.as_u64());
            out.push((name, value, observed_at, max_age_ms));
        }
        Ok(out)
    }

    /// Whether a plugin is installed, enabled, and its circuit is not open.
    pub async fn is_usable(&self, id: &str) -> bool {
        match self.get(id).await {
            Ok(Some(row)) if row.status().is_enabled() => {
                match store::runtime_state(&self.inner.pool, id).await {
                    Ok(Some(state)) => match state.circuit() {
                        CircuitState::Open => false,
                        _ => true,
                    },
                    _ => true,
                }
            }
            _ => false,
        }
    }

    /// Whether the plugin provides the named capability.
    pub async fn provides(&self, id: &str, capability: Capability, name: &str) -> bool {
        match self.get(id).await {
            Ok(Some(row)) => row
                .manifest()
                .map(|m| {
                    m.provides
                        .provided()
                        .iter()
                        .any(|p| p.capability == capability && p.name == name)
                })
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Enabled plugins that declare the named read-only hook, newest id order.
    /// A hook is a side observer: if the plugin is disabled, faulted, or its
    /// circuit is open it simply does not run (core never fails a request on a
    /// hook's behalf, §6.6).
    pub async fn plugins_with_hook(&self, hook: &str) -> Vec<String> {
        let Ok(rows) = self.list().await else {
            return Vec::new();
        };
        let mut ids = Vec::new();
        for row in rows {
            let declares = row
                .manifest()
                .map(|m| m.provides.hooks.iter().any(|h| h == hook))
                .unwrap_or(false);
            if declares && self.is_usable(&row.id).await {
                ids.push(row.id);
            }
        }
        ids
    }

    // -----------------------------------------------------------------------
    // Invocation plumbing
    // -----------------------------------------------------------------------

    fn new_store(
        &self,
        row: &PluginRow,
        limits: &manifest::EffectiveLimits,
        adapter: bool,
    ) -> wasmtime::Store<HostCtx> {
        let manifest = row.manifest().unwrap_or_else(|| Manifest {
            manifest_version: 1,
            id: row.id.clone(),
            name: row.id.clone(),
            version: row.version.clone(),
            plugin_api: "1".into(),
            provides: Default::default(),
            permissions: Default::default(),
            limits: Limits::default(),
            routing_facts_mode: "pure".into(),
        });
        let ctx = HostCtx {
            plugin_id: row.id.clone(),
            network_hosts: manifest.permissions.network_hosts.clone(),
            credential_read: manifest.permissions.credential_read,
            credential_sign: !manifest.permissions.credential_scopes.is_empty(),
            credential_scopes: manifest.permissions.credential_scopes.clone(),
            storage_quota: limits.storage,
            max_outbound_requests: limits.max_outbound_requests,
            max_http_body: limits.max_http_body,
            adapter_stream: adapter,
            routing_facts_pure: manifest.routing_facts_mode != "cached",
            outbound_count: 0,
            http: self.inner.http.clone(),
            backing: self.inner.backing.clone(),
            limits: wasmtime::StoreLimitsBuilder::new().build(),
        };
        // `new_store` replaces the placeholder store-limits field.
        self.inner.runtime.new_store(ctx, limits.memory)
    }

    /// Prepare a ready-to-call instance for a plugin.
    async fn prepare(&self, id: &str, adapter: bool) -> Result<Prepared> {
        let row = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("plugin '{id}' is not installed"))?;
        if !row.status().is_enabled() {
            bail!("plugin '{id}' is not enabled");
        }
        let manifest = row
            .manifest()
            .ok_or_else(|| anyhow!("plugin '{id}' has an unreadable manifest"))?;
        let limits = manifest::effective_limits(&manifest, self.inner.policy)?;
        let component = self.inner.runtime.compile(&row.component)?;
        let linker = self.inner.runtime.linker()?;
        let mut store = self.new_store(&row, &limits, adapter);
        let plugin = self
            .inner
            .runtime
            .instantiate(&linker, &mut store, &component)
            .await?;
        Ok(Prepared {
            store,
            plugin,
            wall_time: Duration::from_millis(limits.wall_time_ms),
        })
    }

    // -----------------------------------------------------------------------
    // ProviderAdapter invocation (§6.3, §7.1)
    //
    // The adapter world is bound separately so the buffered host-http import can
    // never serve as the adapter transport. Adapters are synchronous translation
    // functions; the manager exposes async methods and `PluginAdapter` (the sync
    // `Adapter` impl) bridges them via `block_in_place`.
    // -----------------------------------------------------------------------

    /// Instantiate the adapter world for a plugin.
    async fn prepare_adapter(&self, id: &str) -> Result<AdapterPrepared> {
        let row = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("plugin '{id}' is not installed"))?;
        if !row.status().is_enabled() {
            bail!("plugin '{id}' is not enabled");
        }
        let manifest = row
            .manifest()
            .ok_or_else(|| anyhow!("plugin '{id}' has an unreadable manifest"))?;
        let limits = manifest::effective_limits(&manifest, self.inner.policy)?;
        let component = self.inner.runtime.compile(&row.component)?;
        let linker = self.inner.runtime.linker()?;
        let mut store = self.new_store(&row, &limits, true);
        let plugin = self
            .inner
            .runtime
            .instantiate_adapter(&linker, &mut store, &component)
            .await?;
        Ok(AdapterPrepared {
            store,
            plugin,
            wall_time: Duration::from_millis(limits.wall_time_ms),
        })
    }

    pub async fn adapter_wire_format(&self, id: &str) -> Result<String, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare_adapter(id)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .provider_adapter()
            .call_wire_format(&mut p.store)
            .await
            .map_err(map_call_error);
        self.settle_cancellable(id, &guard, res).await
    }

    pub async fn adapter_build_url(
        &self,
        id: &str,
        provider_json: &str,
        model_json: &str,
    ) -> Result<String, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare_adapter(id)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .provider_adapter()
            .call_build_url(&mut p.store, provider_json, model_json)
            .await
            .map_err(map_call_error)
            .and_then(map_adapter_result);
        self.settle_cancellable(id, &guard, res).await
    }

    pub async fn adapter_apply_auth(
        &self,
        id: &str,
        provider_json: &str,
        credential: &str,
    ) -> Result<String, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare_adapter(id)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .provider_adapter()
            .call_apply_auth(&mut p.store, provider_json, credential)
            .await
            .map_err(map_call_error)
            .and_then(map_adapter_result);
        self.settle_cancellable(id, &guard, res).await
    }

    pub async fn adapter_build_body(
        &self,
        id: &str,
        request_json: &str,
        provider_json: &str,
        model_json: &str,
    ) -> Result<String, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare_adapter(id)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .provider_adapter()
            .call_build_body(&mut p.store, request_json, provider_json, model_json)
            .await
            .map_err(map_call_error)
            .and_then(map_adapter_result);
        self.settle_cancellable(id, &guard, res).await
    }

    pub async fn adapter_classify_error(
        &self,
        id: &str,
        status: u16,
        body: &str,
        headers_json: &str,
    ) -> Result<String, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare_adapter(id)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .provider_adapter()
            .call_classify_error(&mut p.store, status, body, headers_json)
            .await
            .map_err(map_call_error)
            .and_then(map_adapter_result);
        self.settle_cancellable(id, &guard, res).await
    }

    pub async fn adapter_parse_stream_chunk(
        &self,
        id: &str,
        data: &str,
    ) -> Result<String, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare_adapter(id)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .provider_adapter()
            .call_parse_stream_chunk(&mut p.store, data)
            .await
            .map_err(map_call_error)
            .and_then(map_adapter_result);
        self.settle_cancellable(id, &guard, res).await
    }

    pub async fn adapter_parse_full_response(
        &self,
        id: &str,
        body_json: &str,
    ) -> Result<String, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare_adapter(id)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .provider_adapter()
            .call_parse_full_response(&mut p.store, body_json)
            .await
            .map_err(map_call_error)
            .and_then(map_adapter_result);
        self.settle_cancellable(id, &guard, res).await
    }

    /// Record a successful invocation, closing the breaker.
    async fn record_success(&self, id: &str) {
        let _ = store::clear_plugin_failures(&self.inner.pool, id).await;
    }

    /// Record a fault and trip the breaker when the threshold is reached (§15).
    async fn record_fault(&self, id: &str, fault: &PluginFault) {
        use std::sync::atomic::Ordering::Relaxed;
        self.inner.faults.fetch_add(1, Relaxed);
        if matches!(fault, PluginFault::Timeout) {
            self.inner.timeouts.fetch_add(1, Relaxed);
        }
        if !fault.counts_against_circuit() {
            return;
        }
        let _ = store::record_plugin_failure(
            &self.inner.pool,
            id,
            CIRCUIT_FAULT_THRESHOLD,
            CIRCUIT_OPEN_SECS,
            fault.code(),
        )
        .await;
    }

    fn bump_invocation(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.inner.invocations.fetch_add(1, Relaxed);
    }

    // -----------------------------------------------------------------------
    // Capability invocations
    // -----------------------------------------------------------------------

    /// CredentialStrategy::resolve (§6.1). Returns the opaque lease only; the
    /// secret never crosses back to core from a plugin in this path.
    pub async fn credential_resolve(
        &self,
        id: &str,
        provider_id: &str,
        account_id: &str,
        account_label: &str,
    ) -> Result<wit::types::CredentialLease, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare(id, false)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let _guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .credential_strategy()
            .call_resolve(&mut p.store, provider_id, account_id, account_label)
            .await
            .map_err(map_call_error)
            .and_then(map_plugin_result);
        self.settle(id, res).await
    }

    /// ModelSource::discover (§6.2).
    pub async fn model_discover(
        &self,
        id: &str,
        provider_id: &str,
        base_url: &str,
        models_path: &str,
    ) -> Result<Vec<wit::types::DiscoveredModel>, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare(id, false)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let _guard = rt.arm_deadline(&mut p.store, Duration::from_secs(30));
        let res = plugin
            .model_source()
            .call_discover(&mut p.store, provider_id, base_url, models_path)
            .await
            .map_err(map_call_error)
            .and_then(map_plugin_result);
        self.settle(id, res).await
    }

    /// HealthProbe::probe (§6.5).
    pub async fn health_probe(
        &self,
        id: &str,
        provider_id: &str,
        account_id: &str,
    ) -> Result<wit::types::HealthObservation, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare(id, false)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let _guard = rt.arm_deadline(&mut p.store, Duration::from_secs(10));
        let res = plugin
            .health_probe()
            .call_probe(&mut p.store, provider_id, account_id)
            .await
            .map_err(map_call_error)
            .and_then(map_plugin_result);
        self.settle(id, res).await
    }

    /// RoutingFacts::facts (§6.4). `request_json` must carry only request/config
    /// facts core already knows. `cancelled` is polled while the guest runs; if
    /// the client disconnects the guest is epoch-interrupted and the outcome is
    /// reported as [`PluginFault::Cancelled`], never a fault (§7.2).
    pub async fn routing_facts_cancellable(
        &self,
        id: &str,
        request_json: &str,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Vec<wit::types::RoutingFact>, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare(id, false)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let guard = rt.arm_deadline(&mut p.store, Duration::from_millis(25));
        let watchdog = spawn_cancel_watchdog(guard.cancelled.clone(), cancelled);
        let res = plugin
            .routing_facts()
            .call_facts(&mut p.store, request_json)
            .await
            .map_err(map_call_error)
            .and_then(map_plugin_result);
        watchdog.abort();
        self.settle_cancellable(id, &guard, res).await
    }

    /// RoutingFacts::facts (§6.4). `request_json` must carry only request/config
    /// facts core already knows.
    pub async fn routing_facts(
        &self,
        id: &str,
        request_json: &str,
    ) -> Result<Vec<wit::types::RoutingFact>, PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare(id, false)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let _guard = rt.arm_deadline(&mut p.store, Duration::from_millis(25));
        let res = plugin
            .routing_facts()
            .call_facts(&mut p.store, request_json)
            .await
            .map_err(map_call_error)
            .and_then(map_plugin_result);
        self.settle(id, res).await
    }

    /// Read-only hook: on_request_normalized (§6.6).
    pub async fn hook_request_normalized(
        &self,
        id: &str,
        request_json: &str,
    ) -> Result<(), PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare(id, false)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let _guard = rt.arm_deadline(&mut p.store, Duration::from_millis(25));
        let res = plugin
            .hooks()
            .call_on_request_normalized(&mut p.store, request_json)
            .await
            .map_err(map_call_error)
            .and_then(map_plugin_result);
        self.settle(id, res).await
    }

    /// Read-only hook: on_target_candidate (§6.6).
    pub async fn hook_target_candidate(
        &self,
        id: &str,
        target_json: &str,
    ) -> Result<(), PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare(id, false)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let _guard = rt.arm_deadline(&mut p.store, Duration::from_millis(25));
        let res = plugin
            .hooks()
            .call_on_target_candidate(&mut p.store, target_json)
            .await
            .map_err(map_call_error)
            .and_then(map_plugin_result);
        self.settle(id, res).await
    }

    /// Fire-and-forget hook: on_usage_finalized (§6.6). Runs off the request
    /// path; the caller must never await it inline.
    pub async fn hook_usage_finalized(
        &self,
        id: &str,
        usage_json: &str,
    ) -> Result<(), PluginFault> {
        self.bump_invocation();
        let _permit = self.inner.semaphore.acquire().await;
        let mut p = self
            .prepare(id, false)
            .await
            .map_err(|e| PluginFault::Internal(e.to_string()))?;
        let plugin = p.plugin;
        let rt = self.inner.runtime.clone();
        let _guard = rt.arm_deadline(&mut p.store, p.wall_time);
        let res = plugin
            .hooks()
            .call_on_usage_finalized(&mut p.store, usage_json)
            .await
            .map_err(map_call_error)
            .and_then(map_plugin_result);
        self.settle(id, res).await
    }

    /// Validate an installed plugin by instantiating it (§11 self-check).
    pub async fn validate(&self, id: &str) -> Result<Vec<Provided>> {
        let row = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("plugin '{id}' is not installed"))?;
        let manifest = row
            .manifest()
            .ok_or_else(|| anyhow!("plugin '{id}' has an unreadable manifest"))?;
        let limits = manifest::effective_limits(&manifest, self.inner.policy)?;
        let component = self.inner.runtime.compile(&row.component)?;
        let linker = self.inner.runtime.linker()?;
        let mut store = self.new_store(&row, &limits, false);
        let _ = self
            .inner
            .runtime
            .instantiate(&linker, &mut store, &component)
            .await?;
        Ok(manifest.provides.provided())
    }

    /// Resolve a `plugin:<id>/<capability>` reference to an enabled plugin that
    /// provides it. `None` means the binding is unsatisfied (fail closed, §6.0).
    pub async fn resolve_binding(&self, reference: &str, capability: Capability) -> Option<String> {
        let r = super::types::PluginRef::parse(reference)?;
        if self.provides(&r.plugin_id, capability, &r.capability).await
            && self.is_usable(&r.plugin_id).await
        {
            Some(r.plugin_id)
        } else {
            None
        }
    }

    async fn settle<T>(&self, id: &str, res: Result<T, PluginFault>) -> Result<T, PluginFault> {
        match res {
            Ok(v) => {
                self.record_success(id).await;
                Ok(v)
            }
            Err(fault) => {
                self.record_fault(id, &fault).await;
                Err(fault)
            }
        }
    }

    /// As [`settle`], but a cancellation is never counted as a fault (§7.2):
    /// when the client disconnects the guest is epoch-interrupted, and that
    /// termination must not move the plugin toward an open circuit.
    async fn settle_cancellable<T>(
        &self,
        id: &str,
        guard: &DeadlineGuard,
        res: Result<T, PluginFault>,
    ) -> Result<T, PluginFault> {
        match res {
            Ok(v) => {
                self.record_success(id).await;
                Ok(v)
            }
            Err(fault) => {
                if guard.is_cancelled() {
                    // Client-driven cancellation: recorded as a cancellation, not
                    // a plugin fault (AC: a disconnect must not count as a fault).
                    self.inner
                        .cancellations
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Err(PluginFault::Cancelled);
                }
                self.record_fault(id, &fault).await;
                Err(fault)
            }
        }
    }
}

/// Poll an external cancellation flag while a guest call runs and, once set,
/// flip the guard's cancelled flag. The guard's epoch ticker then interrupts the
/// guest on its next tick (§7.2). Returns the watchdog task to abort after the
/// call completes.
fn spawn_cancel_watchdog(
    guard_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    external: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if external.load(std::sync::atomic::Ordering::SeqCst) {
                guard_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
}

struct Prepared {
    store: wasmtime::Store<HostCtx>,
    plugin: bindings::Plugin,
    wall_time: Duration,
}

struct AdapterPrepared {
    store: wasmtime::Store<HostCtx>,
    plugin: crate::plugins::runtime::adapter_bindings::PluginAdapter,
    wall_time: Duration,
}

/// Map a `wasmtime::Result` error from a guest call into a [`PluginFault`].
fn map_call_error(e: wasmtime::Error) -> PluginFault {
    let msg = e.to_string();
    if msg.contains("epoch") || msg.contains("interrupt") || msg.contains("deadline") {
        PluginFault::Timeout
    } else {
        PluginFault::Trap(msg)
    }
}

/// Map the guest's `Result<T, PluginError>` into a [`PluginFault`].
fn map_plugin_result<T>(r: Result<T, wit::types::PluginError>) -> Result<T, PluginFault> {
    r.map_err(|e| PluginFault::PluginError {
        code: e.code,
        message: e.message,
        retryable: e.retryable,
    })
}

/// Like [`map_plugin_result`] but for the separately-bound adapter world, whose
/// generated `PluginError` type is distinct from the `plugin` world's.
fn map_adapter_result<T>(
    r: Result<T, crate::plugins::runtime::adapter_bindings::kinetix::plugin::types::PluginError>,
) -> Result<T, PluginFault> {
    r.map_err(|e| PluginFault::PluginError {
        code: e.code,
        message: e.message,
        retryable: e.retryable,
    })
}

/// Host-side counters surfaced by the admin metrics endpoint (§18).
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct PluginCounters {
    pub invocations: u64,
    pub faults: u64,
    pub timeouts: u64,
    pub cancellations: u64,
    pub http_requests: u64,
}

/// The outcome of a successful install.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub id: String,
    pub version: String,
    pub signature: SignatureStatus,
    pub provides: Vec<Provided>,
}

/// Turn a validated manifest into the all-or-nothing permission grant set (§20).
pub fn permission_grants(manifest: &Manifest) -> Vec<PermissionGrant> {
    let mut grants = Vec::new();
    if !manifest.permissions.network_hosts.is_empty() {
        grants.push(PermissionGrant {
            permission: "network_hosts".into(),
            value_json: serde_json::to_string(&manifest.permissions.network_hosts)
                .unwrap_or_else(|_| "[]".into()),
        });
    }
    if !manifest.permissions.credential_scopes.is_empty() {
        grants.push(PermissionGrant {
            permission: "credential_scopes".into(),
            value_json: serde_json::to_string(&manifest.permissions.credential_scopes)
                .unwrap_or_else(|_| "[]".into()),
        });
    }
    if manifest.permissions.credential_read {
        grants.push(PermissionGrant {
            permission: "credential_read".into(),
            value_json: "true".into(),
        });
    }
    grants
}

/// The set of capabilities an enabled plugin provides, for the dashboard.
pub fn manifest_summary(row: &PluginRow) -> serde_json::Value {
    let manifest = row.manifest();
    serde_json::json!({
        "id": row.id,
        "version": row.version,
        "plugin_api_major": row.plugin_api_major,
        "sha256": row.package_sha256,
        "signature": row.signature,
        "status": row.status().as_str(),
        "provides": manifest.as_ref().map(|m| m.provides.provided()).unwrap_or_default(),
        "permissions": manifest.as_ref().map(|m| m.permissions.clone()).unwrap_or_default(),
        "limits": manifest.as_ref().map(|m| m.limits.clone()).unwrap_or_default(),
    })
}

/// Package-level helper used by the CLI and admin API.
pub fn read_package(path: &std::path::Path) -> Result<Package> {
    package::read_package_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_grants_are_all_or_nothing() {
        let m: Manifest = toml::from_str(
            r#"
manifest_version = 1
id = "x"
name = "X"
version = "1"
plugin_api = "1"
[provides]
model_sources = ["m"]
[permissions]
network_hosts = ["a.example"]
credential_scopes = ["provider:p"]
credential_read = true
"#,
        )
        .unwrap();
        let grants = permission_grants(&m);
        assert_eq!(grants.len(), 3);
        assert!(grants.iter().any(|g| g.permission == "credential_read"));
    }
}
