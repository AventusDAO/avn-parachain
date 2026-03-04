use crate::chain::ChainClient;
use async_trait::async_trait;
use std::sync::Arc;

pub mod keystore_signer_provider;
pub use keystore_signer_provider::KeystoreSignerProvider;

#[async_trait]
pub trait SignerProvider: Send + Sync {
    async fn signed_chain_client(&self) -> anyhow::Result<Arc<dyn ChainClient>>;
    async fn sign_digest(&self, digest: &[u8; 32]) -> anyhow::Result<String>;
}
