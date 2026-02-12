pub mod keystore_signer_provider;

use crate::chain::ChainClient;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait SignerProvider: Send + Sync {
    async fn signed_chain_client(&self) -> anyhow::Result<Arc<dyn ChainClient>>;
}

pub use keystore_signer_provider::KeystoreSignerProvider;
