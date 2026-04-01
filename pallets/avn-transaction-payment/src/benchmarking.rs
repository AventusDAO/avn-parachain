//! # avn-transaction-payment benchmarking
// Copyright 2023 Aventus Network Systems (UK) Ltd.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_system::{EventRecord, RawOrigin};
use sp_runtime::traits::Bounded;

use crate::Pallet as AvnTransactionPayment;

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1 as usize)];
    assert_eq!(event, &system_event);
}

fn add_known_sender<T: Config>(known_sender: &T::AccountId) {
    let adjustment_config =
        FeeAdjustmentConfig::FixedFee(FixedFeeConfig { fee: BalanceOf::<T>::max_value() });
    <KnownSenders<T>>::insert(known_sender, adjustment_config);
}

benchmarks! {
    set_known_sender {
        let known_sender: T::AccountId = account("known_sender", 1, 1);

        let adjustment_input = AdjustmentInput::<T> {
            fee_type: FeeType::FixedFee(FixedFeeConfig {
                fee: BalanceOf::<T>::max_value(),
            }),
            adjustment_type: AdjustmentType::None,
        };

        let adjustment_config = FeeAdjustmentConfig::FixedFee(FixedFeeConfig {
            fee: BalanceOf::<T>::max_value(),
        });
    }: {
        AvnTransactionPayment::<T>::set_known_sender(RawOrigin::Root.into(), known_sender.clone(), adjustment_input)?;
    }
    verify {
        assert_eq!(<KnownSenders<T>>::contains_key(&known_sender), true);
        assert_last_event::<T>(
            Event::<T>::KnownSenderAdded{ known_sender, adjustment: adjustment_config }.into()
        );
    }

    remove_known_sender {
        let known_sender: T::AccountId = account("known_sender", 1, 1);
        add_known_sender::<T>(&known_sender);
    }: {
        AvnTransactionPayment::<T>::remove_known_sender(RawOrigin::Root.into(), known_sender.clone())?;
    }
    verify {
        assert_eq!(<KnownSenders<T>>::contains_key(&known_sender), false);
        assert_last_event::<T>(
            Event::<T>::KnownSenderRemoved{ known_sender }.into()
        );
    }

        set_base_gas_fee_usd {
        let base_fee: u128 = 25_000_000u128;
    }: {
        AvnTransactionPayment::<T>::set_base_gas_fee_usd(
            RawOrigin::Root.into(),
            base_fee
        )?;
    }
    verify {
        assert_eq!(BaseGasFeeUsd::<T>::get(), base_fee);
        assert_last_event::<T>(
            Event::<T>::BaseGasFeeUsdSet { new_base_gas_fee: base_fee }.into()
        );
    }
}

impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::TestRuntime,);
