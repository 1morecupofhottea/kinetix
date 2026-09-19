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

// ===========================================================================
// Per-key IP allowlist (FR-3.4)
// ===========================================================================

/// Best-effort client IP from trusted ingress headers (NFR-3.1: Kinetix listens
/// on localhost only and `cloudflared` is the sole ingress, so these headers are
/// trustworthy). Prefers Cloudflare's `CF-Connecting-IP`, then the first hop of
/// `X-Forwarded-For`, then `X-Real-IP`.
pub fn client_ip(headers: &axum::http::HeaderMap) -> Option<std::net::IpAddr> {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if let Some(ip) = get("cf-connecting-ip").and_then(|v| v.trim().parse().ok()) {
        return Some(ip);
    }
    if let Some(first) = get("x-forwarded-for").and_then(|v| v.split(',').next()) {
        if let Ok(ip) = first.trim().parse() {
            return Some(ip);
        }
    }
    get("x-real-ip").and_then(|v| v.trim().parse().ok())
}

/// Whether `ip` matches any entry in an allowlist of exact addresses or CIDR
/// ranges (`10.0.0.0/8`, `2001:db8::/32`).
pub fn ip_allowed(ip: std::net::IpAddr, allow: &[String]) -> bool {
    allow.iter().any(|entry| ip_matches(ip, entry.trim()))
}

fn ip_matches(ip: std::net::IpAddr, entry: &str) -> bool {
    if let Some((net, prefix)) = entry.split_once('/') {
        let (Ok(net), Ok(prefix)) = (
            net.trim().parse::<std::net::IpAddr>(),
            prefix.trim().parse::<u8>(),
        ) else {
            return false;
        };
        return cidr_contains(net, prefix, ip);
    }
    entry
        .parse::<std::net::IpAddr>()
        .map(|e| e == ip)
        .unwrap_or(false)
}

fn cidr_contains(net: std::net::IpAddr, prefix: u8, ip: std::net::IpAddr) -> bool {
    match (net, ip) {
        (std::net::IpAddr::V4(n), std::net::IpAddr::V4(i)) => {
            if prefix > 32 {
                return false;
            }
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(n) & mask) == (u32::from(i) & mask)
        }
        (std::net::IpAddr::V6(n), std::net::IpAddr::V6(i)) => {
            if prefix > 128 {
                return false;
            }
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (u128::from(n) & mask) == (u128::from(i) & mask)
        }
        _ => false,
    }
}

/// Enforce a key's optional IP allowlist (FR-3.4). When an allowlist is set and
/// the client IP cannot be determined, the request is denied (fail closed).
pub fn enforce_ip(key: &VirtualKeyRow, ip: Option<std::net::IpAddr>) -> Result<(), ProxyError> {
    let allow = key.allowed_ips();
    if allow.is_empty() {
        return Ok(());
    }
    match ip {
        Some(ip) if ip_allowed(ip, &allow) => Ok(()),
        Some(ip) => Err(ProxyError::new(
            crate::types::ErrorKind::Forbidden,
            format!("this virtual key is not permitted from {ip}"),
        )),
        None => Err(ProxyError::new(
            crate::types::ErrorKind::Forbidden,
            "this virtual key requires an IP allowlist match but no client IP could be determined",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_and_exact_matching() {
        assert!(ip_matches("10.1.2.3".parse().unwrap(), "10.0.0.0/8"));
        assert!(!ip_matches("11.1.2.3".parse().unwrap(), "10.0.0.0/8"));
        assert!(ip_matches("203.0.113.7".parse().unwrap(), "203.0.113.7"));
        assert!(!ip_matches("203.0.113.8".parse().unwrap(), "203.0.113.7"));
        assert!(ip_matches("2001:db8::1".parse().unwrap(), "2001:db8::/32"));
        assert!(!ip_matches("2001:db9::1".parse().unwrap(), "2001:db8::/32"));
        assert!(ip_matches("8.8.8.8".parse().unwrap(), "0.0.0.0/0"));
        assert!(!ip_matches("8.8.8.8".parse().unwrap(), "bad-entry"));
    }

    #[test]
    fn empty_allowlist_permits_everything() {
        let mut key = key_with_ips(vec![]);
        key.allowed_ips = "[]".into();
        assert!(enforce_ip(&key, None).is_ok());
    }

    #[test]
    fn allowlist_denies_unknown_or_unlisted_ip() {
        let key = key_with_ips(vec!["10.0.0.0/8".into()]);
        assert!(enforce_ip(&key, Some("10.5.5.5".parse().unwrap())).is_ok());
        assert!(enforce_ip(&key, Some("192.168.1.1".parse().unwrap())).is_err());
        // No determinable client IP fails closed when an allowlist is set.
        assert!(enforce_ip(&key, None).is_err());
    }

    fn key_with_ips(ips: Vec<String>) -> VirtualKeyRow {
        VirtualKeyRow {
            id: "k".into(),
            key_hash: "h".into(),
            name: "n".into(),
            owner: "".into(),
            tag: "".into(),
            allowed_models: "[\"*\"]".into(),
            allowed_providers: "[]".into(),
            rpm_limit: None,
            tpm_limit: None,
            daily_budget: None,
            monthly_budget: None,
            expires_at: None,
            status: "active".into(),
            allowed_ips: serde_json::to_string(&ips).unwrap(),
            body_logging: 0,
            created_at: "".into(),
            revoked_at: None,
        }
    }
}
