// Copyright 2026 Aventus DAO Ltd

//! bridge-watchtower pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite};
use frame_system::RawOrigin;

benchmarks! {
    freeze_lower {
        let lower_id: LowerId = 1;
    }: _(RawOrigin::Root, lower_id)
    verify {
        let tx_id = LowerIdToFreezeTxId::<T>::get(lower_id).expect("freeze is requested");
        assert_eq!(FreezeTxIdToLowerId::<T>::get(tx_id), Some(lower_id));
    }

    toggle_lift_checks {
    }: _(RawOrigin::Root, true)
    verify {
        assert!(LiftCheckingEnabled::<T>::get());
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().as_externality(),
    crate::mock::TestRuntime,
);
