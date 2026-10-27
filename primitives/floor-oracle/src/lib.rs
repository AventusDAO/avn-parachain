// Copyright 2026 Aventus DAO Ltd

//! Types shared by the watchtower floor oracle pallet (`no_std`) and the node side external
//! service (`std`): what a watchtower observes on each channel (the Punks floor, and the raw
//! state of the Exotic perpetuals venue) and what a finalised round looks like.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_core::{H160, H256};
use sp_io::hashing::blake2_256;

/// Signing context of a floor oracle report submitted by a watchtower.
pub const FLOOR_ORACLE_REPORT_CONTEXT: &[u8] = b"wt_floor_oracle_report";

/// External service path serving an observation. The full request is
/// `floor/punks/{chain_id}/{pinned_block}/{punks}`: the chain and block on-chain data is read
/// at and the CryptoPunksMarket contract, as 40 lower-case hex characters without `0x`. The
/// pallet builds it in `floor_service_path`.
pub const FLOOR_SERVICE_PATH: &str = "floor/punks";

/// The Punks market a floor round reads: the EVM chain it is deployed on and the
/// CryptoPunksMarket contract. Root sets it; every round snapshots it so a change cannot
/// split the reports of a running round.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Default,
)]
pub struct FloorMarket {
    /// EVM chain id: 1 for Ethereum mainnet, 11155111 for Sepolia.
    pub chain_id: u64,
    /// The CryptoPunksMarket contract (`0xb47e3cd837dDF8e4c57F05d70Ab865de6e193BBB` on mainnet).
    pub punks: H160,
}

/// `FloorObservation::flags` bits.
pub const FLAG_FLOOR_FAR_BELOW_SALES: u32 = 1 << 0;
pub const FLAG_SALES_UNAVAILABLE: u32 = 1 << 1;
pub const FLAG_OFFCHAIN_UNAVAILABLE: u32 = 1 << 2;

/// One watchtower's view of the Punks floor at a pinned point in time.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Default,
)]
pub struct FloorObservation {
    /// Ethereum block the on-chain data was read at.
    pub observed_eth_block: u64,
    /// Lowest ask, in wei.
    pub floor_wei: u128,
    /// Unix timestamp of the last sale. Zero when unavailable.
    pub last_sale_ts: u64,
    /// Price of the last sale, in wei. Zero when unavailable.
    pub last_sale_wei: u128,
    /// `FLAG_*` bits.
    pub flags: u32,
}

impl FloorObservation {
    /// Hash of the SCALE encoding. Reports agree when their hashes match.
    pub fn hash(&self) -> H256 {
        H256(blake2_256(&self.encode()))
    }

    /// Sanity checks an observation must pass before it can take part in a round.
    pub fn is_plausible(&self) -> bool {
        self.floor_wei > 0 && self.observed_eth_block > 0
    }
}

/// The outcome of a floor oracle round once enough watchtowers agreed.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub struct FinalisedFloorRound {
    pub round_id: u64,
    /// The watchtower proposal the round was decided through.
    pub proposal_id: H256,
    /// Unix time, in seconds, of the checkpoint every watchtower observed the floor at. The
    /// floor's data is as old as this.
    pub checkpoint_time_sec: u64,
    /// The market the round read.
    pub market: FloorMarket,
    /// The observation the round settled on.
    pub observation: FloorObservation,
    /// Watchtowers that reported this exact observation.
    pub agreeing: u32,
    /// Watchtowers that reported at all.
    pub reports: u32,
    /// Agreeing reports needed to finalise.
    pub required: u32,
    pub finalised_at_block: u32,
    /// Unix time, in seconds, the round finalised at.
    pub finalised_at_sec: u64,
}

// ---------------------------------------------------------------------- venue channel

/// Signing context of a venue oracle report submitted by a watchtower.
pub const VENUE_ORACLE_REPORT_CONTEXT: &[u8] = b"wt_venue_oracle_report";

/// External service path serving a venue observation. The full request is
/// `venue/exotic/{chain_id}/{target_ts}/{clearing_house}/{amm}`, both addresses as 40
/// lower-case hex characters without `0x`. The service reads at the latest block whose
/// timestamp is at most `target_ts`. The pallet builds it in `venue_service_path`.
pub const VENUE_SERVICE_PATH: &str = "venue/exotic";

/// The Exotic perpetuals market a venue round reads: the EVM chain it is deployed on and the
/// `ClearingHouse` and market `Amm` contracts. Root sets it; every round snapshots it so a
/// change cannot split the reports of a running round.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Default,
)]
pub struct VenueMarket {
    /// EVM chain id: 8453 for Base mainnet, 31337 for a local Hardhat chain.
    pub chain_id: u64,
    pub clearing_house: H160,
    pub amm: H160,
    /// An observation must come from a block less than this many seconds before the round's
    /// target timestamp. Keep it just above the chain's block time: below it the right block
    /// can fall outside the window, and every extra second makes the finalised state older.
    /// 6 for Base (2s blocks), 15 for Ethereum or a chain mined every 12s.
    pub target_tolerance_secs: u64,
}

/// One watchtower's view of the Exotic market at the block it resolved for a round's target
/// timestamp. Raw contract return values only; the consumer derives anything else off-chain.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Default,
)]
pub struct VenueObservation {
    /// The venue chain block every value was read at: the latest one whose timestamp is at most
    /// the round's target timestamp.
    pub pinned_block: u64,
    /// That block's timestamp, in unix seconds, from `eth_getBlockByNumber`.
    pub pinned_ts: u64,
    /// `Amm.getSpotPrice()`, 18 decimals.
    pub spot_e18: u128,
    /// `Amm.getUnderlyingPrice()`, 18 decimals.
    pub oracle_e18: u128,
    /// `ClearingHouse.openInterestNotional(amm)`, 18 decimals.
    pub oi_notional_e18: u128,
}

impl VenueObservation {
    /// Hash of the SCALE encoding. Reports agree when their hashes match.
    pub fn hash(&self) -> H256 {
        H256(blake2_256(&self.encode()))
    }

    /// Sanity checks an observation must pass before it can take part in a round. The spot
    /// price is never zero on the contract side (it reverts instead); the oracle price and the
    /// open interest are raw values and may legitimately be zero.
    pub fn is_plausible(&self) -> bool {
        self.pinned_block > 0 && self.pinned_ts > 0 && self.spot_e18 > 0
    }
}

/// The outcome of a venue oracle round once enough watchtowers agreed.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub struct FinalisedVenueRound {
    pub round_id: u64,
    /// The watchtower proposal the round was decided through.
    pub proposal_id: H256,
    /// The market the round read.
    pub market: VenueMarket,
    /// The observation the round settled on.
    pub observation: VenueObservation,
    /// Watchtowers that reported this exact observation.
    pub agreeing: u32,
    /// Watchtowers that reported at all.
    pub reports: u32,
    /// Agreeing reports needed to finalise.
    pub required: u32,
    pub finalised_at_block: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plausible() -> FloorObservation {
        FloorObservation {
            observed_eth_block: 20_000_000,
            floor_wei: 50_000_000_000_000_000_000,
            last_sale_ts: 1_700_000_000,
            last_sale_wei: 49_000_000_000_000_000_000,
            flags: 0,
        }
    }

    #[test]
    fn plausible_observation_passes() {
        assert!(plausible().is_plausible());
        // Sales data is optional.
        let mut obs = plausible();
        obs.last_sale_ts = 0;
        obs.last_sale_wei = 0;
        obs.flags = FLAG_SALES_UNAVAILABLE;
        assert!(obs.is_plausible());
    }

    #[test]
    fn implausible_observations_fail() {
        let mut obs = plausible();
        obs.floor_wei = 0;
        assert!(!obs.is_plausible(), "zero floor");

        let mut obs = plausible();
        obs.observed_eth_block = 0;
        assert!(!obs.is_plausible(), "no block");

        assert!(!FloorObservation::default().is_plausible());
    }

    #[test]
    fn encoded_lengths() {
        // u64 + u128 + u64 + u128 + u32
        assert_eq!(plausible().encode().len(), 52);
        assert_eq!(FloorObservation::max_encoded_len(), 52);
    }

    #[test]
    fn observation_round_trip() {
        let obs = plausible();
        assert_eq!(FloorObservation::decode(&mut &obs.encode()[..]).unwrap(), obs);
    }

    #[test]
    fn hash_is_stable_and_sensitive() {
        let obs = plausible();
        assert_eq!(obs.hash(), plausible().hash());
        assert_eq!(obs.hash(), H256(blake2_256(&obs.encode())));

        let mut changed = plausible();
        changed.floor_wei += 1;
        assert_ne!(obs.hash(), changed.hash());

        let mut other_flags = plausible();
        other_flags.flags = FLAG_OFFCHAIN_UNAVAILABLE;
        assert_ne!(obs.hash(), other_flags.hash());
    }

    #[test]
    fn flag_bits_are_distinct() {
        let flags = [FLAG_FLOOR_FAR_BELOW_SALES, FLAG_SALES_UNAVAILABLE, FLAG_OFFCHAIN_UNAVAILABLE];
        assert_eq!(flags.iter().fold(0, |acc, f| acc | f).count_ones(), flags.len() as u32);
        assert_eq!(FLOOR_SERVICE_PATH, "floor/punks");
    }

    // ------------------------------------------------------------------ venue

    fn venue() -> VenueObservation {
        VenueObservation {
            pinned_block: 40_000_000,
            pinned_ts: 1_750_000_000,
            spot_e18: 51_000_000_000_000_000_000,
            oracle_e18: 50_000_000_000_000_000_000,
            oi_notional_e18: 1_000_000_000_000_000_000_000,
        }
    }

    #[test]
    fn venue_plausibility() {
        assert!(venue().is_plausible());
        assert!(VenueObservation { oracle_e18: 0, oi_notional_e18: 0, ..venue() }.is_plausible());
        assert!(!VenueObservation { pinned_block: 0, ..venue() }.is_plausible());
        assert!(!VenueObservation { pinned_ts: 0, ..venue() }.is_plausible());
        assert!(!VenueObservation { spot_e18: 0, ..venue() }.is_plausible());
        assert!(!VenueObservation::default().is_plausible());
    }

    #[test]
    fn venue_encoded_lengths() {
        // u64 + u64 + u128 + u128 + u128
        assert_eq!(venue().encode().len(), 64);
        assert_eq!(VenueObservation::max_encoded_len(), 64);
        // u64 + H160 + H160
        assert_eq!(VenueMarket::max_encoded_len(), 56);
        assert_eq!(FloorMarket::max_encoded_len(), 28);
        assert_eq!(VenueObservation::decode(&mut &venue().encode()[..]).unwrap(), venue());
    }

    #[test]
    fn venue_hash_is_stable_and_sensitive() {
        assert_eq!(venue().hash(), H256(blake2_256(&venue().encode())));
        assert_ne!(venue().hash(), VenueObservation { spot_e18: 1, ..venue() }.hash());
        assert_ne!(venue().hash(), VenueObservation { pinned_ts: 1, ..venue() }.hash());
        assert_eq!(VENUE_SERVICE_PATH, "venue/exotic");
    }
}
