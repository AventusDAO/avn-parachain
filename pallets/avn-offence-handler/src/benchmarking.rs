//! # Avn offence handler pallet
// Copyright 2026 Aventus DAO Ltd

//! avn-offence-handler pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite};
use frame_system::{EventRecord, RawOrigin};

benchmarks! {
    configure_slashing {
        let enabled = true;
    }: _(RawOrigin::Root, enabled)
    verify {
        assert_eq!(<SlashingEnabled<T>>::get(), enabled);
        assert_last_event::<T>(
            Event::<T>::SlashingConfigurationUpdated{ slashing_enabled: enabled }.into()
        );
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::TestExternalitiesBuilder::default().build(|| {}),
    crate::mock::TestRuntime,
);

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1 as usize)];
    assert_eq!(event, &system_event);
}
