// Copyright 2026 Aventus DAO Ltd

use alloy::{
    consensus::Transaction,
    primitives::{Address, Bytes, B256, U256},
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::types::{Filter, Log, TransactionReceipt, TransactionRequest},
    signers::local::PrivateKeySigner,
};
use anyhow::{Context, Result};
use std::sync::Arc;
use url::Url;

pub type SharedProvider = Arc<DynProvider>;

#[derive(Clone)]
pub struct EvmClient {
    pub provider: SharedProvider,
}

impl EvmClient {
    pub fn new(rpc_url: Url, signer: PrivateKeySigner) -> Self {
        let provider = ProviderBuilder::new().wallet(signer).connect_http(rpc_url).erased();

        Self { provider: Arc::new(provider) }
    }

    pub fn new_http(rpc_url: &str) -> Result<Self> {
        let url: Url = rpc_url.parse().context("invalid EVM RPC url")?;
        let provider = ProviderBuilder::new().connect_http(url).erased();
        Ok(Self { provider: Arc::new(provider) })
    }

    pub async fn chain_id(&self) -> Result<u64> {
        Ok(self.provider.get_chain_id().await?)
    }

    pub async fn block_number(&self) -> Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }

    pub async fn call(&self, to: Address, input: Bytes) -> Result<Bytes> {
        let tx = TransactionRequest::default().to(to).input(input.into());
        Ok(self.provider.call(tx).await?)
    }

    pub async fn get_receipt(&self, tx_hash: B256) -> Result<Option<TransactionReceipt>> {
        Ok(self.provider.get_transaction_receipt(tx_hash).await?)
    }

    pub async fn get_transaction_input(&self, tx_hash: B256) -> Result<Option<Bytes>> {
        let tx = self.provider.get_transaction_by_hash(tx_hash).await?;
        Ok(tx.map(|t| t.inner.input().clone()))
    }

    /// NOTE: The signer is configured on the provider via `ProviderBuilder::wallet(...)`,
    /// so we do *not* pass a wallet here.
    pub async fn send_transaction_data(&self, to: Address, data: Bytes) -> Result<B256> {
        let tx = TransactionRequest::default().to(to).value(U256::ZERO).input(data.into());

        let pending = self.provider.send_transaction(tx).await?;
        Ok(*pending.tx_hash())
    }

    pub async fn logs(&self, filter: Filter) -> Result<Vec<Log>> {
        Ok(self.provider.get_logs(&filter).await?)
    }
}
