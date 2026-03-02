use crate::{
    chain::ChainClient, eth_signing::signer_from_keystore, evm::client::EvmClient,
    keystore_utils::get_eth_address_bytes_from_keystore,
};
use async_trait::async_trait;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use url::Url;

pub struct KeystoreSignerProvider {
    keystore_path: PathBuf,
    rpc_url: Url,
    cache: RwLock<Option<CachedClient>>,
}

struct CachedClient {
    eth_address_hex: String,
    client: Arc<dyn ChainClient>,
}

impl KeystoreSignerProvider {
    pub fn new(keystore_path: PathBuf, rpc_url: Url) -> Self {
        Self { keystore_path, rpc_url, cache: RwLock::new(None) }
    }

    async fn current_eth_address_hex(&self) -> anyhow::Result<String> {
        let addr_bytes = get_eth_address_bytes_from_keystore(&self.keystore_path)?;
        Ok(hex::encode(addr_bytes))
    }
}

#[async_trait]
impl crate::signing::SignerProvider for KeystoreSignerProvider {
    async fn signed_chain_client(&self) -> anyhow::Result<Arc<dyn ChainClient>> {
        let eth_address_hex = self.current_eth_address_hex().await?;

        // Cached key matches store so return existing signer
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref() {
                if cached.eth_address_hex == eth_address_hex {
                    return Ok(Arc::clone(&cached.client))
                }
            }
        }

        let mut cache = self.cache.write().await;

        // Cached key mismatch (unitinialised or rotated) so build/rebuild
        if let Some(cached) = cache.as_ref() {
            // double check after acquiring write lock and return if key is already up-to-date
            if cached.eth_address_hex == eth_address_hex {
                return Ok(Arc::clone(&cached.client))
            }

            log::info!(
                "⛓️ external-service: Refreshing Ethereum signer (old: {}, new: {})",
                cached.eth_address_hex,
                eth_address_hex
            );
        } else {
            log::info!(
                "⛓️ external-service: Initialising Ethereum signer (address: {})",
                eth_address_hex
            );
        }

        // Build and cache signer
        let signer = signer_from_keystore(&self.keystore_path)?;
        let signed = EvmClient::new(self.rpc_url.clone(), signer);
        let client: Arc<dyn ChainClient> = Arc::new(signed);

        *cache = Some(CachedClient { eth_address_hex, client: Arc::clone(&client) });

        Ok(client)
    }
}
