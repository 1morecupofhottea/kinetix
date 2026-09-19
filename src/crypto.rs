//! Secret handling: AES-256-GCM encryption for upstream credentials at rest,
//! SHA-256 hashing for virtual keys, and redaction helpers for logs.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Result};
use base64::Engine;
use sha2::{Digest, Sha256};

pub struct Crypto {
    cipher: Aes256Gcm,
}

impl Crypto {
    pub fn new(master_key: &[u8; 32]) -> Self {
        let key = Key::<Aes256Gcm>::from_slice(master_key);
        Crypto {
            cipher: Aes256Gcm::new(key),
        }
    }

    /// Encrypt a secret, returning base64(nonce || ciphertext).
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| anyhow!("encryption failed"))?;
        let mut out = Vec::with_capacity(nonce.len() + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(base64::engine::general_purpose::STANDARD.encode(out))
    }

    pub fn decrypt(&self, encoded: &str) -> Result<String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| anyhow!("invalid ciphertext encoding: {e}"))?;
        if raw.len() < 12 {
            return Err(anyhow!("ciphertext too short"));
        }
        let (nonce_bytes, ct) = raw.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let pt = self
            .cipher
            .decrypt(nonce, ct)
            .map_err(|_| anyhow!("decryption failed (wrong master key?)"))?;
        String::from_utf8(pt).map_err(|e| anyhow!("decrypted secret not utf-8: {e}"))
    }
}

/// Generate a new virtual key with the `sk-kinetix-` prefix.
pub fn generate_virtual_key() -> String {
    let mut bytes = [0u8; 24];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("sk-kinetix-{}", hex::encode(bytes))
}

/// SHA-256 hash of a virtual key, hex encoded. Constant-time compared.
pub fn hash_virtual_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Constant-time comparison of two hex hashes.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Mask a secret for display: keep the first 4 and last 4 characters.
pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 10 {
        return "****".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}...{tail}")
}

/// Redact anything that looks like a secret from a string destined for logs.
pub fn redact(text: &str) -> String {
    // Replace known API key prefixes and long token-like substrings.
    let mut out = String::with_capacity(text.len());
    for token in
        text.split_inclusive(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '=')
    {
        let trimmed = token.trim_matches(|c: char| {
            c.is_whitespace()
                || c == '"'
                || c == '\''
                || c == ','
                || c == '{'
                || c == '}'
                || c == '='
        });
        if looks_like_secret(trimmed) {
            out.push_str("[REDACTED]");
            // preserve trailing delimiter
            if let Some(last) = token.chars().last() {
                if last.is_whitespace() || last == '"' || last == '\'' || last == '=' {
                    out.push(last);
                }
            }
        } else {
            out.push_str(token);
        }
    }
    out
}

fn looks_like_secret(s: &str) -> bool {
    const PREFIXES: [&str; 6] = ["sk-", "AIza", "AQ.", "gsk_", "sk-ant", "ya29."];
    if PREFIXES.iter().any(|p| s.starts_with(p)) && s.len() > 12 {
        return true;
    }
    // Long high-entropy tokens (>=40 chars, mostly alphanumeric)
    s.len() >= 40 && s.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 36
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encryption() {
        let c = Crypto::new(&[7u8; 32]);
        let ct = c.encrypt("AIzaSy-super-secret").unwrap();
        assert_ne!(ct, "AIzaSy-super-secret");
        assert_eq!(c.decrypt(&ct).unwrap(), "AIzaSy-super-secret");
    }

    #[test]
    fn redacts_prefixes() {
        let out = redact("key=AIzaSyABCDEFGHIJKLMNOP");
        assert!(out.contains("[REDACTED]"));
    }
}
