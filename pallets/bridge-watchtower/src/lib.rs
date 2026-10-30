#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{dispatch::DispatchResult, pallet_prelude::*};
use frame_system::pallet_prelude::*;

use pallet_watchtower::Proposal;
use sp_core::H256;
use sp_watchtower::*;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

pub use pallet::*;
#[frame_support::pallet]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_watchtower::Config {}

    #[pallet::call]
    impl<T: Config> Pallet<T> {}

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    impl<T: Config> WatchtowerHooks<Proposal<T>> for Pallet<T> {
        fn on_proposal_submitted(
            _proposal_id: ProposalId,
            _proposal: Proposal<T>,
        ) -> DispatchResult {
            Ok(())
        }

        fn on_voting_completed(
            _proposal_id: ProposalId,
            _external_ref: &H256,
            _result: &ProposalStatusEnum,
        ) {
        }

        fn on_cancelled(_proposal_id: ProposalId, _external_ref: &H256) {}
    }
}
