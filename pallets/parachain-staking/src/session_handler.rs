#![cfg_attr(not(feature = "std"), no_std)]

// use super::Config;
use crate::{Config, Era, ForceNewEra, Pallet as ParachainStaking};
use frame_support::{
    dispatch::DispatchClass,
    pallet_prelude::Weight,
    traits::{EstimateNextSessionRotation, Get},
};
use frame_system::pallet_prelude::BlockNumberFor;
use pallet_session::{self as session, ShouldEndSession};
use sp_runtime::{traits::Saturating, Permill};
use sp_std::prelude::*;

/// Provides the new set of collator_account_ids to the session module when a session is being
/// rotated (ended).
impl<T: Config> session::SessionManager<T::AccountId> for ParachainStaking<T> {
    fn new_session(new_index: u32) -> Option<Vec<T::AccountId>> {
        let collators = ParachainStaking::<T>::selected_candidates().to_vec();

        if collators.is_empty() {
            // we never want to pass an empty set of collators. This would brick the chain.
            log::error!("💥 keeping old session because of empty collator set!");

            None
        } else {
            log::debug!(
                "[AVN] assembling new collators for new session {} with these validators {:#?} at #{:?}",
                new_index,
                collators,
                <frame_system::Pallet<T>>::block_number(),
            );

            Some(collators)
        }
    }

    fn end_session(_end_index: u32) {
        // nothing to do here
    }

    fn start_session(_start_index: u32) {
        // nothing to do here
    }
}

impl<T: Config> ShouldEndSession<BlockNumberFor<T>> for ParachainStaking<T> {
    fn should_end_session(now: BlockNumberFor<T>) -> bool {
        frame_system::Pallet::<T>::register_extra_weight_unchecked(
            T::DbWeight::get().reads(2),
            DispatchClass::Mandatory,
        );

        let era = <Era<T>>::get();

        // always update when a new era should start
        if era.should_update(now) {
            return true;
        }

        if <ForceNewEra<T>>::get() {
            // reset storage value
            <ForceNewEra<T>>::put(false);

            let (_, mut weight) = ParachainStaking::<T>::start_new_era(now, era);
            weight = weight.saturating_add(T::DbWeight::get().writes(1));

            frame_system::Pallet::<T>::register_extra_weight_unchecked(
                weight,
                DispatchClass::Mandatory,
            );

            return true;
        } else {
            return false;
        }
    }
}

impl<T: Config> EstimateNextSessionRotation<BlockNumberFor<T>> for ParachainStaking<T> {
    fn average_session_length() -> BlockNumberFor<T> {
        <Era<T>>::get().length.into()
    }

    fn estimate_current_session_progress(now: BlockNumberFor<T>) -> (Option<Permill>, Weight) {
        let era = <Era<T>>::get();
        let passed_blocks = now.saturating_sub(era.first);

        (
            Some(Permill::from_rational(passed_blocks, era.length.into())),
            // One read for the era info, blocknumber is a free read
            T::DbWeight::get().reads(1),
        )
    }

    fn estimate_next_session_rotation(
        _now: BlockNumberFor<T>,
    ) -> (Option<BlockNumberFor<T>>, Weight) {
        let era = <Era<T>>::get();

        (
            Some(era.first + era.length.into()),
            // One read for the era info, blocknumber is a free read
            T::DbWeight::get().reads(1),
        )
    }
}
