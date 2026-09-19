//! Per-key limits and budgets (FR-3.3, FR-3.7). Violations return a
//! format-correct 429 with a human-readable reason.

use chrono::Utc;

use crate::db::{self, Pool, VirtualKeyRow};
use crate::pool::window_start;
use crate::types::ProxyError;

pub struct KeyLimits;

/// Check all per-key limits. Returns a `ProxyError` when a limit is hit.
pub async fn enforce(
    pool: &Pool,
    key: &VirtualKeyRow,
    requested_model: &str,
) -> Result<(), ProxyError> {
    // Status.
    match key.status.as_str() {
        "revoked" => {
            return Err(ProxyError::unauthorized(
                "this virtual key has been revoked",
            ))
        }
        "disabled" => {
            return Err(ProxyError::new(
                crate::types::ErrorKind::Forbidden,
                "this virtual key is disabled",
            ))
        }
        _ => {}
    }

    // Expiry.
    if let Some(exp) = key.expires_at.as_deref().and_then(db::parse_dt) {
        if exp <= Utc::now() {
            return Err(ProxyError::unauthorized(format!(
                "this virtual key expired on {}",
                exp.to_rfc3339()
            )));
        }
    }

    // Allowed models.
    if !key.permits_model(requested_model) {
        return Err(ProxyError::new(
            crate::types::ErrorKind::Forbidden,
            format!("this key is not allowed to use model '{requested_model}'"),
        ));
    }

    // RPM / TPM over the last 60 seconds.
    let since = (Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
    let (count, tokens) = db::key_usage_since(pool, &key.id, &since)
        .await
        .map_err(|e| ProxyError::internal(e.to_string()))?;

    if let Some(rpm) = key.rpm_limit {
        if rpm > 0 && count >= rpm {
            return Err(ProxyError::rate_limited(
                format!("rate limit exceeded: {rpm} requests per minute"),
                Some(60),
            ));
        }
    }
    if let Some(tpm) = key.tpm_limit {
        if tpm > 0 && tokens >= tpm as i64 {
            return Err(ProxyError::rate_limited(
                format!("token rate limit exceeded: {tpm} tokens per minute"),
                Some(60),
            ));
        }
    }

    // Daily budget.
    if let Some(daily) = key.daily_budget {
        if daily > 0.0 {
            let spent = db::key_spend_since(pool, &key.id, &window_start("daily", None))
                .await
                .map_err(|e| ProxyError::internal(e.to_string()))?;
            if spent >= daily {
                return Err(ProxyError::budget_exceeded(format!(
                    "daily budget exceeded (${spent:.2} of ${daily:.2}); resets at 00:00 UTC"
                )));
            }
        }
    }

    // Monthly budget.
    if let Some(monthly) = key.monthly_budget {
        if monthly > 0.0 {
            let spent = db::key_spend_since(pool, &key.id, &window_start("monthly", None))
                .await
                .map_err(|e| ProxyError::internal(e.to_string()))?;
            if spent >= monthly {
                return Err(ProxyError::budget_exceeded(format!(
                    "monthly budget exceeded (${spent:.2} of ${monthly:.2}); resets on the 1st"
                )));
            }
        }
    }

    Ok(())
}

/// Current spend for a key within the daily/monthly windows (for the dashboard).
pub async fn spend_snapshot(pool: &Pool, key: &VirtualKeyRow) -> (f64, f64) {
    let daily = db::key_spend_since(pool, &key.id, &window_start("daily", None))
        .await
        .unwrap_or(0.0);
    let monthly = db::key_spend_since(pool, &key.id, &window_start("monthly", None))
        .await
        .unwrap_or(0.0);
    (daily, monthly)
}
