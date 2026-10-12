// Copyright 2026 Aventus DAO Ltd

//! Types shared by the watchtower floor oracle pallet (`no_std`) and the node side external
//! service (`std`): what a watchtower observes about the Punks floor, and what a finalised
//! round looks like.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_core::H256;
use sp_io::hashing::blake2_256;

/// Signing context of a floor oracle report submitted by a watchtower.
pub const FLOOR_ORACLE_REPORT_CONTEXT: &[u8] = b"wt_floor_oracle_report";

/// External service path serving an observation. The full request is
/// `floor/punks/{pinned_ts}`.
pub const FLOOR_SERVICE_PATH: &str = "floor/punks";

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
    /// Unix timestamp every watchtower observed the floor at.
    pub pinned_ts: u64,
    /// The observation the round settled on.
    pub observation: FloorObservation,
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
}
