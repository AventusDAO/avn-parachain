//! # avn pallet
// Copyright 2023 Aventus Systems (UK) Ltd.

//! avn pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;

use frame_benchmarking::{benchmarks, impl_benchmark_test_suite};
use frame_system::{EventRecord, RawOrigin};

fn assert_last_event<T: Config>(generic_event: <T as frame_system::Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1 as usize)];
    assert_eq!(event, &system_event);
}

benchmarks! {
    set_bridge_contract {
        let contract_address = H160::from([1; 20]);
        let old_contract = <AvnBridgeContractAddress<T>>::get();
    }: set_bridge_contract(RawOrigin::Root, contract_address.clone())
    verify {
        assert!(<AvnBridgeContractAddress<T>>::get() == contract_address);
        assert_last_event::<T>(Event::AvnBridgeContractUpdated {
            old_contract: old_contract,
            new_contract: contract_address,
            }.into()
        );
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().as_externality(),
    crate::mock::TestRuntime,
);
