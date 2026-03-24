use std::{collections::BTreeMap, sync::Arc};

use jsonrpsee::{core::RpcResult, proc_macros::rpc, types::ErrorObjectOwned};
use pallet_cross_chain_voting_runtime_api::CrossChainVotingApi;
use runtime_common::opaque::Block;
use serde::{Deserialize, Serialize};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{Error as BlockChainError, HeaderBackend, HeaderMetadata};
use sp_core::H160;
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};

const MAX_IDENTITY_ADDRESSES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedBalancesAtTimestampResponse {
    pub block_hash: String,
    pub block_number: u32,
    pub timestamp_ms: u64,
    pub balances: BTreeMap<String, String>,
}

#[rpc(client, server)]
pub trait AvnApi {
    #[method(name = "avn_getLinkedBalancesAtOrBeforeTimestamp")]
    fn get_linked_balances_at_or_before_timestamp(
        &self,
        addresses: Vec<H160>,
        timestamp_sec: u64,
    ) -> RpcResult<LinkedBalancesAtTimestampResponse>;
}

pub struct CrossChainRpc<C> {
    client: Arc<C>,
}

impl<C> CrossChainRpc<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self { client }
    }
}

fn internal_err(message: impl Into<String>) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(-32000, message.into(), None::<()>)
}

fn invalid_params_err(message: impl Into<String>) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(-32602, message.into(), None::<()>)
}

fn validate_addresses_len(addresses: &[H160]) -> RpcResult<()> {
    if addresses.len() > MAX_IDENTITY_ADDRESSES {
        return Err(invalid_params_err(format!(
            "Too many addresses supplied: got {}, max {}",
            addresses.len(),
            MAX_IDENTITY_ADDRESSES
        )))
    }

    Ok(())
}

fn clamp(n: u32, min: u32, max: u32) -> u32 {
    n.max(min).min(max)
}

fn interpolate_next_guess(
    a_num: u32,
    a_ts: u64,
    b_num: u32,
    b_ts: u64,
    target_ts_ms: u64,
    min: u32,
    max: u32,
) -> Option<u32> {
    let block_delta = b_num.checked_sub(a_num)?;
    if block_delta == 0 {
        return None
    }

    let ts_delta = b_ts.checked_sub(a_ts)?;
    if ts_delta == 0 {
        return None
    }

    let ms_per_block = ts_delta as f64 / block_delta as f64;
    if !ms_per_block.is_finite() || ms_per_block <= 0.0 {
        return None
    }

    let projected = b_num as f64 + ((target_ts_ms as f64 - b_ts as f64) / ms_per_block).round();
    Some(clamp(projected as u32, min, max))
}

fn block_hash_by_number<C>(
    client: &Arc<C>,
    block_number: u32,
) -> Result<<Block as BlockT>::Hash, ErrorObjectOwned>
where
    C: HeaderBackend<Block> + Send + Sync + 'static,
{
    client
        .hash(block_number.into())
        .map_err(|e| internal_err(format!("Failed to read block hash for #{block_number}: {e}")))?
        .ok_or_else(|| internal_err(format!("Block hash not found for #{block_number}")))
}

fn block_timestamp<C>(
    client: &Arc<C>,
    hash: <Block as BlockT>::Hash,
) -> Result<u64, ErrorObjectOwned>
where
    C: ProvideRuntimeApi<Block> + Send + Sync + 'static,
    C::Api: pallet_cross_chain_voting_runtime_api::CrossChainVotingApi<Block>,
{
    client
        .runtime_api()
        .current_block_timestamp(hash)
        .map_err(|e| internal_err(format!("Failed to read timestamp at block {hash:?}: {e}")))
}

fn block_info<C>(
    client: &Arc<C>,
    block_number: u32,
) -> Result<(u32, <Block as BlockT>::Hash, u64), ErrorObjectOwned>
where
    C: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
    C::Api: pallet_cross_chain_voting_runtime_api::CrossChainVotingApi<Block>,
{
    let hash = block_hash_by_number(client, block_number)?;
    let ts = block_timestamp(client, hash)?;
    Ok((block_number, hash, ts))
}

fn binary_search_range<C>(
    client: &Arc<C>,
    mut lo: u32,
    mut hi: u32,
    target_ts_ms: u64,
) -> Result<(u32, <Block as BlockT>::Hash, u64), ErrorObjectOwned>
where
    C: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
    C::Api: pallet_cross_chain_voting_runtime_api::CrossChainVotingApi<Block>,
{
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let (_, _, ts) = block_info(client, mid)?;
        if ts <= target_ts_ms {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }

    block_info(client, lo)
}

/// Finds the highest block whose timestamp is less than or equal to `target_ts_ms`.
///
/// Strategy:
/// 1. Make an initial estimate based on the configured block time.
/// 2. Refine the estimate using interpolation/secant-style guesses.
/// 3. Fall back to binary search once the range is tight or interpolation stalls.
fn find_block_at_or_before_timestamp<C>(
    client: &Arc<C>,
    latest_number: u32,
    latest_ts: u64,
    target_ts_ms: u64,
    block_time_ms: u64,
) -> Result<(u32, <Block as BlockT>::Hash, u64), ErrorObjectOwned>
where
    C: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
    C::Api: pallet_cross_chain_voting_runtime_api::CrossChainVotingApi<Block>,
{
    let blocks_back = ((latest_ts - target_ts_ms) / block_time_ms) as u32;
    let initial_estimate = latest_number.saturating_sub(blocks_back);

    let (mut a_num, _, mut a_ts) = block_info(client, initial_estimate)?;

    if a_ts == target_ts_ms {
        return block_info(client, a_num)
    }

    let (mut lo, mut hi) = if a_ts < target_ts_ms { (a_num, latest_number) } else { (0, a_num) };

    let step = ((a_ts.abs_diff(target_ts_ms)) / block_time_ms).max(1) as u32;
    let second_guess = if a_ts < target_ts_ms {
        clamp(a_num.saturating_add(step), lo, hi)
    } else {
        clamp(a_num.saturating_sub(step), lo, hi)
    };

    if second_guess == a_num {
        return binary_search_range(client, lo, hi, target_ts_ms)
    }

    let (mut b_num, _, mut b_ts) = block_info(client, second_guess)?;

    if b_ts == target_ts_ms {
        return block_info(client, b_num)
    }

    if b_ts < target_ts_ms {
        lo = lo.max(b_num);
    } else {
        hi = hi.min(b_num);
    }

    for _ in 0..6 {
        let Some(next_guess) =
            interpolate_next_guess(a_num, a_ts, b_num, b_ts, target_ts_ms, lo, hi)
        else {
            break;
        };

        if next_guess == a_num || next_guess == b_num {
            break
        }

        let (c_num, _, c_ts) = block_info(client, next_guess)?;

        if c_ts == target_ts_ms {
            return block_info(client, c_num)
        }

        if c_ts < target_ts_ms {
            lo = lo.max(c_num);
        } else {
            hi = hi.min(c_num);
        }

        if hi.saturating_sub(lo) <= 1 {
            break
        }

        a_num = b_num;
        a_ts = b_ts;
        b_num = c_num;
        b_ts = c_ts;
    }

    binary_search_range(client, lo, hi, target_ts_ms)
}

impl<C> AvnApiServer for CrossChainRpc<C>
where
    C: ProvideRuntimeApi<Block>
        + HeaderBackend<Block>
        + HeaderMetadata<Block, Error = BlockChainError>
        + Send
        + Sync
        + 'static,
    C::Api: pallet_cross_chain_voting_runtime_api::CrossChainVotingApi<Block>,
{
    fn get_linked_balances_at_or_before_timestamp(
        &self,
        addresses: Vec<H160>,
        timestamp_sec: u64,
    ) -> RpcResult<LinkedBalancesAtTimestampResponse> {
        validate_addresses_len(&addresses)?;

        let client = self.client.clone();

        let latest_hash = client.info().finalized_hash;

        let latest_header = client
            .header(latest_hash)
            .map_err(|e| internal_err(format!("Failed to read finalized header: {e}")))?
            .ok_or_else(|| internal_err("Missing finalized header"))?;

        let latest_number: u32 = (*latest_header.number())
            .try_into()
            .map_err(|_| internal_err("Finalized block number does not fit into u32"))?;

        let target_ts_ms = timestamp_sec
            .checked_mul(1000)
            .ok_or_else(|| internal_err("Timestamp overflow"))?;

        let latest_ts = client
            .runtime_api()
            .current_block_timestamp(latest_hash)
            .map_err(|e| internal_err(format!("Failed to read latest block timestamp: {e}")))?;

        let block_time_ms = client
            .runtime_api()
            .block_time_ms(latest_hash)
            .map_err(|e| internal_err(format!("Failed to read expected block time: {e}")))?;

        if block_time_ms == 0 {
            return Err(internal_err("Runtime returned zero expected block time"))
        }

        let (block_number, block_hash, timestamp_ms) = if target_ts_ms >= latest_ts {
            (latest_number, latest_hash, latest_ts)
        } else {
            find_block_at_or_before_timestamp(
                &client,
                latest_number,
                latest_ts,
                target_ts_ms,
                block_time_ms,
            )?
        };

        let requested_addresses = addresses;

        let balances = client
            .runtime_api()
            .get_total_linked_balances(block_hash, requested_addresses.clone())
            .map_err(|e| internal_err(format!("Failed to read linked balances: {e}")))?;

        let balances = requested_addresses
            .into_iter()
            .zip(balances)
            .map(|(address, balance)| (format!("{address:#x}").to_lowercase(), balance.to_string()))
            .collect();

        Ok(LinkedBalancesAtTimestampResponse {
            block_hash: format!("{block_hash:#x}"),
            block_number,
            timestamp_ms,
            balances,
        })
    }
}
