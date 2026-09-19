//! Authentication for both surfaces.
//!
//! - Public API: virtual keys (`Authorization: Bearer` for OpenAI clients,
//!   `x-api-key` for Anthropic clients, FR-3.5).
//! - Admin API/dashboard: env admin token -> signed session cookie, plus an
//!   optional Cloudflare Access JWT check (defense in depth, NFR-3.2).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use axum_extra::extract::cookie::CookieJar;

use crate::app::AppState;
use crate::crypto;
use crate::db::{self, VirtualKeyRow};
use crate::types::ProxyError;

pub const SESSION_COOKIE: &str = "kinetix_admin";

/// Extract the presented virtual key from either auth style.
pub fn extract_virtual_key(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("authorization").and_then(|h| h.to_str().ok()) {
        if let Some(token) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
            return Some(token.trim().to_string());
        }
    }
    if let Some(v) = headers.get("x-api-key").and_then(|h| h.to_str().ok()) {
        if !v.trim().is_empty() {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Look up the virtual key row for a presented key, checking the hash in
/// constant time (NFR-3.3).
pub async fn authenticate_virtual_key(
    state: &AppState,
    presented: &str,
) -> Result<VirtualKeyRow, ProxyError> {
    let hash = crypto::hash_virtual_key(presented);
    let row = db::get_virtual_key_by_hash(&state.pool, &hash)
        .await
        .map_err(|e| ProxyError::internal(e.to_string()))?;
    match row {
        Some(row) if crypto::constant_time_eq(&row.key_hash, &hash) => Ok(row),
        _ => Err(ProxyError::unauthorized(
            "invalid or unknown API key. Provide a Kinetix virtual key (sk-kinetix-...).",
        )),
    }
}

/// The authenticated virtual key, if any, resolved from request headers.
pub struct AuthKey(pub Option<VirtualKeyRow>);

impl FromRequestParts<AppState> for AuthKey {
    type Rejection = ProxyError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match extract_virtual_key(&parts.headers) {
            Some(presented) => {
                let row = authenticate_virtual_key(state, &presented).await?;
                Ok(AuthKey(Some(row)))
            }
            None => Ok(AuthKey(None)),
        }
    }
}

/// An authenticated admin session.
pub struct AdminAuth {
    pub actor: String,
}

impl FromRequestParts<AppState> for AdminAuth {
    type Rejection = ProxyError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Cloudflare Access JWT (if configured).
        if let Some(aud) = &state.config.cf_access_aud {
            let jwt = parts
                .headers
                .get("cf-access-jwt-assertion")
                .and_then(|h| h.to_str().ok());
            match jwt {
                Some(token) => {
                    validate_cf_access(state, token, aud).map_err(|e| {
                        ProxyError::new(crate::types::ErrorKind::Forbidden, e)
                    })?;
                }
                None => {
                    return Err(ProxyError::new(
                        crate::types::ErrorKind::Forbidden,
                        "missing Cloudflare Access assertion",
                    ))
                }
            }
        }

        // Session cookie or direct admin token header.
        let jar = CookieJar::from_headers(&parts.headers);
        let cookie_token = jar.get(SESSION_COOKIE).map(|c| c.value().to_string());
        let header_token = parts
            .headers
            .get("x-kinetix-admin-token")
            .and_then(|h| h.to_str().ok())
            .map(String::from);

        let presented = cookie_token.or(header_token);
        match presented {
            Some(t) if verify_session(state, &t) => Ok(AdminAuth {
                actor: "admin".to_string(),
            }),
            _ => Err(ProxyError::unauthorized(
                "admin authentication required",
            )),
        }
    }
}

/// Verify a session token: it must be an HMAC of "admin" with the master key.
pub fn verify_session(state: &AppState, token: &str) -> bool {
    let expected = session_token(state);
    crypto::constant_time_eq(token, &expected)
}

/// Build the (deterministic) session token for the configured admin token.
/// Rotating the admin token invalidates all sessions.
pub fn session_token(state: &AppState) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(&state.config.master_key)
        .expect("hmac accepts any key length");
    mac.update(b"kinetix-admin-session:");
    mac.update(state.config.admin_token.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Verify a raw admin token submitted at login.
pub fn verify_admin_password(state: &AppState, password: &str) -> bool {
    crypto::constant_time_eq(
        &crypto::hash_virtual_key(password),
        &crypto::hash_virtual_key(&state.config.admin_token),
    )
}

fn validate_cf_access(state: &AppState, token: &str, aud: &str) -> Result<(), String> {
    use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};

    let header = decode_header(token).map_err(|e| format!("invalid Access token: {e}"))?;
    let team = state
        .config
        .cf_access_team_domain
        .clone()
        .ok_or_else(|| "Cloudflare Access team domain not configured".to_string())?;

    // Fetch the JWKS (cached by reqwest's connection pool; the dashboard is
    // low-traffic so a per-request fetch is acceptable and always fresh).
    let jwks_url = format!("https://{team}/cdn-cgi/access/certs");
    let jwks: serde_json::Value = reqwest::blocking::get(&jwks_url)
        .map_err(|e| format!("failed to fetch Access certs: {e}"))?
        .json()
        .map_err(|e| format!("invalid Access certs: {e}"))?;

    let kid = header.kid.ok_or_else(|| "Access token missing kid".to_string())?;
    let keys = jwks
        .get("keys")
        .and_then(|k| k.as_array())
        .ok_or_else(|| "Access certs missing keys".to_string())?;
    let jwk = keys
        .iter()
        .find(|k| k.get("kid").and_then(|x| x.as_str()) == Some(kid.as_str()))
        .ok_or_else(|| "no matching Access key".to_string())?;

    let n = jwk.get("n").and_then(|x| x.as_str()).ok_or("missing n")?;
    let e = jwk.get("e").and_then(|x| x.as_str()).ok_or("missing e")?;
    let key = DecodingKey::from_rsa_components(n, e).map_err(|e| e.to_string())?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[aud]);
    decode::<serde_json::Value>(token, &key, &validation)
        .map(|_| ())
        .map_err(|e| format!("Access token rejected: {e}"))
}
