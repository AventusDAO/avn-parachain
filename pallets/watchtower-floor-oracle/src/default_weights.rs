//! Weights for pallet_watchtower_floor_oracle
//!
//! THESE VALUES ARE PLACEHOLDERS, WRITTEN BY HAND IN THE FORMAT OF THE SUBSTRATE BENCHMARK CLI.
//! Regenerate with the benchmark CLI before release:
//!
//! ./avn-parachain-collator benchmark pallet --chain dev --wasm-execution=compiled
//!   --template frame-weight-template.hbs --pallet pallet_watchtower_floor_oracle
//!   --extrinsic '*' --steps 50 --repeat 20 --output watchtower_floor_oracle_weights.rs

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for pallet_watchtower_floor_oracle.
pub trait WeightInfo {
	fn seed_chain() -> Weight;
	fn request_checkpoint() -> Weight;
	fn submit_checkpoint_vote(v: u32, ) -> Weight;
	fn submit_checkpoint_vote_with_quorum(v: u32, ) -> Weight;
}

/// Weights for pallet_watchtower_floor_oracle using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `WatchtowerFloorOracle::MaxCheckpointAge` (r:0 w:1)
	/// Storage: `WatchtowerFloorOracle::Checkpoints` (r:1 w:1)
	/// Storage: `Timestamp::Now` (r:1 w:0)
	fn seed_chain() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `3493`
		// Minimum execution time: 20_000_000 picoseconds.
		Weight::from_parts(21_000_000, 3493)
			.saturating_add(T::DbWeight::get().reads(2_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
	/// Storage: `WatchtowerFloorOracle::MaxCheckpointAge` (r:1 w:0)
	/// Storage: `WatchtowerFloorOracle::Checkpoints` (r:1 w:0)
	/// Storage: `WatchtowerFloorOracle::CheckpointRound` (r:1 w:1)
	/// Storage: `WatchtowerFloorOracle::NextRoundId` (r:1 w:1)
	/// Storage: `Timestamp::Now` (r:1 w:0)
	fn request_checkpoint() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `200`
		//  Estimated: `10774`
		// Minimum execution time: 30_000_000 picoseconds.
		Weight::from_parts(31_000_000, 10774)
			.saturating_add(T::DbWeight::get().reads(5_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
	/// Storage: `WatchtowerFloorOracle::CheckpointRound` (r:1 w:1)
	/// Storage: `Avn::Validators` (r:1 w:0)
	/// The range of component `v` is `[3, 10]`.
	fn submit_checkpoint_vote(v: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `300 + v * (40 ±0)`
		//  Estimated: `10774`
		// Minimum execution time: 30_000_000 picoseconds.
		Weight::from_parts(31_000_000, 10774)
			// Standard Error: 10_000
			.saturating_add(Weight::from_parts(500_000, 0).saturating_mul(v.into()))
			.saturating_add(T::DbWeight::get().reads(2_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `WatchtowerFloorOracle::CheckpointRound` (r:1 w:1)
	/// Storage: `Avn::Validators` (r:1 w:0)
	/// Storage: `Timestamp::Now` (r:1 w:0)
	/// Storage: `WatchtowerFloorOracle::Checkpoints` (r:0 w:1)
	/// The range of component `v` is `[3, 10]`.
	fn submit_checkpoint_vote_with_quorum(v: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `300 + v * (40 ±0)`
		//  Estimated: `10774`
		// Minimum execution time: 40_000_000 picoseconds.
		Weight::from_parts(41_000_000, 10774)
			// Standard Error: 10_000
			.saturating_add(Weight::from_parts(500_000, 0).saturating_mul(v.into()))
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	fn seed_chain() -> Weight {
		Weight::from_parts(21_000_000, 3493)
			.saturating_add(RocksDbWeight::get().reads(2_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
	fn request_checkpoint() -> Weight {
		Weight::from_parts(31_000_000, 10774)
			.saturating_add(RocksDbWeight::get().reads(5_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
	fn submit_checkpoint_vote(v: u32, ) -> Weight {
		Weight::from_parts(31_000_000, 10774)
			.saturating_add(Weight::from_parts(500_000, 0).saturating_mul(v.into()))
			.saturating_add(RocksDbWeight::get().reads(2_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn submit_checkpoint_vote_with_quorum(v: u32, ) -> Weight {
		Weight::from_parts(41_000_000, 10774)
			.saturating_add(Weight::from_parts(500_000, 0).saturating_mul(v.into()))
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
}
