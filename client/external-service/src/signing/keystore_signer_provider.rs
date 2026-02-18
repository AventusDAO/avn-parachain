use crate::{chain::ChainClient, eth_signing::signer_from_keystore, evm::client::EvmClient};
use async_trait::async_trait;
use std::{path::PathBuf, sync::Arc};
use url::Url;

pub struct KeystoreSignerProvider {
    keystore_path: PathBuf,
    rpc_url: Url,
}

impl KeystoreSignerProvider {
    pub fn new(keystore_path: PathBuf, rpc_url: Url) -> Self {
        Self { keystore_path, rpc_url }
    }
}

#[async_trait]
impl crate::signing::SignerProvider for KeystoreSignerProvider {
    async fn signed_chain_client(&self) -> anyhow::Result<Arc<dyn ChainClient>> {
        let signer = signer_from_keystore(&self.keystore_path)?;
        let signed = EvmClient::new(self.rpc_url.clone(), signer);
        Ok(Arc::new(signed))
    }
}
