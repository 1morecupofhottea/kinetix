//! Key-pool health state and account/soft-quota bookkeeping (FR-4, FR-12).
//!
//! Selection is health-aware: disabled/cooldown/exhausted accounts are skipped,
//! and selection order depends on the route strategy. State changes are
//! persisted so they survive restarts (open issue: rate-limit state location).

use chrono::{DateTime, Duration, Utc};

use crate::db::{self, AccountRow, Pool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    Healthy,
    Cooldown,
    Exhausted,
    Disabled,
}

impl AccountStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "cooldown" => AccountStatus::Cooldown,
            "exhausted" => AccountStatus::Exhausted,
            "disabled" => AccountStatus::Disabled,
            _ => AccountStatus::Healthy,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountStatus::Healthy => "healthy",
            AccountStatus::Cooldown => "cooldown",
            AccountStatus::Exhausted => "exhausted",
            AccountStatus::Disabled => "disabled",
        }
    }
}

/// Effective status accounting for elapsed cooldown/quota-reset windows.
pub fn effective_status(account: &AccountRow) -> AccountStatus {
    let now = Utc::now();
    match AccountStatus::parse(&account.status) {
        AccountStatus::Cooldown => {
            if let Some(until) = account.cooldown_until.as_deref().and_then(db::parse_dt) {
                if until <= now {
                    AccountStatus::Healthy
                } else {
                    AccountStatus::Cooldown
                }
            } else {
                AccountStatus::Healthy
            }
        }
        AccountStatus::Exhausted => {
            if let Some(reset) = account.quota_reset_at.as_deref().and_then(db::parse_dt) {
                if reset <= now {
                    AccountStatus::Healthy
                } else {
                    AccountStatus::Exhausted
                }
            } else {
                // No reset time known: treat as exhausted but eligible for probing.
                AccountStatus::Exhausted
            }
        }
        other => other,
    }
}

pub fn is_available(account: &AccountRow) -> bool {
    matches!(
        effective_status(account),
        AccountStatus::Healthy | AccountStatus::Cooldown // cooldown probed below
    ) && effective_status(account) == AccountStatus::Healthy
}

/// Mark an account rate-limited for `cooldown` seconds.
pub async fn mark_rate_limited(
    pool: &Pool,
    account_id: &str,
    cooldown_secs: u64,
    error: &str,
) -> anyhow::Result<DateTime<Utc>> {
    let until = Utc::now() + Duration::seconds(cooldown_secs as i64);
    db::set_account_status(
        pool,
        account_id,
        "cooldown",
        Some(&until.to_rfc3339()),
        None,
        Some(&crate::crypto::redact(error)),
    )
    .await?;
    Ok(until)
}

/// Mark an account quota-exhausted until `reset_at` (or a default window).
pub async fn mark_exhausted(
    pool: &Pool,
    account_id: &str,
    reset_at: Option<DateTime<Utc>>,
    default_window_secs: i64,
    error: &str,
) -> anyhow::Result<DateTime<Utc>> {
    let reset = reset_at.unwrap_or_else(|| Utc::now() + Duration::seconds(default_window_secs));
    db::set_account_status(
        pool,
        account_id,
        "exhausted",
        None,
        Some(&reset.to_rfc3339()),
        Some(&crate::crypto::redact(error)),
    )
    .await?;
    Ok(reset)
}

pub async fn mark_healthy(pool: &Pool, account_id: &str) -> anyhow::Result<()> {
    db::set_account_status(pool, account_id, "healthy", None, None, None).await
}

/// Clear a cooldown and put the account back in service.
pub async fn clear_cooldown(pool: &Pool, account_id: &str) -> anyhow::Result<()> {
    db::set_account_status(pool, account_id, "healthy", None, None, None).await
}

/// The soonest recovery time across a set of accounts (for `Retry-After`).
pub fn soonest_recovery(accounts: &[AccountRow]) -> Option<DateTime<Utc>> {
    let mut soonest: Option<DateTime<Utc>> = None;
    for a in accounts {
        let t = match AccountStatus::parse(&a.status) {
            AccountStatus::Cooldown => a.cooldown_until.as_deref().and_then(db::parse_dt),
            AccountStatus::Exhausted => a.quota_reset_at.as_deref().and_then(db::parse_dt),
            _ => None,
        };
        if let Some(t) = t {
            soonest = Some(match soonest {
                Some(cur) if cur < t => cur,
                _ => t,
            });
        }
    }
    soonest
}

/// Whether a soft quota (FR-12.5) is reached for an account.
pub async fn soft_quota_reached(
    pool: &Pool,
    account: &AccountRow,
) -> anyhow::Result<bool> {
    let Some(limit) = account.soft_quota_usd else {
        return Ok(false);
    };
    if limit <= 0.0 {
        return Ok(false);
    }
    let since = window_start(&account.quota_type, account.quota_window_s);
    let spent = db::account_spend_since(pool, &account.id, &since).await?;
    Ok(spent >= limit)
}

/// Compute the start of a quota window as an ISO timestamp.
pub fn window_start(quota_type: &str, window_secs: Option<i64>) -> String {
    let now = Utc::now();
    let start = match quota_type {
        "daily" => now.date_naive().and_hms_opt(0, 0, 0).map(|d| d.and_utc()).unwrap_or(now),
        "monthly" => {
            let first = now.date_naive().with_day(1).unwrap_or(now.date_naive());
            first.and_hms_opt(0, 0, 0).map(|d| d.and_utc()).unwrap_or(now)
        }
        "rolling" => now - Duration::seconds(window_secs.unwrap_or(86400)),
        _ => now - Duration::days(1),
    };
    start.to_rfc3339()
}

use chrono::Datelike;
