#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use frame_support::{dispatch::DispatchResult, pallet_prelude::*, storage::with_storage_layer};
use frame_system::pallet_prelude::*;

use pallet_avn::{BridgeInterface, BridgeInterfaceNotification};
use pallet_watchtower::Proposal;
use sp_avn_common::{
    eth::{lower_params_match, EthereumId},
    event_types::{EthEvent, EthTransactionId, EventData, LowerLockedData},
    BridgeContractMethod, LowerDataProvider, UINT32,
};
use sp_core::{H160, H256, U256};
use sp_std::prelude::*;
use sp_watchtower::*;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);
pub const PALLET_ID: &'static [u8; 17] = b"bridge_watchtower";

pub type LowerId = u32;

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

mod benchmarking;

pub mod default_weights;
pub use default_weights::WeightInfo;

pub use pallet::*;
#[frame_support::pallet]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_watchtower::Config {
        /// Sends the freeze transactions to Ethereum.
        type BridgeInterface: BridgeInterface;
        /// The lowers T2 issued, against which locked lowers are checked.
        type LowerDataProvider: LowerDataProvider;
        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    /// The freeze transaction being checked.
    #[pallet::storage]
    pub type LowerIdToFreezeTxId<T: Config> =
        StorageMap<_, Blake2_128Concat, LowerId, EthereumId, OptionQuery>;

    /// The freeze transaction sent to Ethereum.
    #[pallet::storage]
    pub type FreezeTxIdToLowerId<T: Config> =
        StorageMap<_, Blake2_128Concat, EthereumId, LowerId, OptionQuery>;

    /// Lowers that should be frozen but whose freeze could not be sent or failed on Ethereum.
    /// Cleared once a freeze succeeds.
    #[pallet::storage]
    pub type FailedFreezes<T: Config> = StorageMap<_, Blake2_128Concat, LowerId, (), OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A lower locked on T1 matches the lower T2 issued.
        LowerLockVerified { lower_id: LowerId, eth_tx_hash: EthTransactionId },
        /// A lower locked on T1 does not match any lower T2 issued and ready to be claimed.
        LowerLockMismatch {
            lower_id: LowerId,
            t1_recipient: H160,
            token_contract: H160,
            amount: U256,
            t2_sender: H256,
            unlock_time: u64,
            eth_tx_hash: EthTransactionId,
        },
        /// A transaction to freeze a lower on T1 was sent.
        LowerFreezeRequested { lower_id: LowerId, tx_id: EthereumId },
        /// A transaction to freeze a lower on T1 could not be sent.
        LowerFreezePublishFailed { lower_id: LowerId },
        /// A lower was frozen on T1.
        LowerFrozen { lower_id: LowerId, tx_id: EthereumId },
        /// The transaction to freeze a lower failed on T1.
        LowerFreezeFailed { lower_id: LowerId, tx_id: EthereumId },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// A freeze transaction is already in flight for this lower.
        FreezeAlreadyRequested,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Sends a transaction to freeze a lower locked on T1, e.g. to retry a failed freeze.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::freeze_lower())]
        pub fn freeze_lower(origin: OriginFor<T>, lower_id: LowerId) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                !LowerIdToFreezeTxId::<T>::contains_key(lower_id),
                Error::<T>::FreezeAlreadyRequested
            );

            Self::request_freeze(lower_id)?;
            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    impl<T: Config> Pallet<T> {
        fn process_lower_locked(eth_tx_hash: EthTransactionId, data: &LowerLockedData) {
            if Self::lower_lock_matches(data) {
                Self::deposit_event(Event::<T>::LowerLockVerified {
                    lower_id: data.lower_id,
                    eth_tx_hash,
                });
                return
            }

            Self::deposit_event(Event::<T>::LowerLockMismatch {
                lower_id: data.lower_id,
                t1_recipient: data.t1_recipient,
                token_contract: data.token_contract,
                amount: data.amount,
                t2_sender: data.t2_sender,
                unlock_time: data.unlock_time,
                eth_tx_hash,
            });

            if LowerIdToFreezeTxId::<T>::contains_key(data.lower_id) {
                return
            }

            if let Err(e) = Self::request_freeze(data.lower_id) {
                log::error!(
                    "❌ Failed to send the freeze of mismatched lower {:?}: {:?}",
                    data.lower_id,
                    e
                );
                FailedFreezes::<T>::insert(data.lower_id, ());
                Self::deposit_event(Event::<T>::LowerFreezePublishFailed {
                    lower_id: data.lower_id,
                });
            }
        }

        /// Whether a locked lower matches the lower T2 issued with the same id.
        fn lower_lock_matches(data: &LowerLockedData) -> bool {
            let Some(params) = T::LowerDataProvider::ready_to_claim_lower_params(data.lower_id)
            else {
                return false
            };
            // No T2 lower holds more than a u128
            let Ok(amount) = u128::try_from(data.amount) else { return false };

            lower_params_match(
                &params,
                &data.token_contract,
                amount,
                &data.t1_recipient,
                &data.t2_sender,
            )
        }

        fn request_freeze(lower_id: LowerId) -> Result<EthereumId, DispatchError> {
            let params = vec![(UINT32.to_vec(), lower_id.to_string().into_bytes())];
            // Publishing can fail after writing to storage, which the event hook does not revert
            let tx_id = with_storage_layer(|| {
                T::BridgeInterface::publish(
                    BridgeContractMethod::FreezeLower.name_as_bytes(),
                    &params,
                    PALLET_ID.to_vec(),
                )
            })?;

            LowerIdToFreezeTxId::<T>::insert(lower_id, tx_id);
            FreezeTxIdToLowerId::<T>::insert(tx_id, lower_id);
            Self::deposit_event(Event::<T>::LowerFreezeRequested { lower_id, tx_id });

            Ok(tx_id)
        }
    }

    impl<T: Config> BridgeInterfaceNotification for Pallet<T> {
        fn process_result(
            tx_id: EthereumId,
            caller_id: Vec<u8>,
            succeeded: bool,
        ) -> DispatchResult {
            if caller_id != PALLET_ID.to_vec() {
                return Ok(())
            }

            let Some(lower_id) = FreezeTxIdToLowerId::<T>::take(tx_id) else { return Ok(()) };
            LowerIdToFreezeTxId::<T>::remove(lower_id);

            if succeeded {
                FailedFreezes::<T>::remove(lower_id);
                Self::deposit_event(Event::<T>::LowerFrozen { lower_id, tx_id });
            } else {
                FailedFreezes::<T>::insert(lower_id, ());
                Self::deposit_event(Event::<T>::LowerFreezeFailed { lower_id, tx_id });
            }

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
