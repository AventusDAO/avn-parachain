// Copyright 2026 Aventus DAO Ltd

//! Punks floor observation for the watchtower floor oracle.
//!
//! Serves `GET /floor/punks/{pinned_ts}/{pinned_block}`. Every watchtower reads the same
//! Ethereum block, so the answer is deterministic and nodes agree when their RPCs are honest.
//!
//! What is read, at `pinned_block`:
//! - **floor**: `punksOfferedForSale(i)` for all 10,000 punks through Multicall3, in chunks. The
//!   floor is the lowest public ask (`isForSale`, no `onlySellTo`, non-zero `minValue`).
//! - **last sale**: the latest `PunkBought` log with a non-zero price in the `SALES_WINDOW_BLOCKS`
//!   before the pinned block, with its block timestamp.
//!
//! RPC policy is failover: the configured endpoints (or `DEFAULT_RPC_URLS`) are tried in order
//! and the first one that serves the whole observation wins. An endpoint must be Ethereum
//! mainnet and must already consider the pinned block final.

use crate::{evm::client::EvmClient, ETH_FINALITY};
use alloy::{
    primitives::{address, Address, B256, U256},
    providers::{CallItem, MulticallBuilder, Provider},
    rpc::types::{BlockId, BlockNumberOrTag, Filter},
    sol,
    sol_types::{SolCall, SolEvent},
};
use anyhow::{anyhow, bail, Context, Result};
use sp_floor_oracle::{FloorObservation, FLAG_FLOOR_FAR_BELOW_SALES, FLAG_SALES_UNAVAILABLE};

/// Reputable public Ethereum mainnet endpoints, tried in this order unless overridden with
/// `--floor-rpc-url`.
pub const DEFAULT_RPC_URLS: [&str; 5] = [
    "https://ethereum-rpc.publicnode.com",
    "https://cloudflare-eth.com",
    "https://rpc.ankr.com/eth",
    "https://eth.llamarpc.com",
    "https://eth.drpc.org",
];

pub const ETHEREUM_MAINNET_CHAIN_ID: u64 = 1;
pub const CRYPTOPUNKS_MARKET: Address = address!("0xb47e3cd837dDF8e4c57F05d70Ab865de6e193BBB");
pub const TOTAL_PUNKS: u64 = 10_000;
/// Offers read per Multicall3 call. 500 reads stay well inside public RPC gas caps.
pub const MULTICALL_CHUNK: u64 = 500;
/// Blocks before the pinned block scanned for sales (about one day). Public RPCs cap
/// `eth_getLogs` ranges around 10,000 blocks.
pub const SALES_WINDOW_BLOCKS: u64 = 7_000;
/// A floor below this share of the last sale is flagged.
pub const FAR_BELOW_SALES_BPS: u128 = 5_000;

sol! {
    #[derive(Debug)]
    function punksOfferedForSale(uint256 punkIndex) external view returns (
        bool isForSale,
        uint256 punkIndex_,
        address seller,
        uint256 minValue,
        address onlySellTo
    );

    event PunkBought(uint256 indexed punkIndex, uint256 value, address indexed fromAddress, address indexed toAddress);
}

/// One public ask as read from the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer {
    pub is_for_sale: bool,
    pub min_value: U256,
    pub only_sell_to: Address,
}

/// The lowest public ask, if any. Private offers (`onlySellTo` set) and zero asks are ignored.
pub fn floor_of(offers: impl IntoIterator<Item = Offer>) -> Option<u128> {
    offers
        .into_iter()
        .filter(|o| o.is_for_sale && o.only_sell_to == Address::ZERO && !o.min_value.is_zero())
        .map(|o| u128::try_from(o.min_value).unwrap_or(u128::MAX))
        .min()
}

/// Flags derived from the floor and the last sale.
pub fn flags_for(floor_wei: u128, last_sale_wei: Option<u128>) -> u32 {
    match last_sale_wei {
        None => FLAG_SALES_UNAVAILABLE,
        Some(sale)
            if floor_wei.saturating_mul(10_000) < sale.saturating_mul(FAR_BELOW_SALES_BPS) =>
            FLAG_FLOOR_FAR_BELOW_SALES,
        Some(_) => 0,
    }
}

/// Tries each endpoint in order; the first complete observation wins.
pub async fn observe(
    rpc_urls: &[String],
    pinned_ts: u64,
    pinned_block: u64,
) -> Result<FloorObservation> {
    let mut errors = Vec::new();
    for url in rpc_urls {
        match observe_with(url, pinned_block).await {
            Ok(observation) => {
                log::info!(
                    "floor oracle: observation from {} at block {} (pinned_ts={}): floor_wei={}, flags={}",
                    url,
                    pinned_block,
                    pinned_ts,
                    observation.floor_wei,
                    observation.flags
                );
                return Ok(observation)
            },
            Err(e) => {
                log::warn!("floor oracle: {} failed for block {}: {:#}", url, pinned_block, e);
                errors.push(format!("{url}: {e:#}"));
            },
        }
    }
    bail!("no endpoint could serve block {pinned_block}: {}", errors.join(" | "))
}

async fn observe_with(url: &str, pinned_block: u64) -> Result<FloorObservation> {
    let client = EvmClient::new_http(url)?;
    let provider = client.provider.clone();

    let chain_id = client.chain_id().await.context("chain id")?;
    if chain_id != ETHEREUM_MAINNET_CHAIN_ID {
        bail!("endpoint is chain {chain_id}, expected Ethereum mainnet");
    }
    let latest = client.block_number().await.context("latest block")?;
    if latest < pinned_block.saturating_add(ETH_FINALITY) {
        bail!("block {pinned_block} is not final yet on this endpoint (latest {latest})");
    }

    let offers = read_offers(&provider, pinned_block).await?;
    let floor_wei = floor_of(offers).ok_or_else(|| anyhow!("no punk is publicly for sale"))?;

    let last_sale = last_sale(&provider, pinned_block).await.context("last sale")?;
    let (last_sale_ts, last_sale_wei) = last_sale.unwrap_or((0, 0));

    Ok(FloorObservation {
        observed_eth_block: pinned_block,
        floor_wei,
        last_sale_ts,
        last_sale_wei,
        flags: flags_for(floor_wei, last_sale.map(|(_, wei)| wei)),
    })
}

/// All 10,000 offers at `block`, read through Multicall3 in chunks.
async fn read_offers<P: Provider + Clone>(provider: &P, block: u64) -> Result<Vec<Offer>> {
    let mut offers = Vec::with_capacity(TOTAL_PUNKS as usize);
    let mut start = 0u64;
    while start < TOTAL_PUNKS {
        let end = (start + MULTICALL_CHUNK).min(TOTAL_PUNKS);
        let mut multicall =
            MulticallBuilder::new_dynamic(provider.clone()).block(BlockId::number(block));
        for index in start..end {
            let input = punksOfferedForSaleCall { punkIndex: U256::from(index) }.abi_encode();
            multicall = multicall.add_call_dynamic(CallItem::<punksOfferedForSaleCall>::new(
                CRYPTOPUNKS_MARKET,
                input.into(),
            ));
        }
        let results = multicall
            .aggregate3()
            .await
            .with_context(|| format!("multicall for punks {start}..{end}"))?;
        for (offset, result) in results.into_iter().enumerate() {
            let ret = result
                .map_err(|e| anyhow!("punk {} read failed: {:?}", start + offset as u64, e))?;
            offers.push(Offer {
                is_for_sale: ret.isForSale,
                min_value: ret.minValue,
                only_sell_to: ret.onlySellTo,
            });
        }
        start = end;
    }
    Ok(offers)
}

/// Latest `PunkBought` with a non-zero price in the window before `block`, as
/// `(block timestamp, price in wei)`.
async fn last_sale<P: Provider>(provider: &P, block: u64) -> Result<Option<(u64, u128)>> {
    let from_block = block.saturating_sub(SALES_WINDOW_BLOCKS);
    let filter = Filter::new()
        .address(CRYPTOPUNKS_MARKET)
        .event_signature(B256::from(PunkBought::SIGNATURE_HASH))
        .from_block(from_block)
        .to_block(block);
    let logs = provider.get_logs(&filter).await.context("PunkBought logs")?;

    let latest = logs
        .into_iter()
        .filter_map(|log| {
            let price = u128::try_from(U256::from_be_slice(log.data().data.get(..32)?)).ok()?;
            if price == 0 {
                return None
            }
            Some((log.block_number?, log.log_index.unwrap_or_default(), price))
        })
        .max_by_key(|(block_number, log_index, _)| (*block_number, *log_index));

    let Some((sale_block, _, price)) = latest else { return Ok(None) };
    let header = provider
        .get_block_by_number(BlockNumberOrTag::Number(sale_block))
        .await
        .context("sale block")?
        .ok_or_else(|| anyhow!("sale block {sale_block} not found"))?;
    Ok(Some((header.header.timestamp, price)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(is_for_sale: bool, min_value: u128, only_sell_to: Address) -> Offer {
        Offer { is_for_sale, min_value: U256::from(min_value), only_sell_to }
    }

    #[test]
    fn floor_ignores_private_zero_and_unlisted_offers() {
        let private = address!("0x0000000000000000000000000000000000000001");
        let offers = vec![
            offer(false, 1, Address::ZERO),
            offer(true, 0, Address::ZERO),
            offer(true, 5, private),
            offer(true, 70, Address::ZERO),
            offer(true, 60, Address::ZERO),
        ];
        assert_eq!(floor_of(offers), Some(60));
        assert_eq!(floor_of(vec![offer(false, 1, Address::ZERO)]), None);
    }

    #[test]
    fn flags() {
        assert_eq!(flags_for(50, None), FLAG_SALES_UNAVAILABLE);
        assert_eq!(flags_for(50, Some(60)), 0);
        assert_eq!(flags_for(50, Some(100)), 0, "exactly half is not far below");
        assert_eq!(flags_for(49, Some(100)), FLAG_FLOOR_FAR_BELOW_SALES);
    }

    #[test]
    fn bindings_match_the_contract_abi() {
        use sp_io::hashing::keccak_256;
        let selector = &keccak_256(b"punksOfferedForSale(uint256)")[..4];
        assert_eq!(punksOfferedForSaleCall::SELECTOR, selector);
        assert_eq!(
            PunkBought::SIGNATURE_HASH.as_slice(),
            &keccak_256(b"PunkBought(uint256,uint256,address,address)")
        );
    }
}
