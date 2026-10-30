#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{dispatch::DispatchResult, pallet_prelude::*};
use frame_system::pallet_prelude::*;

use pallet_avn::BridgeInterfaceNotification;
use pallet_watchtower::Proposal;
use sp_avn_common::{
    eth::EthereumId,
    event_types::{EthEvent, EthTransactionId, EventData, LowerLockedData},
};
use sp_core::{H160, H256, U256};
use sp_std::prelude::*;
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

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A lower was claimed on T1 and its funds are held until its unlock time.
        LowerLockDetected {
            lower_id: u32,
            t1_recipient: H160,
            token_contract: H160,
            amount: U256,
            t2_sender: H256,
            unlock_time: u64,
            eth_tx_hash: EthTransactionId,
        },
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {}

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    impl<T: Config> Pallet<T> {
        fn process_lower_locked(eth_tx_hash: EthTransactionId, data: &LowerLockedData) {
            Self::deposit_event(Event::<T>::LowerLockDetected {
                lower_id: data.lower_id,
                t1_recipient: data.t1_recipient,
                token_contract: data.token_contract,
                amount: data.amount,
                t2_sender: data.t2_sender,
                unlock_time: data.unlock_time,
                eth_tx_hash,
            });
        }
    }

    impl<T: Config> BridgeInterfaceNotification for Pallet<T> {
        fn process_result(
            _tx_id: EthereumId,
            _caller_id: Vec<u8>,
            _succeeded: bool,
        ) -> DispatchResult {
            Ok(())
        }

        // Never fails: by now the event is accepted, so an error would only stop the
        // notification reaching the remaining consumers.
        fn on_incoming_event_processed(event: &EthEvent) -> DispatchResult {
            if let EventData::LogLowerLocked(data) = &event.event_data {
                Self::process_lower_locked(event.event_id.transaction_hash, data);
            }

            Ok(())
        }
    }

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
