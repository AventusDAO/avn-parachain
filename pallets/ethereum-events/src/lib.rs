//! # Ethereum event checker Pallet
//!
//! This pallet provides functionality to get ethereum events.

#![cfg_attr(not(feature = "std"), no_std)]

// TODO [TYPE: review][PRI: low]: Find a way of not using strings directly in the runtime. (probably
// irrelevant)
#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
use frame_support::{
    dispatch::DispatchResult,
    ensure,
    traits::{Get, IsSubType},
};
use frame_system::{
    offchain::{SendTransactionTypes, SubmitTransaction},
    pallet_prelude::BlockNumberFor,
};
use sp_core::{ConstU32, H160, H256};
use sp_runtime::{
    offchain::storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
    scale_info::TypeInfo,
    traits::{CheckedAdd, Dispatchable, Hash, IdentifyAccount, Verify, Zero},
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
        ValidTransaction,
    },
    DispatchError, RuntimeDebug,
};
use sp_std::{cmp, prelude::*};

use codec::{Decode, Encode, MaxEncodedLen};
use sp_application_crypto::RuntimeAppPublic;
use sp_avn_common::{
    bounds::ProcessingBatchBound,
    event_discovery::EthereumEventsFilterTrait,
    event_types::{
        AddedValidatorData, AvtGrowthLiftedData, AvtLowerClaimedData, Challenge, ChallengeReason,
        CheckResult, EthEventCheckResult, EthEventId, EventData, LiftedData, NftCancelListingData,
        NftEndBatchListingData, NftMintData, NftTransferToData, ProcessedEventHandler,
        TotalSupplyUpdatedData, ValidEvents, Validator,
    },
    verify_signature, EthQueryRequest, EthQueryResponse, EthQueryResponseType, EthTransaction,
    IngressCounter, InnerCallValidator, Proof,
};

use pallet_session::historical::IdentificationTuple;
use sp_staking::offence::ReportOffence;

use pallet_avn::{
    self as avn, Error as avn_error, EventMigration, ProcessedEventsChecker, MAX_VALIDATOR_ACCOUNTS,
};
pub mod offence;
use crate::offence::{
    create_and_report_invalid_log_offence, EthereumLogOffenceType, InvalidEthereumLogOffence,
};

pub mod event_parser;
use crate::event_parser::{find_event, get_status, parse_response_to_json};
use sp_runtime::BoundedVec;

pub type AVN<T> = avn::Pallet<T>;
pub use pallet::*;

const VALIDATED_EVENT_LOCAL_STORAGE: &'static [u8; 28] = b"eth_events::validated_events";

const PALLET_ID: &'static [u8; 20] = b"eth_events::last_run";

const ERROR_CODE_EVENT_NOT_IN_UNCHECKED: u8 = 0;
const ERROR_CODE_INVALID_EVENT_DATA: u8 = 1;
const ERROR_CODE_IS_PRIMARY_HAS_ERROR: u8 = 2;
const ERROR_CODE_VALIDATOR_NOT_PRIMARY: u8 = 3;
const ERROR_CODE_EVENT_NOT_IN_PENDING_CHALLENGES: u8 = 4;

const MINIMUM_EVENT_CHALLENGE_PERIOD: u32 = 60;

pub const SIGNED_ADD_ETHEREUM_LOG_CONTEXT: &'static [u8] =
    b"authorization for add ethereum log operation";
#[cfg(test)]
mod mock;
#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/session_handler_tests.rs"]
mod session_handler_tests;

#[cfg(test)]
#[path = "tests/test_offchain_worker_calls.rs"]
mod test_offchain_worker_calls;

#[path = "tests/test_offchain_worker.rs"]
mod test_offchain_worker;

#[path = "tests/test_process_event.rs"]
mod test_process_event;

#[path = "tests/test_parse_event.rs"]
mod test_parse_event;

#[cfg(test)]
#[path = "tests/test_challenges.rs"]
mod test_challenges;

#[cfg(test)]
#[path = "tests/test_insert_nft_contract.rs"]
mod test_insert_nft_contract;

#[cfg(test)]
#[path = "tests/test_set_event_challenge_period.rs"]
mod test_set_event_challenge_period;

#[cfg(test)]
#[path = "tests/test_initial_events.rs"]
mod test_initial_events;

#[cfg(test)]
#[path = "tests/test_ethereum_logs.rs"]
mod tests_ethereum_logs;

mod benchmarking;

pub mod default_weights;
pub use default_weights::WeightInfo;

#[derive(Encode, Decode, Clone, PartialEq, Debug, Eq, TypeInfo)]
pub enum EthereumContracts {
    AvnBridgeContract,
    NftMarketplace,
}

const SUBMIT_CHECKEVENT_RESULT_CONTEXT: &'static [u8] = b"submit_checkevent_result";
const CHALLENGE_EVENT_CONTEXT: &'static [u8] = b"challenge_event";
const PROCESS_EVENT_CONTEXT: &'static [u8] = b"process_event";

const MAX_NUMBER_OF_UNCHECKED_EVENTS: u32 = 500;
const MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES: u32 = 50;
const MAX_CHALLENGES: u32 = 50;

pub type MaxUncheckedEvents = ConstU32<MAX_NUMBER_OF_UNCHECKED_EVENTS>;
pub type MaxEventsPendingChallenges = ConstU32<MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES>;
pub type MaxChallenges = ConstU32<MAX_CHALLENGES>;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    // Public interface of this pallet
    #[pallet::config(with_default)]
    pub trait Config:
        SendTransactionTypes<Call<Self>>
        + frame_system::Config
        + avn::Config
        + pallet_session::historical::Config
    {
        #[pallet::no_default_bounds]
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        #[pallet::no_default_bounds]
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + IsSubType<Call<Self>>
            + From<Call<Self>>;

        type ProcessedEventHandler: ProcessedEventHandler;

        /// Minimum number of blocks that have passed after an ethereum transaction has been mined
        type MinEthBlockConfirmation: Get<u64>;

        ///  A type that gives the pallet the ability to report offences
        #[pallet::no_default]
        type ReportInvalidEthereumLog: ReportOffence<
            Self::AccountId,
            IdentificationTuple<Self>,
            InvalidEthereumLogOffence<IdentificationTuple<Self>>,
        >;

        /// A type that can be used to verify signatures
        #[pallet::no_default]
        type Public: IdentifyAccount<AccountId = Self::AccountId>;

        /// The signature type used by accounts/transactions.
        #[cfg(not(feature = "runtime-benchmarks"))]
        #[pallet::no_default]
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;

        #[cfg(feature = "runtime-benchmarks")]
        #[pallet::no_default]
        type Signature: Verify<Signer = Self::Public>
            + Member
            + Decode
            + Encode
            + TypeInfo
            + From<sp_core::sr25519::Signature>;

        /// Weight information for the extrinsics in this pallet.
        type WeightInfo: WeightInfo;
        type ProcessedEventsChecker: ProcessedEventsChecker;
        type ProcessedEventsHandler: EthereumEventsFilterTrait;
    }

    /// Default implementations of [`DefaultConfig`], which can be used to implement [`Config`].
    pub mod config_preludes {
        use super::*;
        use frame_support::derive_impl;
        pub struct TestDefaultConfig;
        use frame_support::parameter_types;

        parameter_types! {
            pub const MinEthBlockConfirmation: u64 = 2;
        }

        #[derive_impl(frame_system::config_preludes::TestDefaultConfig as frame_system::DefaultConfig, no_aggregated_types)]
        impl frame_system::DefaultConfig for TestDefaultConfig {}

        #[frame_support::register_default_impl(TestDefaultConfig)]
        impl DefaultConfig for TestDefaultConfig {
            #[inject_runtime_type]
            type RuntimeEvent = ();
            #[inject_runtime_type]
            type RuntimeCall = ();
            type ProcessedEventHandler = ();
            type ProcessedEventsHandler = ();
            type MinEthBlockConfirmation = MinEthBlockConfirmation;
            type ProcessedEventsChecker = ();
            type WeightInfo = ();
        }
    }
    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        // T1 Event added to the pending queue
        /// EthereumEventAdded(EthEventId, AddedBy, T1 contract address)
        EthereumEventAdded {
            eth_event_id: EthEventId,
            added_by: T::AccountId,
            t1_contract_address: H160,
        },
        // T1 Event's validity checked (does it exist?)
        /// EventValidated(EthEventId, CheckResult, ValidatedBy)
        EventValidated {
            eth_event_id: EthEventId,
            check_result: CheckResult,
            validated_by: T::AccountId,
        },
        /// EventProcessed(EthEventId, Processor, Outcome)
        EventProcessed {
            eth_event_id: EthEventId,
            processor: T::AccountId,
            outcome: bool,
        },
        /// EventChallenged(EthEventId, Challenger, ChallengeReason)
        EventChallenged {
            eth_event_id: EthEventId,
            challenger: T::AccountId,
            challenge_reason: ChallengeReason,
        },
        /// ChallengeSucceeded(T1 event, CheckResult)
        ChallengeSucceeded {
            eth_event_id: EthEventId,
            check_result: CheckResult,
        },
        /// OffenceReported(OffenceType, Offenders)
        OffenceReported {
            offence_type: EthereumLogOffenceType,
            offenders: Vec<IdentificationTuple<T>>,
        },
        /// EventAccepted(EthEventId)
        EventAccepted {
            eth_event_id: EthEventId,
        },
        /// EventRejected(EthEventId, CheckResult, HasSuccessfullChallenge)
        EventRejected {
            eth_event_id: EthEventId,
            check_result: CheckResult,
            successful_challenge: bool,
        },
        /// EventChallengePeriodUpdated(EventChallengePeriodInBlocks)
        EventChallengePeriodUpdated {
            block: BlockNumberFor<T>,
        },
        CallDispatched {
            relayer: T::AccountId,
            hash: T::Hash,
        },
        /// NFT related Ethereum event was added(EthEventId, AddedBy)
        NftEthereumEventAdded {
            eth_event_id: EthEventId,
            account_id: T::AccountId,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        ChallengeLimitReached,
        EventLimitReached,
        OffendersLimitReached,
        NumValidatorAccountsLimitReached,
        DuplicateEvent,
        MissingEventToCheck,
        UnrecognizedEventSignature,
        EventParsingFailed,
        VectorBoundsExceeded,
        ErrorSigning,
        ErrorSubmittingTransaction,
        InvalidKey,
        PendingChallengeEventNotFound,
        InvalidEventToChallenge,
        Overflow,
        DuplicateChallenge,
        ErrorSavingValidationToLocalDB,
        MalformedHash,
        InvalidContractAddress,
        InvalidEventToProcess,
        ChallengingOwnEvent,
        InvalidContractType,
        InvalidEventChallengePeriod,
        SenderIsNotSigner,
        UnauthorizedTransaction,
        UnauthorizedSignedAddEthereumLogTransaction,
        UncheckedEventsOverflow,
        PrevChallengesOverflow,
        EventsPendingChallengeOverflow,
        ErrorAddingEthereumLog,
    }

    #[pallet::storage]
    #[pallet::getter(fn ingress_counter)]
    pub type TotalIngresses<T: Config> = StorageValue<_, IngressCounter, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn unchecked_events)]
    pub type UncheckedEvents<T: Config> = StorageValue<
        _,
        BoundedVec<(EthEventId, IngressCounter, BlockNumberFor<T>), MaxUncheckedEvents>,
        ValueQuery,
    >;

    // //TODO [TYPE: business logic][PRI: high][CRITICAL][NOTE: clarify]: What happens to invalid
    // events (missing) in this list?
    #[pallet::storage]
    #[pallet::getter(fn events_pending_challenge)]
    pub type EventsPendingChallenge<T: Config> = StorageValue<
        _,
        BoundedVec<
            (
                EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
                IngressCounter,
                BlockNumberFor<T>,
            ),
            MaxEventsPendingChallenges,
        >,
        ValueQuery,
    >;

    // Should be a set as requires quick access but Substrate doesn't support sets: they recommend
    // using a bool HashMap. This map holds all events that have been processed, regardless of
    // the outcome of the execution of the events.
    #[deprecated]
    #[pallet::storage]
    pub type ProcessedEvents<T: Config> =
        StorageMap<_, Blake2_128Concat, EthEventId, bool, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn challenges)]
    pub type Challenges<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        EthEventId,
        BoundedVec<T::AccountId, MaxChallenges>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn quorum_factor)]
    pub type QuorumFactor<T: Config> = StorageValue<_, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn event_challenge_period)]
    pub type EventChallengePeriod<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn nft_t1_contracts)]
    pub type NftT1Contracts<T: Config> = StorageMap<_, Blake2_128Concat, H160, (), ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn proxy_nonce)]
    pub type ProxyNonces<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u64, ValueQuery>;

    #[pallet::storage]
    pub(crate) type StorageVersion<T> = StorageValue<_, Releases, ValueQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub quorum_factor: u32,
        pub event_challenge_period: BlockNumberFor<T>,
        pub lift_tx_hashes: Vec<H256>,
        pub processed_events: Vec<(H256, H256, bool)>,
        pub nft_t1_contracts: Vec<(H160, ())>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                quorum_factor: 4 as u32,
                event_challenge_period: BlockNumberFor::<T>::from(300 as u32),
                lift_tx_hashes: Vec::<H256>::new(),
                processed_events: Vec::<(H256, H256, bool)>::new(),
                nft_t1_contracts: Vec::<(H160, ())>::new(),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            assert_ne!(self.quorum_factor, 0, "Quorum factor cannot be 0");
            QuorumFactor::<T>::put(self.quorum_factor);

            StorageVersion::<T>::put(Releases::default());

            EventChallengePeriod::<T>::put(self.event_challenge_period);

            for (signature, transaction_hash, value) in self.processed_events.iter() {
                T::ProcessedEventsChecker::add_processed_event(
                    &EthEventId { signature: *signature, transaction_hash: *transaction_hash },
                    *value,
                )
                .expect("Failed to create genesis config for processed events");
            }

            for (key, value) in self.nft_t1_contracts.iter() {
                NftT1Contracts::<T>::insert(key, value);
            }

            let unchecked_lift_events = self
                .lift_tx_hashes
                .iter()
                .map(|&tx_hash| {
                    let ingress_counter = Pallet::<T>::get_next_ingress_counter();
                    return (
                        EthEventId {
                            signature: ValidEvents::Lifted.signature(),
                            transaction_hash: tx_hash,
                        },
                        ingress_counter,
                        BlockNumberFor::<T>::zero(),
                    )
                })
                .collect::<Vec<(EthEventId, IngressCounter, BlockNumberFor<T>)>>();

            let bounded_unchecked_events = BoundedVec::<
                (EthEventId, IngressCounter, BlockNumberFor<T>),
                MaxUncheckedEvents,
            >::try_from(unchecked_lift_events);

            assert!(bounded_unchecked_events.is_ok());
            UncheckedEvents::<T>::put(bounded_unchecked_events.unwrap());
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(2)]
        #[pallet::weight( <T as pallet::Config>::WeightInfo::submit_checkevent_result(MAX_VALIDATOR_ACCOUNTS, MAX_NUMBER_OF_UNCHECKED_EVENTS))]
        pub fn submit_checkevent_result(
            origin: OriginFor<T>,
            result: EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
            ingress_counter: u64,
            // Signature and structural validation is already done in validate unsigned so no need
            // to do it here. This is not used, but we must have this field so it can be
            // used in the logic of validate_unsigned
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
            _validator: Validator<T::AuthorityId, T::AccountId>,
        ) -> DispatchResult {
            ensure_none(origin)?;
            // TODO [TYPE: test][PRI: medium][CRITICAL][JIRA: 348]: Test if rotating keys will break
            // this.
            ensure!(Self::is_validator(&result.checked_by), Error::<T>::InvalidKey);

            let event_index = Self::unchecked_events().iter().position(|(event, counter, _)| {
                event == &result.event.event_id && counter == &ingress_counter
            });
            if let Some(event_index) = event_index {
                let current_block = <frame_system::Pallet<T>>::block_number();
                let mut result = result;
                result.ready_for_processing_after_block = current_block
                    .checked_add(&Self::event_challenge_period())
                    .ok_or(Error::<T>::Overflow)?;
                result.min_challenge_votes =
                    (AVN::<T>::active_validators().len() as u32) / Self::quorum_factor();

                // Insert first and remove
                <EventsPendingChallenge<T>>::mutate(|pending_events| {
                    if let Err(_) =
                        pending_events.try_push((result.clone(), ingress_counter, current_block))
                    {
                        log::error!("Failed to push to pending_events");
                    }
                });

                <UncheckedEvents<T>>::mutate(|events| events.remove(event_index));

                Self::deposit_event(Event::<T>::EventValidated {
                    eth_event_id: result.event.event_id,
                    check_result: result.result,
                    validated_by: result.checked_by,
                });
            } else {
                Err(Error::<T>::MissingEventToCheck)?
            }

            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            Ok(())
        }

        #[pallet::call_index(3)]
        #[pallet::weight( <T as pallet::Config>::WeightInfo::process_event_with_successful_challenge(
            MAX_VALIDATOR_ACCOUNTS,
            MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES
        )
            .max(<T as Config>::WeightInfo::process_event_without_successful_challenge(
                MAX_VALIDATOR_ACCOUNTS,
                MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES
            )))]
        pub fn process_event(
            origin: OriginFor<T>,
            event_id: EthEventId,
            _ingress_counter: IngressCounter, /* this is not used in this function, but is added
                                               * here so that `_signature` can use this value to
                                               * become different from previous calls. */
            validator: Validator<T::AuthorityId, T::AccountId>,
            // Signature and structural validation is already done in validate unsigned so no need
            // to do it here
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;
            // TODO [TYPE: test][PRI: medium][CRITICAL][JIRA: 348]: Test if rotating keys will break
            // this.
            ensure!(Self::is_validator(&validator.account_id), Error::<T>::InvalidKey);

            let event_index = Self::get_pending_event_index(&event_id)?;
            // Not using the passed in `checked` to be sure the details have not been changed
            let (validated, _ingress_counter, _) = &Self::events_pending_challenge()[event_index];

            ensure!(
                <frame_system::Pallet<T>>::block_number() >
                    validated.ready_for_processing_after_block,
                Error::<T>::InvalidEventToProcess
            );

            let successful_challenge = Self::is_challenge_successful(validated);

            // Once an event is added to the `ProcessedEvents` set, it cannot be processed again.
            // If there is a successfull challenge on an `Invalid` event, it means the event should
            // have been valid so DO NOT add it to the processed set to allow the event to be
            // processed again in the future. TODO [TYPE: security][PRI:
            // medium][CRITICAL][JIRA: 152]: Deal with transaction replay attacks

            let event_has_not_been_processed =
                !T::ProcessedEventsChecker::processed_event_exists(&event_id);
            let event_was_declared_invalid = validated.result == CheckResult::Invalid;
            let event_can_be_resubmitted = event_was_declared_invalid ||
                (successful_challenge && event_has_not_been_processed);
            if !event_can_be_resubmitted {
                if T::ProcessedEventsChecker::add_processed_event(&event_id, true).is_err() {
                    log::error!(
                        "Unexpected error while registering processing of event {:?}",
                        &event_id
                    );
                }
            }
            <EventsPendingChallenge<T>>::mutate(|pending_events| {
                pending_events.remove(event_index)
            });
            // TODO: Remove this event's challenges from the Challenges map too.
            Self::deposit_event(Event::<T>::EventProcessed {
                eth_event_id: event_id.clone(),
                processor: validator.account_id.clone(),
                outcome: event_has_not_been_processed && !successful_challenge,
            });
            if successful_challenge {
                Self::deposit_event(Event::<T>::ChallengeSucceeded {
                    eth_event_id: event_id.clone(),
                    check_result: validated.result.clone(),
                });

                // Now report the offence of the validator who submitted the check
                create_and_report_invalid_log_offence::<T>(
                    &validator.account_id,
                    &vec![validated.checked_by.clone()],
                    EthereumLogOffenceType::IncorrectValidationResultSubmitted,
                );
            } else {
                let offenders_accounts = Challenges::<T>::take(&event_id);
                if !offenders_accounts.is_empty() {
                    create_and_report_invalid_log_offence::<T>(
                        &validator.account_id,
                        &offenders_accounts,
                        EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                    );
                }
            }

            if validated.result == CheckResult::Ok &&
                !successful_challenge &&
                event_has_not_been_processed
            {
                // Let everyone know we have processed an event.
                let processing_outcome =
                    T::ProcessedEventHandler::on_event_processed(&validated.event);

                if let Ok(_) = processing_outcome {
                    Self::deposit_event(Event::<T>::EventAccepted { eth_event_id: event_id });
                } else {
                    log::error!("💔 Processing ethereum event failed: {:?}", processing_outcome);
                    Self::deposit_event(Event::<T>::EventRejected {
                        eth_event_id: event_id,
                        check_result: validated.result.clone(),
                        successful_challenge,
                    });
                }
            } else {
                Self::deposit_event(Event::<T>::EventRejected {
                    eth_event_id: event_id,
                    check_result: validated.result.clone(),
                    successful_challenge,
                });
            }

            let final_weight = if successful_challenge {
                <T as Config>::WeightInfo::process_event_with_successful_challenge(
                    MAX_VALIDATOR_ACCOUNTS,
                    MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES,
                )
            } else {
                <T as Config>::WeightInfo::process_event_without_successful_challenge(
                    MAX_VALIDATOR_ACCOUNTS,
                    MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES,
                )
            };

            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            Ok(Some(final_weight).into())
        }

        #[pallet::call_index(4)]
        #[pallet::weight( <T as pallet::Config>::WeightInfo::challenge_event(
            MAX_VALIDATOR_ACCOUNTS,
            MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES,
            MAX_CHALLENGES
        ))]
        pub fn challenge_event(
            origin: OriginFor<T>,
            challenge: Challenge<T::AccountId>,
            ingress_counter: IngressCounter,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
            _validator: Validator<T::AuthorityId, T::AccountId>,
        ) -> DispatchResult {
            ensure_none(origin)?;
            ensure!(Self::is_validator(&challenge.challenged_by), Error::<T>::InvalidKey);

            let events_pending_challenge = Self::events_pending_challenge();
            let checked = events_pending_challenge
                .iter()
                .filter(|(e, counter, _)| {
                    e.event.event_id == challenge.event_id && *counter == ingress_counter
                })
                .map(|(event, _counter, _)| event)
                .last(); // returns the most recent occurrence of event_id (in the unexpected case there is more
                         // than one)
            ensure!(checked.is_some(), Error::<T>::InvalidEventToChallenge);
            ensure!(
                checked.expect("Not None").checked_by != challenge.challenged_by,
                Error::<T>::ChallengingOwnEvent
            );

            // TODO [TYPE: business logic][PRI: medium][CRITICAL][JIRA: 349]: Make sure the
            // challenge period has not passed. Note: the current block number can be
            // different to the block_number the offchain worker was invoked in
            if <Challenges<T>>::contains_key(&challenge.event_id) {
                ensure!(
                    !Self::challenges(challenge.event_id.clone())
                        .iter()
                        .any(|challenger| challenger == &challenge.challenged_by),
                    Error::<T>::DuplicateChallenge
                );

                <Challenges<T>>::mutate(challenge.event_id.clone(), |prev_challenges| {
                    if let Err(_) = prev_challenges.try_push(challenge.challenged_by.clone()) {
                        log::error!("Failed to push to prev_challenges");
                    }
                });
            } else {
                <Challenges<T>>::insert(
                    challenge.event_id.clone(),
                    BoundedVec::truncate_from(vec![challenge.challenged_by.clone()]),
                );
            }

            Self::deposit_event(Event::<T>::EventChallenged {
                eth_event_id: challenge.event_id,
                challenger: challenge.challenged_by,
                challenge_reason: challenge.challenge_reason,
            });

            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            Ok(())
        }

        /// Submits an ethereum transaction hash into the chain
        #[deprecated(
            since = "5.5.0",
            note = "This extrinsic is being deprecated, ethereum events will be automatically imported by EthBridge pallet."
        )]
        #[pallet::call_index(5)]
        #[pallet::weight( <T as pallet::Config>::WeightInfo::add_ethereum_log(
            MAX_NUMBER_OF_UNCHECKED_EVENTS,
            MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES
        ))]
        pub fn add_ethereum_log(
            origin: OriginFor<T>,
            event_type: ValidEvents,
            tx_hash: H256,
        ) -> DispatchResult {
            let account_id = ensure_signed(origin)?;
            ensure!(&tx_hash != &H256::zero(), Error::<T>::MalformedHash);

            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            return Self::add_event(event_type, tx_hash, account_id)
        }

        // # </weight>
        #[pallet::call_index(6)]
        #[pallet::weight( <T as pallet::Config>::WeightInfo::signed_add_ethereum_log(
            MAX_NUMBER_OF_UNCHECKED_EVENTS,
            MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES
        ))]
        pub fn signed_add_ethereum_log(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            event_type: ValidEvents,
            tx_hash: H256,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            ensure!(&tx_hash != &H256::zero(), Error::<T>::MalformedHash);

            let sender_nonce = Self::proxy_nonce(&sender);
            let signed_payload = Self::encode_signed_add_ethereum_log_params(
                &proof,
                &event_type,
                &tx_hash,
                sender_nonce,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedAddEthereumLogTransaction
            );

            <ProxyNonces<T>>::mutate(&sender, |n| *n += 1);

            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            return Self::add_event(event_type, tx_hash, sender)
        }

        /// Sets the address for ethereum contracts
        #[pallet::call_index(7)]
        #[pallet::weight(
            <T as pallet::Config>::WeightInfo::set_nft_contract_map_storage())]
        pub fn insert_nft_contract(origin: OriginFor<T>, contract_address: H160) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(&contract_address != &H160::zero(), Error::<T>::InvalidContractAddress);

            <NftT1Contracts<T>>::insert(contract_address, ());

            Ok(())
        }

        #[pallet::call_index(8)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_event_challenge_period())]
        pub fn set_event_challenge_period(
            origin: OriginFor<T>,
            event_challenge_period_in_blocks: BlockNumberFor<T>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                event_challenge_period_in_blocks >= MINIMUM_EVENT_CHALLENGE_PERIOD.into(),
                Error::<T>::InvalidEventChallengePeriod
            );
            EventChallengePeriod::<T>::put(event_challenge_period_in_blocks);
            Self::deposit_event(Event::<T>::EventChallengePeriodUpdated {
                block: event_challenge_period_in_blocks,
            });
            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Offchain Worker entry point.
        fn offchain_worker(block_number: BlockNumberFor<T>) {
            let setup_result = AVN::<T>::pre_run_setup(block_number, PALLET_ID.to_vec());
            if let Err(e) = setup_result {
                if sp_io::offchain::is_validator() {
                    match e {
                        _ if e == DispatchError::from(avn_error::<T>::OffchainWorkerAlreadyRun) => {
                            ();
                        },
                        _ => {
                            log::error!("💔 Unable to run offchain worker: {:?}", e);
                        },
                    };
                }

                return
            }
            let (this_validator, finalised_block) = setup_result.expect("We have a validator");

            // Only primary validators can check and process events
            let is_primary =
                AVN::<T>::is_primary_for_block(block_number, &this_validator.account_id);
            if is_primary.is_err() {
                log::error!("Error checking if validator can check result");
                return
            }

            // =============================== Main Logic ===========================
            if is_primary.expect("Already checked for error.") {
                Self::try_check_event(block_number, &this_validator, finalised_block);
                Self::try_process_event(block_number, &this_validator, finalised_block);
            } else {
                Self::try_validate_event(block_number, &this_validator, finalised_block);
            }
        }
    }

    // Transactions sent by the validator nodes to report the result of checking an event is free
    // Instead we will validate the signature before we allow it to get to the mempool
    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        // https://substrate.dev/rustdocs/master/sp_runtime/traits/trait.ValidateUnsigned.html
        type Call = Call<T>;

        // TODO [TYPE: security][PRI: high][JIRA: 152][CRITICAL]: Are we open to transaction replay
        // attacks, or signature re-use?
        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            if let Call::submit_checkevent_result {
                result,
                ingress_counter,
                signature,
                validator,
            } = call
            {
                if !Self::unchecked_events().iter().any(|(event, counter, _)| {
                    event == &result.event.event_id && counter == ingress_counter
                }) {
                    return InvalidTransaction::Custom(ERROR_CODE_EVENT_NOT_IN_UNCHECKED).into()
                }

                if !result.event.event_data.is_valid() {
                    return InvalidTransaction::Custom(ERROR_CODE_INVALID_EVENT_DATA).into()
                }

                if AVN::<T>::is_primary_for_block(result.checked_at_block, &result.checked_by)
                    .map_err(|_| InvalidTransaction::Custom(ERROR_CODE_IS_PRIMARY_HAS_ERROR))? ==
                    false
                {
                    return InvalidTransaction::Custom(ERROR_CODE_VALIDATOR_NOT_PRIMARY).into()
                }

                if validator.account_id != result.checked_by {
                    return InvalidTransaction::BadProof.into()
                }

                if !Self::data_signature_is_valid(
                    &(SUBMIT_CHECKEVENT_RESULT_CONTEXT, result, ingress_counter),
                    &validator,
                    signature,
                ) {
                    return InvalidTransaction::BadProof.into()
                };

                ValidTransaction::with_tag_prefix("EthereumEvents")
                    .priority(TransactionPriority::max_value())
                    .and_provides(vec![(
                        "check",
                        result.event.event_id.hashed(<T as frame_system::Config>::Hashing::hash),
                    )
                        .encode()])
                    .longevity(64_u64)
                    .propagate(true)
                    .build()
            } else if let Call::process_event { event_id, ingress_counter, validator, signature } =
                call
            {
                if !Self::events_pending_challenge().iter().any(|(pending, counter, _)| {
                    &pending.event.event_id == event_id && counter == ingress_counter
                }) {
                    return InvalidTransaction::Custom(ERROR_CODE_EVENT_NOT_IN_PENDING_CHALLENGES)
                        .into()
                }

                // TODO [TYPE: security][PRI: high][CRITICAL][JIRA: 350]: Check if `validator` is a
                // primary. Beware of using the current block_number() because it may not be the
                // same as what triggered the offchain worker.
                if !Self::data_signature_is_valid(
                    &(PROCESS_EVENT_CONTEXT, &event_id, ingress_counter),
                    validator,
                    signature,
                ) {
                    return InvalidTransaction::BadProof.into()
                };

                ValidTransaction::with_tag_prefix("EthereumEvents")
                    .priority(TransactionPriority::max_value())
                    .and_provides(vec![(
                        "process",
                        event_id.hashed(<T as frame_system::Config>::Hashing::hash),
                    )
                        .encode()])
                    .longevity(64_u64)
                    .propagate(true)
                    .build()
            } else if let Call::challenge_event {
                challenge,
                ingress_counter,
                signature,
                validator,
            } = call
            {
                if !Self::events_pending_challenge().iter().any(|(pending, counter, _)| {
                    pending.event.event_id == challenge.event_id && ingress_counter == counter
                }) {
                    return InvalidTransaction::Custom(ERROR_CODE_EVENT_NOT_IN_PENDING_CHALLENGES)
                        .into()
                }

                // TODO [TYPE: business logic][PRI: medium][CRITICAL][JIRA: 351]: Make sure the
                // challenge period has not passed. Note: the current block number
                // can be different to the block_number the offchain worker was invoked in so
                // by the time the tx gets here the window may have passed.

                if validator.account_id != challenge.challenged_by {
                    return InvalidTransaction::BadProof.into()
                }

                if !Self::data_signature_is_valid(
                    &(CHALLENGE_EVENT_CONTEXT, challenge, ingress_counter),
                    &validator,
                    signature,
                ) {
                    return InvalidTransaction::BadProof.into()
                };

                ValidTransaction::with_tag_prefix("EthereumEvents")
                    .priority(TransactionPriority::max_value())
                    .and_provides(vec![(
                        "challenge",
                        challenge.challenged_by.clone(),
                        challenge.event_id.hashed(<T as frame_system::Config>::Hashing::hash),
                    )
                        .encode()])
                    .longevity(64_u64)
                    .propagate(true)
                    .build()
            } else {
                return InvalidTransaction::Call.into()
            }
        }
    }
}

// implement offchain worker sub-functions
impl<T: Config> Pallet<T> {
    fn try_check_event(
        block_number: BlockNumberFor<T>,
        validator: &Validator<T::AuthorityId, T::AccountId>,
        finalised_block_number: BlockNumberFor<T>,
    ) {
        let event_to_check = Self::get_events_to_check_if_required(finalised_block_number);

        if let Some(event_to_check) = event_to_check {
            log::info!("** Checking events");

            // TODO [TYPE: efficiency][PRI: low]: Can we do more than 1 here?
            let result = Self::check_event_and_submit_result(
                block_number,
                &event_to_check.0,
                event_to_check.1,
                validator,
            );
            if let Err(e) = result {
                log::error!("Error checking for events: {:#?}", e);
            }
        }
    }

    fn try_process_event(
        block_number: BlockNumberFor<T>,
        validator: &Validator<T::AuthorityId, T::AccountId>,
        finalised_block_number: BlockNumberFor<T>,
    ) {
        if let Some((event_to_process, ingress_counter, _)) =
            Self::get_next_event_to_process(block_number, finalised_block_number)
        {
            log::info!("** Processing events");

            // TODO [TYPE: efficiency][PRI: low]: Can we do more than 1 here?
            let result = Self::send_event(event_to_process, ingress_counter, validator);
            if let Err(e) = result {
                log::error!("Error processing events: {:#?}", e);
            }
        }
    }

    fn try_validate_event(
        block_number: BlockNumberFor<T>,
        validator: &Validator<T::AuthorityId, T::AccountId>,
        finalised_block_number: BlockNumberFor<T>,
    ) {
        if let Some((event_to_validate, ingress_counter, _)) =
            Self::get_next_event_to_validate(&validator.account_id, finalised_block_number)
        {
            log::info!("** Validating events");

            // TODO [TYPE: efficiency][PRI: low]: Can we do more than 1 here?
            let result =
                Self::validate_event(block_number, event_to_validate, ingress_counter, validator);
            if let Err(e) = result {
                log::error!("Error validating events: {:#?}", e);
            }
        }
    }
}

// TODO [TYPE: review][PRI: medium][CRITICAL]: Check error handling. Is this still relevant?
impl<T: Config> Pallet<T> {
    fn is_challenge_successful(
        validated: &EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
    ) -> bool {
        let required_challenge_votes =
            (AVN::<T>::active_validators().len() as u32) / Self::quorum_factor();
        let total_num_of_challenges =
            Self::challenges(validated.event.event_id.clone()).len() as u32;

        return total_num_of_challenges >
            cmp::max(validated.min_challenge_votes, required_challenge_votes)
    }

    fn get_pending_event_index(event_id: &EthEventId) -> Result<usize, Error<T>> {
        // `rposition: there should be at most one occurrence of this event,
        // but in case there is more, we pick the most recent one
        let event_index = Self::events_pending_challenge()
            .iter()
            .rposition(|(pending, _counter, _)| *event_id == pending.event.event_id);
        ensure!(event_index.is_some(), Error::<T>::PendingChallengeEventNotFound);
        return Ok(event_index.expect("Checked for error"))
    }

    fn parse_tier1_event(
        event_id: EthEventId,
        data: Option<Vec<u8>>,
        topics: Vec<Vec<u8>>,
    ) -> Result<EventData, Error<T>> {
        if event_id.signature == ValidEvents::AddedValidator.signature() {
            let event_data = <AddedValidatorData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 AddedValidator Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;

            return Ok(EventData::LogAddedValidator(event_data))
        } else if event_id.signature == ValidEvents::Lifted.signature() {
            let event_data = <LiftedData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 Lifted Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogLifted(event_data))
        } else if event_id.signature == ValidEvents::LiftedToPredictionMarket.signature() {
            let event_data = <LiftedData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 Prediction market lift Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogLiftedToPredictionMarket(event_data))
        } else if event_id.signature == ValidEvents::NftMint.signature() {
            let event_data = <NftMintData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 AvnMintTo Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogNftMinted(event_data))
        } else if event_id.signature == ValidEvents::NftTransferTo.signature() {
            let event_data = <NftTransferToData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 AvnTransferTo Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogNftTransferTo(event_data))
        } else if event_id.signature == ValidEvents::NftCancelListing.signature() {
            let event_data = <NftCancelListingData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 AvnCancelNftListing Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogNftCancelListing(event_data))
        } else if event_id.signature == ValidEvents::NftEndBatchListing.signature() {
            let event_data = <NftEndBatchListingData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 AvnCancelNftBatchListing Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogNftEndBatchListing(event_data))
        } else if event_id.signature == ValidEvents::AvtGrowthLifted.signature() {
            let event_data = <AvtGrowthLiftedData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 LogGrowth Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogAvtGrowthLifted(event_data))
        } else if event_id.signature == ValidEvents::AvtLowerClaimed.signature() {
            let event_data = <AvtLowerClaimedData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 LogLowerClaimed Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogLowerClaimed(event_data))
        } else if event_id.signature == ValidEvents::TotalSupplyUpdated.signature() {
            let event_data = <TotalSupplyUpdatedData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 LogT1TotalSupplyUpdated Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogT1TotalSupplyUpdated(event_data))
        } else if event_id.signature == ValidEvents::LowerReverted.signature() {
            let event_data = <LowerRevertedData>::parse_bytes(data, topics).map_err(|e| {
                log::warn!("Error parsing T1 LogLowerReverted Event: {:#?}", e);
                Error::<T>::EventParsingFailed
            })?;
            return Ok(EventData::LogLowerReverted(event_data))
        } else {
            return Err(Error::<T>::UnrecognizedEventSignature)
        }
    }

    fn get_events_to_check_if_required(
        finalised_block_number: BlockNumberFor<T>,
    ) -> Option<(EthEventId, IngressCounter, BlockNumberFor<T>)> {
        if Self::unchecked_events().is_empty() {
            return None
        }

        return Self::unchecked_events()
            .into_iter()
            .filter(|e| e.2 <= finalised_block_number)
            .nth(0)
    }

    fn get_next_event_to_validate(
        validator_account_id: &T::AccountId,
        finalised_block_number: BlockNumberFor<T>,
    ) -> Option<(
        EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
        IngressCounter,
        BlockNumberFor<T>,
    )> {
        let storage = StorageValueRef::persistent(VALIDATED_EVENT_LOCAL_STORAGE);
        let validated_events = storage.get::<Vec<EthEventId>>();

        let mut stored_validated_events: Vec<EthEventId> = Vec::<EthEventId>::new();
        let mut node_has_never_validated_events = true;

        match validated_events {
            Ok(Some(returned_validated_events)) => {
                node_has_never_validated_events = false;
                stored_validated_events = returned_validated_events;
            },
            _ => {},
        };

        return Self::events_pending_challenge()
            .into_iter()
            .filter(|(checked, _counter, submitted_at_block)| {
                Self::can_validate_this_event(
                    checked,
                    validator_account_id,
                    &stored_validated_events,
                    node_has_never_validated_events,
                ) && submitted_at_block <= &finalised_block_number
            })
            .nth(0)
    }

    fn can_validate_this_event(
        checked: &EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
        validator_account_id: &T::AccountId,
        validated_events: &Vec<EthEventId>,
        node_has_never_validated_events: bool,
    ) -> bool {
        if checked.checked_by == *validator_account_id {
            return false
        }
        if node_has_never_validated_events {
            return true
        }

        let node_has_not_validated_this_event =
            |event_id| !validated_events.as_slice().contains(event_id);

        return node_has_not_validated_this_event(&checked.event.event_id)
    }

    fn get_next_event_to_process(
        block_number: BlockNumberFor<T>,
        finalised_block_number: BlockNumberFor<T>,
    ) -> Option<(
        EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
        IngressCounter,
        BlockNumberFor<T>,
    )> {
        return Self::events_pending_challenge()
            .into_iter()
            .filter(|(checked, _counter, submitted_at_block)| {
                block_number > checked.ready_for_processing_after_block &&
                    submitted_at_block <= &finalised_block_number
            })
            .last()
    }

    fn send_event(
        checked: EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
        ingress_counter: IngressCounter,
        validator: &Validator<T::AuthorityId, T::AccountId>,
    ) -> Result<(), Error<T>> {
        let signature = validator
            .key
            .sign(&(PROCESS_EVENT_CONTEXT, &checked.event.event_id, ingress_counter).encode())
            .ok_or(Error::<T>::ErrorSigning)?;

        SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(
            Call::process_event {
                event_id: checked.event.event_id,
                ingress_counter,
                validator: validator.clone(),
                signature,
            }
            .into(),
        )
        .map_err(|_| Error::<T>::ErrorSubmittingTransaction)?;

        Ok(())
    }

    fn check_event_and_submit_result(
        block_number: BlockNumberFor<T>,
        event_id: &EthEventId,
        ingress_counter: IngressCounter,
        validator: &Validator<T::AuthorityId, T::AccountId>,
    ) -> Result<(), Error<T>> {
        let result = Self::check_event(block_number, event_id, validator);
        if result.result == CheckResult::HttpErrorCheckingEvent {
            // TODO [TYPE: review][PRI: high][CRITICAL]: should there be a punishment for this?
            log::info!("Http error checking event, skipping check");
            return Ok(())
        }

        if result.result == CheckResult::InsufficientConfirmations {
            // TODO [TYPE: review][PRI: medium][JIRA: SYS-358]: Is the correct behaviour? A young
            // event will block the queue.
            log::info!("Event is not old enough, skipping check");
            return Ok(())
        }

        let signature = validator
            .key
            .sign(&(SUBMIT_CHECKEVENT_RESULT_CONTEXT, &result, ingress_counter).encode())
            .ok_or(Error::<T>::ErrorSigning)?;
        SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(
            Call::submit_checkevent_result {
                result,
                ingress_counter,
                signature,
                validator: validator.clone(),
            }
            .into(),
        )
        .map_err(|_| Error::<T>::ErrorSubmittingTransaction)?;

        log::info!("Check result submitted successfully");
        Ok(())
    }

    fn validate_event(
        block_number: BlockNumberFor<T>,
        checked: EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
        ingress_counter: IngressCounter,
        validator: &Validator<T::AuthorityId, T::AccountId>,
    ) -> Result<(), Error<T>> {
        let validated = Self::check_event(block_number, &checked.event.event_id, validator);
        if validated.result == CheckResult::HttpErrorCheckingEvent {
            // TODO [TYPE: review][PRI: high][CRITICAL]: should there be a punishment for this?
            log::info!("Http error validating event, not challenging");
            return Ok(())
        }

        Self::save_validated_event_in_local_storage(checked.event.event_id.clone())?;

        // Note: Any errors after saving to local storage will mean the event will not be validated
        // again
        let challenge =
            Self::get_challenge_if_required(checked, validated, validator.account_id.clone());
        if let Some(challenge) = challenge {
            let signature = validator
                .key
                .sign(&(CHALLENGE_EVENT_CONTEXT, &challenge, ingress_counter).encode())
                .ok_or(Error::<T>::ErrorSigning)?;
            // TODO [TYPE: business logic][PRI: medium][CRITICAL][JIRA: 349]: Allow for this event
            // to be resubmitted if it fails here
            SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(
                Call::challenge_event {
                    challenge,
                    ingress_counter,
                    signature,
                    validator: validator.clone(),
                }
                .into(),
            )
            .map_err(|_| Error::<T>::ErrorSubmittingTransaction)?;

            log::info!("Validation result submitted successfully");
        }

        Ok(())
    }

    fn get_challenge_if_required(
        checked: EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
        validated: EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
        validator_account_id: T::AccountId,
    ) -> Option<Challenge<T::AccountId>> {
        if checked.event.event_id != validated.event.event_id {
            log::info!("Checked and validated have different event id's, not challenging");
            return None
        }

        if (validated.result == checked.result &&
            validated.event.event_data == checked.event.event_data) ||
            (validated.result == CheckResult::Invalid && checked.result == CheckResult::Invalid)
        {
            log::info!("Validation matches original check, not challenging");
            return None
        }

        let challenge_reason = match validated {
            EthEventCheckResult { result: CheckResult::Ok, .. } => {
                if checked.result == CheckResult::Ok {
                    ChallengeReason::IncorrectEventData
                } else {
                    ChallengeReason::IncorrectResult
                }
            },
            EthEventCheckResult { result: CheckResult::Invalid, .. }
                if checked.result == CheckResult::Ok =>
                ChallengeReason::IncorrectResult,
            _ => ChallengeReason::Unknown, /* We shouldn't get here but in case we do, set it to
                                            * Unknown */
        };

        if challenge_reason == ChallengeReason::Unknown {
            return None
        }

        return Some(Challenge::new(checked.event.event_id, challenge_reason, validator_account_id))
    }

    fn save_validated_event_in_local_storage(event_id: EthEventId) -> Result<(), Error<T>> {
        let storage = StorageValueRef::persistent(VALIDATED_EVENT_LOCAL_STORAGE);
        let result =
            storage.mutate(|events: Result<Option<Vec<EthEventId>>, StorageRetrievalError>| {
                match events {
                    Ok(Some(mut events)) => {
                        events.push(event_id);
                        Ok(events)
                    },
                    Ok(None) => Ok(vec![event_id]),
                    _ => Err(()),
                }
            });
        match result {
            Err(MutateStorageError::ValueFunctionFailed(_)) =>
                Err(Error::<T>::ErrorSavingValidationToLocalDB),
            Err(MutateStorageError::ConcurrentModification(_)) =>
                Err(Error::<T>::ErrorSavingValidationToLocalDB),
            Ok(_) => return Ok(()),
        }
    }

    fn check_event(
        block_number: BlockNumberFor<T>,
        event_id: &EthEventId,
        validator: &Validator<T::AuthorityId, T::AccountId>,
    ) -> EthEventCheckResult<BlockNumberFor<T>, T::AccountId> {
        // Make an external HTTP request to fetch the event.
        // Note this call will block until response is received.
        let body = Self::fetch_event(event_id);

        // analyse the body to see if the event exists and is correctly formed
        return Self::compute_result(block_number, body, event_id, &validator.account_id)
    }

    // This function must not panic!!.
    // The outcome of the check must be reported back, even if the check fails
    fn compute_result(
        block_number: BlockNumberFor<T>,
        response_body: Result<Vec<u8>, DispatchError>,
        event_id: &EthEventId,
        validator_account_id: &T::AccountId,
    ) -> EthEventCheckResult<BlockNumberFor<T>, T::AccountId> {
        let ready_after_block: BlockNumberFor<T> = 0u32.into();
        let invalid_result = EthEventCheckResult::new(
            ready_after_block,
            CheckResult::Invalid,
            event_id,
            &EventData::EmptyEvent,
            validator_account_id.clone(),
            block_number,
            Default::default(),
        );

        // check if the body has been received successfully
        if let Err(e) = response_body {
            log::error!("Http error fetching event: {:?}", e);
            return EthEventCheckResult::new(
                ready_after_block,
                CheckResult::HttpErrorCheckingEvent,
                event_id,
                &EventData::EmptyEvent,
                validator_account_id.clone(),
                block_number,
                Default::default(),
            )
        }

        let (response_data_object, num_confirmations) =
            parse_response_to_json(response_body.expect("Checked for error."))
                .unwrap_or((vec![], 0));

        if response_data_object.is_empty() {
            log::error!("❌ Response data json is empty");
            return invalid_result
        };

        // make sure the transaction has been successfully executed
        let status = get_status(&response_data_object).unwrap_or(0);
        if status != 1 {
            log::error!("❌ Transaction was not executed successfully on Ethereum");
            return invalid_result
        }

        let event_object: Option<(_, _, _)> = find_event(&response_data_object, event_id.signature);
        if event_object.is_none() {
            log::error!("❌ Event missing from response or response is not valid. Response: {:?}, event topic: {:?}", response_data_object, event_id.signature);
            return invalid_result
        }
        let (data, topics, contract_address) = event_object.expect("Value is not none");

        if Self::is_event_contract_valid(&contract_address, event_id) == false {
            log::error!("❌ Event contract address {:?} is not recognised", contract_address);
            return invalid_result
        }

        let parsed_event = Self::parse_tier1_event(event_id.clone(), data, topics);
        if let Err(e) = parsed_event {
            log::error!("❌ Unable to parse tier 1 event data {:?}", e);
            return invalid_result
        }

        if num_confirmations < <T as Config>::MinEthBlockConfirmation::get() {
            log::error!(
                "📢 There aren't enough confirmations for this event. Current confirmations: {:?}",
                num_confirmations
            );
            return EthEventCheckResult::new(
                ready_after_block,
                CheckResult::InsufficientConfirmations,
                event_id,
                &EventData::EmptyEvent,
                validator_account_id.clone(),
                block_number,
                Default::default(),
            )
        }

        return EthEventCheckResult::new(
            ready_after_block,
            CheckResult::Ok,
            event_id,
            &parsed_event.expect("Value is not an error"),
            validator_account_id.clone(),
            block_number,
            Default::default(),
        )
    }

    fn fetch_event(event_id: &EthEventId) -> Result<Vec<u8>, DispatchError> {
        let calldata = EthQueryRequest {
            tx_hash: event_id.transaction_hash,
            response_type: EthQueryResponseType::TransactionReceipt,
        };
        let sender = [0; 32];
        let contract_address = AVN::<T>::get_bridge_contract_address();
        let ethereum_call = EthTransaction::new(sender, contract_address, calldata.encode());

        AVN::<T>::post_data_to_service("/eth/query".to_string(), ethereum_call.encode(), None)
    }

    fn event_exists_in_system(event_id: &EthEventId) -> bool {
        return T::ProcessedEventsChecker::processed_event_exists(&event_id) ||
            Self::unchecked_events().iter().any(|(event, _, _)| event == event_id) ||
            Self::events_pending_challenge()
                .iter()
                .any(|(event, _counter, _)| &event.event.event_id == event_id)
    }

    /// Adds an event: tx_hash must be a nonzero hash
    fn add_event(event_type: ValidEvents, tx_hash: H256, sender: T::AccountId) -> DispatchResult {
        let filter = T::ProcessedEventsHandler::get_primary();
        ensure!(filter.contains(&event_type), Error::<T>::ErrorAddingEthereumLog);
        ensure!(event_type.is_primary(), Error::<T>::InvalidEventToProcess);

        let event_id = EthEventId { signature: event_type.signature(), transaction_hash: tx_hash };
        ensure!(!Self::event_exists_in_system(&event_id), Error::<T>::DuplicateEvent);

        let ingress_counter = Self::get_next_ingress_counter();
        <UncheckedEvents<T>>::try_append((
            event_id.clone(),
            ingress_counter,
            <frame_system::Pallet<T>>::block_number(),
        ))
        .map_err(|_| Error::<T>::UncheckedEventsOverflow)?;

        if event_type.is_nft_event() {
            Self::deposit_event(Event::<T>::NftEthereumEventAdded {
                eth_event_id: event_id,
                account_id: sender,
            });
        } else {
            let eth_contract_address: H160 = Some(AVN::<T>::get_bridge_contract_address())
                .or_else(|| Some(H160::zero()))
                .expect("Always return a default value");
            Self::deposit_event(Event::<T>::EthereumEventAdded {
                eth_event_id: event_id,
                added_by: sender,
                t1_contract_address: eth_contract_address,
            });
        }

        Ok(())
    }

    fn is_event_contract_valid(contract_address: &H160, event_id: &EthEventId) -> bool {
        let event_type = ValidEvents::try_from(&event_id.signature).ok();
        if let Some(event_type) = event_type {
            if event_type.is_nft_event() {
                return <NftT1Contracts<T>>::contains_key(contract_address)
            }

            let non_nft_contract_address = Some(AVN::<T>::get_bridge_contract_address());
            return non_nft_contract_address.is_some() &&
                non_nft_contract_address.expect("checked for none") == *contract_address
        }

        return false
    }

    fn data_signature_is_valid<D: Encode>(
        data: &D,
        validator: &Validator<T::AuthorityId, T::AccountId>,
        signature: &<T::AuthorityId as RuntimeAppPublic>::Signature,
    ) -> bool {
        // verify that the incoming (unverified) pubkey is actually a validator
        if !Self::is_validator(&validator.account_id) {
            return false
        }

        // check signature (this is expensive so we do it last).
        let signature_valid =
            data.using_encoded(|encoded_data| validator.key.verify(&encoded_data, &signature));

        return signature_valid
    }

    fn is_validator(account_id: &T::AccountId) -> bool {
        return AVN::<T>::active_validators().into_iter().any(|v| v.account_id == *account_id)
    }

    fn encode_signed_add_ethereum_log_params(
        proof: &Proof<T::Signature, T::AccountId>,
        event_type: &ValidEvents,
        tx_hash: &H256,
        sender_nonce: u64,
    ) -> Vec<u8> {
        return (
            SIGNED_ADD_ETHEREUM_LOG_CONTEXT,
            proof.relayer.clone(),
            event_type,
            tx_hash,
            sender_nonce,
        )
            .encode()
    }

    fn get_encoded_call_param(
        call: &<T as Config>::RuntimeCall,
    ) -> Option<(&Proof<T::Signature, T::AccountId>, Vec<u8>)> {
        let call = match call.is_sub_type() {
            Some(call) => call,
            None => return None,
        };

        match call {
            Call::signed_add_ethereum_log { proof, event_type, tx_hash } => {
                let sender_nonce = Self::proxy_nonce(&proof.signer);
                let encoded_data = Self::encode_signed_add_ethereum_log_params(
                    &proof,
                    &event_type,
                    &tx_hash,
                    sender_nonce,
                );
                return Some((&proof, encoded_data))
            },

            _ => return None,
        }
    }

    pub fn get_next_ingress_counter() -> IngressCounter {
        let ingress_counter = Self::ingress_counter() + 1; // default value in storage is 0, so first root_hash has counter 1
        TotalIngresses::<T>::put(ingress_counter);
        return ingress_counter
    }
}

impl<T: Config> ProcessedEventsChecker for Pallet<T> {
    fn processed_event_exists(event_id: &EthEventId) -> bool {
        return <ProcessedEvents<T>>::contains_key(event_id)
    }

    fn add_processed_event(event_id: &EthEventId, accepted: bool) -> Result<(), ()> {
        ensure!(!Self::processed_event_exists(event_id), ());
        <ProcessedEvents<T>>::insert(event_id.clone(), accepted);
        Ok(())
    }

    fn get_events_to_migrate() -> Option<BoundedVec<EventMigration, ProcessingBatchBound>> {
        let batch_size: u32 = ProcessingBatchBound::get();
        let entry_return =
            |event: &EthEventId, outcome: bool| ProcessedEvents::<T>::insert(event, outcome);
        let migration_batch: Vec<EventMigration> = ProcessedEvents::<T>::iter()
            .take(batch_size as usize)
            .map(|(event_id, outcome)| EventMigration {
                event_id,
                outcome,
                entry_return_impl: entry_return,
            })
            .collect();

        if migration_batch.is_empty() {
            return None
        }

        migration_batch.iter().for_each(|event_to_migrate| {
            ProcessedEvents::<T>::remove(&event_to_migrate.event_id);
        });
        Some(BoundedVec::<EventMigration, ProcessingBatchBound>::truncate_from(migration_batch))
    }
}

impl<T: Config> InnerCallValidator for Pallet<T> {
    type Call = <T as Config>::RuntimeCall;

    fn signature_is_valid(call: &Box<Self::Call>) -> bool {
        if let Some((proof, signed_payload)) = Self::get_encoded_call_param(call) {
            return verify_signature::<T::Signature, T::AccountId>(
                &proof,
                &signed_payload.as_slice(),
            )
            .is_ok()
        }

        return false
    }
}

// A value placed in storage that represents the current version of the EthereumEvents pallet
// storage. This value is used by the `on_runtime_upgrade` logic to determine whether we run its
// storage migration logic.
#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, RuntimeDebug, MaxEncodedLen, TypeInfo)]
enum Releases {
    Unknown,
    V2_0_0,
    V3_0_0,
    V4_0_0,
}

//Todo: Change this once merged
impl Default for Releases {
    fn default() -> Self {
        Releases::V4_0_0
    }
}
