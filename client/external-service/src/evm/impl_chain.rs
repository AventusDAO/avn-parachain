// Copyright 2026 Aventus DAO Ltd

use super::client::EvmClient;
use crate::chain::{ChainClient, ChainLog, ChainReceipt, LogFilter};
use alloy::rpc::types::Filter;
use alloy_primitives::{Address as AlloyAddress, Bytes as AlloyBytes, B256 as AlloyB256};
use anyhow::Result;
use sp_core::{H160, H256};

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
        Ok(self.chain_id().await?)
    }

    async fn block_number(&self) -> Result<u64> {
        Ok(self.block_number().await?)
    }

    async fn get_logs(&self, filter: LogFilter) -> Result<Vec<ChainLog>> {
        let alloy_filter = build_alloy_filter(filter);
        let logs = self.logs(alloy_filter).await?;

        let out = logs
            .into_iter()
            .map(|l| ChainLog {
                address: alloy_address_to_h160(l.address()),
                topics: l.topics().iter().map(|t| H256::from_slice(t.as_slice())).collect(),
                data: l.data().data.to_vec(),
                transaction_hash: l.transaction_hash.map(|h| H256::from_slice(h.as_slice())),
                block_number: l.block_number,
            })
            .collect();

        Ok(out)
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

    async fn read_call(&self, to: H160, data: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        let to = AlloyAddress::from_slice(to.as_bytes());
        let input = AlloyBytes::from(data);
        let out = EvmClient::call(self, to, input).await?;
        Ok(out.to_vec())
    }

    async fn send_transaction(&self, to: H160, data: Vec<u8>) -> anyhow::Result<H256> {
        let to = AlloyAddress::from_slice(to.as_bytes());
        let input = AlloyBytes::from(data);
        let tx_hash = EvmClient::send_transaction_data(self, to, input).await?;
        Ok(H256::from_slice(tx_hash.as_slice()))
    }
}
