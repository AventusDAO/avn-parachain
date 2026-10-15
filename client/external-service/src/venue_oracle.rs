// Copyright 2026 Aventus DAO Ltd

//! Venue observation for the watchtower venue oracle: the raw state of an Exotic perpetuals
//! market at the block a round's target timestamp names.
//!
//! Serves `GET /venue/exotic/{chain_id}/{target_ts}/{clearing_house}/{amm}`. The block read is
//! the latest one whose timestamp is at most `target_ts`. It is only resolved once the endpoint
//! has a block after the target, so a later block cannot still change the answer; until then
//! the request fails and the watchtower retries next parachain block. Every value is read at
//! that block's hash with plain `eth_call`s (no Multicall3, so a local Hardhat chain works too):
//! - `Amm.getSpotPrice()` -> `spot_e18`
//! - `Amm.getUnderlyingPrice()` -> `oracle_e18`. The contract reverts before the first price push;
//!   the observation then fails rather than reporting zero, which would be a value the contract
//!   never returned.
//! - `ClearingHouse.openInterestNotional(amm)` -> `oi_notional_e18`
//!
//! Values are passed through untouched. RPC policy is failover, as for the floor: endpoints
//! are tried in order and the first that serves the whole observation wins. An endpoint must
//! report the requested chain id.
//!
//! Unlike the floor, the block is not required to be `safe`: on Base that lags by minutes, and
//! the venue oracle wants state seconds old. A reorg around the target splits the reports, and
//! the round fails instead of finalising a wrong value.

use crate::evm::client::EvmClient;
use alloy::{
    primitives::{Address, B256, U256},
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

/// What the pallet asks for: the market, as configured on chain, and the round's target
/// timestamp in unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VenueRequest {
    pub chain_id: u64,
    pub target_ts: u64,
    pub clearing_house: Address,
    pub amm: Address,
}

/// The venue chain has no block after the round's target yet. The normal state on the first
/// attempts of a round, since the target is only seconds old; the watchtower retries next
/// parachain block. Kept apart from real failures so it can be logged quietly.
#[derive(Debug)]
pub struct TargetNotReached {
    pub target_ts: u64,
    pub head: u64,
    pub head_ts: u64,
}

impl core::fmt::Display for TargetNotReached {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "no block after target {} yet (latest {} at {})",
            self.target_ts, self.head, self.head_ts
        )
    }
}

impl std::error::Error for TargetNotReached {}

/// True when `error` only says the venue chain has not passed the target yet.
pub fn is_target_not_reached(error: &anyhow::Error) -> bool {
    error.downcast_ref::<TargetNotReached>().is_some()
}

/// Tries each endpoint in order; the first complete observation wins. When none serves one and
/// at least one is merely behind the target, fails with [`TargetNotReached`]; with the default
/// list of public endpoints, one of them failing for another reason is common and must not turn
/// the normal wait into an error.
pub async fn observe(rpc_urls: &[String], request: &VenueRequest) -> Result<VenueObservation> {
    let mut errors = Vec::new();
    let mut not_reached = None;
    for url in rpc_urls {
        match observe_with(url, request).await {
            Ok(observation) => {
                log::info!(
                    "venue oracle: observation from {} for chain {} at target {} (block {}, ts {}): spot_e18={}, oracle_e18={}, oi_notional_e18={}",
                    url,
                    request.chain_id,
                    request.target_ts,
                    observation.pinned_block,
                    observation.pinned_ts,
                    observation.spot_e18,
                    observation.oracle_e18,
                    observation.oi_notional_e18
                );
                return Ok(observation)
            },
            Err(e) => match e.downcast::<TargetNotReached>() {
                Ok(behind) => {
                    log::debug!("venue oracle: {url} for chain {}: {behind}", request.chain_id);
                    not_reached.get_or_insert(behind);
                },
                Err(e) => {
                    log::warn!(
                        "venue oracle: {} failed for chain {} target {}: {:#}",
                        url,
                        request.chain_id,
                        request.target_ts,
                        e
                    );
                    errors.push(format!("{url}: {e:#}"));
                },
            },
        }
    }
    match not_reached {
        // At least one endpoint is up and merely behind: the normal wait, retried next block.
        // Endpoints that failed for real were each logged above.
        Some(behind) => Err(behind.into()),
        None => bail!(
            "no endpoint could serve chain {} target {}: {}",
            request.chain_id,
            request.target_ts,
            errors.join(" | ")
        ),
    }
}

async fn observe_with(url: &str, request: &VenueRequest) -> Result<VenueObservation> {
    let client = EvmClient::new_http(url)?;
    let provider = client.provider.clone();

    let chain_id = client.chain_id().await.context("chain id")?;
    if chain_id != request.chain_id {
        bail!("endpoint is chain {chain_id}, expected {}", request.chain_id);
    }

    let (block, pinned_ts, hash) = block_at(&provider, request.target_ts).await?;

    let spot_e18 =
        read_u128(&provider, request.amm, getSpotPriceCall {}.abi_encode(), hash, "getSpotPrice")
            .await?;
    let oracle_e18 = read_u128(
        &provider,
        request.amm,
        getUnderlyingPriceCall {}.abi_encode(),
        hash,
        "getUnderlyingPrice",
    )
    .await?;
    let oi_notional_e18 = read_u128(
        &provider,
        request.clearing_house,
        openInterestNotionalCall { amm: request.amm }.abi_encode(),
        hash,
        "openInterestNotional",
    )
    .await?;

    Ok(VenueObservation { pinned_block: block, pinned_ts, spot_e18, oracle_e18, oi_notional_e18 })
}

/// How far an endpoint's head may trail the target and still count as "not reached yet". The
/// target is only seconds old, so a head further behind is a stalled or syncing endpoint: a
/// real failure, not the normal wait.
pub const MAX_NOT_REACHED_LAG_SECS: u64 = 60;

/// The latest block whose timestamp is at most `target_ts`, as `(number, timestamp, hash)`.
/// Fails while the endpoint has no block after the target: a block still to come could then
/// also be at or before it, and nodes asking at different moments would disagree.
async fn block_at<P: Provider>(provider: &P, target_ts: u64) -> Result<(u64, u64, B256)> {
    let head = header(provider, BlockNumberOrTag::Latest).await.context("latest block")?;
    if head.timestamp <= target_ts {
        let lag = target_ts - head.timestamp;
        if lag > MAX_NOT_REACHED_LAG_SECS {
            bail!(
                "endpoint head {} at {} is {lag}s behind target {target_ts}: stalled or syncing",
                head.number,
                head.timestamp
            );
        }
        return Err(TargetNotReached { target_ts, head: head.number, head_ts: head.timestamp }.into())
    }
    let found = latest_at_or_before(
        head.number,
        head.timestamp,
        target_ts,
        |n| header(provider, BlockNumberOrTag::Number(n)),
        |block| block.timestamp,
    )
    .await?;
    Ok((found.number, found.timestamp, found.hash))
}

/// Number, timestamp and hash of a block, checking the endpoint returned the one asked for.
async fn header<P: Provider>(provider: &P, tag: BlockNumberOrTag) -> Result<BlockHeader> {
    let block = provider
        .get_block_by_number(tag)
        .await
        .with_context(|| format!("block {tag} header"))?
        .ok_or_else(|| anyhow!("block {tag} not found"))?;
    if let BlockNumberOrTag::Number(n) = tag {
        if block.header.number != n {
            bail!("endpoint returned block {} for {n}", block.header.number);
        }
    }
    Ok(BlockHeader {
        number: block.header.number,
        timestamp: block.header.timestamp,
        hash: block.header.hash,
    })
}

struct BlockHeader {
    number: u64,
    timestamp: u64,
    hash: B256,
}

/// Binary search for the latest block at or before `target_ts`, given a head after it.
/// Returns that block as `fetch` produced it, so the caller need not fetch it again.
///
/// On chains whose timestamps rise by at least a second per block (Ethereum, Base), the answer
/// is at most `head_ts - target_ts` blocks below the head, so the search starts there and needs
/// a handful of lookups when the target is seconds old. On chains where blocks can share a
/// timestamp that guess can still be after the target; the lower bound then steps back,
/// doubling each time, until it is not. `fetch` loads a block by number and `ts_of` reads its
/// timestamp.
pub async fn latest_at_or_before<B, F, Fut>(
    head: u64,
    head_ts: u64,
    target_ts: u64,
    mut fetch: F,
    ts_of: impl Fn(&B) -> u64,
) -> Result<B>
where
    F: FnMut(u64) -> Fut,
    Fut: core::future::Future<Output = Result<B>>,
{
    if head_ts <= target_ts {
        bail!("head {head} at {head_ts} is not after target {target_ts}");
    }
    let mut hi = head;
    let mut step = head_ts - target_ts;
    let mut lo = head.saturating_sub(step);
    let mut found = fetch(lo).await?;
    while ts_of(&found) > target_ts {
        if lo == 0 {
            bail!("no block at or before target {target_ts} (genesis is after it)");
        }
        hi = lo;
        step = step.saturating_mul(2);
        lo = lo.saturating_sub(step);
        found = fetch(lo).await?;
    }
    // Invariant: ts(lo) <= target < ts(hi), and `found` is block `lo`.
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        let block = fetch(mid).await?;
        if ts_of(&block) <= target_ts {
            lo = mid;
            found = block;
        } else {
            hi = mid;
        }
    }
    Ok(found)
}

/// One `eth_call` pinned at the block with `hash`, decoded as a single `uint256`. Reading by
/// hash keeps all values from one block even if the chain reorganises between calls.
async fn read_u128<P: Provider>(
    provider: &P,
    to: Address,
    input: Vec<u8>,
    hash: B256,
    what: &str,
) -> Result<u128> {
    let tx = TransactionRequest::default().to(to).input(input.into());
    let out = provider
        .call(tx)
        .block(BlockId::hash(hash))
        .await
        .with_context(|| format!("{what} on {to} at block {hash}"))?;
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

    /// Runs the search over a chain given as timestamps from block 0, returning the block
    /// number found. Also checks the returned block is the one fetched for that number.
    async fn search(timestamps: &[u64], target_ts: u64) -> Result<u64> {
        let head = timestamps.len() as u64 - 1;
        let (number, ts) = latest_at_or_before(
            head,
            timestamps[head as usize],
            target_ts,
            |n| {
                let ts = timestamps[n as usize];
                async move { Ok((n, ts)) }
            },
            |block: &(u64, u64)| block.1,
        )
        .await?;
        assert_eq!(ts, timestamps[number as usize]);
        Ok(number)
    }

    #[tokio::test]
    async fn finds_the_latest_block_at_or_before_the_target() {
        // Two second blocks from ts 100 (block 0) to ts 140 (block 20).
        let chain: Vec<u64> = (0..=20).map(|n| 100 + 2 * n).collect();
        assert_eq!(search(&chain, 130).await.unwrap(), 15, "exact timestamp");
        assert_eq!(search(&chain, 131).await.unwrap(), 15, "between two blocks");
        assert_eq!(search(&chain, 139).await.unwrap(), 19, "just before the head");
        assert_eq!(search(&chain, 100).await.unwrap(), 0, "genesis");
        assert!(search(&chain, 140).await.is_err(), "no block after the target yet");
        assert!(search(&chain, 99).await.is_err(), "target before genesis");
    }

    #[tokio::test]
    async fn steps_back_when_blocks_share_timestamps() {
        // Four blocks per second, as on a chain with sub-second blocks: the first guess from
        // the head lands after the target.
        let chain: Vec<u64> = (0..=40).map(|n| 100 + n / 4).collect();
        assert_eq!(chain[40], 110);
        assert_eq!(search(&chain, 105).await.unwrap(), 23, "last block at ts 105");
        assert_eq!(search(&chain, 100).await.unwrap(), 3, "last block at ts 100");
        assert!(search(&chain, 99).await.is_err(), "target before genesis");
    }

    #[tokio::test]
    async fn handles_uneven_block_times() {
        // Gaps of 1 to 12 seconds, as on an idle or interval mined local chain.
        let chain = [10, 11, 12, 24, 25, 37, 38, 39, 51];
        for (target, expected) in [(11, 1), (23, 2), (24, 3), (36, 4), (50, 7)] {
            assert_eq!(search(&chain, target).await.unwrap(), expected, "target {target}");
        }
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
