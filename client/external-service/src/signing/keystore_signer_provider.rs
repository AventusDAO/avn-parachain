use async_trait::async_trait;
use crate::{
    chain::ChainClient,
    eth_signing::signer_from_keystore,
    evm::client::EvmClient,
    keystore_utils::get_eth_address_bytes_from_keystore,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use url::Url;

pub struct KeystoreSignerProvider {
    keystore_path: PathBuf,
    rpc_url: Url,
    client: RwLock<Option<Arc<dyn ChainClient>>>,
}

impl KeystoreSignerProvider {
    pub fn new(keystore_path: PathBuf, rpc_url: Url) -> Self {
        Self { keystore_path, rpc_url, client: RwLock::new(None) }
    }

    fn eth_address_hex(&self) -> anyhow::Result<String> {
        let addr_bytes = get_eth_address_bytes_from_keystore(&self.keystore_path)?;
        Ok(hex::encode(addr_bytes))
    }
}

#[async_trait]
impl crate::signing::SignerProvider for KeystoreSignerProvider {
    async fn signed_chain_client(&self) -> anyhow::Result<Arc<dyn ChainClient>> {
        {
            let guard = self.client.read().await;
            if let Some(client) = guard.as_ref() {
                return Ok(Arc::clone(client))
            }
        }

        let mut guard = self.client.write().await;

        if let Some(client) = guard.as_ref() {
            return Ok(Arc::clone(client))
        }

        let eth_address_hex = self.eth_address_hex()?;
        log::info!(
            "⛓️ external-service: Initialising Ethereum signer (address: {})",
            eth_address_hex
        );

        let signer = signer_from_keystore(&self.keystore_path)?;
        let signed = EvmClient::new(self.rpc_url.clone(), signer);
        let client: Arc<dyn ChainClient> = Arc::new(signed);

        *guard = Some(Arc::clone(&client));
        Ok(client)
    }
}