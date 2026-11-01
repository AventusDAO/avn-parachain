#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::{format, string::ToString};

use frame_support::{dispatch::DispatchResult, pallet_prelude::*, storage::with_storage_layer};
use frame_system::pallet_prelude::*;

use pallet_avn::{BridgeInterface, BridgeInterfaceNotification};
use pallet_watchtower::Proposal;
use sp_avn_common::{
    eth::{lower_params_match, EthereumId},
    event_types::{EthEvent, EthTransactionId, EventData, LowerLockedData},
    BridgeContractMethod, LowerDataProvider, LoweringControl, UINT32,
};
use sp_core::{H160, H256, U256};
use sp_runtime::{traits::SaturatedConversion, Perbill};
use sp_std::prelude::*;
use sp_watchtower::*;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);
pub const PALLET_ID: &'static [u8; 17] = b"bridge_watchtower";

pub type LowerId = u32;

const LIFT_CHECK_CONTEXT: &[u8] = b"bridge_watchtower:lift";

/// Why a lift could not be verified against T1.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub enum LiftCheckFailure {
    /// The watchtower proposal could not be submitted.
    RequestFailed,
    /// Watchtowers voted that the lift does not match T1.
    Rejected,
    /// The vote did not resolve in time.
    Expired,
    /// The vote was cancelled, e.g. no committee could be selected.
    Cancelled,
    /// The vote ended with a status that is not a result.
    UnexpectedResult,
}

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/test_lift_vote.rs"]
mod test_lift_vote;
#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

mod benchmarking;
mod lift_vote;

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
    pub trait Config:
        frame_system::Config + pallet_watchtower::Config + pallet_avn::Config
    {
        /// Sends the freeze transactions to Ethereum.
        type BridgeInterface: BridgeInterface;
        /// The lowers T2 issued, against which locked lowers are checked.
        type LowerDataProvider: LowerDataProvider;
        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
        /// Runs the watchtower votes that check lifts against T1.
        type ExternalValidator: WatchtowerInterface<AccountId = Self::AccountId>;
        /// Stops lowering when a lift cannot be verified.
        type LoweringControl: LoweringControl;
        /// Share of the committee needed to decide a lift check.
        #[pallet::constant]
        type LiftCheckThreshold: Get<Perbill>;
        /// Number of watchtowers randomly selected to check each lift.
        #[pallet::constant]
        type LiftCheckCommitteeSize: Get<u32>;
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

    /// Lifts waiting for, or going through, a watchtower check, by the external ref of their
    /// proposal.
    #[pallet::storage]
    pub type LiftChecks<T: Config> = StorageMap<_, Blake2_128Concat, H256, EthEvent, OptionQuery>;

    /// The lift whose check is being voted on.
    #[pallet::storage]
    pub type ActiveLiftCheck<T: Config> = StorageValue<_, (ProposalId, EthEvent), OptionQuery>;

    /// Whether processed lifts are checked by the watchtowers. Off until enough watchtowers are
    /// online to vote. Turning it off only stops new checks; requested ones still run.
    #[pallet::storage]
    pub type LiftCheckingEnabled<T: Config> = StorageValue<_, bool, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A lower locked on T1 matches the lower T2 issued.
        LowerLockVerified { lower_id: LowerId, eth_tx_hash: EthTransactionId },
        /// A lower locked on T1 does not match any unclaimed lower T2 issued.
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
        /// Watchtowers were asked to check a lift against T1.
        LiftCheckRequested { eth_tx_hash: EthTransactionId, external_ref: H256 },
        /// Watchtowers confirmed a lift matches T1.
        LiftVerified { eth_tx_hash: EthTransactionId },
        /// A lift could not be verified against T1. Lowering is disabled.
        LiftFailedVerification { eth_tx_hash: EthTransactionId, reason: LiftCheckFailure },
        /// Processed lifts are checked by the watchtowers.
        LiftChecksEnabled,
        /// Processed lifts are no longer checked. Checks already requested still run.
        LiftChecksDisabled,
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

        /// Starts or stops the watchtower checks of processed lifts.
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::toggle_lift_checks())]
        pub fn toggle_lift_checks(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;

            LiftCheckingEnabled::<T>::put(enabled);

            if enabled {
                Self::deposit_event(Event::<T>::LiftChecksEnabled);
            } else {
                Self::deposit_event(Event::<T>::LiftChecksDisabled);
            }

            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn offchain_worker(now: BlockNumberFor<T>) {
            // Only watchtower nodes vote on lift checks
            if sp_io::offchain::is_validator() {
                return
            }

            Self::vote_on_lift_check(now);
        }

        fn integrity_test() {
            let size = T::LiftCheckCommitteeSize::get();
            assert!(
                size >= T::ExternalValidator::min_committee_size() &&
                    size <= T::ExternalValidator::max_committee_size(),
                "LiftCheckCommitteeSize is outside the watchtower committee bounds"
            );
        }
    }

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
            let Some(params) = T::LowerDataProvider::unclaimed_lower_params(data.lower_id) else {
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

        pub fn lift_check_ref(eth_tx_hash: &EthTransactionId) -> H256 {
            H256(sp_io::hashing::blake2_256(&(LIFT_CHECK_CONTEXT, eth_tx_hash).encode()))
        }

        /// Asks the watchtowers to check a lift against T1. If that is not possible, the lift
        /// cannot be verified so lowering is disabled.
        fn process_lift(event: &EthEvent) {
            if !LiftCheckingEnabled::<T>::get() {
                return
            }

            let eth_tx_hash = event.event_id.transaction_hash;
            match Self::request_lift_check(event) {
                Ok(external_ref) => Self::deposit_event(Event::<T>::LiftCheckRequested {
                    eth_tx_hash,
                    external_ref,
                }),
                Err(e) => {
                    log::error!(
                        "❌ Failed to request the check of lift {:?}: {:?}",
                        eth_tx_hash,
                        e
                    );
                    Self::fail_lift_verification(eth_tx_hash, LiftCheckFailure::RequestFailed);
                },
            }
        }

        fn request_lift_check(event: &EthEvent) -> Result<H256, DispatchError> {
            let eth_tx_hash = event.event_id.transaction_hash;
            let external_ref = Self::lift_check_ref(&eth_tx_hash);
            let request = ProposalRequest {
                title: format!("Verify lift {:?}", eth_tx_hash).into_bytes(),
                payload: RawPayload::Inline(event.encode()),
                threshold: T::LiftCheckThreshold::get(),
                source: ProposalSource::Internal(ProposalType::Lift),
                // Expiry still disables lowering, but is reported apart from a rejection
                decision_rule: DecisionRule::ExpireUnresolved,
                external_ref,
                created_at: frame_system::Pallet::<T>::block_number().saturated_into::<u32>(),
                vote_duration: None,
                committee_size: Some(T::LiftCheckCommitteeSize::get()),
                voting: VotingKind::Binary,
            };
            with_storage_layer(|| T::ExternalValidator::submit_proposal(None, request))?;

            LiftChecks::<T>::insert(external_ref, event);
            Ok(external_ref)
        }

        fn fail_lift_verification(eth_tx_hash: EthTransactionId, reason: LiftCheckFailure) {
            Self::deposit_event(Event::<T>::LiftFailedVerification { eth_tx_hash, reason });
            T::LoweringControl::disable_lowering();
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
            match &event.event_data {
                EventData::LogLowerLocked(data) =>
                    Self::process_lower_locked(event.event_id.transaction_hash, data),
                EventData::LogLifted(_) | EventData::LogAvtGrowthLifted(_) =>
                    Self::process_lift(event),
                _ => {},
            }

            Ok(())
        }
    }

    impl<T: Config> WatchtowerHooks<Proposal<T>> for Pallet<T> {
        // Never fails: an error would cancel the check, which disables lowering
        fn on_proposal_submitted(proposal_id: ProposalId, proposal: Proposal<T>) -> DispatchResult {
            if proposal.source != ProposalSource::Internal(ProposalType::Lift) {
                return Ok(())
            }

            if let Some(event) = LiftChecks::<T>::get(proposal.external_ref) {
                ActiveLiftCheck::<T>::put((proposal_id, event));
            }

            Ok(())
        }

        fn on_voting_completed(
            proposal_id: ProposalId,
            external_ref: &H256,
            result: &ProposalStatusEnum,
        ) {
            let Some(event) = LiftChecks::<T>::take(external_ref) else { return };
            if ActiveLiftCheck::<T>::get().is_some_and(|(id, _)| id == proposal_id) {
                ActiveLiftCheck::<T>::kill();
            }

            let eth_tx_hash = event.event_id.transaction_hash;
            let reason = match result {
                ProposalStatusEnum::Resolved { passed: true } => {
                    Self::deposit_event(Event::<T>::LiftVerified { eth_tx_hash });
                    return
                },
                ProposalStatusEnum::Resolved { passed: false } => LiftCheckFailure::Rejected,
                ProposalStatusEnum::Expired => LiftCheckFailure::Expired,
                ProposalStatusEnum::Cancelled => LiftCheckFailure::Cancelled,
                // Never reported as a result. The check is over either way, so fail safe.
                ProposalStatusEnum::Queued |
                ProposalStatusEnum::Active |
                ProposalStatusEnum::Unknown => LiftCheckFailure::UnexpectedResult,
            };
            Self::fail_lift_verification(eth_tx_hash, reason);
        }

        fn on_cancelled(proposal_id: ProposalId, external_ref: &H256) {
            Self::on_voting_completed(proposal_id, external_ref, &ProposalStatusEnum::Cancelled);
        }
    }
}
