//! Weights for pallet_bridge_watchtower
//!
//! PLACEHOLDER: these are estimates, not benchmark results. Regenerate them with the benchmark
//! CLI (see the other pallets' `default_weights.rs` for the command) before release.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for pallet_bridge_watchtower.
pub trait WeightInfo {
	fn freeze_lower() -> Weight;
}

/// Weights for pallet_bridge_watchtower using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `BridgeWatchtower::LowerIdToFreezeTxId` (r:1 w:1)
	/// Storage: `EthBridge` request and transaction state (r:6 w:3)
	/// Storage: `BridgeWatchtower::FreezeTxIdToLowerId` (r:0 w:1)
	fn freeze_lower() -> Weight {
		Weight::from_parts(60_000_000, 8_000)
			.saturating_add(T::DbWeight::get().reads(7_u64))
			.saturating_add(T::DbWeight::get().writes(5_u64))
	}
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	/// Storage: `BridgeWatchtower::LowerIdToFreezeTxId` (r:1 w:1)
	/// Storage: `EthBridge` request and transaction state (r:6 w:3)
	/// Storage: `BridgeWatchtower::FreezeTxIdToLowerId` (r:0 w:1)
	fn freeze_lower() -> Weight {
		Weight::from_parts(60_000_000, 8_000)
			.saturating_add(RocksDbWeight::get().reads(7_u64))
			.saturating_add(RocksDbWeight::get().writes(5_u64))
	}
}
