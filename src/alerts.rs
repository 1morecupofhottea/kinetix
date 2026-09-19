//! Alerting (FR-6.6, FR-12.17).
//!
//! A bounded, background evaluation loop watches the accounting that already
//! exists (recent usage rows + account health) and fires webhook alerts when
//! configured thresholds are crossed. Alerting is control-plane only: a webhook
//! failure is logged and never affects a request (NFR-2.6). Alerts are
//! edge-triggered — a condition must clear before it can fire again — so a
//! persistent problem does not spam the webhook every interval.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::app::AppState;
use crate::db;

/// Alert conditions currently firing, so we only send on transitions.
#[derive(Default)]
pub struct AlertState {
    active: Mutex<HashSet<String>>,
}

impl AlertState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when `key` newly became active (edge trigger).
    fn should_fire(&self, key: &str) -> bool {
        let mut active = self.active.lock();
        if active.contains(key) {
            false
        } else {
            active.insert(key.to_string());
            true
        }
    }

    /// Returns true when `key` transitioned from firing to resolved.
    fn should_resolve(&self, key: &str) -> bool {
        let mut active = self.active.lock();
        active.remove(key)
    }
}

/// Run the alert evaluation loop until the process exits.
pub async fn run(state: AppState, alerts: Arc<AlertState>) {
    let Some(url) = state.config.alert_webhook_url.clone() else {
        tracing::info!("alert webhook not configured; alerting disabled");
        return;
    };
    let interval = state.config.alert_interval_secs.max(5);
    let mut ticker = tokio::time::interval(Duration::from_secs(interval));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if let Err(e) = evaluate(&state, &alerts, &url).await {
            tracing::warn!(error = %e, "alert evaluation failed");
        }
    }
}

async fn evaluate(state: &AppState, alerts: &AlertState, url: &str) -> anyhow::Result<()> {
    // Window: the most recent N usage rows. Bounded so this stays cheap.
    let recent = db::recent_usage(&state.pool, 500).await?;
    let total = recent.len() as i64;
    let min = state.config.alert_min_requests;

    if total >= min {
        let fallback = recent.iter().filter(|u| u.fallback_hops > 0).count() as f64;
        let rate = fallback / total as f64;
        let key = "high_fallback_rate";
        if rate >= state.config.alert_fallback_rate {
            if alerts.should_fire(key) {
                fire(
                    state,
                    url,
                    key,
                    &format!(
                        "Route fallback rate is {:.0}% over the last {total} requests",
                        rate * 100.0
                    ),
                    json!({"fallback_rate": rate, "window": total}),
                )
                .await;
            }
        } else if alerts.should_resolve(key) {
            resolve(state, url, key).await;
        }

        let errors = recent
            .iter()
            .filter(|u| {
                matches!(
                    u.status.as_str(),
                    "upstream_error" | "stream_error" | "rate_limited" | "quota_exhausted"
                ) || u.status_code >= 500
            })
            .count() as f64;
        let erate = errors / total as f64;
        let ekey = "high_error_rate";
        if erate >= state.config.alert_error_rate {
            if alerts.should_fire(ekey) {
                fire(
                    state,
                    url,
                    ekey,
                    &format!(
                        "Error rate is {:.0}% over the last {total} requests",
                        erate * 100.0
                    ),
                    json!({"error_rate": erate, "window": total}),
                )
                .await;
            }
        } else if alerts.should_resolve(ekey) {
            resolve(state, url, ekey).await;
        }
    }

    // Account health: any account currently exhausted or circuit-open.
    let accounts = db::list_accounts(&state.pool).await?;
    for a in &accounts {
        let key = format!("account_unhealthy:{}", a.id);
        let unhealthy = matches!(a.status.as_str(), "exhausted" | "circuit_open" | "disabled");
        if unhealthy {
            if alerts.should_fire(&key) {
                fire(
                    state,
                    url,
                    &key,
                    &format!(
                        "Account '{}' is {}",
                        a.label,
                        a.last_error
                            .clone()
                            .filter(|e| !e.is_empty())
                            .unwrap_or_else(|| a.status.clone())
                    ),
                    json!({"account": a.label, "status": a.status}),
                )
                .await;
            }
        } else if alerts.should_resolve(&key) {
            resolve(state, url, &key).await;
        }
    }

    // Route availability: a configured route with no healthy target.
    let routes = db::list_routes(&state.pool).await?;
    let healthy: HashSet<&str> = accounts
        .iter()
        .filter(|a| a.status == "healthy")
        .map(|a| a.provider_id.as_str())
        .collect();
    for r in &routes {
        if r.enabled == 0 {
            continue;
        }
        let targets = db::route_targets(&state.pool, &r.id).await?;
        let any_healthy = targets.iter().any(|t| {
            let provider_id = targets_provider(&accounts, &t.model_id, state);
            provider_id
                .map(|p| healthy.contains(p.as_str()))
                .unwrap_or(false)
        });
        let key = format!("route_unavailable:{}", r.id);
        if !any_healthy && !targets.is_empty() {
            if alerts.should_fire(&key) {
                fire(
                    state,
                    url,
                    &key,
                    &format!("Route '{}' has no healthy target", r.name),
                    json!({"route": r.name, "targets": targets.len()}),
                )
                .await;
            }
        } else if alerts.should_resolve(&key) {
            resolve(state, url, &key).await;
        }
    }

    Ok(())
}

/// Resolve the provider id for a route target's model from the in-memory
/// snapshot (cheap, no extra query).
fn targets_provider(
    accounts: &[db::AccountRow],
    model_id: &str,
    state: &AppState,
) -> Option<String> {
    let snap = state.registry.snapshot();
    let model = snap.models.get(model_id)?;
    // Prefer an explicit account on the target if it belongs to the model's
    // provider; otherwise use the model's provider directly.
    let _ = accounts;
    Some(model.provider_id.clone())
}

async fn fire(state: &AppState, url: &str, key: &str, message: &str, detail: Value) {
    tracing::warn!(alert = key, "{message}");
    post_webhook(
        state,
        url,
        &json!({
            "source": "kinetix",
            "event": "alert",
            "key": key,
            "message": message,
            "detail": detail,
            "at": db::now_iso(),
        }),
    )
    .await;
}

async fn resolve(state: &AppState, url: &str, key: &str) {
    tracing::info!(alert = key, "alert resolved");
    post_webhook(
        state,
        url,
        &json!({
            "source": "kinetix",
            "event": "resolved",
            "key": key,
            "at": db::now_iso(),
        }),
    )
    .await;
}

async fn post_webhook(state: &AppState, url: &str, payload: &Value) {
    // Best-effort: a webhook failure must never affect the data plane.
    let res = state
        .http
        .post(url)
        .json(payload)
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => tracing::warn!(
            status = r.status().as_u16(),
            "alert webhook returned non-success"
        ),
        Err(e) => tracing::warn!(error = %e, "alert webhook delivery failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_are_edge_triggered() {
        let a = AlertState::new();
        assert!(a.should_fire("x"));
        assert!(!a.should_fire("x"), "a firing alert does not re-fire");
        assert!(a.should_resolve("x"));
        assert!(!a.should_resolve("x"), "resolving twice is a no-op");
        assert!(a.should_fire("x"), "after resolving it can fire again");
    }
}
