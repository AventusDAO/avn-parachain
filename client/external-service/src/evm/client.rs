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

use crate::chain::{ChainClient, ChainLog, ChainReceipt, LogFilter};
use alloy_primitives::{Address as AlloyAddress, Bytes as AlloyBytes, B256 as AlloyB256};
use sp_core::{H160, H256};

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

    pub async fn is_block_finalised(&self, target_block: u64, confirmations: u64) -> Result<bool> {
        let latest = self.block_number().await?;
        Ok(latest >= target_block.saturating_add(confirmations))
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

fn h160_to_alloy(a: H160) -> AlloyAddress {
    AlloyAddress::from_slice(a.as_bytes())
}

fn h256_to_alloy(h: H256) -> AlloyB256 {
    AlloyB256::from_slice(h.as_bytes())
}

fn alloy_address_to_h160(a: AlloyAddress) -> H160 {
    H160::from_slice(a.as_slice())
}

fn map_topics(v: Vec<H256>) -> Vec<AlloyB256> {
    v.into_iter().map(h256_to_alloy).collect()
}

fn build_alloy_filter(f: LogFilter) -> Filter {
    let mut filter = Filter::new().from_block(f.from_block).to_block(f.to_block);

    let addresses: Vec<_> = f.addresses.into_iter().map(h160_to_alloy).collect();
    filter = filter.address(addresses);

    let [t0, t1, t2, t3] = f.topics;

    if let Some(t0) = t0 {
        filter = filter.event_signature(map_topics(t0));
    }
    if let Some(t1) = t1 {
        filter = filter.topic1(map_topics(t1));
    }
    if let Some(t2) = t2 {
        filter = filter.topic2(map_topics(t2));
    }
    if let Some(t3) = t3 {
        filter = filter.topic3(map_topics(t3));
    }

    filter
}

#[async_trait::async_trait]
impl ChainClient for EvmClient {
    async fn chain_id(&self) -> Result<u64> {
        EvmClient::chain_id(self).await
    }

    async fn block_number(&self) -> Result<u64> {
        EvmClient::block_number(self).await
    }

    async fn get_logs(&self, filter: LogFilter) -> Result<Vec<ChainLog>> {
        let alloy_filter = build_alloy_filter(filter);
        let logs = self.logs(alloy_filter).await?;

        Ok(logs
            .into_iter()
            .map(|l| ChainLog {
                address: alloy_address_to_h160(l.address()),
                topics: l.topics().iter().map(|t| H256::from_slice(t.as_slice())).collect(),
                data: l.data().data.to_vec(),
                transaction_hash: l.transaction_hash.map(|h| H256::from_slice(h.as_slice())),
                block_number: l.block_number,
            })
            .collect())
    }

    async fn get_receipt(&self, tx: H256) -> Result<Option<ChainReceipt>> {
        let tx_hash = h256_to_alloy(tx);
        let r = self.get_receipt(tx_hash).await?;

        if let Some(receipt) = r {
            let json = serde_json::to_vec(&receipt)?;
            Ok(Some(ChainReceipt { block_number: receipt.block_number, json }))
        } else {
            Ok(None)
        }
    }

    async fn get_transaction_input(&self, tx: H256) -> Result<Option<Vec<u8>>> {
        let tx_hash = h256_to_alloy(tx);
        let input = self.get_transaction_input(tx_hash).await?;
        Ok(input.map(|b| b.to_vec()))
    }

    async fn read_call(&self, to: H160, data: Vec<u8>) -> Result<Vec<u8>> {
        let to = AlloyAddress::from_slice(to.as_bytes());
        let input = AlloyBytes::from(data);
        let out = self.call(to, input).await?;
        Ok(out.to_vec())
    }

    async fn send_transaction(&self, to: H160, data: Vec<u8>) -> Result<H256> {
        let to = AlloyAddress::from_slice(to.as_bytes());
        let input = AlloyBytes::from(data);
        let tx_hash = self.send_transaction_data(to, input).await?;
        Ok(H256::from_slice(tx_hash.as_slice()))
    }
}
