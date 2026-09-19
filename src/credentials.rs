//! Credential strategies (FR-11.2). A strategy supplies and refreshes the
//! credential for an account and reports health to the pool exactly like a
//! static key does. Built-ins only do static API keys today; plugins can add
//! login-session strategies later without touching the pool logic.

use async_trait::async_trait;
use anyhow::Result;

use crate::crypto::Crypto;
use crate::db::AccountRow;

#[async_trait]
pub trait CredentialStrategy: Send + Sync {
    fn name(&self) -> &'static str;
    /// Resolve the plaintext credential to send upstream for this account.
    async fn credential(&self, account: &AccountRow) -> Result<String>;
}

/// Static API key stored encrypted at rest (NFR-3.4).
pub struct StaticKeyStrategy {
    crypto: std::sync::Arc<Crypto>,
}

impl StaticKeyStrategy {
    pub fn new(crypto: std::sync::Arc<Crypto>) -> Self {
        StaticKeyStrategy { crypto }
    }
}

#[async_trait]
impl CredentialStrategy for StaticKeyStrategy {
    fn name(&self) -> &'static str {
        "static_api_key"
    }

    async fn credential(&self, account: &AccountRow) -> Result<String> {
        self.crypto.decrypt(&account.secret_enc)
    }
}
