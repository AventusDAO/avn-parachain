// Copyright 2026 Aventus DAO Ltd

//! Venue observation for the watchtower venue oracle: the raw state of an Exotic perpetuals
//! market at a pinned block.
//!
//! Serves `GET /venue/exotic/{chain_id}/{pinned_block}/{clearing_house}/{amm}`. Every value is
//! read as of `pinned_block` with plain `eth_call`s (no Multicall3, so a local Hardhat chain
//! works too), plus the block header for its timestamp:
//! - `Amm.getSpotPrice()` -> `spot_e18`
//! - `Amm.getUnderlyingPrice()` -> `oracle_e18`. The contract reverts before the first price push;
//!   the observation then fails rather than reporting zero, which would be a value the contract
//!   never returned.
//! - `ClearingHouse.openInterestNotional(amm)` -> `oi_notional_e18`
//!
//! Values are passed through untouched. RPC policy is failover, as for the floor: endpoints
//! are tried in order and the first that serves the whole observation wins. An endpoint must
//! report the requested chain id and must already consider the pinned block safe.

use crate::evm::client::{ensure_safe, EvmClient};
use alloy::{
    primitives::{Address, U256},
    providers::Provider,
    rpc::types::{BlockId, BlockNumberOrTag, TransactionRequest},
    sol,
    sol_types::{SolCall, SolValue},
};
use anyhow::{anyhow, bail, Context, Result};
use sp_floor_oracle::VenueObservation;

/// Public Base mainnet endpoints, tried in this order unless overridden with
/// `--venue-rpc-url`. Reads at a pinned block need archive state once the block is more than
/// a few minutes old; endpoints without it fail over to the next one.
pub const DEFAULT_RPC_URLS: [&str; 5] = [
    "https://mainnet.base.org",
    "https://base-rpc.publicnode.com",
    "https://base.llamarpc.com",
    "https://base.drpc.org",
    "https://1rpc.io/base",
];

sol! {
    function getSpotPrice() external view returns (uint256);
    function getUnderlyingPrice() external view returns (uint256);
    function openInterestNotional(address amm) external view returns (uint256);
}

/// What the pallet asks for: the market, as configured on chain, and the pinned block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VenueRequest {
    pub chain_id: u64,
    pub pinned_block: u64,
    pub clearing_house: Address,
    pub amm: Address,
}

/// Tries each endpoint in order; the first complete observation wins.
pub async fn observe(rpc_urls: &[String], request: &VenueRequest) -> Result<VenueObservation> {
    let mut errors = Vec::new();
    for url in rpc_urls {
        match observe_with(url, request).await {
            Ok(observation) => {
                log::info!(
                    "venue oracle: observation from {} for chain {} at block {}: spot_e18={}, oracle_e18={}, oi_notional_e18={}",
                    url,
                    request.chain_id,
                    request.pinned_block,
                    observation.spot_e18,
                    observation.oracle_e18,
                    observation.oi_notional_e18
                );
                return Ok(observation)
            },
            Err(e) => {
                log::warn!(
                    "venue oracle: {} failed for chain {} block {}: {:#}",
                    url,
                    request.chain_id,
                    request.pinned_block,
                    e
                );
                errors.push(format!("{url}: {e:#}"));
            },
        }
    }
    bail!(
        "no endpoint could serve chain {} block {}: {}",
        request.chain_id,
        request.pinned_block,
        errors.join(" | ")
    )
}

async fn observe_with(url: &str, request: &VenueRequest) -> Result<VenueObservation> {
    let client = EvmClient::new_http(url)?;
    let provider = client.provider.clone();
    let block = request.pinned_block;

    let chain_id = client.chain_id().await.context("chain id")?;
    if chain_id != request.chain_id {
        bail!("endpoint is chain {chain_id}, expected {}", request.chain_id);
    }
    ensure_safe(&provider, block).await?;

    let header = provider
        .get_block_by_number(BlockNumberOrTag::Number(block))
        .await
        .context("pinned block header")?
        .ok_or_else(|| anyhow!("block {block} not found"))?;
    if header.header.number != block {
        bail!("endpoint returned block {} for {block}", header.header.number);
    }

    let spot_e18 =
        read_u128(&provider, request.amm, getSpotPriceCall {}.abi_encode(), block, "getSpotPrice")
            .await?;
    let oracle_e18 = read_u128(
        &provider,
        request.amm,
        getUnderlyingPriceCall {}.abi_encode(),
        block,
        "getUnderlyingPrice",
    )
    .await?;
    let oi_notional_e18 = read_u128(
        &provider,
        request.clearing_house,
        openInterestNotionalCall { amm: request.amm }.abi_encode(),
        block,
        "openInterestNotional",
    )
    .await?;

    Ok(VenueObservation {
        pinned_block: block,
        pinned_ts: header.header.timestamp,
        spot_e18,
        oracle_e18,
        oi_notional_e18,
    })
}

/// One `eth_call` pinned at `block`, decoded as a single `uint256`.
async fn read_u128<P: Provider>(
    provider: &P,
    to: Address,
    input: Vec<u8>,
    block: u64,
    what: &str,
) -> Result<u128> {
    let tx = TransactionRequest::default().to(to).input(input.into());
    let out = provider
        .call(tx)
        .block(BlockId::number(block))
        .await
        .with_context(|| format!("{what} on {to} at block {block}"))?;
    let value = U256::abi_decode(&out).with_context(|| format!("{what} returned {out}"))?;
    u128_from(value).with_context(|| what.to_string())
}

/// An 18 decimal value above `u128::MAX` is a broken read, not data.
pub fn u128_from(value: U256) -> Result<u128> {
    u128::try_from(value).map_err(|_| anyhow!("value {value} does not fit in u128"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sp_io::hashing::keccak_256;

    #[test]
    fn bindings_match_the_contract_abi() {
        assert_eq!(getSpotPriceCall::SELECTOR, &keccak_256(b"getSpotPrice()")[..4]);
        assert_eq!(getUnderlyingPriceCall::SELECTOR, &keccak_256(b"getUnderlyingPrice()")[..4]);
        assert_eq!(
            openInterestNotionalCall::SELECTOR,
            &keccak_256(b"openInterestNotional(address)")[..4]
        );
        let amm = Address::repeat_byte(0xA1);
        let encoded = openInterestNotionalCall { amm }.abi_encode();
        assert_eq!(encoded.len(), 4 + 32);
        assert_eq!(&encoded[16..], amm.as_slice());
    }

    #[test]
    fn return_values_decode_as_uint256() {
        let value = U256::from(51_000_000_000_000_000_000u128);
        let out = value.abi_encode();
        assert_eq!(u128_from(U256::abi_decode(&out).unwrap()).unwrap(), 51_000_000_000_000_000_000);
        assert!(u128_from(U256::MAX).is_err());
        assert_eq!(u128_from(U256::ZERO).unwrap(), 0, "zero is a raw value, not an error");
    }
}
