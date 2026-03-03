// Copyright 2026 Aventus DAO Ltd

use crate::{
    chain::ChainClient,
    eth_signing::{eth_priv_key_from_keystore, signer_from_keystore},
    evm::client::EvmClient,
    keystore_utils::get_eth_address_bytes_from_keystore,
};
use async_trait::async_trait;
use codec::Encode;
use sp_core::{ecdsa, Pair};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use url::Url;

pub struct KeystoreSignerProvider {
    keystore_path: PathBuf,
    rpc_url: Url,
    key_cache: RwLock<Option<CachedKey>>,
    client_cache: RwLock<Option<Arc<dyn ChainClient>>>,
}

struct CachedKey {
    eth_address_hex: String,
    ecdsa_pair: ecdsa::Pair,
}

impl KeystoreSignerProvider {
    pub fn new(keystore_path: PathBuf, rpc_url: Url) -> Self {
        Self {
            keystore_path,
            rpc_url,
            key_cache: RwLock::new(None),
            client_cache: RwLock::new(None),
        }
    }

    fn load_signing_key(&self) -> anyhow::Result<CachedKey> {
        let eth_address: Vec<u8> = get_eth_address_bytes_from_keystore(&self.keystore_path)?;
        if eth_address.len() != 20 {
            anyhow::bail!("eth address must be 20 bytes");
        }
        let eth_address_hex = hex::encode(&eth_address);
        let priv_key = eth_priv_key_from_keystore(&self.keystore_path)?;
        let ecdsa_pair = ecdsa::Pair::from_seed_slice(&priv_key)
            .map_err(|_| anyhow::anyhow!("private key must be 32 bytes"))?;

        Ok(CachedKey { eth_address_hex, ecdsa_pair })
    }

    async fn ensure_signing_key(&self) -> anyhow::Result<()> {
        {
            let guard = self.key_cache.read().await;
            if guard.is_some() {
                return Ok(())
            }
        }

        let mut guard = self.key_cache.write().await;
        if guard.is_some() {
            return Ok(())
        }

        *guard = Some(self.load_signing_key()?);
        Ok(())
    }
}

#[async_trait]
impl crate::signing::SignerProvider for KeystoreSignerProvider {
    async fn signed_chain_client(&self) -> anyhow::Result<Arc<dyn ChainClient>> {
        {
            let guard = self.client_cache.read().await;
            if let Some(client) = guard.as_ref() {
                return Ok(Arc::clone(client))
            }
        }

        let mut guard = self.client_cache.write().await;
        if let Some(client) = guard.as_ref() {
            return Ok(Arc::clone(client))
        }

        self.ensure_signing_key().await?;
        if let Some(keys) = self.key_cache.read().await.as_ref() {
            log::info!(
                "⛓️ external-service: Initialising Ethereum signer (address: {})",
                keys.eth_address_hex
            );
        }

        let signer = signer_from_keystore(&self.keystore_path)?;
        let signed = EvmClient::new(self.rpc_url.clone(), signer);
        let client: Arc<dyn ChainClient> = Arc::new(signed);

        *guard = Some(Arc::clone(&client));
        Ok(client)
    }

    async fn sign_digest(&self, digest: &[u8; 32]) -> anyhow::Result<String> {
        self.ensure_signing_key().await?;

        let guard = self.key_cache.read().await;
        let keys = guard.as_ref().ok_or_else(|| anyhow::anyhow!("Key cache not initialised"))?;

        let sig = keys.ecdsa_pair.sign_prehashed(digest);
        Ok(hex::encode(sig.encode()))
    }
}
