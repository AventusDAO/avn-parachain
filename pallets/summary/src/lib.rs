#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use codec::{Decode, Encode, MaxEncodedLen};
use sp_avn_common::{
    event_types::Validator,
    ocw_lock::{self as OcwLock},
    safe_add_block_numbers, safe_sub_block_numbers, BridgeContractMethod, IngressCounter,
};
use sp_runtime::{
    scale_info::TypeInfo,
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
        ValidTransaction,
    },
    DispatchError,
};
use sp_std::prelude::*;
use sp_watchtower::{
    DecisionRule, ProposalId, ProposalRequest, ProposalSource, ProposalStatusEnum, ProposalType,
    RawPayload, WatchtowerHooks, WatchtowerInterface,
};

use avn::BridgeInterfaceNotification;
use core::convert::TryInto;
use frame_support::{
    dispatch::DispatchResult, ensure, pallet_prelude::StorageVersion, traits::Get,
};
use frame_system::{
    self as system, ensure_none, ensure_root,
    offchain::{CreateInherent, CreateTransactionBase, SubmitTransaction},
};
pub use pallet::*;
use pallet_avn::{
    self as avn,
    vote::{
        approve_vote_validate_unsigned, end_voting_period_validate_unsigned, process_approve_vote,
        process_reject_vote, reject_vote_validate_unsigned, VotingSessionData,
        VotingSessionManager,
    },
    Error as avn_error, MAX_VALIDATOR_ACCOUNTS,
};
use pallet_session::historical::IdentificationTuple;
use sp_application_crypto::RuntimeAppPublic;
use sp_core::H256;
use sp_staking::offence::ReportOffence;

pub mod offence;
use crate::offence::{create_and_report_summary_offence, SummaryOffence, SummaryOffenceType};

pub use sp_avn_common::eth::EthereumId;

const PALLET_ID: &'static [u8; 8] = b"summary-";
const UPDATE_BLOCK_NUMBER_CONTEXT: &'static [u8] = b"update_last_processed_block_number";
const ADVANCE_SLOT_CONTEXT: &'static [u8] = b"advance_slot";

// Error codes returned by validate unsigned methods
const ERROR_CODE_VALIDATOR_IS_NOT_PRIMARY: u8 = 10;
const ERROR_CODE_INVALID_ROOT_RANGE: u8 = 30;

const MIN_SCHEDULE_PERIOD: u32 = 30; // 6 MINUTES
const DEFAULT_SCHEDULE_PERIOD: u32 = 28800; // 1 DAY
const MIN_VOTING_PERIOD: u32 = 25; // 5 MINUTES
const MAX_VOTING_PERIOD: u32 = 28800; // 1 DAY
const DEFAULT_VOTING_PERIOD: u32 = 600; // 30 MINUTES

const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

// used in benchmarks and weights calculation only
const MAX_OFFENDERS: u32 = 2; // maximum of offenders need to be less one third of minimum validators so the benchmark won't panic
const MAX_NUMBER_OF_ROOT_DATA_PER_RANGE: u32 = 2;

pub mod vote;
use crate::vote::*;

pub mod challenge;
use crate::challenge::*;

pub mod types;
pub mod utils;
use crate::types::*;

use pallet_avn::BridgeInterface;
use sp_avn_common::{RootId, RootRange};

mod benchmarking;
pub mod default_weights;
pub use default_weights::WeightInfo;

pub type AVN<T> = avn::Pallet<T>;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{pallet_prelude::*, Blake2_128Concat};
    use frame_system::pallet_prelude::*;

    // Public interface of this pallet
    #[pallet::config(with_default)]
    pub trait Config<I: 'static = ()>:
        frame_system::Config
        + avn::Config
        + pallet_session::historical::Config
        + CreateTransactionBase<Call<Self, I>>
        + CreateInherent<Call<Self, I>>
    {
        #[pallet::no_default_bounds]
        type RuntimeEvent: From<Event<Self, I>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// A period (in block number) to detect when a validator failed to advance the current slot
        /// number
        #[pallet::no_default_bounds]
        type AdvanceSlotGracePeriod: Get<BlockNumberFor<Self>>;

        /// Minimum age of block (in block number) to include in a tree.
        /// This will give grandpa a chance to finalise the blocks
        #[pallet::no_default_bounds]
        type MinBlockAge: Get<BlockNumberFor<Self>>;

        type AccountToBytesConvert: pallet_avn::AccountToBytesConverter<Self::AccountId>;

        ///  A type that gives the pallet the ability to report offences
        #[pallet::no_default_bounds]
        type ReportSummaryOffence: ReportOffence<
            Self::AccountId,
            IdentificationTuple<Self>,
            SummaryOffence<IdentificationTuple<Self>>,
        >;

        /// Weight information for the extrinsics in this pallet.
        type WeightInfo: WeightInfo;
        /// An Ethereum bridge provider
        type BridgeInterface: avn::BridgeInterface;
        /// A flag to determine if summaries will be automatically sent to Ethereum
        type AutoSubmitSummaries: Get<bool>;
        /// A unique instance id to differentiate different instances
        type InstanceId: Get<u8>;
        /// A flag to determine if external validation is enabled
        type ExternalValidationEnabled: Get<bool>;
        /// A type that provides external validation of summary roots. Use Noop implementation to
        /// disable.
        #[pallet::no_default_bounds]
        type ExternalValidator: WatchtowerInterface;
    }

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T, I = ()>(_);

    #[pallet::event]
    /// This attribute generate the function `deposit_event` to deposit one of this pallet event,
    /// it is optional, it is also possible to provide a custom implementation.
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config<I>, I: 'static = ()> {
        /// Schedule period and voting period are updated
        SchedulePeriodAndVotingPeriodUpdated {
            schedule_period: BlockNumberFor<T>,
            voting_period: BlockNumberFor<T>,
        },
        /// Root hash of summary between from block number and to block number is calculated by a
        /// validator
        SummaryCalculated {
            from: BlockNumberFor<T>,
            to: BlockNumberFor<T>,
            root_hash: H256,
            submitter: T::AccountId,
        },
        /// Vote by a voter for a root id is added
        VoteAdded { voter: T::AccountId, root_id: RootId<BlockNumberFor<T>>, agree_vote: bool },
        /// Voting for the root id is finished, true means the root is approved
        VotingEnded { root_id: RootId<BlockNumberFor<T>>, vote_approved: bool },
        /// A summary offence by a list of offenders is reported
        SummaryOffenceReported {
            offence_type: SummaryOffenceType,
            offenders: Vec<IdentificationTuple<T>>,
        },
        /// A new slot between a range of blocks for a validator is advanced by an account
        SlotAdvanced {
            advanced_by: T::AccountId,
            new_slot: BlockNumberFor<T>,
            slot_validator: T::AccountId,
            slot_end: BlockNumberFor<T>,
        },
        /// A summary created by a challengee is challenged by a challenger for a reason
        ChallengeAdded {
            challenge_reason: SummaryChallengeReason,
            challenger: T::AccountId,
            challengee: T::AccountId,
        },
        /// An offence about a summary not be published by a challengee is reported
        SummaryNotPublishedOffence {
            challengee: T::AccountId,
            /* slot number where no summary was published */
            void_slot: BlockNumberFor<T>,
            /* slot where a block was last published */
            last_published: BlockNumberFor<T>,
            /* block number for end of the void slot */
            end_vote: BlockNumberFor<T>,
        },
        /// A summary root validated
        SummaryRootValidated {
            root_hash: H256,
            ingress_counter: IngressCounter,
            block_range: RootRange<BlockNumberFor<T>>,
        },
        /// Root failed validation so admin review requested
        AdminReviewRequested {
            root_id: RootId<BlockNumberFor<T>>,
            proposal_id: ProposalId,
            external_ref: H256,
            status: ProposalStatusEnum,
        },
        /// Root has been validated successfully
        RootPassedValidation { root_id: RootId<BlockNumberFor<T>>, root_hash: H256 },
        /// Root challenge has been resolved by an admin
        RootChallengeResolved { root_id: RootId<BlockNumberFor<T>>, accepted: bool },
        /// A new schedule period has been set
        SchedulePeriodSet { new_period: BlockNumberFor<T> },
        /// A new voting period has been set
        VotingPeriodSet { new_period: BlockNumberFor<T> },
        /// A new external validation threshold has been set
        ExternalValidationThresholdSet { new_threshold: u32 },
    }

    #[pallet::error]
    pub enum Error<T, I = ()> {
        Overflow,
        ErrorCalculatingChosenValidator,
        ErrorConvertingBlockNumber,
        ErrorGettingSummaryDataFromService,
        InvalidSummaryRange,
        ErrorSubmittingTransaction,
        InvalidKey,
        ErrorSigning,
        InvalidHexString,
        InvalidUTF8Bytes,
        InvalidRootHashLength,
        SummaryPendingOrApproved,
        RootHasAlreadyBeenRegisteredForVoting,
        InvalidRoot,
        DuplicateVote,
        ErrorEndingVotingPeriod,
        ErrorSubmitCandidateTxnToTier1,
        VotingSessionIsNotValid,
        ErrorRecoveringPublicKeyFromSignature,
        ECDSASignatureNotValid,
        RootDataNotFound,
        InvalidChallenge,
        WrongValidator,
        GracePeriodElapsed,
        TooEarlyToAdvance,
        InvalidIngressCounter,
        SchedulePeriodIsTooShort,
        VotingPeriodIsTooShort,
        VotingPeriodIsTooLong,
        VotingPeriodIsEqualOrLongerThanSchedulePeriod,
        CurrentSlotValidatorNotFound,
        ErrorPublishingSummary,
        /// There is no rootId for the given external reference
        ExternalRefNotFound,
        /// Threshold should be between 1 and 100
        InvalidExternalValidationThreshold,
        /// External validation threshold not set
        ExternalValidationThresholdNotSet,
        /// There is no external validation request for the given rootId
        ExternalValidationRequestNotFound,
        /// There is no external validation status for the given rootId
        ExternalValidationStatusMissing,
    }

    // Note for SYS-152 (see notes in fn end_voting)):
    // A new instance of root_range should only be accepted into the system
    // (record_summary_calculation) if:
    // - there is no previous instance of that root_range in roots
    // - if there is any such an instance, it does not exist in PendingApprovals and it is not
    //   validated
    // It does not help to remove the root_range from Roots. If that were the case, we would lose
    // the information the root has already been processed and so cannot be submitted (ie voted
    // on) again.

    #[pallet::storage]
    #[pallet::getter(fn get_next_block_to_process)]
    pub type NextBlockToProcess<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::storage]
    pub type TxIdToRoot<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, EthereumId, RootId<BlockNumberFor<T>>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn block_number_for_next_slot)]
    pub type NextSlotAtBlock<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn current_slot)]
    pub type CurrentSlot<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn slot_validator)]
    pub type CurrentSlotsValidator<T: Config<I>, I: 'static = ()> =
        StorageValue<_, T::AccountId, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn last_summary_slot)]
    pub type SlotOfLastPublishedSummary<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::storage]
    pub type Roots<T: Config<I>, I: 'static = ()> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        RootRange<BlockNumberFor<T>>,
        Blake2_128Concat,
        IngressCounter,
        RootData<T::AccountId>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn get_vote )]
    pub type VotesRepository<T: Config<I>, I: 'static = ()> = StorageMap<
        _,
        Blake2_128Concat,
        RootId<BlockNumberFor<T>>,
        VotingSessionData<T::AccountId, BlockNumberFor<T>>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn get_pending_roots)]
    pub type PendingApproval<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, RootRange<BlockNumberFor<T>>, IngressCounter, ValueQuery>;

    /// The total ingresses of roots
    #[pallet::storage]
    #[pallet::getter(fn get_ingress_counter)]
    pub type TotalIngresses<T: Config<I>, I: 'static = ()> =
        StorageValue<_, IngressCounter, ValueQuery>;

    /// A period (in block number) where summaries are calculated
    #[pallet::storage]
    #[pallet::getter(fn schedule_period)]
    pub type SchedulePeriod<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    /// A period (in block number) where validators are allowed to vote on the validity of a root
    /// hash
    #[pallet::storage]
    #[pallet::getter(fn voting_period)]
    pub type VotingPeriod<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn anchor_roots_counter)]
    pub type AnchorRootsCounter<T: Config<I>, I: 'static = ()> = StorageValue<_, u32, ValueQuery>;

    // Roots created to be anchored to other chains (apart from Ethereum)
    #[pallet::storage]
    #[pallet::getter(fn anchor_roots)]
    pub type AnchorRoots<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, u32, H256, ValueQuery>;

    /// Map from RootId to the status of its external validation
    #[pallet::storage]
    pub type ExternalValidationStatus<T: Config<I>, I: 'static = ()> = StorageMap<
        _,
        Blake2_128Concat,
        RootId<BlockNumberFor<T>>,
        ExternalValidationEnum,
        OptionQuery,
    >;

    /// Map from external reference (H256) to RootId
    #[pallet::storage]
    pub type ExternalValidationRef<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, H256, RootId<BlockNumberFor<T>>, OptionQuery>;

    /// Roots that failed external validation and are pending admin review
    #[pallet::storage]
    pub type PendingAdminReviews<T: Config<I>, I: 'static = ()> = StorageMap<
        _,
        Blake2_128Concat,
        RootId<BlockNumberFor<T>>,
        ExternalValidationData,
        OptionQuery,
    >;

    /// The threshold required for external validation to pass (in percent, e.g. 51 means 51%)
    #[pallet::storage]
    pub type ExternalValidationThreshold<T: Config<I>, I: 'static = ()> =
        StorageValue<_, u32, OptionQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config<I>, I: 'static = ()> {
        /// Dummy marker.
        pub _phantom: sp_std::marker::PhantomData<I>,
        pub schedule_period: BlockNumberFor<T>,
        pub voting_period: BlockNumberFor<T>,
    }

    // #[cfg(feature = "std")]
    impl<T: Config<I>, I: 'static> Default for GenesisConfig<T, I> {
        fn default() -> Self {
            Self {
                _phantom: Default::default(),
                schedule_period: BlockNumberFor::<T>::from(DEFAULT_SCHEDULE_PERIOD),
                voting_period: BlockNumberFor::<T>::from(DEFAULT_VOTING_PERIOD),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config<I>, I: 'static> BuildGenesisConfig for GenesisConfig<T, I> {
        fn build(&self) {
            let mut schedule_period_in_blocks = self.schedule_period;
            if schedule_period_in_blocks == 0u32.into() {
                schedule_period_in_blocks = DEFAULT_SCHEDULE_PERIOD.into();
            }
            assert!(
                Pallet::<T, I>::validate_schedule_period(schedule_period_in_blocks).is_ok(),
                "Schedule Period must be a valid value"
            );
            <NextSlotAtBlock<T, I>>::put(schedule_period_in_blocks);
            <SchedulePeriod<T, I>>::put(schedule_period_in_blocks);

            let mut voting_period_in_blocks = self.voting_period;
            if voting_period_in_blocks == 0u32.into() {
                voting_period_in_blocks = MIN_VOTING_PERIOD.into();
            }
            assert!(
                Pallet::<T, I>::validate_voting_period(
                    voting_period_in_blocks,
                    schedule_period_in_blocks
                )
                .is_ok(),
                "Voting Period must be a valid value"
            );
            <VotingPeriod<T, I>>::put(voting_period_in_blocks);

            let maybe_first_validator =
                AVN::<T>::validators().into_iter().map(|v| v.account_id).nth(0);
            assert!(maybe_first_validator.is_some(), "You must add validators to run the AvN");

            <CurrentSlotsValidator<T, I>>::put(
                maybe_first_validator.expect("Validator is checked for none"),
            );

            STORAGE_VERSION.put::<Pallet<T, I>>();
        }
    }

    #[pallet::call(weight(<T as Config<I>>::WeightInfo))]
    impl<T: Config<I>, I: 'static> Pallet<T, I> {
        #[allow(deprecated)]
        #[deprecated(note = "This extrinsic is deprecated, please use `set_admin_config` instead.")]
        #[pallet::weight(<T as pallet::Config<I>>::WeightInfo::set_periods())]
        #[pallet::call_index(0)]
        pub fn set_periods(
            origin: OriginFor<T>,
            schedule_period_in_blocks: BlockNumberFor<T>,
            voting_period_in_blocks: BlockNumberFor<T>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            Self::validate_schedule_period(schedule_period_in_blocks)?;
            Self::validate_voting_period(voting_period_in_blocks, schedule_period_in_blocks)?;

            let next_block_to_process = <NextBlockToProcess<T, I>>::get();
            let new_slot_at_block =
                safe_add_block_numbers(next_block_to_process, schedule_period_in_blocks)
                    .map_err(|_| Error::<T, I>::Overflow)?;

            <SchedulePeriod<T, I>>::put(schedule_period_in_blocks);
            <VotingPeriod<T, I>>::put(voting_period_in_blocks);
            <NextSlotAtBlock<T, I>>::put(new_slot_at_block);

            Self::deposit_event(Event::<T, I>::SchedulePeriodAndVotingPeriodUpdated {
                schedule_period: schedule_period_in_blocks,
                voting_period: voting_period_in_blocks,
            });
            Ok(())
        }

        #[pallet::weight(<T as pallet::Config<I>>::WeightInfo::record_summary_calculation(
            MAX_VALIDATOR_ACCOUNTS,
            MAX_NUMBER_OF_ROOT_DATA_PER_RANGE
        ))]
        #[pallet::call_index(1)]
        pub fn record_summary_calculation(
            origin: OriginFor<T>,
            new_block_number: BlockNumberFor<T>,
            root_hash: H256,
            ingress_counter: IngressCounter,
            validator: Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
            _signature: <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;
            ensure!(
                Self::get_ingress_counter() + 1 == ingress_counter,
                Error::<T, I>::InvalidIngressCounter
            );
            ensure!(AVN::<T>::is_validator(&validator.account_id), Error::<T, I>::InvalidKey);

            let root_range = RootRange::new(Self::get_next_block_to_process(), new_block_number);
            let root_id = RootId::new(root_range, ingress_counter);
            let expected_target_block = Self::get_target_block()?;
            let current_block_number = <system::Pallet<T>>::block_number();

            ensure!(
                Self::summary_is_neither_pending_nor_approved(&root_id.range),
                Error::<T, I>::SummaryPendingOrApproved
            );
            ensure!(
                !<VotesRepository<T, I>>::contains_key(root_id),
                Error::<T, I>::RootHasAlreadyBeenRegisteredForVoting
            );
            ensure!(new_block_number == expected_target_block, Error::<T, I>::InvalidSummaryRange);

            let quorum = AVN::<T>::quorum();
            let voting_period_end =
                safe_add_block_numbers(current_block_number, Self::voting_period())
                    .map_err(|_| Error::<T, I>::Overflow)?;

            <TotalIngresses<T, I>>::put(ingress_counter);
            <Roots<T, I>>::insert(
                &root_id.range,
                ingress_counter,
                RootData::new(root_hash, validator.account_id.clone(), None),
            );
            <PendingApproval<T, I>>::insert(root_id.range, ingress_counter);
            <VotesRepository<T, I>>::insert(
                root_id,
                VotingSessionData::new(
                    root_id.session_id(),
                    quorum,
                    voting_period_end,
                    current_block_number,
                ),
            );

            Self::deposit_event(Event::<T, I>::SummaryCalculated {
                from: root_id.range.from_block,
                to: root_id.range.to_block,
                root_hash,
                submitter: validator.account_id,
            });
            Ok(())
        }

        #[pallet::weight(<T as pallet::Config<I>>::WeightInfo::approve_root_with_end_voting(MAX_VALIDATOR_ACCOUNTS, MAX_OFFENDERS).max(
            <T as Config<I>>::WeightInfo::approve_root_without_end_voting(MAX_VALIDATOR_ACCOUNTS)
        ))]
        #[pallet::call_index(2)]
        pub fn approve_root(
            origin: OriginFor<T>,
            root_id: RootId<BlockNumberFor<T>>,
            validator: Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;
            let _ = Self::try_get_root_data(&root_id)?;

            let voting_session = Self::get_root_voting_session(&root_id);

            process_approve_vote::<T>(&voting_session, validator.account_id.clone())?;

            Self::deposit_event(Event::<T, I>::VoteAdded {
                voter: validator.account_id,
                root_id,
                agree_vote: true,
            });
            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            Ok(())
        }

        #[pallet::weight(<T as pallet::Config<I>>::WeightInfo::reject_root_with_end_voting(MAX_VALIDATOR_ACCOUNTS, MAX_OFFENDERS).max(
            <T as Config<I>>::WeightInfo::reject_root_without_end_voting(MAX_VALIDATOR_ACCOUNTS)
        ))]
        #[pallet::call_index(3)]
        pub fn reject_root(
            origin: OriginFor<T>,
            root_id: RootId<BlockNumberFor<T>>,
            validator: Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;
            let voting_session = Self::get_root_voting_session(&root_id);
            process_reject_vote::<T>(&voting_session, validator.account_id.clone())?;

            Self::deposit_event(Event::<T, I>::VoteAdded {
                voter: validator.account_id,
                root_id,
                agree_vote: false,
            });
            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            Ok(())
        }

        #[pallet::weight(<T as pallet::Config<I>>::WeightInfo::end_voting_period_with_rejected_valid_votes(MAX_VALIDATOR_ACCOUNTS, MAX_OFFENDERS).max(
            <T as Config<I>>::WeightInfo::end_voting_period_with_approved_invalid_votes(MAX_VALIDATOR_ACCOUNTS, MAX_OFFENDERS)
        ))]
        #[pallet::call_index(4)]
        pub fn end_voting_period(
            origin: OriginFor<T>,
            root_id: RootId<BlockNumberFor<T>>,
            validator: Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;
            //Event is deposited in end_voting because this function can get called from
            // `approve_root` or `reject_root`
            Self::end_voting(validator.account_id, &root_id)?;

            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            Ok(())
        }

        #[pallet::weight(<T as pallet::Config<I>>::WeightInfo::advance_slot_with_offence(MAX_VALIDATOR_ACCOUNTS).max(
            <T as Config<I>>::WeightInfo::advance_slot_without_offence(MAX_VALIDATOR_ACCOUNTS)
        ))]
        #[pallet::call_index(5)]
        pub fn advance_slot(
            origin: OriginFor<T>,
            validator: Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;

            Self::update_slot_number(validator)?;

            // TODO [TYPE: weightInfo][PRI: medium]: Return accurate weight
            Ok(())
        }

        #[pallet::weight(<T as pallet::Config<I>>::WeightInfo::add_challenge(MAX_VALIDATOR_ACCOUNTS))]
        #[pallet::call_index(6)]
        pub fn add_challenge(
            origin: OriginFor<T>,
            challenge: SummaryChallenge<T::AccountId>,
            validator: Validator<T::AuthorityId, T::AccountId>,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;
            ensure!(
                challenge.is_valid::<T, I>(
                    Self::current_slot(),
                    <frame_system::Pallet<T>>::block_number(),
                    &challenge.challengee
                ),
                Error::<T, I>::InvalidChallenge
            );
            // QUESTION: offence: do we slash the author of an invalid challenge?
            // I think it is probably too harsh. It may not be valid for timing reasons:
            // it arrived too early, or the slot has already moved and the validator changed

            let offender = challenge.challengee.clone();
            let challenge_type = match challenge.challenge_reason {
                SummaryChallengeReason::SlotNotAdvanced(_) =>
                    Some(SummaryOffenceType::SlotNotAdvanced),
                SummaryChallengeReason::Unknown => None,
            };

            // if this fails, it is a bug. All challenge types should have a corresponding offence
            // type except for Unknown which we should never produce
            ensure!(!challenge_type.is_none(), Error::<T, I>::InvalidChallenge);

            create_and_report_summary_offence::<T, I>(
                &validator.account_id,
                &vec![offender],
                challenge_type.expect("Already checked"),
            );

            Self::update_slot_number(validator)?;

            Self::deposit_event(Event::<T, I>::ChallengeAdded {
                challenge_reason: challenge.challenge_reason,
                challenger: challenge.challenger,
                challengee: challenge.challengee,
            });

            Ok(())
        }

        /// Set admin configurations
        #[pallet::call_index(7)]
        #[pallet::weight(
             <T as pallet::Config<I>>::WeightInfo::set_external_validation_threshold()
            .max(<T as pallet::Config<I>>::WeightInfo::set_schedule_period())
            .max(<T as pallet::Config<I>>::WeightInfo::set_voting_period())
        )]
        pub fn set_admin_config(
            origin: OriginFor<T>,
            config: AdminConfig<BlockNumberFor<T>>,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            match config {
                AdminConfig::ExternalValidationThreshold(threshold) => {
                    <ExternalValidationThreshold<T, I>>::mutate(|p| *p = Some(threshold));
                    ensure!(
                        threshold > 0 && threshold <= 100,
                        Error::<T, I>::InvalidExternalValidationThreshold
                    );
                    <ExternalValidationThreshold<T, I>>::put(threshold);
                    Self::deposit_event(Event::ExternalValidationThresholdSet {
                        new_threshold: threshold,
                    });
                    return Ok(Some(
                        <T as Config<I>>::WeightInfo::set_external_validation_threshold(),
                    )
                    .into())
                },
                AdminConfig::SchedulePeriod(period) => {
                    Self::validate_schedule_period(period)?;
                    let voting_period = <VotingPeriod<T, I>>::get();
                    Self::validate_voting_period(voting_period, period)?;

                    let next_block_to_process = <NextBlockToProcess<T, I>>::get();
                    let new_slot_at_block = safe_add_block_numbers(next_block_to_process, period)
                        .map_err(|_| Error::<T, I>::Overflow)?;

                    <SchedulePeriod<T, I>>::put(period);
                    <NextSlotAtBlock<T, I>>::put(new_slot_at_block);

                    Self::deposit_event(Event::SchedulePeriodSet { new_period: period });
                    return Ok(Some(<T as Config<I>>::WeightInfo::set_schedule_period()).into())
                },
                AdminConfig::VotingPeriod(period) => {
                    let schedule_period = <SchedulePeriod<T, I>>::get();
                    Self::validate_voting_period(period, schedule_period)?;

                    <VotingPeriod<T, I>>::mutate(|p| *p = period);

                    Self::deposit_event(Event::VotingPeriodSet { new_period: period });
                    return Ok(Some(<T as Config<I>>::WeightInfo::set_voting_period()).into())
                },
            }
        }

        #[pallet::weight(
             <T as pallet::Config<I>>::WeightInfo::admin_resolve_challenge_accepted()
            .max(<T as pallet::Config<I>>::WeightInfo::admin_resolve_challenge_rejected())
        )]
        #[pallet::call_index(8)]
        pub fn admin_resolve_challenge(
            origin: OriginFor<T>,
            root_id: RootId<BlockNumberFor<T>>,
            accepted: bool,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            let pending_review = <PendingAdminReviews<T, I>>::get(&root_id)
                .ok_or(Error::<T, I>::ExternalValidationRequestNotFound)?;

            let root_data = Self::try_get_root_data(&root_id)?;
            let external_validation_status = <ExternalValidationStatus<T, I>>::get(&root_id)
                .ok_or(Error::<T, I>::ExternalValidationStatusMissing)?;

            ensure!(
                external_validation_status == ExternalValidationEnum::PendingAdminReview,
                Error::<T, I>::InvalidRoot
            );

            if accepted {
                Self::process_accepted_root(&root_id, root_data.root_hash)?;
            }

            Self::cleanup_external_validation_data(&root_id, &pending_review.external_ref);
            Self::deposit_event(Event::<T, I>::RootChallengeResolved { root_id, accepted });

            let weight = if accepted {
                <T as Config<I>>::WeightInfo::admin_resolve_challenge_accepted()
            } else {
                <T as Config<I>>::WeightInfo::admin_resolve_challenge_rejected()
            };

            Ok(Some(weight).into())
        }
    }

    #[pallet::hooks]
    impl<T: Config<I>, I: 'static> Hooks<BlockNumberFor<T>> for Pallet<T, I> {
        fn offchain_worker(block_number: BlockNumberFor<T>) {
            let setup_result = AVN::<T>::pre_run_setup(block_number, Self::pallet_id());
            if let Err(e) = setup_result {
                if sp_io::offchain::is_validator() {
                    match e {
                        _ if e == DispatchError::from(avn_error::<T>::OffchainWorkerAlreadyRun) => {
                            ();
                        },
                        _ => {
                            log::error!(
                                "💔️ Instance({}) Unable to run offchain worker: {:?}",
                                T::InstanceId::get(),
                                e
                            );
                        },
                    };
                }

                return
            }
            let (this_validator, _) = setup_result.expect("We have a validator");

            Self::advance_slot_if_required(block_number, &this_validator);
            Self::process_summary_if_required(block_number, &this_validator);
            cast_votes_if_required::<T, I>(&this_validator);
            end_voting_if_required::<T, I>(block_number, &this_validator);
            challenge_slot_if_required::<T, I>(block_number, &this_validator);
        }

        fn on_runtime_upgrade() -> Weight {
            let onchain = Pallet::<T, I>::on_chain_storage_version();

            if onchain < 1 {
                log::info!(
                    "💽 Running Summary pallet migration with current storage version {:?} / onchain {:?}",
                    Pallet::<T, I>::in_code_storage_version(),
                    onchain
                );

                let schedule_period_in_blocks: BlockNumberFor<T> = DEFAULT_SCHEDULE_PERIOD.into();
                <NextSlotAtBlock<T, I>>::put(schedule_period_in_blocks);
                <SchedulePeriod<T, I>>::put(schedule_period_in_blocks);

                let voting_period_in_blocks: BlockNumberFor<T> = MIN_VOTING_PERIOD.into();
                <VotingPeriod<T, I>>::put(voting_period_in_blocks);

                let maybe_first_validator =
                    AVN::<T>::validators().into_iter().map(|v| v.account_id).nth(0);

                <CurrentSlotsValidator<T, I>>::put(
                    maybe_first_validator.expect("Validator is checked for none"),
                );

                STORAGE_VERSION.put::<Pallet<T, I>>();

                return T::DbWeight::get().reads_writes(0, 5)
            }

            Weight::zero()
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config<I>, I: 'static> ValidateUnsigned for Pallet<T, I> {
        type Call = Call<T, I>;

        fn validate_unsigned(source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            if let Call::record_summary_calculation { .. } = call {
                return Self::record_summary_validate_unsigned(source, call)
            } else if let Call::end_voting_period { root_id, validator, signature } = call {
                let root_voting_session = Self::get_root_voting_session(root_id);
                return end_voting_period_validate_unsigned::<T>(
                    &root_voting_session,
                    validator,
                    signature,
                )
            } else if let Call::approve_root { root_id, validator, signature } = call {
                if !<Roots<T, I>>::contains_key(root_id.range, root_id.ingress_counter) {
                    return InvalidTransaction::Custom(ERROR_CODE_INVALID_ROOT_RANGE).into()
                }

                let root_voting_session = Self::get_root_voting_session(root_id);

                return approve_vote_validate_unsigned::<T>(
                    &root_voting_session,
                    validator,
                    signature,
                )
            } else if let Call::reject_root { root_id, validator, signature } = call {
                let root_voting_session = Self::get_root_voting_session(root_id);
                return reject_vote_validate_unsigned::<T>(
                    &root_voting_session,
                    validator,
                    signature,
                )
            } else if let Call::add_challenge { challenge, validator, signature } = call {
                return add_challenge_validate_unsigned::<T, I>(challenge, validator, signature)
            } else if let Call::advance_slot { .. } = call {
                return Self::advance_slot_validate_unsigned(source, call)
            } else {
                return InvalidTransaction::Call.into()
            }
        }
    }
    impl<T: Config<I>, I: 'static> Pallet<T, I> {
        pub fn update_block_number_context() -> Vec<u8> {
            let mut context = Vec::with_capacity(1 + UPDATE_BLOCK_NUMBER_CONTEXT.len());
            context.push(T::InstanceId::get());
            context.extend_from_slice(UPDATE_BLOCK_NUMBER_CONTEXT);
            context
        }

        pub fn advance_block_context() -> Vec<u8> {
            let mut context = Vec::with_capacity(1 + ADVANCE_SLOT_CONTEXT.len());
            context.push(T::InstanceId::get());
            context.extend_from_slice(ADVANCE_SLOT_CONTEXT);
            context
        }

        fn validate_schedule_period(
            schedule_period_in_blocks: BlockNumberFor<T>,
        ) -> DispatchResult {
            ensure!(
                schedule_period_in_blocks >= MIN_SCHEDULE_PERIOD.into(),
                Error::<T, I>::SchedulePeriodIsTooShort
            );

            Ok(())
        }

        fn validate_voting_period(
            voting_period_in_blocks: BlockNumberFor<T>,
            schedule_period_in_blocks: BlockNumberFor<T>,
        ) -> DispatchResult {
            ensure!(
                voting_period_in_blocks >= MIN_VOTING_PERIOD.into(),
                Error::<T, I>::VotingPeriodIsTooShort
            );
            ensure!(
                voting_period_in_blocks < schedule_period_in_blocks,
                Error::<T, I>::VotingPeriodIsEqualOrLongerThanSchedulePeriod
            );
            ensure!(
                voting_period_in_blocks <= MAX_VOTING_PERIOD.into(),
                Error::<T, I>::VotingPeriodIsTooLong
            );
            Ok(())
        }

        pub fn grace_period_elapsed(block_number: BlockNumberFor<T>) -> bool {
            let diff = safe_sub_block_numbers::<BlockNumberFor<T>>(
                block_number,
                Self::block_number_for_next_slot(),
            )
            .unwrap_or(0u32.into());
            return diff > T::AdvanceSlotGracePeriod::get()
        }

        // Check if this validator is allowed
        // the slot's validator is challenged if it does not advance the slot inside the challenge
        // window. But this challenge will be checked later than when it was submitted, so it is
        // possible storage has changed by then. To prevent the validator escape the challenge, we
        // can allow it this change only inside the challenge window. Other validators can however
        // move the slot after the challenge window.
        pub fn validator_can_advance_slot(
            validator: &Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
        ) -> DispatchResult {
            let current_block_number = <frame_system::Pallet<T>>::block_number();
            ensure!(
                current_block_number >= Self::block_number_for_next_slot(),
                Error::<T, I>::TooEarlyToAdvance
            );

            let current_slot_validator =
                Self::slot_validator().ok_or(Error::<T, I>::CurrentSlotValidatorNotFound)?;

            if Self::grace_period_elapsed(current_block_number) {
                if validator.account_id == current_slot_validator {
                    return Err(Error::<T, I>::GracePeriodElapsed)?
                }
            } else {
                if validator.account_id != current_slot_validator {
                    return Err(Error::<T, I>::WrongValidator)?
                }
            }

            Ok(())
        }

        pub fn update_slot_number(
            validator: Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
        ) -> DispatchResult {
            Self::validator_can_advance_slot(&validator)?;
            // QUESTION: should we slash a validator who tries to advance the slot when it is not
            // their turn? This code is always called inside an unsigned transaction, so
            // in consensus. We can raise offences here.
            Self::register_offence_if_no_summary_created_in_slot(&validator);

            let new_slot_number =
                safe_add_block_numbers::<BlockNumberFor<T>>(Self::current_slot(), 1u32.into())
                    .map_err(|_| Error::<T, I>::Overflow)?;

            let new_validator_account_id =
                AVN::<T>::calculate_primary_validator_for_block(new_slot_number)?;

            let next_slot_start_block = safe_add_block_numbers::<BlockNumberFor<T>>(
                Self::block_number_for_next_slot(),
                Self::schedule_period(),
            )
            .map_err(|_| Error::<T, I>::Overflow)?;

            <CurrentSlot<T, I>>::put(new_slot_number);
            <CurrentSlotsValidator<T, I>>::put(new_validator_account_id.clone());
            <NextSlotAtBlock<T, I>>::put(next_slot_start_block);

            Self::deposit_event(Event::<T, I>::SlotAdvanced {
                advanced_by: validator.account_id,
                new_slot: new_slot_number,
                slot_validator: new_validator_account_id,
                slot_end: next_slot_start_block,
            });

            Ok(())
        }

        pub fn get_root_voting_session(
            root_id: &RootId<BlockNumberFor<T>>,
        ) -> Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>> {
            return Box::new(RootVotingSession::<T, I>::new(root_id))
                as Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>>
        }

        // // This can be called by other validators to verify the root hash
        pub fn compute_root_hash(
            from_block: BlockNumberFor<T>,
            to_block: BlockNumberFor<T>,
        ) -> Result<H256, DispatchError> {
            let from_block_number: u32 = TryInto::<u32>::try_into(from_block)
                .map_err(|_| Error::<T, I>::ErrorConvertingBlockNumber)?;
            let to_block_number: u32 = TryInto::<u32>::try_into(to_block)
                .map_err(|_| Error::<T, I>::ErrorConvertingBlockNumber)?;

            let mut url_path = "roothash/".to_string();
            url_path.push_str(&from_block_number.to_string());
            url_path.push_str(&"/".to_string());
            url_path.push_str(&to_block_number.to_string());

            let response = AVN::<T>::get_data_from_service(url_path);

            if let Err(e) = response {
                log::error!(
                    "💔️ Instance({}) Error getting summary data from external service: {:?}",
                    T::InstanceId::get(),
                    e
                );
                return Err(Error::<T, I>::ErrorGettingSummaryDataFromService)?
            }

            let root_hash = Self::validate_response(response.expect("checked for error"))?;
            log::trace!(target: "avn", "🥽 Instance({}) Calculated root hash {:?} for range [{:?}, {:?}]", T::InstanceId::get(), &root_hash, &from_block_number, &to_block_number);

            return Ok(root_hash)
        }

        pub fn create_root_lock_name(block_number: BlockNumberFor<T>) -> Vec<u8> {
            let mut name = b"create_summary::".to_vec();
            name.extend_from_slice(&mut block_number.encode());
            name
        }

        pub fn get_advance_slot_lock_name(block_number: BlockNumberFor<T>) -> Vec<u8> {
            let mut name = b"advance_slot::".to_vec();
            name.extend_from_slice(&mut block_number.encode());
            name
        }

        pub fn advance_slot_if_required(
            block_number: BlockNumberFor<T>,
            this_validator: &Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
        ) {
            let current_slot_validator = Self::slot_validator();
            if current_slot_validator.is_none() {
                log::error!(
                    "💔 Instance({}) Current slot validator is not found. Cannot advance slot for block: {:?}",
                    T::InstanceId::get(), block_number
                );
                return
            }

            if this_validator.account_id == current_slot_validator.expect("Checked for none") &&
                block_number >= Self::block_number_for_next_slot()
            {
                let advance_slot_lock_name = Self::get_advance_slot_lock_name(Self::current_slot());
                let mut lock = AVN::<T>::get_ocw_locker(&advance_slot_lock_name);

                // Protect against sending more than once. When guard is out of scope the lock will
                // be released.
                if let Ok(guard) = lock.try_lock() {
                    let result = Self::dispatch_advance_slot(this_validator);
                    if let Err(e) = result {
                        log::warn!(
                            "💔️ Instance({}) Error starting a new summary creation slot: {:?}",
                            T::InstanceId::get(),
                            e
                        );
                        //free the lock so we can potentially retry
                        drop(guard);
                        return
                    }

                    // If there are no errors, keep the lock to prevent doing the same logic again
                    guard.forget();
                };
            }
        }

        // called from OCW - no storage changes allowed here
        pub fn process_summary_if_required(
            block_number: BlockNumberFor<T>,
            this_validator: &Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
        ) {
            let target_block = Self::get_target_block();
            if target_block.is_err() {
                log::error!("💔️ Error getting target block.");
                return
            }
            let last_block_in_range = target_block.expect("Valid block number");

            if Self::can_process_summary(block_number, last_block_in_range, this_validator) {
                let root_lock_name = Self::create_root_lock_name(last_block_in_range);
                let mut lock = AVN::<T>::get_ocw_locker(&root_lock_name);

                // Protect against sending more than once. When guard is out of scope the lock will
                // be released.
                if let Ok(guard) = lock.try_lock() {
                    log::warn!(
                        "ℹ️  Processing summary for range {:?} - {:?}. Slot {:?}",
                        Self::get_next_block_to_process(),
                        last_block_in_range,
                        Self::current_slot()
                    );

                    let summary = Self::process_summary(last_block_in_range, this_validator);

                    if let Err(e) = summary {
                        log::warn!("💔️ Error processing summary: {:?}", e);
                        //free the lock so we can potentially retry
                        drop(guard);
                        return
                    }

                    // If there are no errors, keep the lock to prevent doing the same logic again
                    guard.forget();
                };
            }
        }

        fn register_offence_if_no_summary_created_in_slot(
            reporter: &Validator<T::AuthorityId, T::AccountId>,
        ) {
            if Self::last_summary_slot() < Self::current_slot() {
                let maybe_current_slot_validator = Self::slot_validator();
                if maybe_current_slot_validator.is_none() {
                    log::error!(
                        "💔 Current slot validator is not found. Unable to register offence"
                    );
                    return
                }
                let current_slot_validator =
                    maybe_current_slot_validator.expect("Checked for none");

                create_and_report_summary_offence::<T, I>(
                    &reporter.account_id,
                    &vec![current_slot_validator.clone()],
                    SummaryOffenceType::NoSummaryCreated,
                );

                Self::deposit_event(Event::<T, I>::SummaryNotPublishedOffence {
                    challengee: current_slot_validator,
                    void_slot: Self::current_slot(),
                    last_published: Self::last_summary_slot(),
                    end_vote: Self::block_number_for_next_slot(),
                });
            }
        }

        // // called from OCW - no storage changes allowed here
        fn can_process_summary(
            current_block_number: BlockNumberFor<T>,
            last_block_in_range: BlockNumberFor<T>,
            this_validator: &Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
        ) -> bool {
            if OcwLock::is_locked::<frame_system::Pallet<T>>(&Self::create_root_lock_name(
                last_block_in_range,
            )) {
                return false
            }

            let target_block_with_buffer =
                safe_add_block_numbers(last_block_in_range, T::MinBlockAge::get());

            if target_block_with_buffer.is_err() {
                log::warn!(
                    "💔️ Error checking if we can process a summary for blocks {:?} to {:?}",
                    current_block_number,
                    last_block_in_range
                );

                return false
            }
            let target_block_with_buffer = target_block_with_buffer.expect("Already checked");

            let root_range = RootRange::new(Self::get_next_block_to_process(), last_block_in_range);

            let current_slot_validator = Self::slot_validator();
            let is_slot_validator = current_slot_validator.is_some() &&
                this_validator.account_id == current_slot_validator.expect("checked for none");
            let slot_is_active = current_block_number < Self::block_number_for_next_slot();
            let blocks_are_old_enough = current_block_number > target_block_with_buffer;

            return is_slot_validator &&
                slot_is_active &&
                blocks_are_old_enough &&
                Self::summary_is_neither_pending_nor_approved(&root_range)
        }

        // called from OCW - no storage changes allowed here
        pub fn process_summary(
            last_block_in_range: BlockNumberFor<T>,
            validator: &Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
        ) -> DispatchResult {
            let root_hash =
                Self::compute_root_hash(Self::get_next_block_to_process(), last_block_in_range)?;
            Self::record_summary(last_block_in_range, root_hash, validator)?;

            Ok(())
        }

        // // called from OCW - no storage changes allowed here
        fn record_summary(
            last_processed_block_number: BlockNumberFor<T>,
            root_hash: H256,
            validator: &Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
        ) -> DispatchResult {
            let ingress_counter = Self::get_ingress_counter() + 1; // default value in storage is 0, so first root_hash has counter 1

            let signature = validator
                .key
                .sign(
                    &(
                        Self::update_block_number_context(),
                        root_hash,
                        ingress_counter,
                        last_processed_block_number,
                    )
                        .encode(),
                )
                .ok_or(Error::<T, I>::ErrorSigning)?;

            log::trace!(
                target: "avn",
                "🖊️  Worker records summary calculation: {:?} last processed block {:?} ingress: {:?}]",
                &root_hash,
                &last_processed_block_number,
                &ingress_counter
            );

            let xt = T::create_inherent(
                Call::record_summary_calculation {
                    new_block_number: last_processed_block_number,
                    root_hash,
                    ingress_counter,
                    validator: validator.clone(),
                    signature,
                }
                .into(),
            );
            SubmitTransaction::<T, Call<T, I>>::submit_transaction(xt)
                .map_err(|_| Error::<T, I>::ErrorSubmittingTransaction)?;

            Ok(())
        }

        fn dispatch_advance_slot(
            validator: &Validator<<T as avn::Config>::AuthorityId, T::AccountId>,
        ) -> DispatchResult {
            let signature = validator
                .key
                .sign(&(Self::advance_block_context(), Self::current_slot()).encode())
                .ok_or(Error::<T, I>::ErrorSigning)?;

            let xt = T::create_inherent(
                Call::advance_slot { validator: validator.clone(), signature }.into(),
            );

            SubmitTransaction::<T, Call<T, I>>::submit_transaction(xt)
                .map_err(|_| Error::<T, I>::ErrorSubmittingTransaction)?;

            Ok(())
        }

        pub fn get_target_block() -> Result<BlockNumberFor<T>, Error<T, I>> {
            let end_block_number = safe_add_block_numbers::<BlockNumberFor<T>>(
                Self::get_next_block_to_process(),
                Self::schedule_period(),
            )
            .map_err(|_| Error::<T, I>::Overflow)?;

            if Self::get_next_block_to_process() == 0u32.into() {
                return Ok(end_block_number)
            }

            Ok(safe_sub_block_numbers::<BlockNumberFor<T>>(end_block_number, 1u32.into())
                .map_err(|_| Error::<T, I>::Overflow)?)
        }

        fn validate_response(response: Vec<u8>) -> Result<H256, Error<T, I>> {
            if response.len() != 64 {
                log::error!(
                    "❌ Instance({}) Root hash is not valid: {:?}",
                    T::InstanceId::get(),
                    response
                );
                return Err(Error::<T, I>::InvalidRootHashLength)?
            }

            let root_hash = core::str::from_utf8(&response);
            if let Err(e) = root_hash {
                log::error!(
                    "❌ Instance({}) Error converting root hash bytes to string: {:?}",
                    T::InstanceId::get(),
                    e
                );
                return Err(Error::<T, I>::InvalidUTF8Bytes)?
            }

            let mut data: [u8; 32] = [0; 32];
            hex::decode_to_slice(root_hash.expect("Checked for error"), &mut data[..])
                .map_err(|_| Error::<T, I>::InvalidHexString)?;

            return Ok(H256::from_slice(&data))
        }

        pub fn send_root_to_ethereum(
            root_id: &RootId<BlockNumberFor<T>>,
            root_data: &RootData<T::AccountId>,
        ) -> DispatchResult {
            // There are a couple possible reasons for failure here.
            // 1. We fail before sending to T1: likely a bug on our part
            // 2. Quorum mismatch. There is no guarantee that between accepting a root and
            // submitting it to T1, the tier2 session hasn't changed and with it
            // the quorum, making ethereum-transactions reject it
            // In either case, we should not slash anyone.
            let function_name: &[u8] = BridgeContractMethod::PublishRoot.name_as_bytes();
            let params = vec![(b"bytes32".to_vec(), root_data.root_hash.as_fixed_bytes().to_vec())];
            let tx_id = T::BridgeInterface::publish(function_name, &params, Self::pallet_id())
                .map_err(|e| DispatchError::Other(e.into()))?;

            <Roots<T, I>>::mutate(root_id.range, root_id.ingress_counter, |root| {
                root.tx_id = Some(tx_id)
            });

            <TxIdToRoot<T, I>>::insert(tx_id, root_id);

            Ok(())
        }

        pub fn get_next_approved_root_id() -> Result<u32, DispatchError> {
            AnchorRootsCounter::<T, I>::try_mutate(|counter| {
                let current_counter = *counter;
                *counter = counter.checked_add(1).ok_or(Error::<T, I>::Overflow)?;
                Ok(current_counter)
            })
        }

        pub fn end_voting(
            reporter: T::AccountId,
            root_id: &RootId<BlockNumberFor<T>>,
        ) -> DispatchResult {
            let voting_session = Self::get_root_voting_session(&root_id);

            ensure!(voting_session.is_valid(), Error::<T, I>::VotingSessionIsNotValid);

            let vote = Self::get_vote(root_id);
            ensure!(Self::can_end_vote(&vote), Error::<T, I>::ErrorEndingVotingPeriod);

            let root_is_approved = vote.is_approved();

            let root_data = Self::try_get_root_data(&root_id)?;
            if root_is_approved {
                if root_data.root_hash != Self::empty_root() {
                    if T::ExternalValidationEnabled::get() {
                        Self::submit_root_for_external_validation(root_id, root_data.root_hash)?;
                    } else {
                        Self::process_accepted_root(root_id, root_data.root_hash)?;
                    }
                }
                // If we get here, then we did not get an error when submitting to T1.

                create_and_report_summary_offence::<T, I>(
                    &reporter,
                    &vote.nays,
                    SummaryOffenceType::RejectedValidRoot,
                );

                let next_block_to_process = safe_add_block_numbers::<BlockNumberFor<T>>(
                    root_id.range.to_block,
                    1u32.into(),
                )
                .map_err(|_| Error::<T, I>::Overflow)?;

                <NextBlockToProcess<T, I>>::put(next_block_to_process);
                <Roots<T, I>>::mutate(root_id.range, root_id.ingress_counter, |root| {
                    root.is_validated = true
                });
                <SlotOfLastPublishedSummary<T, I>>::put(Self::current_slot());

                Self::deposit_event(Event::<T, I>::SummaryRootValidated {
                    root_hash: root_data.root_hash,
                    ingress_counter: root_id.ingress_counter,
                    block_range: root_id.range,
                });
            } else {
                // We didn't get enough votes to approve this root

                let root_creator =
                    root_data.added_by.ok_or(Error::<T, I>::CurrentSlotValidatorNotFound)?;
                create_and_report_summary_offence::<T, I>(
                    &reporter,
                    &vec![root_creator],
                    SummaryOffenceType::CreatedInvalidRoot,
                );

                create_and_report_summary_offence::<T, I>(
                    &reporter,
                    &vote.ayes,
                    SummaryOffenceType::ApprovedInvalidRoot,
                );
            }

            <PendingApproval<T, I>>::remove(root_id.range);

            // When we get here, the root's voting session has ended and it has been removed from
            // PendingApproval If the root was approved, it is now marked as validated.
            // Otherwise, it stays false. If there was an error when submitting to T1, none of
            // this happened and it is still pending and not validated. In either case, the whole
            // voting history remains in storage

            // NOTE: when SYS-152 work is added here, root_range could exist several times in the
            // voting history, since a root_range that is rejected must eventually be
            // submitted again. But at any given time, there should be a single instance
            // of root_range in the PendingApproval queue. It is possible to keep
            // several instances of root_range in the Roots repository. But that should
            // not change the logic in this area: we should still validate an approved
            // (root_range, counter) and remove this pair from PendingApproval if no
            // errors occur.

            Self::deposit_event(Event::<T, I>::VotingEnded {
                root_id: *root_id,
                vote_approved: root_is_approved,
            });

            Ok(())
        }

        fn can_end_vote(vote: &VotingSessionData<T::AccountId, BlockNumberFor<T>>) -> bool {
            return vote.has_outcome() ||
                <system::Pallet<T>>::block_number() >= vote.end_of_voting_period
        }

        fn record_summary_validate_unsigned(
            _source: TransactionSource,
            call: &Call<T, I>,
        ) -> TransactionValidity {
            if let Call::record_summary_calculation {
                new_block_number,
                root_hash,
                ingress_counter,
                validator,
                signature,
            } = call
            {
                let current_slot_validator = Self::slot_validator();
                if current_slot_validator.is_none() ||
                    validator.account_id != current_slot_validator.expect("checked for none")
                {
                    return InvalidTransaction::Custom(ERROR_CODE_VALIDATOR_IS_NOT_PRIMARY).into()
                }

                let signed_data = &(
                    Self::update_block_number_context(),
                    root_hash,
                    ingress_counter,
                    new_block_number,
                );
                if !AVN::<T>::signature_is_valid(signed_data, &validator, signature) {
                    return InvalidTransaction::BadProof.into()
                };

                return ValidTransaction::with_tag_prefix("Summary")
                    .priority(TransactionPriority::max_value())
                    .and_provides(vec![(
                        Self::update_block_number_context(),
                        root_hash,
                        ingress_counter,
                    )
                        .encode()])
                    .longevity(64_u64)
                    .propagate(true)
                    .build()
            }

            return InvalidTransaction::Call.into()
        }

        fn advance_slot_validate_unsigned(
            _source: TransactionSource,
            call: &Call<T, I>,
        ) -> TransactionValidity {
            if let Call::advance_slot { validator, signature } = call {
                let current_slot_validator = Self::slot_validator();
                if current_slot_validator.is_none() ||
                    validator.account_id != current_slot_validator.expect("checked for none")
                {
                    return InvalidTransaction::Custom(ERROR_CODE_VALIDATOR_IS_NOT_PRIMARY).into()
                }

                // QUESTION: slash here? If we check the signature validity first, then fail the
                // check for slot_validator we would prove someone tried to advance
                // the slot outside their turn. Should this be slashable?

                let current_slot = Self::current_slot();
                let signed_data = &(Self::advance_block_context(), current_slot);
                if !AVN::<T>::signature_is_valid(signed_data, &validator, signature) {
                    return InvalidTransaction::BadProof.into()
                };

                return ValidTransaction::with_tag_prefix("Summary")
                    .priority(TransactionPriority::max_value())
                    .and_provides(vec![(Self::advance_block_context(), current_slot).encode()])
                    .longevity(64_u64)
                    .propagate(true)
                    .build()
            }

            return InvalidTransaction::Call.into()
        }

        pub fn empty_root() -> H256 {
            return H256::from_slice(&[0; 32])
        }

        fn summary_is_neither_pending_nor_approved(
            root_range: &RootRange<BlockNumberFor<T>>,
        ) -> bool {
            let has_been_approved =
                <Roots<T, I>>::iter_prefix_values(root_range).any(|root| root.is_validated);
            let is_pending = <PendingApproval<T, I>>::contains_key(root_range);

            return !is_pending && !has_been_approved
        }

        pub fn try_get_root_data(
            root_id: &RootId<BlockNumberFor<T>>,
        ) -> Result<RootData<T::AccountId>, Error<T, I>> {
            if <Roots<T, I>>::contains_key(root_id.range, root_id.ingress_counter) {
                return Ok(<Roots<T, I>>::get(root_id.range, root_id.ingress_counter))
            }

            Err(Error::<T, I>::RootDataNotFound)?
        }

        pub(crate) fn pallet_id() -> Vec<u8> {
            [PALLET_ID.to_vec(), vec![T::InstanceId::get()]].concat()
        }
    }

    impl<T: Config<I>, I: 'static, P> WatchtowerHooks<P> for Pallet<T, I> {
        fn on_proposal_submitted(_id: ProposalId, _p: P) -> DispatchResult {
            Ok(())
        }

        fn on_voting_completed(
            proposal_id: ProposalId,
            external_ref: &H256,
            result: &ProposalStatusEnum,
        ) {
            if let Ok(root_id) = Self::get_root_id_by_external_ref(external_ref) {
                if matches!(result, ProposalStatusEnum::Expired) ||
                    matches!(result, ProposalStatusEnum::Resolved { passed: true })
                {
                    let root_data = match Self::try_get_root_data(&root_id) {
                        Ok(data) => data,
                        Err(e) => {
                            log::error!(
                                "💔 Root data not found. ProposalId {:?}. External ref {:?}. Err: {:?}",
                                proposal_id,
                                external_ref,
                                e
                            );
                            return
                        },
                    };

                    Self::set_summary_status(&root_id, ExternalValidationEnum::Accepted);

                    if let Err(e) = Self::process_accepted_root(&root_id, root_data.root_hash) {
                        log::error!("💔 Processing on_voting_completed error. ProposalId {:?}. External ref {:?}. Err: {:?}",
                            proposal_id,
                            external_ref,
                            e
                        );
                        return
                    };

                    Self::cleanup_external_validation_data(&root_id, external_ref);
                } else {
                    Self::setup_root_for_admin_review(
                        root_id,
                        proposal_id,
                        external_ref.clone(),
                        result.clone(),
                    );
                }
            }
        }

        fn on_cancelled(proposal_id: ProposalId, external_ref: &H256) {
            if let Ok(root_id) = Self::get_root_id_by_external_ref(external_ref) {
                Self::setup_root_for_admin_review(
                    root_id,
                    proposal_id,
                    external_ref.clone(),
                    ProposalStatusEnum::Cancelled,
                );
            }
        }
    }
}

impl<T: Config<I>, I: 'static> BridgeInterfaceNotification for Pallet<T, I> {
    fn process_result(tx_id: u32, caller_id: Vec<u8>, succeeded: bool) -> DispatchResult {
        let matches_caller = if T::AutoSubmitSummaries::get() {
            // This is to enable backwards compatibility since the id of the pallet has changed.
            // The instance that is auto submitting summaries is allowed to process old results.
            // So pallet with id "summary-1" that used to be "summary" should handle the old results
            // as well. This can be removed once this has been rolled out.
            Self::pallet_id().starts_with(&caller_id)
        } else {
            caller_id == Self::pallet_id()
        };
        if matches_caller && <TxIdToRoot<T, I>>::contains_key(tx_id) {
            if succeeded {
                let root_id = <TxIdToRoot<T, I>>::get(tx_id);
                <Roots<T, I>>::mutate(root_id.range, root_id.ingress_counter, |root| {
                    root.is_finalised = true;
                });
                log::info!(
                    "✅  Transaction with ID {} was successfully published to Ethereum.",
                    tx_id
                );
                // Reclaim storage space
                <TxIdToRoot<T, I>>::remove(tx_id);
            } else {
                log::error!("❌ Transaction with ID {} failed to publish to Ethereum.", tx_id);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;

#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/tests_vote.rs"]
mod tests_vote;

#[cfg(test)]
#[path = "tests/tests_validate_unsigned.rs"]
mod tests_validate_unsigned;

#[cfg(test)]
#[path = "tests/tests_slot_logic.rs"]
mod tests_slots;

#[cfg(test)]
#[path = "tests/tests_challenge.rs"]
mod tests_challenge;

#[cfg(test)]
#[path = "tests/tests_set_periods.rs"]
mod tests_set_periods;

#[cfg(test)]
#[path = "tests/test_ocw_locks.rs"]
mod test_ocw_locks;

#[cfg(test)]
#[path = "tests/anchor_tests.rs"]
mod anchor_tests;
