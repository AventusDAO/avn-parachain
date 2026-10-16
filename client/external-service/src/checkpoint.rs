// Copyright 2026 Aventus DAO Ltd

//! Checkpoint block for the collator offchain worker: `GET /checkpoint/block/{chain_id}`.
//!
//! Every collator must vote the SAME block number for a checkpoint round to reach quorum, yet
//! collators read the chain seconds apart. The head is therefore taken back by a confirmation
//! depth and rounded down to a fixed interval per chain:
//!
//! `vote = ((latest - confirmations) / pin_interval) * pin_interval`
//!
//! Two collators only disagree when the head crosses an interval boundary between their reads,
//! which the next round resolves. Short chains (local Hardhat right after start) fall back to
//! `max(1, latest - confirmations)`. The vote is never zero.
//!
//! The endpoint for a chain is picked from the lists the node has (floor, venue, bridge), by
//! probing `eth_chainId`, and the first match is cached for the life of the process.

use crate::evm::client::EvmClient;
use anyhow::{anyhow, Context, Result};
use std::{collections::HashMap, future::Future, sync::Mutex};

/// How a chain's head is turned into a vote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainParams {
    /// Blocks taken off the head before rounding.
    pub confirmations: u64,
    /// The vote is a multiple of this.
    pub pin_interval: u64,
}

/// Depth of Ethereum checkpoints, about two minutes. Not finality: post-Merge reorgs are one or
/// two blocks. A deeper one makes floor reports disagree and the round fail if it lands while
/// watchtowers are reading, but a floor read from the orphaned block can finalise if most of them
/// read before it (see `floor_oracle`). Kept apart from `ETH_FINALITY`, which the bridge uses for
/// its own, stricter purpose.
pub const ETHEREUM_CHECKPOINT_CONFIRMATIONS: u64 = 10;

/// The floor read requires the same depth, so a floor round can read at its checkpoint as soon
/// as it starts.
pub const ETHEREUM: ChainParams =
    ChainParams { confirmations: ETHEREUM_CHECKPOINT_CONFIRMATIONS, pin_interval: 50 };
pub const BASE: ChainParams = ChainParams { confirmations: 10, pin_interval: 150 };
pub const HARDHAT: ChainParams = ChainParams { confirmations: 2, pin_interval: 10 };
pub const DEFAULT: ChainParams = ETHEREUM;

pub fn params_for(chain_id: u64) -> ChainParams {
    match chain_id {
        1 => ETHEREUM,
        8453 => BASE,
        31337 => HARDHAT,
        _ => DEFAULT,
    }
}

/// The block to vote for a chain whose head is `latest`.
pub fn vote_block(latest: u64, params: &ChainParams) -> u32 {
    let confirmed = latest.saturating_sub(params.confirmations);
    let vote = if latest < params.confirmations + params.pin_interval {
        confirmed.max(1)
    } else {
        (confirmed / params.pin_interval) * params.pin_interval
    };
    u32::try_from(vote.max(1)).unwrap_or(u32::MAX)
}

/// Endpoints known to the node, grouped by the list they came from, plus the per-chain cache
/// of which endpoint answered for which chain id.
pub struct Endpoints {
    pub candidates: Vec<Vec<String>>,
    cache: Mutex<HashMap<u64, String>>,
}

impl Endpoints {
    pub fn new(candidates: Vec<Vec<String>>) -> Self {
        Self { candidates, cache: Mutex::new(HashMap::new()) }
    }

    fn cached(&self, chain_id: u64) -> Option<String> {
        self.cache.lock().ok().and_then(|cache| cache.get(&chain_id).cloned())
    }

    fn remember(&self, chain_id: u64, url: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(chain_id, url.to_string());
        }
    }

    fn forget(&self, chain_id: u64) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(&chain_id);
        }
    }
}

/// Why no block could be served; the route maps it to a status code.
#[derive(Debug)]
pub enum BlockError {
    /// No configured endpoint reports `chain_id`.
    NoEndpoint(u64),
    Other(anyhow::Error),
}

impl std::fmt::Display for BlockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockError::NoEndpoint(chain_id) =>
                write!(f, "no configured RPC endpoint serves chain {chain_id}"),
            BlockError::Other(e) => write!(f, "{e:#}"),
        }
    }
}

/// The block every collator should vote for `chain_id` right now.
pub async fn block(endpoints: &Endpoints, chain_id: u64) -> Result<u32, BlockError> {
    let url = match endpoints.cached(chain_id) {
        Some(url) => url,
        None => {
            let url = select_endpoint(&endpoints.candidates, chain_id, probe_chain_id)
                .await
                .ok_or(BlockError::NoEndpoint(chain_id))?;
            log::info!("checkpoint: chain {chain_id} served by {url}");
            endpoints.remember(chain_id, &url);
            url
        },
    };

    match latest_block(&url).await {
        Ok(latest) => {
            let vote = vote_block(latest, &params_for(chain_id));
            log::debug!("checkpoint: chain {chain_id} head {latest} -> vote {vote} via {url}");
            Ok(vote)
        },
        Err(e) => {
            // Drop the cached choice so the next request probes again.
            endpoints.forget(chain_id);
            Err(BlockError::Other(e.context(format!("eth_blockNumber on {url}"))))
        },
    }
}

/// First endpoint, list by list, whose `probe` reports `chain_id`. Failures are skipped.
pub async fn select_endpoint<F, Fut>(
    candidates: &[Vec<String>],
    chain_id: u64,
    probe: F,
) -> Option<String>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<u64>>,
{
    for url in candidates.iter().flatten() {
        match probe(url.clone()).await {
            Ok(id) if id == chain_id => return Some(url.clone()),
            Ok(id) => log::debug!("checkpoint: {url} is chain {id}, not {chain_id}"),
            Err(e) => log::debug!("checkpoint: {url} did not answer eth_chainId: {e:#}"),
        }
    }
    None
}

async fn probe_chain_id(url: String) -> Result<u64> {
    EvmClient::new_http(&url)?.chain_id().await.context("eth_chainId")
}

async fn latest_block(url: &str) -> Result<u64> {
    EvmClient::new_http(url)
        .map_err(|e| anyhow!("{e:#}"))?
        .block_number()
        .await
        .context("eth_blockNumber")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn votes_are_rounded_down_after_confirmations() {
        // Ethereum: 10 confirmations, interval 50.
        assert_eq!(vote_block(26_178_045, &ETHEREUM), 26_178_000);
        assert_eq!(vote_block(26_178_060, &ETHEREUM), 26_178_050, "boundary crossed");
        assert_eq!(vote_block(26_178_059, &ETHEREUM), 26_178_000, "just before the boundary");
        // Base: 10 confirmations, interval 150.
        assert_eq!(vote_block(40_000_210, &BASE), 40_000_200);
        assert_eq!(vote_block(40_000_209, &BASE), 40_000_050, "just before the boundary");
        // Hardhat: 2 confirmations, interval 10.
        assert_eq!(vote_block(57, &HARDHAT), 50);
        assert_eq!(params_for(31337), HARDHAT);
        assert_eq!(params_for(424242), DEFAULT);
    }

    /// The floor read requires the pinned block to be `confirmations` deep. Once the head is
    /// past that depth every vote already is, so the read does not wait on an agreed checkpoint.
    /// A chain no longer than `confirmations` still votes block 1, which is not deep enough: the
    /// floor read waits until the chain grows (a local Hardhat chain needs interval mining).
    #[test]
    fn votes_are_as_deep_as_the_floor_read_requires_once_the_chain_is() {
        for params in [ETHEREUM, BASE, HARDHAT] {
            let start = params.confirmations + 1;
            for latest in start..start + 3 * params.pin_interval {
                let vote = vote_block(latest, &params) as u64;
                assert!(
                    latest - vote >= params.confirmations,
                    "{params:?} head {latest} voted {vote}"
                );
            }
            // At or below the depth the vote is block 1, and that is short of the depth: the
            // floor read waits.
            for latest in 1..=params.confirmations {
                let vote = vote_block(latest, &params) as u64;
                assert_eq!(vote, 1, "{params:?} head {latest}");
                assert!(
                    latest - vote < params.confirmations,
                    "{params:?} head {latest}: block 1 would already be deep enough"
                );
            }
        }
        assert_eq!(vote_block(2, &HARDHAT), 1, "head 2: one block deep, the read waits");
        assert_eq!(vote_block(5, &ETHEREUM), 1, "head 5: four blocks deep, the read waits");
    }

    #[test]
    fn short_chains_never_vote_zero() {
        assert_eq!(vote_block(0, &HARDHAT), 1);
        assert_eq!(vote_block(1, &HARDHAT), 1);
        assert_eq!(vote_block(2, &HARDHAT), 1);
        assert_eq!(vote_block(5, &HARDHAT), 3, "latest - confirmations below one interval");
        assert_eq!(vote_block(11, &HARDHAT), 9);
        assert_eq!(vote_block(12, &HARDHAT), 10, "first full interval");
        assert_eq!(vote_block(30, &ETHEREUM), 20);
        assert_eq!(vote_block(u64::MAX, &ETHEREUM), u32::MAX);
    }

    #[tokio::test]
    async fn the_first_endpoint_reporting_the_chain_wins() {
        let candidates = vec![
            vec!["eth-a".to_string(), "eth-b".to_string()],
            vec!["base-a".to_string()],
            vec!["dead".to_string(), "hardhat".to_string()],
        ];
        let probe = |url: String| async move {
            match url.as_str() {
                "eth-a" | "eth-b" => Ok(1),
                "base-a" => Ok(8453),
                "hardhat" => Ok(31337),
                _ => Err(anyhow!("connection refused")),
            }
        };
        assert_eq!(select_endpoint(&candidates, 1, probe).await.as_deref(), Some("eth-a"));
        assert_eq!(select_endpoint(&candidates, 8453, probe).await.as_deref(), Some("base-a"));
        assert_eq!(select_endpoint(&candidates, 31337, probe).await.as_deref(), Some("hardhat"));
        assert_eq!(select_endpoint(&candidates, 11155111, probe).await, None);
    }

    #[tokio::test]
    async fn an_unknown_chain_is_a_not_found_error() {
        let endpoints = Endpoints::new(vec![]);
        match block(&endpoints, 11155111).await {
            Err(BlockError::NoEndpoint(11155111)) => {},
            other => panic!("expected NoEndpoint, got {other:?}"),
        }
    }
}
