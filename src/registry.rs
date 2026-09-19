//! Runtime registry: an in-memory snapshot of the admin-configured providers,
//! accounts, models, aliases, and combos. Reloaded from the database whenever
//! configuration changes so edits take effect without a restart (FR-10.13).
//!
//! In-flight requests keep the `Arc<Registry>` snapshot they started with.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::RwLock;

use crate::db::{self, AccountRow, AliasRow, ComboRow, ComboTargetRow, ModelRow, Pool, ProviderRow};

#[derive(Clone)]
pub struct Registry {
    inner: Arc<RwLock<Snapshot>>,
}

#[derive(Default)]
pub struct Snapshot {
    pub providers: HashMap<String, ProviderRow>,
    pub provider_order: Vec<String>,
    pub accounts: HashMap<String, AccountRow>,
    pub models: HashMap<String, ModelRow>,
    /// (provider_id, upstream_id) -> model_id
    pub model_by_upstream: HashMap<(String, String), String>,
    pub aliases: HashMap<String, AliasRow>,
    pub combos: HashMap<String, ComboRow>,
    pub combo_targets: HashMap<String, Vec<ComboTargetRow>>,
}

/// A resolved route for a client-requested model name.
#[derive(Debug, Clone)]
pub enum Route {
    /// A single (provider, model) target.
    Single { provider_id: String, model_id: String },
    /// A combo with an ordered list of targets.
    Combo {
        combo: ComboRow,
        targets: Vec<ResolvedTarget>,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub account: AccountRow,
    pub model: ModelRow,
    pub provider: ProviderRow,
    pub priority: i64,
    pub weight: i64,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            inner: Arc::new(RwLock::new(Snapshot::default())),
        }
    }

    pub async fn reload(&self, pool: &Pool) -> Result<()> {
        let providers = db::list_providers(pool).await?;
        let accounts = db::list_accounts(pool).await?;
        let models = db::list_models(pool).await?;
        let aliases = db::list_aliases(pool).await?;
        let combos = db::list_combos(pool).await?;

        let mut snap = Snapshot::default();
        for p in providers {
            if p.enabled == 0 {
                continue;
            }
            snap.provider_order.push(p.id.clone());
            snap.providers.insert(p.id.clone(), p);
        }
        for a in accounts {
            snap.accounts.insert(a.id.clone(), a);
        }
        for m in models {
            snap.model_by_upstream
                .insert((m.provider_id.clone(), m.upstream_id.clone()), m.id.clone());
            snap.models.insert(m.id.clone(), m);
        }
        for a in aliases {
            snap.aliases.insert(a.alias.clone(), a);
        }
        for c in combos {
            let targets = db::combo_targets(pool, &c.id).await?;
            snap.combo_targets.insert(c.id.clone(), targets);
            snap.combos.insert(c.id.clone(), c);
        }

        *self.inner.write() = snap;
        Ok(())
    }

    pub fn snapshot(&self) -> parking_lot::RwLockReadGuard<'_, Snapshot> {
        self.inner.read()
    }

    /// Resolve a client-facing model name to a route.
    ///
    /// Order: exact alias -> combo by name -> `provider/model-id` ->
    /// bare upstream model id (first provider that has it).
    pub fn resolve(&self, requested: &str) -> Option<Route> {
        let snap = self.inner.read();

        // 1. Alias table.
        if let Some(alias) = snap.aliases.get(requested) {
            if alias.target_type == "combo" {
                if let Some(route) = self.build_combo(&snap, &alias.target_id) {
                    return Some(route);
                }
            } else if let Some(m) = snap.models.get(&alias.target_id) {
                return Some(Route::Single {
                    provider_id: m.provider_id.clone(),
                    model_id: m.id.clone(),
                });
            }
        }

        // 2. Combo by name.
        if let Some(combo) = snap.combos.values().find(|c| c.name == requested) {
            if let Some(route) = self.build_combo(&snap, &combo.id) {
                return Some(route);
            }
        }

        // 3. `provider/model-id` (provider matched by name or id).
        if let Some((prov_part, model_part)) = requested.split_once('/') {
            let provider = snap
                .providers
                .values()
                .find(|p| p.name == prov_part || p.id == prov_part);
            if let Some(p) = provider {
                if let Some(mid) = snap
                    .model_by_upstream
                    .get(&(p.id.clone(), model_part.to_string()))
                {
                    if let Some(m) = snap.models.get(mid) {
                        return Some(Route::Single {
                            provider_id: m.provider_id.clone(),
                            model_id: m.id.clone(),
                        });
                    }
                }
            }
        }

        // 4. Bare upstream model id.
        if let Some(m) = snap.models.values().find(|m| m.upstream_id == requested) {
            return Some(Route::Single {
                provider_id: m.provider_id.clone(),
                model_id: m.id.clone(),
            });
        }

        None
    }

    fn build_combo(&self, snap: &Snapshot, combo_id: &str) -> Option<Route> {
        let combo = snap.combos.get(combo_id)?.clone();
        if combo.enabled == 0 {
            return None;
        }
        let mut targets = Vec::new();
        for t in snap.combo_targets.get(combo_id).into_iter().flatten() {
            let Some(model) = snap.models.get(&t.model_id).cloned() else {
                continue;
            };
            let Some(provider) = snap.providers.get(&model.provider_id).cloned() else {
                continue;
            };
            // Account: explicit, or the first healthy account of the provider.
            let account = match &t.account_id {
                Some(aid) => snap.accounts.get(aid).cloned(),
                None => snap
                    .accounts
                    .values()
                    .filter(|a| a.provider_id == model.provider_id && a.status != "disabled")
                    .min_by_key(|a| a.priority)
                    .cloned(),
            };
            let Some(account) = account else { continue };
            targets.push(ResolvedTarget {
                account,
                model,
                provider,
                priority: t.priority,
                weight: t.weight,
            });
        }
        if targets.is_empty() {
            return None;
        }
        Some(Route::Combo { combo, targets })
    }

    /// All enabled models the registry knows, for `/v1/models`.
    pub fn enabled_models(&self) -> Vec<ModelRow> {
        let snap = self.inner.read();
        snap.models.values().filter(|m| m.enabled != 0).cloned().collect()
    }

    /// Provider by id.
    pub fn provider(&self, id: &str) -> Option<ProviderRow> {
        self.inner.read().providers.get(id).cloned()
    }

    pub fn model(&self, id: &str) -> Option<ModelRow> {
        self.inner.read().models.get(id).cloned()
    }

    pub fn account(&self, id: &str) -> Option<AccountRow> {
        self.inner.read().accounts.get(id).cloned()
    }

    pub fn combo_name(&self, id: &str) -> Option<String> {
        self.inner.read().combos.get(id).map(|c| c.name.clone())
    }

    pub fn aliases(&self) -> Vec<AliasRow> {
        self.inner.read().aliases.values().cloned().collect()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}
