#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
    vec,
};

use frame_support::{dispatch::DispatchResult, pallet_prelude::*, traits::IsType};
use frame_system::{
    offchain::{SendTransactionTypes, SubmitTransaction},
    pallet_prelude::*,
    WeightInfo,
};

use codec::Decode;
use log;
pub use pallet_avn::{self as avn};
use pallet_watchtower::{NodesInterface, Payload, Proposal, WATCHTOWER_UNSIGNED_VOTE_CONTEXT};
pub use sp_avn_common::{
    ocw_lock::{self as OcwLock, OcwStorageError},
    RootId, RootRange,
};
use sp_core::H256;
pub use sp_runtime::{
    offchain::storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
    traits::{AtLeast32Bit, Dispatchable, ValidateUnsigned},
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
        ValidTransaction,
    },
    Perbill, RuntimeAppPublic, Saturating,
};
use sp_std::prelude::*;
use sp_watchtower::*;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);
pub const OC_DB_PREFIX: &[u8] = b"sum_wt::ocw::";
const BLOCK_INCLUSION_PERIOD: u32 = 5;

pub type AVN<T> = avn::Pallet<T>;

pub mod root_utils;

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
    pub trait Config:
        SendTransactionTypes<Call<Self>>
        + frame_system::Config
        + pallet_watchtower::Config
        + pallet_avn::Config
    {
        type RuntimeEvent: From<Event<Self>>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>
            + Clone
            + Eq
            + PartialEq
            + core::fmt::Debug;

        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + From<Call<Self>>;

        /// Weight information for extrinsics in this pallet
        type WeightInfo: WeightInfo;
    }

    #[pallet::storage]
    #[pallet::getter(fn root_info)]
    pub type RootInfo<T: Config> =
        StorageValue<_, (ProposalId, RootData<BlockNumberFor<T>>), OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A summary watchtower proposal was submitted.
        SummaryVerificationRequested {
            proposal_id: ProposalId,
            root_data: RootData<BlockNumberFor<T>>,
        },
        /// A summary watchtower proposal validation was replaced before finalization
        ProposalValidationReplaced { aborted_proposal_id: ProposalId, new_proposal_id: ProposalId },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The summary data in the proposal is invalid.
        InvalidSummaryProposal,
        /// External payloads are not supported.
        ExternalPayloadNotSupported,
        /// Failed to acquire offchain db lock.
        FailedToAcquireOcwDbLock,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {}

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn offchain_worker(now: BlockNumberFor<T>) {
            log::debug!("Watchtower OCW running for block {:?}", now);

            if Self::ocw_already_run(now).is_err() {
                return
            }

            let maybe_node_info = T::Watchtowers::get_node_from_local_signing_keys();
            let (watchtower, signing_key) = match maybe_node_info {
                Some(info) => info,
                None => return,
            };

            if let Some((proposal_id, root_data)) = RootInfo::<T>::get() {
                let finalised_block = AVN::<T>::get_finalised_block_from_external_service();
                if let Ok(finalised_block) = finalised_block {
                    if root_data.root_id.range.to_block > finalised_block {
                        log::debug!(
                            "Root data to_block {:?} is greater than finalised block {:?}, skipping validation for now.",
                            root_data.root_id.range.to_block,
                            finalised_block
                        );
                        return
                    }
                }

                Self::process_pending_validation(
                    proposal_id,
                    root_data,
                    watchtower,
                    signing_key,
                    now,
                );
            }
        }
    }

    impl<T: Config> Pallet<T> {
        fn process_new_proposal(
            _proposer: Option<T::AccountId>,
            proposal_id: ProposalId,
            proposal: Proposal<T>,
        ) -> DispatchResult {
            // Try to decode payload as inline with root data.
            let (root_id, root_hash) = match &proposal.payload {
                Payload::Inline(data) => {
                    match <(RootId<BlockNumberFor<T>>, H256)>::decode(&mut &data[..]) {
                        Ok((root_id, root_hash)) => (root_id, root_hash),
                        Err(_) => return Err(Error::<T>::InvalidSummaryProposal.into()),
                    }
                },
                _ => return Err(Error::<T>::ExternalPayloadNotSupported.into()),
            };

            let current_block = <frame_system::Pallet<T>>::block_number();

            ensure!(
                root_id.range.from_block <= root_id.range.to_block &&
                    root_id.range.to_block <= current_block,
                Error::<T>::InvalidSummaryProposal
            );

            Self::remove_root_if_needed(proposal_id);

            let root_data = RootData::<BlockNumberFor<T>> { root_id: root_id.clone(), root_hash };
            RootInfo::<T>::put((proposal_id, root_data.clone()));
            Self::deposit_event(Event::SummaryVerificationRequested { proposal_id, root_data });

            Ok(())
        }

        fn remove_root_if_needed(new_proposal_id: ProposalId) {
            if let Some((id, data)) = RootInfo::<T>::get() {
                // This should not happen, but if it does, we remove the old proposal
                log::warn!(
                    "Aborting validation of proposal {:?}, root data {:?} due to new proposal.",
                    id,
                    data
                );
                RootInfo::<T>::kill();
                Self::deposit_event(Event::ProposalValidationReplaced {
                    aborted_proposal_id: id,
                    new_proposal_id,
                });
            }
        }

        fn process_pending_validation(
            proposal_id: ProposalId,
            root_data: RootData<BlockNumberFor<T>>,
            watchtower: T::AccountId,
            signing_key: T::SignerId,
            now: BlockNumberFor<T>,
        ) {
            if Self::vote_in_progress(proposal_id, watchtower.clone(), now) {
                log::debug!(
                    "Vote already in progress. Proposal {:?}, Watchtower {:?}",
                    proposal_id,
                    watchtower
                );

                return
            }

            let result = Self::validate_root(now, &root_data, &proposal_id);
            let in_favor = match result {
                Ok(in_favor) => in_favor,
                Err(e) => {
                    log::error!("Error validating root data: {:?}. Error: {:?}", root_data, e);
                    return
                },
            };

            if let Err(e) = Self::submit_vote(proposal_id, in_favor, signing_key, watchtower, now) {
                log::error!("Error voting on proposal {:?}. Error: {:?}", proposal_id, e);
            };
        }

        fn submit_vote(
            proposal_id: ProposalId,
            in_favor: bool,
            signing_key: T::SignerId,
            watchtower: T::AccountId,
            block_number: BlockNumberFor<T>,
        ) -> Result<(), &'static str> {
            let data_to_sign =
                (WATCHTOWER_UNSIGNED_VOTE_CONTEXT, proposal_id, in_favor, &watchtower);
            let signature = match signing_key.sign(&data_to_sign.encode()) {
                Some(sig) => sig,
                None => return Err("Failed to sign vote data"),
            };

            let call = pallet_watchtower::Call::unsigned_vote {
                proposal_id,
                in_favor,
                watchtower: watchtower.clone(),
                signature,
            };

            match SubmitTransaction::<T, pallet_watchtower::Call<T>>::submit_unsigned_transaction(
                call.into(),
            ) {
                Ok(()) => {
                    Self::record_vote_submission(block_number, proposal_id, watchtower)?;
                    return Ok(())
                },
                Err(_e) => Err("Error submitting summary watchtower vote."),
            }
        }

        pub fn record_vote_submission(
            block_number: BlockNumberFor<T>,
            proposal_id: ProposalId,
            watchtower: T::AccountId,
        ) -> Result<(), Error<T>> {
            let mut key = OC_DB_PREFIX.to_vec();
            key.extend((proposal_id, watchtower).encode());

            let storage = StorageValueRef::persistent(&key);
            let result =
                storage.mutate(|_: Result<Option<BlockNumberFor<T>>, StorageRetrievalError>| {
                    Ok(block_number)
                });
            match result {
                Err(MutateStorageError::ValueFunctionFailed(e)) => Err(e),
                Err(MutateStorageError::ConcurrentModification(_)) =>
                    Err(Error::<T>::FailedToAcquireOcwDbLock),
                Ok(_) => return Ok(()),
            }
        }

        pub fn vote_in_progress(
            proposal_id: ProposalId,
            watchtower: T::AccountId,
            block_number: BlockNumberFor<T>,
        ) -> bool {
            let mut key = OC_DB_PREFIX.to_vec();
            key.extend((proposal_id, watchtower).encode());

            match StorageValueRef::persistent(&key).get::<BlockNumberFor<T>>().ok().flatten() {
                Some(last_submission) => {
                    // Allow BLOCK_INCLUSION_PERIOD blocks for the transaction to be included
                    let deadline = last_submission
                        .saturating_add(BlockNumberFor::<T>::from(BLOCK_INCLUSION_PERIOD));
                    return block_number <= deadline
                },
                _ => false,
            }
        }

        pub fn ocw_already_run(block_number: BlockNumberFor<T>) -> Result<(), ()> {
            // Offchain workers could run multiple times for the same block number (re-orgs...)
            // so we need to make sure we only run this once per block
            let caller_id = b"summary_watchtower".to_vec();
            OcwLock::record_block_run(block_number, caller_id).map_err(|e| match e {
                OcwStorageError::OffchainWorkerAlreadyRun => {
                    log::warn!(
                        "❌ Summary watchtower OCW has already run for block number {:?}",
                        block_number
                    );
                },
                OcwStorageError::ErrorRecordingOffchainWorkerRun => {
                    log::error!(
                        "❌ Unable to record ocw run for block {:?}, skipping",
                        block_number
                    );
                },
            })?;

            Ok(())
        }
    }

    impl<T: Config> WatchtowerHooks<Proposal<T>> for Pallet<T> {
        fn on_proposal_submitted(proposal_id: ProposalId, proposal: Proposal<T>) -> DispatchResult {
            match &proposal.source {
                // it source is Internal(ProposalType::Anchor) or Internal(ProposalType::Summary)
                // then process it
                ProposalSource::Internal(internal_type) => match internal_type {
                    ProposalType::Anchor | ProposalType::Summary =>
                        Self::process_new_proposal(None, proposal_id, proposal),
                    _ => Ok(()),
                },
                _ => Ok(()),
            }
        }

        fn on_voting_completed(
            proposal_id: ProposalId,
            _external_ref: &H256,
            _result: &ProposalStatusEnum,
        ) {
            // If this is our stored proposal, and it is finalised, remove it from storage.
            if let Some((stored_proposal_id, _)) = RootInfo::<T>::get() {
                if stored_proposal_id == proposal_id {
                    RootInfo::<T>::kill();
                }
            }
        }

        fn on_cancelled(proposal_id: ProposalId, _external_ref: &H256) {
            if let Some((stored_proposal_id, _)) = RootInfo::<T>::get() {
                if stored_proposal_id == proposal_id {
                    RootInfo::<T>::kill();
                }
            }
        }
    }

    #[derive(
        Encode, Decode, Default, Clone, Copy, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen,
    )]
    pub struct RootData<B: AtLeast32Bit> {
        pub root_id: RootId<B>,
        pub root_hash: H256,
    }
}
