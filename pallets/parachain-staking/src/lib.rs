#![doc = include_str!("../README.md")]
// Copyright 2019-2022 PureStake Inc.
// Copyright 2025 Aventus Network Services Ltd.
// This file is part of Moonbeam and Aventus.

// Moonbeam is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonbeam is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonbeam.  If not, see <http://www.gnu.org/licenses/>.
#![recursion_limit = "256"]
#![cfg_attr(not(feature = "std"), no_std)]

pub mod calls;
mod nomination_requests;
pub mod proxy_methods;
pub mod session_handler;
mod set;
pub mod types;
pub mod weights;

#[cfg(any(test, feature = "runtime-benchmarks"))]
mod benchmarks;
#[cfg(test)]
#[path = "tests/bond_extra_tests.rs"]
mod bond_extra_tests;
#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/nominate_tests.rs"]
mod nominate_tests;
#[cfg(test)]
#[path = "tests/schedule_revoke_nomination_tests.rs"]
mod schedule_revoke_nomination_tests;
#[cfg(test)]
#[path = "tests/schedule_unbond_tests.rs"]
mod schedule_unbond_tests;
#[cfg(test)]
#[path = "tests/test_admin_settings.rs"]
mod test_admin_settings;
#[cfg(test)]
#[path = "tests/test_bounded_ordered_set.rs"]
mod test_bounded_ordered_set;
#[cfg(test)]
#[path = "tests/test_growth.rs"]
mod test_growth;
#[cfg(test)]
#[path = "tests/test_reward_payout.rs"]
mod test_reward_payout;
#[cfg(test)]
#[path = "tests/test_staking_pot.rs"]
mod test_staking_pot;
#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

use frame_support::pallet;
pub use weights::WeightInfo;

pub use nomination_requests::{CancelledScheduledRequest, NominationAction, ScheduledRequest};
pub use pallet::*;
pub use types::*;

pub type AVN<T> = pallet_avn::Pallet<T>;
pub const PALLET_ID: &'static [u8; 17] = b"parachain_staking";
pub const MAX_OFFENDERS: u32 = 2;

fn is_staking_enabled() -> bool {
    if cfg!(not(test)) {
        false
    } else {
        true
    }
}

#[pallet]
pub mod pallet {
    #[cfg(not(feature = "std"))]
    extern crate alloc;
    #[cfg(not(feature = "std"))]
    use alloc::{format, string::String};

    use crate::{is_staking_enabled, set::BoundedOrderedSet};
    pub use crate::{
        nomination_requests::{CancelledScheduledRequest, NominationAction, ScheduledRequest},
        proxy_methods::*,
        set::OrderedSet,
        types::*,
        WeightInfo, AVN, MAX_OFFENDERS, PALLET_ID,
    };
    pub use frame_support::{
        dispatch::{GetDispatchInfo, PostDispatchInfo},
        pallet_prelude::*,
        traits::{
            tokens::WithdrawReasons, Currency, ExistenceRequirement, Get, Imbalance, IsSubType,
            LockIdentifier, LockableCurrency, ReservableCurrency, ValidatorRegistration,
        },
        transactional, PalletId,
    };
    pub use frame_system::{
        offchain::{CreateInherent, CreateTransactionBase, SubmitTransaction},
        pallet_prelude::*,
    };
    pub use pallet_avn::{
        self as avn, AccountToBytesConverter, BridgeInterface, BridgeInterfaceNotification,
        CollatorPayoutDustHandler, Error as avn_error, OnGrowthLiftedHandler,
        ProcessedEventsChecker,
    };

    pub use sp_avn_common::{
        bounds::VotingSessionIdBound, event_types::Validator, safe_add_block_numbers,
        verify_signature, BridgeContractMethod, IngressCounter, Proof,
    };
    pub use sp_runtime::{
        traits::{
            AccountIdConversion, Bounded, CheckedAdd, CheckedDiv, CheckedSub, Dispatchable,
            IdentifyAccount, Member, Saturating, StaticLookup, Verify, Zero,
        },
        Perbill,
    };
    pub use sp_std::{collections::btree_map::BTreeMap, prelude::*};
    pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(3);
    use sp_avn_common::eth::EthereumId;

    /// Pallet for parachain staking
    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(PhantomData<T>);

    pub type EraIndex = u32;
    pub type GrowthPeriodIndex = u32;
    pub type RewardPoint = u32;
    pub type BalanceOf<T> =
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
    pub type PositiveImbalanceOf<T> = <<T as Config>::Currency as Currency<
        <T as frame_system::Config>::AccountId,
    >>::PositiveImbalance;

    pub const COLLATOR_LOCK_ID: LockIdentifier = *b"stkngcol";
    pub const NOMINATOR_LOCK_ID: LockIdentifier = *b"stkngnom";

    const MAX_GROWTHS_TO_PROCESS: usize = 10;

    pub type CollatorMaxScores = ConstU32<10000>;

    /// Configuration trait of this pallet.
    #[pallet::config]
    pub trait Config:
        frame_system::Config
        + pallet_session::Config
        + pallet_avn::Config
        + pallet_session::historical::Config
        + CreateTransactionBase<Call<Self>>
        + CreateInherent<Call<Self>>
    {
        /// The overarching call type.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
            + GetDispatchInfo
            + From<frame_system::Call<Self>>
            + IsSubType<Call<Self>>;
        /// Overarching event type
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// The currency type
        type Currency: Currency<Self::AccountId>
            + ReservableCurrency<Self::AccountId>
            + LockableCurrency<Self::AccountId>;
        /// Minimum number of blocks per era
        #[pallet::constant]
        type MinBlocksPerEra: Get<u32>;
        /// Number of eras after which block authors are rewarded
        #[pallet::constant]
        type RewardPaymentDelay: Get<EraIndex>;
        /// Minimum number of selected candidates every era
        #[pallet::constant]
        type MinSelectedCandidates: Get<u32>;
        /// Maximum top nominations counted per candidate
        #[pallet::constant]
        type MaxTopNominationsPerCandidate: Get<u32>;
        /// Maximum bottom nominations (not counted) per candidate
        #[pallet::constant]
        type MaxBottomNominationsPerCandidate: Get<u32>;
        /// Maximum nominations per nominator
        #[pallet::constant]
        type MaxNominationsPerNominator: Get<u32>;
        /// Minimum stake, per collator, that must be maintained by an account that is nominating
        #[pallet::constant]
        type MinNominationPerCollator: Get<BalanceOf<Self>>;
        /// Number of eras to MinNominationPerCollator before we process a new growth period
        #[pallet::constant]
        type ErasPerGrowthPeriod: Get<GrowthPeriodIndex>;
        /// Id of the account that will hold funds to be paid as staking reward
        #[pallet::constant]
        type RewardPotId: Get<PalletId>;
        /// A way to check if an event has been processed by Ethereum events
        type ProcessedEventsChecker: ProcessedEventsChecker;
        /// A type that can be used to verify signatures
        type Public: IdentifyAccount<AccountId = Self::AccountId>;
        /// The signature type used by accounts/transactions.
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;
        /// A hook to verify if a collator is registed as a validator (with keys) in the session
        /// pallet
        type CollatorSessionRegistration: ValidatorRegistration<Self::AccountId>;
        /// A handler to notify the runtime of any remaining amount after paying collators
        type CollatorPayoutDustHandler: CollatorPayoutDustHandler<BalanceOf<Self>>;
        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
        /// Maximum candidates
        #[pallet::constant]
        type MaxCandidates: Get<u32>;

        type AccountToBytesConvert: pallet_avn::AccountToBytesConverter<Self::AccountId>;

        type BridgeInterface: pallet_avn::BridgeInterface;

        #[pallet::constant]
        type GrowthEnabled: Get<bool>;
    }

    #[pallet::error]
    pub enum Error<T> {
        NominatorDNE,
        CandidateDNE,
        NominationDNE,
        NominatorExists,
        CandidateExists,
        CandidateBondBelowMin,
        InsufficientBalance,
        NominatorBondBelowMin,
        NominationBelowMin,
        AlreadyOffline,
        AlreadyActive,
        NominatorAlreadyLeaving,
        NominatorNotLeaving,
        NominatorCannotLeaveYet,
        CandidateAlreadyLeaving,
        CandidateNotLeaving,
        CandidateLimitReached,
        CandidateCannotLeaveYet,
        CannotGoOnlineIfLeaving,
        ExceedMaxNominationsPerNominator,
        AlreadyNominatedCandidate,
        InvalidSchedule,
        CannotSetBelowMin,
        EraLengthMustBeAtLeastTotalSelectedCollators,
        NoWritingSameValue,
        TooLowCandidateCountWeightHintJoinCandidates,
        TooLowCandidateCountWeightHintCancelLeaveCandidates,
        TooLowCandidateCountToLeaveCandidates,
        TooLowNominationCountToNominate,
        TooLowCandidateNominationCountToNominate,
        TooLowCandidateNominationCountToLeaveCandidates,
        TooLowNominationCountToLeaveNominators,
        PendingCandidateRequestsDNE,
        PendingCandidateRequestAlreadyExists,
        PendingCandidateRequestNotDueYet,
        PendingNominationRequestDNE,
        PendingNominationRequestAlreadyExists,
        PendingNominationRequestNotDueYet,
        CannotNominateLessThanOrEqualToLowestBottomWhenFull,
        PendingNominationRevoke,
        ErrorPayingCollator,
        GrowthAlreadyProcessed,
        UnauthorizedProxyTransaction,
        SenderIsNotSigner,
        UnauthorizedSignedNominateTransaction,
        UnauthorizedSignedBondExtraTransaction,
        UnauthorizedSignedCandidateBondExtraTransaction,
        UnauthorizedSignedCandidateUnbondTransaction,
        UnauthorizedSignedUnbondTransaction,
        UnauthorizedSignedRemoveBondTransaction,
        UnauthorizedSignedScheduleLeaveNominatorsTransaction,
        UnauthorizedSignedExecuteLeaveNominatorsTransaction,
        UnauthorizedSignedExecuteNominationRequestTransaction,
        UnauthorizedSignedExecuteCandidateUnbondTransaction,
        AdminSettingsValueIsNotValid,
        CandidateSessionKeysNotFound,
        FailedToWithdrawFullAmount,
        GrowthDataNotFound,
        InvalidGrowthData,
        ErrorConvertingBalance,
        Overflow,
        ErrorPublishingGrowth,
        StakingNotAllowed,
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(crate) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Started new era.
        NewEra {
            starting_block: BlockNumberFor<T>,
            era: EraIndex,
            selected_collators_number: u32,
            total_balance: BalanceOf<T>,
        },
        /// Account joined the set of collator candidates.
        JoinedCollatorCandidates {
            account: T::AccountId,
            amount_locked: BalanceOf<T>,
            new_total_amt_locked: BalanceOf<T>,
        },
        /// Candidate selected for collators. Total Exposed Amount includes all nominations.
        CollatorChosen {
            era: EraIndex,
            collator_account: T::AccountId,
            total_exposed_amount: BalanceOf<T>,
        },
        /// Candidate requested to decrease a self bond.
        CandidateBondLessRequested {
            candidate: T::AccountId,
            amount_to_decrease: BalanceOf<T>,
            execute_era: EraIndex,
        },
        /// Candidate has increased a self bond.
        CandidateBondedMore {
            candidate: T::AccountId,
            amount: BalanceOf<T>,
            new_total_bond: BalanceOf<T>,
        },
        /// Candidate has decreased a self bond.
        CandidateBondedLess {
            candidate: T::AccountId,
            amount: BalanceOf<T>,
            new_bond: BalanceOf<T>,
        },
        /// Candidate temporarily leave the set of collator candidates without unbonding.
        CandidateWentOffline { candidate: T::AccountId },
        /// Candidate rejoins the set of collator candidates.
        CandidateBackOnline { candidate: T::AccountId },
        /// Candidate has requested to leave the set of candidates.
        CandidateScheduledExit {
            exit_allowed_era: EraIndex,
            candidate: T::AccountId,
            scheduled_exit: EraIndex,
        },
        /// Cancelled request to leave the set of candidates.
        CancelledCandidateExit { candidate: T::AccountId },
        /// Cancelled request to decrease candidate's bond.
        CancelledCandidateBondLess {
            candidate: T::AccountId,
            amount: BalanceOf<T>,
            execute_era: EraIndex,
        },
        /// Candidate has left the set of candidates.
        CandidateLeft {
            ex_candidate: T::AccountId,
            unlocked_amount: BalanceOf<T>,
            new_total_amt_locked: BalanceOf<T>,
        },
        /// Nominator requested to decrease a bond for the collator candidate.
        NominationDecreaseScheduled {
            nominator: T::AccountId,
            candidate: T::AccountId,
            amount_to_decrease: BalanceOf<T>,
            execute_era: EraIndex,
        },
        // Nomination increased.
        NominationIncreased {
            nominator: T::AccountId,
            candidate: T::AccountId,
            amount: BalanceOf<T>,
            in_top: bool,
        },
        // Nomination decreased.
        NominationDecreased {
            nominator: T::AccountId,
            candidate: T::AccountId,
            amount: BalanceOf<T>,
            in_top: bool,
        },
        /// Nominator requested to leave the set of nominators.
        NominatorExitScheduled { era: EraIndex, nominator: T::AccountId, scheduled_exit: EraIndex },
        /// Nominator requested to revoke nomination.
        NominationRevocationScheduled {
            era: EraIndex,
            nominator: T::AccountId,
            candidate: T::AccountId,
            scheduled_exit: EraIndex,
        },
        /// Nominator has left the set of nominators.
        NominatorLeft { nominator: T::AccountId, unstaked_amount: BalanceOf<T> },
        /// Nomination revoked.
        NominationRevoked {
            nominator: T::AccountId,
            candidate: T::AccountId,
            unstaked_amount: BalanceOf<T>,
        },
        /// Nomination kicked.
        NominationKicked {
            nominator: T::AccountId,
            candidate: T::AccountId,
            unstaked_amount: BalanceOf<T>,
        },
        /// Cancelled a pending request to exit the set of nominators.
        NominatorExitCancelled { nominator: T::AccountId },
        /// Cancelled request to change an existing nomination.
        CancelledNominationRequest {
            nominator: T::AccountId,
            cancelled_request: CancelledScheduledRequest<BalanceOf<T>>,
            collator: T::AccountId,
        },
        /// New nomination (increase of the existing one).
        Nomination {
            nominator: T::AccountId,
            locked_amount: BalanceOf<T>,
            candidate: T::AccountId,
            nominator_position: NominatorAdded<BalanceOf<T>>,
        },
        /// Nomination from candidate state has been remove.
        NominatorLeftCandidate {
            nominator: T::AccountId,
            candidate: T::AccountId,
            unstaked_amount: BalanceOf<T>,
            total_candidate_staked: BalanceOf<T>,
        },
        /// Paid the account (nominator or collator) the balance as liquid rewards.
        Rewarded { account: T::AccountId, rewards: BalanceOf<T> },
        /// There was an error attempting to pay the nominator their staking reward.
        ErrorPayingStakingReward { payee: T::AccountId, rewards: BalanceOf<T> },
        /// Set total selected candidates to this value.
        TotalSelectedSet { old: u32, new: u32 },
        /// Set blocks per era
        BlocksPerEraSet {
            current_era: EraIndex,
            first_block: BlockNumberFor<T>,
            old: u32,
            new: u32,
        },
        /// Not enough fund to cover the staking reward payment.
        NotEnoughFundsForEraPayment { reward_pot_balance: BalanceOf<T> },
        /// A collator has been paid for producing blocks
        CollatorPaid { account: T::AccountId, amount: BalanceOf<T>, period: GrowthPeriodIndex },
        /// An admin settings value has been updated
        AdminSettingsUpdated { value: AdminSettings<BalanceOf<T>> },
        /// Starting a new growth trigger for the specified period.
        TriggeringGrowth { growth_period: u32 },
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            let mut weight = <T as Config>::WeightInfo::base_on_initialize();
            let mut era = <Era<T>>::get();
            if era.should_update(n) {
                let start_new_era_weight;
                (era, start_new_era_weight) = Self::start_new_era(n, era);
                weight = weight.saturating_add(start_new_era_weight);
            }

            if is_staking_enabled() {
                weight = weight.saturating_add(Self::handle_delayed_payouts(era.current));
            }

            // add on_finalize weight
            weight = weight.saturating_add(
                // read Author, Points, AwardedPts
                // write Points, AwardedPts
                T::DbWeight::get().reads(3).saturating_add(T::DbWeight::get().writes(2)),
            );
            weight
        }
    }

    #[pallet::storage]
    #[pallet::getter(fn delay)]
    /// Number of eras to wait before executing any staking action
    pub type Delay<T: Config> = StorageValue<_, EraIndex, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn total_selected)]
    /// The total candidates selected every era
    pub type TotalSelected<T: Config> = StorageValue<_, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn era)]
    /// Current era index and next era scheduled transition
    pub(crate) type Era<T: Config> = StorageValue<_, EraInfo<BlockNumberFor<T>>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn nominator_state)]
    /// Get nominator state associated with an account if account is nominating else None
    pub(crate) type NominatorState<T: Config> = StorageMap<
        _,
        Twox64Concat,
        T::AccountId,
        Nominator<T::AccountId, BalanceOf<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn candidate_info)]
    /// Get collator candidate info associated with an account if account is candidate else None
    pub type CandidateInfo<T: Config> =
        StorageMap<_, Twox64Concat, T::AccountId, CandidateMetadata<BalanceOf<T>>, OptionQuery>;

    /// Stores outstanding nomination requests per collator.
    #[pallet::storage]
    #[pallet::getter(fn nomination_scheduled_requests)]
    pub(crate) type NominationScheduledRequests<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        BoundedVec<ScheduledRequest<T::AccountId, BalanceOf<T>>, T::MaxNominationsPerNominator>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn top_nominations)]
    /// Top nominations for collator candidate
    pub(crate) type TopNominations<T: Config> = StorageMap<
        _,
        Twox64Concat,
        T::AccountId,
        Nominations<T::AccountId, BalanceOf<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn bottom_nominations)]
    /// Bottom nominations for collator candidate
    pub(crate) type BottomNominations<T: Config> = StorageMap<
        _,
        Twox64Concat,
        T::AccountId,
        Nominations<T::AccountId, BalanceOf<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn selected_candidates)]
    /// The collator candidates selected for the current era
    pub type SelectedCandidates<T: Config> =
        StorageValue<_, BoundedVec<T::AccountId, T::MaxCandidates>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn total)]
    /// Total capital locked by this staking pallet
    pub(crate) type Total<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn candidate_pool)]
    /// The pool of collator candidates, each with their total backing stake
    pub(crate) type CandidatePool<T: Config> = StorageValue<
        _,
        BoundedOrderedSet<Bond<T::AccountId, BalanceOf<T>>, T::MaxCandidates>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn at_stake)]
    /// Snapshot of collator nomination stake at the start of the era
    pub type AtStake<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        EraIndex,
        Twox64Concat,
        T::AccountId,
        CollatorSnapshot<T::AccountId, BalanceOf<T>>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn delayed_payouts)]
    /// Delayed payouts
    pub type DelayedPayouts<T: Config> =
        StorageMap<_, Twox64Concat, EraIndex, DelayedPayout<BalanceOf<T>>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn staked)]
    /// Total counted stake for selected candidates in the era
    pub type Staked<T: Config> = StorageMap<_, Twox64Concat, EraIndex, BalanceOf<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn points)]
    /// Total points awarded to collators for block production in the era
    pub type Points<T: Config> = StorageMap<_, Twox64Concat, EraIndex, RewardPoint, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn awarded_pts)]
    /// Points for each collator per era
    pub type AwardedPts<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        EraIndex,
        Twox64Concat,
        T::AccountId,
        RewardPoint,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn locked_era_payout)]
    /// Total amount of payouts we are waiting to take out of this pallet's pot.
    pub type LockedEraPayout<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn growth_period_info)]
    /// Tracks the current growth period where collator will get paid for producing blocks
    pub(crate) type GrowthPeriod<T: Config> = StorageValue<_, GrowthPeriodInfo, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn growth)]
    /// Data to calculate growth and collator payouts.
    pub type Growth<T: Config> = StorageMap<
        _,
        Twox64Concat,
        GrowthPeriodIndex,
        GrowthInfo<T::AccountId, BalanceOf<T>>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn processed_growth_periods)]
    pub type ProcessedGrowthPeriods<T: Config> =
        StorageMap<_, Twox64Concat, GrowthPeriodIndex, (), ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn new_era_forced)]
    pub type ForceNewEra<T: Config> = StorageValue<_, bool, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn min_collator_stake)]
    /// Minimum stake required for any candidate to be a collator
    pub type MinCollatorStake<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn min_total_nominator_stake)]
    /// Minimum total stake that must be maintained for any registered on-chain account to be a
    /// nominator
    pub type MinTotalNominatorStake<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn proxy_nonce)]
    /// An account nonce that represents the number of proxy transactions from this account
    pub type ProxyNonces<T: Config> = StorageMap<_, Twox64Concat, T::AccountId, u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn get_pending_growths)]
    pub type PendingApproval<T: Config> =
        StorageMap<_, Blake2_128Concat, GrowthPeriodIndex, IngressCounter, ValueQuery>;

    /// The last period we triggered growth
    #[pallet::storage]
    #[pallet::getter(fn last_triggered_growth_period)]
    pub type LastTriggeredGrowthPeriod<T: Config> = StorageValue<_, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn published_growth)]
    /// Map to keep track of growth we have published on Ethereum
    pub type PublishedGrowth<T: Config> =
        StorageMap<_, Twox64Concat, EthereumId, GrowthPeriodIndex, ValueQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub candidates: Vec<(T::AccountId, BalanceOf<T>)>,
        /// Vec of tuples of the format (nominator AccountId, collator AccountId, nomination
        /// Amount)
        pub nominations: Vec<(T::AccountId, T::AccountId, BalanceOf<T>)>,
        pub delay: EraIndex,
        pub min_collator_stake: BalanceOf<T>,
        pub min_total_nominator_stake: BalanceOf<T>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                candidates: vec![],
                nominations: vec![],
                delay: 1,
                min_collator_stake: Default::default(),
                min_total_nominator_stake: Default::default(),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            let mut candidate_count = 0u32;
            // Initialize the candidates
            for &(ref candidate, balance) in &self.candidates {
                assert!(
                    <Pallet<T>>::get_collator_stakable_free_balance(candidate) >= balance,
                    "Account does not have enough balance to bond as a candidate."
                );
                candidate_count = candidate_count.saturating_add(1u32);
                if let Err(error) = <Pallet<T>>::join_candidates(
                    T::RuntimeOrigin::from(Some(candidate.clone()).into()),
                    balance,
                    candidate_count,
                ) {
                    log::warn!("Join candidates failed in genesis with error {:?}", error);
                } else {
                    candidate_count = candidate_count.saturating_add(1u32);
                }
            }
            let mut col_nominator_count: BTreeMap<T::AccountId, u32> = BTreeMap::new();
            let mut del_nomination_count: BTreeMap<T::AccountId, u32> = BTreeMap::new();
            // Initialize the nominations
            for &(ref nominator, ref target, balance) in &self.nominations {
                assert!(
                    <Pallet<T>>::get_nominator_stakable_free_balance(nominator) >= balance,
                    "Account does not have enough balance to place nomination."
                );
                let cd_count =
                    if let Some(x) = col_nominator_count.get(target) { *x } else { 0u32 };
                let dd_count =
                    if let Some(x) = del_nomination_count.get(nominator) { *x } else { 0u32 };
                if let Err(error) = <Pallet<T>>::nominate(
                    T::RuntimeOrigin::from(Some(nominator.clone()).into()),
                    target.clone(),
                    balance,
                    cd_count,
                    dd_count,
                ) {
                    log::warn!("Nominate failed in genesis with error {:?}", error);
                } else {
                    if let Some(x) = col_nominator_count.get_mut(target) {
                        *x = x.saturating_add(1u32);
                    } else {
                        col_nominator_count.insert(target.clone(), 1u32);
                    };
                    if let Some(x) = del_nomination_count.get_mut(nominator) {
                        *x = x.saturating_add(1u32);
                    } else {
                        del_nomination_count.insert(nominator.clone(), 1u32);
                    };
                }
            }

            // Validate and set delay
            assert!(self.delay > 0, "Delay must be greater than 0.");
            <Delay<T>>::put(self.delay);

            // Set min staking values
            <MinCollatorStake<T>>::put(self.min_collator_stake);
            <MinTotalNominatorStake<T>>::put(self.min_total_nominator_stake);

            // Set total selected candidates to minimum config
            <TotalSelected<T>>::put(T::MinSelectedCandidates::get());

            // Choose top TotalSelected collator candidates
            let (v_count, _, total_staked) = <Pallet<T>>::select_top_candidates(1u32);

            // Start Era 1 at Block 0. Set the genesis era length too.
            let era: EraInfo<BlockNumberFor<T>> =
                EraInfo::new(1u32, 0u32.into(), T::MinBlocksPerEra::get() + 2);
            <Era<T>>::put(era);

            // Snapshot total stake
            <Staked<T>>::insert(1u32, <Total<T>>::get());

            // Set the first GrowthInfo
            <Growth<T>>::insert(0u32, GrowthInfo::new(1u32));

            <Pallet<T>>::deposit_event(Event::NewEra {
                starting_block: BlockNumberFor::<T>::zero(),
                era: 1u32,
                selected_collators_number: v_count,
                total_balance: total_staked,
            });

            // Set storage version
            STORAGE_VERSION.put::<Pallet<T>>();
            log::debug!(
                "Staking storage chain/current storage version: {:?} / {:?}",
                Pallet::<T>::on_chain_storage_version(),
                Pallet::<T>::in_code_storage_version(),
            );
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::weight(<T as Config>::WeightInfo::set_total_selected())]
        /// Set the total number of collator candidates selected per era
        /// - changes are not applied until the start of the next era
        #[pallet::call_index(0)]
        pub fn set_total_selected(origin: OriginFor<T>, new: u32) -> DispatchResultWithPostInfo {
            frame_system::ensure_root(origin)?;
            ensure!(new >= T::MinSelectedCandidates::get(), Error::<T>::CannotSetBelowMin);
            let old = <TotalSelected<T>>::get();
            ensure!(old != new, Error::<T>::NoWritingSameValue);
            ensure!(
                new <= <Era<T>>::get().length,
                Error::<T>::EraLengthMustBeAtLeastTotalSelectedCollators,
            );
            <TotalSelected<T>>::put(new);
            Self::deposit_event(Event::TotalSelectedSet { old, new });
            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::set_blocks_per_era())]
        /// Set blocks per era
        /// - if called with `new` less than length of current era, will transition immediately
        /// in the next block
        #[pallet::call_index(1)]
        pub fn set_blocks_per_era(origin: OriginFor<T>, new: u32) -> DispatchResultWithPostInfo {
            frame_system::ensure_root(origin)?;
            ensure!(new >= T::MinBlocksPerEra::get(), Error::<T>::CannotSetBelowMin);
            let mut era = <Era<T>>::get();
            let (now, first, old) = (era.current, era.first, era.length);
            ensure!(old != new, Error::<T>::NoWritingSameValue);
            ensure!(
                new >= <TotalSelected<T>>::get(),
                Error::<T>::EraLengthMustBeAtLeastTotalSelectedCollators,
            );
            era.length = new;
            <Era<T>>::put(era);
            Self::deposit_event(Event::BlocksPerEraSet {
                current_era: now,
                first_block: first,
                old,
                new,
            });

            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::join_candidates(*candidate_count))]
        /// Join the set of collator candidates
        #[pallet::call_index(2)]
        pub fn join_candidates(
            origin: OriginFor<T>,
            bond: BalanceOf<T>,
            candidate_count: u32,
        ) -> DispatchResultWithPostInfo {
            let acc = ensure_signed(origin)?;
            ensure!(!Self::is_candidate(&acc), Error::<T>::CandidateExists);
            ensure!(!Self::is_nominator(&acc), Error::<T>::NominatorExists);
            ensure!(bond >= <MinCollatorStake<T>>::get(), Error::<T>::CandidateBondBelowMin);
            ensure!(
                T::CollatorSessionRegistration::is_registered(&acc),
                Error::<T>::CandidateSessionKeysNotFound
            );

            let mut candidates = <CandidatePool<T>>::get();
            let old_count = candidates.0.len() as u32;
            ensure!(
                candidate_count >= old_count,
                Error::<T>::TooLowCandidateCountWeightHintJoinCandidates
            );

            match candidates.try_insert(Bond { owner: acc.clone(), amount: bond }) {
                Err(_) => Err(Error::<T>::CandidateLimitReached)?,
                Ok(false) => Err(Error::<T>::CandidateExists)?,
                Ok(true) => {},
            };
            ensure!(
                Self::get_collator_stakable_free_balance(&acc) >= bond,
                Error::<T>::InsufficientBalance,
            );
            T::Currency::set_lock(COLLATOR_LOCK_ID, &acc, bond, WithdrawReasons::all());
            let candidate = CandidateMetadata::new(bond);
            <CandidateInfo<T>>::insert(&acc, candidate);
            let empty_nominations: Nominations<T::AccountId, BalanceOf<T>> = Default::default();
            // insert empty top nominations
            <TopNominations<T>>::insert(&acc, empty_nominations.clone());
            // insert empty bottom nominations
            <BottomNominations<T>>::insert(&acc, empty_nominations);
            <CandidatePool<T>>::put(candidates);
            let new_total = <Total<T>>::get().saturating_add(bond);
            <Total<T>>::put(new_total);
            Self::deposit_event(Event::JoinedCollatorCandidates {
                account: acc,
                amount_locked: bond,
                new_total_amt_locked: new_total,
            });
            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::schedule_leave_candidates(*candidate_count))]
        /// Request to leave the set of candidates. If successful, the account is immediately
        /// removed from the candidate pool to prevent selection as a collator.
        #[pallet::call_index(3)]
        pub fn schedule_leave_candidates(
            origin: OriginFor<T>,
            candidate_count: u32,
        ) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;
            let mut state = <CandidateInfo<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
            let (now, when) = state.schedule_leave::<T>()?;
            let mut candidates = <CandidatePool<T>>::get();
            ensure!(
                candidate_count >= candidates.0.len() as u32,
                Error::<T>::TooLowCandidateCountToLeaveCandidates
            );
            if candidates.remove(&Bond::from_owner(collator.clone())) {
                <CandidatePool<T>>::put(candidates);
            }
            <CandidateInfo<T>>::insert(&collator, state);
            Self::deposit_event(Event::CandidateScheduledExit {
                exit_allowed_era: now,
                candidate: collator,
                scheduled_exit: when,
            });
            Ok(().into())
        }

        #[pallet::weight(
			<T as Config>::WeightInfo::execute_leave_candidates(*candidate_nomination_count)
		)]
        /// Execute leave candidates request
        #[pallet::call_index(4)]
        pub fn execute_leave_candidates(
            origin: OriginFor<T>,
            candidate: T::AccountId,
            candidate_nomination_count: u32,
        ) -> DispatchResultWithPostInfo {
            ensure_signed(origin)?;
            let state = <CandidateInfo<T>>::get(&candidate).ok_or(Error::<T>::CandidateDNE)?;
            ensure!(
                state.nomination_count <= candidate_nomination_count,
                Error::<T>::TooLowCandidateNominationCountToLeaveCandidates
            );
            state.can_leave::<T>()?;
            let return_stake = |bond: Bond<T::AccountId, BalanceOf<T>>| -> DispatchResult {
                // remove nomination from nominator state
                let mut nominator = NominatorState::<T>::get(&bond.owner).expect(
                    "Collator state and nominator state are consistent.
						Collator state has a record of this nomination. Therefore,
						Nominator state also has a record. qed.",
                );

                if let Some(remaining) = nominator.rm_nomination::<T>(&candidate) {
                    Self::nomination_remove_request_with_state(
                        &candidate,
                        &bond.owner,
                        &mut nominator,
                    );

                    if remaining.is_zero() {
                        // we do not remove the scheduled nomination requests from other collators
                        // since it is assumed that they were removed incrementally before only the
                        // last nomination was left.
                        <NominatorState<T>>::remove(&bond.owner);
                        T::Currency::remove_lock(NOMINATOR_LOCK_ID, &bond.owner);
                    } else {
                        <NominatorState<T>>::insert(&bond.owner, nominator);
                    }
                } else {
                    // TODO: review. we assume here that this nominator has no remaining staked
                    // balance, so we ensure the lock is cleared
                    T::Currency::remove_lock(NOMINATOR_LOCK_ID, &bond.owner);
                }
                Ok(())
            };
            // total backing stake is at least the candidate self bond
            let mut total_backing = state.bond;
            // return all top nominations
            let top_nominations =
                <TopNominations<T>>::take(&candidate).expect("CandidateInfo existence checked");
            for bond in top_nominations.nominations {
                return_stake(bond)?;
            }
            total_backing = total_backing.saturating_add(top_nominations.total);
            // return all bottom nominations
            let bottom_nominations =
                <BottomNominations<T>>::take(&candidate).expect("CandidateInfo existence checked");
            for bond in bottom_nominations.nominations {
                return_stake(bond)?;
            }
            total_backing = total_backing.saturating_add(bottom_nominations.total);
            // return stake to collator
            T::Currency::remove_lock(COLLATOR_LOCK_ID, &candidate);
            <CandidateInfo<T>>::remove(&candidate);
            <NominationScheduledRequests<T>>::remove(&candidate);
            <TopNominations<T>>::remove(&candidate);
            <BottomNominations<T>>::remove(&candidate);
            let new_total_staked = <Total<T>>::get().saturating_sub(total_backing);
            <Total<T>>::put(new_total_staked);
            Self::deposit_event(Event::CandidateLeft {
                ex_candidate: candidate,
                unlocked_amount: total_backing,
                new_total_amt_locked: new_total_staked,
            });
            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::cancel_leave_candidates(*candidate_count))]
        /// Cancel open request to leave candidates
        /// - only callable by collator account
        /// - result upon successful call is the candidate is active in the candidate pool
        #[pallet::call_index(5)]
        pub fn cancel_leave_candidates(
            origin: OriginFor<T>,
            candidate_count: u32,
        ) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;
            let mut state = <CandidateInfo<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
            ensure!(state.is_leaving(), Error::<T>::CandidateNotLeaving);
            state.go_online();
            let mut candidates = <CandidatePool<T>>::get();
            ensure!(
                candidates.0.len() as u32 <= candidate_count,
                Error::<T>::TooLowCandidateCountWeightHintCancelLeaveCandidates
            );

            match candidates
                .try_insert(Bond { owner: collator.clone(), amount: state.total_counted })
            {
                Err(_) => Err(Error::<T>::CandidateLimitReached)?,
                Ok(false) => Err(Error::<T>::AlreadyActive)?,
                Ok(true) => {},
            };
            <CandidatePool<T>>::put(candidates);
            <CandidateInfo<T>>::insert(&collator, state);
            Self::deposit_event(Event::CancelledCandidateExit { candidate: collator });
            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::go_offline())]
        /// Temporarily leave the set of collator candidates without unbonding
        #[pallet::call_index(6)]
        pub fn go_offline(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;
            let mut state = <CandidateInfo<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
            ensure!(state.is_active(), Error::<T>::AlreadyOffline);
            state.go_offline();
            let mut candidates = <CandidatePool<T>>::get();
            if candidates.remove(&Bond::from_owner(collator.clone())) {
                <CandidatePool<T>>::put(candidates);
            }
            <CandidateInfo<T>>::insert(&collator, state);
            Self::deposit_event(Event::CandidateWentOffline { candidate: collator });
            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::go_online())]
        /// Rejoin the set of collator candidates if previously had called `go_offline`
        #[pallet::call_index(7)]
        pub fn go_online(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;
            let mut state = <CandidateInfo<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
            ensure!(!state.is_active(), Error::<T>::AlreadyActive);
            ensure!(!state.is_leaving(), Error::<T>::CannotGoOnlineIfLeaving);
            state.go_online();
            let mut candidates = <CandidatePool<T>>::get();
            let maybe_inserted_candidate = candidates
                .try_insert(Bond { owner: collator.clone(), amount: state.total_counted })
                .map_err(|_| Error::<T>::CandidateLimitReached)?;
            ensure!(maybe_inserted_candidate, Error::<T>::AlreadyActive);

            <CandidatePool<T>>::put(candidates);
            <CandidateInfo<T>>::insert(&collator, state);
            Self::deposit_event(Event::CandidateBackOnline { candidate: collator });
            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::candidate_bond_extra())]
        /// Increase collator candidate self bond by `more`
        #[pallet::call_index(8)]
        pub fn candidate_bond_extra(
            origin: OriginFor<T>,
            more: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;
            return Self::call_candidate_bond_extra(&collator, more)
        }

        #[pallet::weight(<T as Config>::WeightInfo::signed_candidate_bond_extra())]
        #[transactional]
        /// Increase collator candidate self bond by `more`
        #[pallet::call_index(9)]
        pub fn signed_candidate_bond_extra(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            extra_amount: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;

            ensure!(collator == proof.signer, Error::<T>::SenderIsNotSigner);

            let collator_nonce = Self::proxy_nonce(&collator);
            let signed_payload = encode_signed_candidate_bond_extra_params::<T>(
                proof.relayer.clone(),
                &extra_amount,
                collator_nonce,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedCandidateBondExtraTransaction
            );

            // Defer any additional validation to the common logic
            Self::call_candidate_bond_extra(&collator, extra_amount)?;

            <ProxyNonces<T>>::mutate(&collator, |n| *n += 1);

            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::schedule_candidate_unbond())]
        /// Request by collator candidate to decrease self bond by `less`
        #[pallet::call_index(10)]
        pub fn schedule_candidate_unbond(
            origin: OriginFor<T>,
            less: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;
            return Self::call_schedule_candidate_unbond(&collator, less)
        }

        #[pallet::weight(<T as Config>::WeightInfo::execute_candidate_unbond())]
        /// Execute pending request to adjust the collator candidate self bond
        #[pallet::call_index(11)]
        pub fn execute_candidate_unbond(
            origin: OriginFor<T>,
            candidate: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            ensure_signed(origin)?; // we may want to reward this if caller != candidate
            return Self::call_execute_candidate_unbond(&candidate)
        }

        #[pallet::weight(<T as Config>::WeightInfo::signed_execute_candidate_unbond())]
        #[transactional]
        /// Execute pending request to adjust the collator candidate self bond
        #[pallet::call_index(12)]
        pub fn signed_execute_candidate_unbond(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            candidate: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            let sender = ensure_signed(origin)?; // we may want to reward this if caller != candidate

            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);

            let sender_nonce = Self::proxy_nonce(&sender);
            let signed_payload = encode_signed_execute_candidate_unbond_params::<T>(
                proof.relayer.clone(),
                &candidate,
                sender_nonce,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedExecuteCandidateUnbondTransaction
            );

            Self::call_execute_candidate_unbond(&candidate)?;

            <ProxyNonces<T>>::mutate(&sender, |n| *n += 1);

            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::cancel_candidate_unbond())]
        /// Cancel pending request to adjust the collator candidate self bond
        #[pallet::call_index(13)]
        pub fn cancel_candidate_unbond(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;
            let mut state = <CandidateInfo<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;
            state.cancel_unbond::<T>(collator.clone())?;
            <CandidateInfo<T>>::insert(&collator, state);
            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::signed_schedule_candidate_unbond())]
        #[transactional]
        /// Signed request by collator candidate to decrease self bond by `less`
        #[pallet::call_index(14)]
        pub fn signed_schedule_candidate_unbond(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            less: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let collator = ensure_signed(origin)?;

            ensure!(collator == proof.signer, Error::<T>::SenderIsNotSigner);

            let collator_nonce = Self::proxy_nonce(&collator);
            let signed_payload = encode_signed_schedule_candidate_unbond_params::<T>(
                proof.relayer.clone(),
                &less,
                collator_nonce,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedCandidateUnbondTransaction
            );

            Self::call_schedule_candidate_unbond(&collator, less)?;

            <ProxyNonces<T>>::mutate(&collator, |n| *n += 1);

            Ok(().into())
        }

        #[pallet::weight(
			<T as Config>::WeightInfo::nominate(
				*candidate_nomination_count,
				*nomination_count
			)
		)]
        /// If caller is not a nominator and not a collator, then join the set of nominators
        /// If caller is a nominator, then makes nomination to change their nomination state
        #[pallet::call_index(15)]
        pub fn nominate(
            origin: OriginFor<T>,
            candidate: T::AccountId,
            amount: BalanceOf<T>,
            candidate_nomination_count: u32,
            nomination_count: u32,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            ensure!(is_staking_enabled(), Error::<T>::StakingNotAllowed);
            return Self::call_nominate(
                &nominator,
                candidate,
                amount,
                candidate_nomination_count,
                nomination_count,
            )
        }

        #[pallet::weight(<T as Config>::WeightInfo::signed_nominate(
            T::MaxNominationsPerNominator::get(), T::MaxTopNominationsPerCandidate::get())
        )]
        #[transactional]
        #[pallet::call_index(16)]
        pub fn signed_nominate(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            targets: Vec<<T::Lookup as StaticLookup>::Source>,
            #[pallet::compact] amount: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            ensure!(is_staking_enabled(), Error::<T>::StakingNotAllowed);
            ensure!(nominator == proof.signer, Error::<T>::SenderIsNotSigner);

            let nominator_nonce = Self::proxy_nonce(&nominator);
            let signed_payload = encode_signed_nominate_params::<T>(
                proof.relayer.clone(),
                &targets,
                &amount,
                nominator_nonce,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedNominateTransaction
            );

            Self::split_and_nominate(&nominator, targets, amount)?;

            <ProxyNonces<T>>::mutate(&nominator, |n| *n += 1);

            Ok(().into())
        }

        /// If successful, the caller is scheduled to be
        /// allowed to exit via a [NominationAction::Revoke] towards all existing nominations.
        /// Success forbids future nomination requests until the request is invoked or cancelled.
        #[pallet::weight(<T as Config>::WeightInfo::schedule_leave_nominators())]
        #[pallet::call_index(17)]
        pub fn schedule_leave_nominators(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            Self::nominator_schedule_revoke_all(nominator)
        }

        #[pallet::weight(<T as Config>::WeightInfo::signed_schedule_leave_nominators())]
        #[transactional]
        #[pallet::call_index(18)]
        pub fn signed_schedule_leave_nominators(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;

            ensure!(nominator == proof.signer, Error::<T>::SenderIsNotSigner);

            let nominator_nonce = Self::proxy_nonce(&nominator);
            let signed_payload = encode_signed_schedule_leave_nominators_params::<T>(
                proof.relayer.clone(),
                nominator_nonce,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedScheduleLeaveNominatorsTransaction
            );

            Self::nominator_schedule_revoke_all(nominator.clone())?;

            <ProxyNonces<T>>::mutate(&nominator, |n| *n += 1);

            Ok(().into())
        }

        /// Execute the right to exit the set of nominators and revoke all ongoing nominations.
        #[pallet::weight(<T as Config>::WeightInfo::execute_leave_nominators(*nomination_count))]
        #[pallet::call_index(19)]
        pub fn execute_leave_nominators(
            origin: OriginFor<T>,
            nominator: T::AccountId,
            nomination_count: u32,
        ) -> DispatchResultWithPostInfo {
            ensure_signed(origin)?;
            Self::nominator_execute_scheduled_revoke_all(nominator, nomination_count)
        }

        /// Execute the right to exit the set of nominators and revoke all ongoing nominations.
        /// Any account can call this extrinsic
        #[pallet::weight(<T as Config>::WeightInfo::signed_execute_leave_nominators(T::MaxNominationsPerNominator::get()))]
        #[transactional]
        #[pallet::call_index(20)]
        pub fn signed_execute_leave_nominators(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            nominator: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            let sender = ensure_signed(origin)?;

            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);

            let sender_nonce = Self::proxy_nonce(&sender);
            let signed_payload = encode_signed_execute_leave_nominators_params::<T>(
                proof.relayer.clone(),
                &nominator,
                sender_nonce,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedExecuteLeaveNominatorsTransaction
            );

            if let Some(nominator_state) = <NominatorState<T>>::get(&nominator) {
                let nomination_count = nominator_state.nominations.0.len() as u32;

                Self::nominator_execute_scheduled_revoke_all(nominator.clone(), nomination_count)?;

                <ProxyNonces<T>>::mutate(&sender, |n| *n += 1);

                return Ok(().into())
            }

            Err(Error::<T>::NominatorDNE)?
        }

        /// Cancel a pending request to exit the set of nominators. Success clears the pending exit
        /// request (thereby resetting the delay upon another `leave_nominators` call).
        #[pallet::weight(<T as Config>::WeightInfo::cancel_leave_nominators())]
        #[pallet::call_index(21)]
        pub fn cancel_leave_nominators(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            Self::nominator_cancel_scheduled_revoke_all(nominator)
        }

        #[pallet::weight(<T as Config>::WeightInfo::schedule_revoke_nomination())]
        /// Request to revoke an existing nomination. If successful, the nomination is scheduled
        /// to be allowed to be revoked via the `execute_nomination_request` extrinsic.
        #[pallet::call_index(22)]
        pub fn schedule_revoke_nomination(
            origin: OriginFor<T>,
            collator: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            Self::nomination_schedule_revoke(collator, nominator)
        }

        #[pallet::weight(<T as Config>::WeightInfo::signed_schedule_revoke_nomination())]
        #[transactional]
        /// Signed request to revoke an existing nomination. If successful, the nomination is
        /// scheduled to be allowed to be revoked via the `execute_nomination_request`
        /// extrinsic.
        #[pallet::call_index(23)]
        pub fn signed_schedule_revoke_nomination(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            collator: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            ensure!(nominator == proof.signer, Error::<T>::SenderIsNotSigner);

            let nominator_nonce = Self::proxy_nonce(&nominator);
            let signed_payload = encode_signed_schedule_revoke_nomination_params::<T>(
                proof.relayer.clone(),
                &collator,
                nominator_nonce,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedRemoveBondTransaction
            );

            Self::nomination_schedule_revoke(collator, nominator.clone())?;

            <ProxyNonces<T>>::mutate(&nominator, |n| *n += 1);

            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::bond_extra())]
        /// Bond more for nominators wrt a specific collator candidate.
        #[pallet::call_index(24)]
        pub fn bond_extra(
            origin: OriginFor<T>,
            candidate: T::AccountId,
            more: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            ensure!(is_staking_enabled(), Error::<T>::StakingNotAllowed);
            return Self::call_bond_extra(&nominator, candidate, more)
        }

        /// Bond a maximum of 'extra_amount' amount.
        #[pallet::weight(<T as Config>::WeightInfo::signed_bond_extra())]
        #[transactional]
        #[pallet::call_index(25)]
        pub fn signed_bond_extra(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            #[pallet::compact] extra_amount: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            ensure!(is_staking_enabled(), Error::<T>::StakingNotAllowed);
            ensure!(nominator == proof.signer, Error::<T>::SenderIsNotSigner);

            let nominator_nonce = Self::proxy_nonce(&nominator);
            let signed_payload = encode_signed_bond_extra_params::<T>(
                proof.relayer.clone(),
                &extra_amount,
                nominator_nonce,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedBondExtraTransaction
            );

            ensure!(
                Self::get_nominator_stakable_free_balance(&nominator) >= extra_amount,
                Error::<T>::InsufficientBalance
            );

            // Top up existing nominations only.
            let state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
            let nominations = state.nominations.0;
            let num_nominations = nominations.len() as u32;
            let amount_per_collator = Perbill::from_rational(1, num_nominations) * extra_amount;
            ensure!(
                amount_per_collator >= T::MinNominationPerCollator::get(),
                Error::<T>::NominationBelowMin
            );

            let dust = extra_amount.saturating_sub(amount_per_collator * num_nominations.into());
            let mut remaining_amount_to_nominate = extra_amount;
            // This is only possible because we won't have more than 20 collators. If that changes,
            // we should not use a loop here.
            for (index, nomination) in nominations.into_iter().enumerate() {
                let mut actual_amount = amount_per_collator;
                if Self::collator_should_get_dust(dust, num_nominations.into(), index as u64) {
                    actual_amount = amount_per_collator + dust;
                }

                // make sure we don't bond more than what the user asked
                actual_amount = remaining_amount_to_nominate.min(actual_amount);

                Self::call_bond_extra(&nominator, nomination.owner, actual_amount)?;

                remaining_amount_to_nominate -= actual_amount;
            }

            <ProxyNonces<T>>::mutate(&nominator, |n| *n += 1);

            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::schedule_nominator_unbond())]
        /// Request bond less for nominators wrt a specific collator candidate.
        #[pallet::call_index(26)]
        pub fn schedule_nominator_unbond(
            origin: OriginFor<T>,
            candidate: T::AccountId,
            less: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            Self::nomination_schedule_bond_decrease(candidate, nominator, less)
        }

        #[pallet::weight(<T as Config>::WeightInfo::signed_schedule_nominator_unbond())]
        #[transactional]
        #[pallet::call_index(27)]
        pub fn signed_schedule_nominator_unbond(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            less: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;

            ensure!(nominator == proof.signer, Error::<T>::SenderIsNotSigner);

            let nominator_nonce = Self::proxy_nonce(&nominator);
            let signed_payload = encode_signed_schedule_nominator_unbond_params::<T>(
                proof.relayer.clone(),
                &less,
                nominator_nonce,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedUnbondTransaction
            );

            let (payers, mut outstanding_withdrawal) =
                Self::identify_collators_to_withdraw_from(&nominator, less)?;

            // Deal with any outstanding amount to withdraw and schedule decrease
            for mut stake in payers.into_iter() {
                if !outstanding_withdrawal.is_zero() {
                    let max_amount_to_withdraw = stake.free_amount.min(outstanding_withdrawal);
                    stake.reserved_amount += max_amount_to_withdraw;
                    outstanding_withdrawal -= max_amount_to_withdraw;
                }

                Self::nomination_schedule_bond_decrease(
                    stake.owner,
                    nominator.clone(),
                    stake.reserved_amount,
                )?;
            }

            // Make sure we have unbonded the full amount requested by the user
            ensure!(
                outstanding_withdrawal == BalanceOf::<T>::zero(),
                Error::<T>::FailedToWithdrawFullAmount
            );

            <ProxyNonces<T>>::mutate(&nominator, |n| *n += 1);

            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::execute_nominator_unbond())]
        /// Execute pending request to change an existing nomination
        #[pallet::call_index(28)]
        pub fn execute_nomination_request(
            origin: OriginFor<T>,
            nominator: T::AccountId,
            candidate: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            ensure_signed(origin)?; // we may want to reward caller if caller != nominator
            Self::nomination_execute_scheduled_request(candidate, nominator)
        }

        #[pallet::weight(<T as Config>::WeightInfo::signed_execute_nominator_unbond())]
        #[transactional]
        /// Execute pending request to change an existing nomination
        #[pallet::call_index(29)]
        pub fn signed_execute_nomination_request(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            nominator: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            let sender = ensure_signed(origin)?;

            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);

            let sender_nonce = Self::proxy_nonce(&sender);
            let signed_payload = encode_signed_execute_nomination_request_params::<T>(
                proof.relayer.clone(),
                &nominator,
                sender_nonce,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedExecuteNominationRequestTransaction
            );

            let now = <Era<T>>::get().current;
            let state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
            for bond in state.nominations.0 {
                let collator = bond.owner;
                let scheduled_requests = &<NominationScheduledRequests<T>>::get(&collator);

                let request_idx = scheduled_requests
                    .iter()
                    .position(|req| req.nominator == nominator)
                    .ok_or(<Error<T>>::PendingNominationRequestDNE)?;

                if scheduled_requests[request_idx].when_executable <= now {
                    Self::nomination_execute_scheduled_request(collator, nominator.clone())?;
                }
            }

            <ProxyNonces<T>>::mutate(&sender, |n| *n += 1);

            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::cancel_nominator_unbond())]
        /// Cancel request to change an existing nomination.
        #[pallet::call_index(30)]
        pub fn cancel_nomination_request(
            origin: OriginFor<T>,
            candidate: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            let nominator = ensure_signed(origin)?;
            Self::nomination_cancel_request(candidate, nominator)
        }

        /// Hotfix to remove existing empty entries for candidates that have left.
        #[pallet::weight(
			T::DbWeight::get().reads_writes(2 * candidates.len() as u64, candidates.len() as u64)
		)]
        #[pallet::call_index(31)]
        pub fn hotfix_remove_nomination_requests_exited_candidates(
            origin: OriginFor<T>,
            candidates: Vec<T::AccountId>,
        ) -> DispatchResult {
            ensure_signed(origin)?;
            ensure!(candidates.len() < 100, <Error<T>>::InsufficientBalance);
            for candidate in &candidates {
                ensure!(
                    <CandidateInfo<T>>::get(&candidate).is_none(),
                    <Error<T>>::CandidateNotLeaving
                );
                ensure!(
                    <NominationScheduledRequests<T>>::get(&candidate).is_empty(),
                    <Error<T>>::CandidateNotLeaving
                );
            }

            for candidate in candidates {
                <NominationScheduledRequests<T>>::remove(candidate);
            }

            Ok(().into())
        }

        #[pallet::weight(<T as Config>::WeightInfo::set_admin_setting())]
        #[pallet::call_index(32)]
        pub fn set_admin_setting(
            origin: OriginFor<T>,
            value: AdminSettings<BalanceOf<T>>,
        ) -> DispatchResult {
            frame_system::ensure_root(origin)?;
            ensure!(value.is_valid::<T>(), Error::<T>::AdminSettingsValueIsNotValid);

            match value {
                AdminSettings::Delay(d) => <Delay<T>>::put(d),
                AdminSettings::MinCollatorStake(s) => <MinCollatorStake<T>>::put(s),
                AdminSettings::MinTotalNominatorStake(s) => <MinTotalNominatorStake<T>>::put(s),
            }

            Self::deposit_event(Event::AdminSettingsUpdated { value });

            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        pub fn start_new_era(
            block_number: BlockNumberFor<T>,
            mut era: EraInfo<BlockNumberFor<T>>,
        ) -> (EraInfo<BlockNumberFor<T>>, Weight) {
            // mutate era
            era.update(block_number);

            if is_staking_enabled() {
                // pay all stakers for T::RewardPaymentDelay eras ago
                Self::prepare_staking_payouts(era.current);
            }

            // select top collator candidates for next era
            let (collator_count, nomination_count, total_staked) =
                Self::select_top_candidates(era.current);

            // start next era
            <Era<T>>::put(era);
            // snapshot total stake
            <Staked<T>>::insert(era.current, <Total<T>>::get());

            Self::deposit_event(Event::NewEra {
                starting_block: era.first,
                era: era.current,
                selected_collators_number: collator_count,
                total_balance: total_staked,
            });

            let weight = <T as Config>::WeightInfo::era_transition_on_initialize(
                collator_count,
                nomination_count,
            );
            return (era, weight)
        }

        pub fn is_nominator(acc: &T::AccountId) -> bool {
            <NominatorState<T>>::get(acc).is_some()
        }

        pub fn is_candidate(acc: &T::AccountId) -> bool {
            <CandidateInfo<T>>::get(acc).is_some()
        }

        pub fn is_selected_candidate(acc: &T::AccountId) -> bool {
            <SelectedCandidates<T>>::get().binary_search(acc).is_ok()
        }

        /// Returns an account's free balance which is not locked in nomination staking
        pub fn get_nominator_stakable_free_balance(acc: &T::AccountId) -> BalanceOf<T> {
            let mut balance = T::Currency::free_balance(acc);
            if let Some(state) = <NominatorState<T>>::get(acc) {
                balance = balance.saturating_sub(state.total());
            }
            balance
        }
        /// Returns an account's free balance which is not locked in collator staking
        pub fn get_collator_stakable_free_balance(acc: &T::AccountId) -> BalanceOf<T> {
            let mut balance = T::Currency::free_balance(acc);
            if let Some(info) = <CandidateInfo<T>>::get(acc) {
                balance = balance.saturating_sub(info.bond);
            }
            balance
        }
        /// Caller must ensure candidate is active before calling
        pub(crate) fn update_active(candidate: T::AccountId, total: BalanceOf<T>) {
            let mut candidates = <CandidatePool<T>>::get();
            candidates.remove(&Bond::from_owner(candidate.clone()));

            if let Err(_) | Ok(false) =
                candidates.try_insert(Bond { owner: candidate, amount: total })
            {
                log::error!("💔 Error while trying to update active candidates. Since a candidate was just removed this should never fail.");
            }
            <CandidatePool<T>>::put(candidates);
        }

        /// Compute total reward for era based on the amount in the reward pot
        pub fn compute_total_reward_to_pay() -> BalanceOf<T> {
            let total_unpaid_reward_amount = Self::reward_pot();
            let mut payout = total_unpaid_reward_amount.checked_sub(&Self::locked_era_payout()).or_else(|| {
				log::error!("💔 Error calculating era payout. Not enough funds in total_unpaid_reward_amount.");

				//This is a bit strange but since we are dealing with money, log it.
				Self::deposit_event(Event::NotEnoughFundsForEraPayment {reward_pot_balance: total_unpaid_reward_amount});
				Some(BalanceOf::<T>::zero())
			}).expect("We have a default value");

            <LockedEraPayout<T>>::mutate(|lp| {
                *lp = lp
                    .checked_add(&payout)
                    .or_else(|| {
                        log::error!("💔 Error - locked_era_payout overflow. Reducing era payout");
                        // In the unlikely event where the value will overflow the LockedEraPayout,
                        // return the difference to avoid errors
                        payout =
                            BalanceOf::<T>::max_value().saturating_sub(Self::locked_era_payout());
                        Some(BalanceOf::<T>::max_value())
                    })
                    .expect("We have a default value");
            });

            return payout
        }

        /// Remove nomination from candidate state
        /// Amount input should be retrieved from nominator and it informs the storage lookups
        pub(crate) fn nominator_leaves_candidate(
            candidate: T::AccountId,
            nominator: T::AccountId,
            amount: BalanceOf<T>,
        ) -> DispatchResult {
            let mut state = <CandidateInfo<T>>::get(&candidate).ok_or(Error::<T>::CandidateDNE)?;
            state.rm_nomination_if_exists::<T>(&candidate, nominator.clone(), amount)?;
            let new_total_locked = <Total<T>>::get().saturating_sub(amount);
            <Total<T>>::put(new_total_locked);
            let new_total = state.total_counted;
            <CandidateInfo<T>>::insert(&candidate, state);
            Self::deposit_event(Event::NominatorLeftCandidate {
                nominator,
                candidate,
                unstaked_amount: amount,
                total_candidate_staked: new_total,
            });
            Ok(())
        }

        fn prepare_staking_payouts(now: EraIndex) {
            // payout is now - delay eras ago => now - delay > 0 else return early
            let delay = T::RewardPaymentDelay::get();
            if now <= delay {
                return
            }
            let era_to_payout = now.saturating_sub(delay);
            let total_points = <Points<T>>::get(era_to_payout);
            if total_points.is_zero() {
                return
            }
            // Remove stake because it has been processed.
            let total_staked = <Staked<T>>::take(era_to_payout);

            let total_reward_to_pay = Self::compute_total_reward_to_pay();

            let payout = DelayedPayout {
                total_staking_reward: total_reward_to_pay, /* TODO: Remove one of the duplicated
                                                            * fields */
            };

            <DelayedPayouts<T>>::insert(era_to_payout, &payout);

            let growth_enabled = T::GrowthEnabled::get();
            if growth_enabled {
                let collator_scores_vec: Vec<CollatorScore<T::AccountId>> =
                    <AwardedPts<T>>::iter_prefix(era_to_payout)
                        .map(|(collator, points)| CollatorScore::new(collator, points))
                        .collect::<Vec<CollatorScore<T::AccountId>>>();
                let collator_scores = BoundedVec::truncate_from(collator_scores_vec);
                Self::update_collator_payout(
                    era_to_payout,
                    total_staked,
                    payout,
                    total_points,
                    collator_scores,
                );
            }
        }

        /// Wrapper around pay_one_collator_reward which handles the following logic:
        /// * whether or not a payout needs to be made
        /// * cleaning up when payouts are done
        /// * returns the weight consumed by pay_one_collator_reward if applicable
        fn handle_delayed_payouts(now: EraIndex) -> Weight {
            let delay = T::RewardPaymentDelay::get();

            // don't underflow uint
            if now < delay {
                return Weight::from_parts(0 as u64, 0).into()
            }

            let paid_for_era = now.saturating_sub(delay);

            if let Some(payout_info) = <DelayedPayouts<T>>::get(paid_for_era) {
                let result = Self::pay_one_collator_reward(paid_for_era, payout_info);
                if result.0.is_none() {
                    // result.0 indicates whether or not a payout was made
                    // clean up storage items that we no longer need
                    <DelayedPayouts<T>>::remove(paid_for_era);
                    <Points<T>>::remove(paid_for_era);
                }
                result.1 // weight consumed by pay_one_collator_reward
            } else {
                Weight::from_parts(0 as u64, 0).into()
            }
        }

        /// Payout a single collator from the given era.
        ///
        /// Returns an optional tuple of (Collator's AccountId, total paid)
        /// or None if there were no more payouts to be made for the era.
        pub(crate) fn pay_one_collator_reward(
            paid_for_era: EraIndex,
            payout_info: DelayedPayout<BalanceOf<T>>,
        ) -> (Option<(T::AccountId, BalanceOf<T>)>, Weight) {
            // TODO: it would probably be optimal to roll Points into the DelayedPayouts storage
            // item so that we do fewer reads each block
            let total_points = <Points<T>>::get(paid_for_era);
            if total_points.is_zero() {
                // TODO: this case is obnoxious... it's a value query, so it could mean one of two
                // different logic errors:
                // 1. we removed it before we should have
                // 2. we called pay_one_collator_reward when we were actually done with deferred
                //    payouts
                log::warn!("pay_one_collator_reward called with no <Points<T>> for the era!");
                return (None, Weight::from_parts(0 as u64, 0).into())
            }

            let reward_pot_account_id = Self::compute_reward_pot_account_id();
            let pay_reward = |amount: BalanceOf<T>, to: T::AccountId| {
                let result = T::Currency::transfer(
                    &reward_pot_account_id,
                    &to,
                    amount,
                    ExistenceRequirement::KeepAlive,
                );
                if let Ok(_) = result {
                    Self::deposit_event(Event::Rewarded { account: to.clone(), rewards: amount });

                    // Update storage with the amount we paid
                    <LockedEraPayout<T>>::mutate(|p| {
                        *p = p.saturating_sub(amount.into());
                    });
                } else {
                    log::error!("💔 Error paying staking reward: {:?}", result);
                    Self::deposit_event(Event::ErrorPayingStakingReward {
                        payee: to.clone(),
                        rewards: amount,
                    });
                }
            };

            if let Some((collator, pts)) = <AwardedPts<T>>::iter_prefix(paid_for_era).drain().next()
            {
                let pct_due = Perbill::from_rational(pts, total_points);
                let total_reward_for_collator = pct_due * payout_info.total_staking_reward;

                // Take the snapshot of block author and nominations
                let state = <AtStake<T>>::take(paid_for_era, &collator);
                let num_nominators = state.nominations.len();

                // pay collator's due portion first
                let collator_pct = Perbill::from_rational(state.bond, state.total);
                let collator_reward = collator_pct * total_reward_for_collator;
                pay_reward(collator_reward, collator.clone());

                // pay nominators due portion, if there are any
                for Bond { owner, amount } in state.nominations {
                    let percent = Perbill::from_rational(amount, state.total);
                    let nominator_reward = percent * total_reward_for_collator;
                    if !nominator_reward.is_zero() {
                        pay_reward(nominator_reward, owner.clone());
                    }
                }

                (
                    Some((collator, total_reward_for_collator)),
                    <T as Config>::WeightInfo::pay_one_collator_reward(num_nominators as u32),
                )
            } else {
                // Note that we don't clean up storage here; it is cleaned up in
                // handle_delayed_payouts()
                (None, Weight::from_parts(0 as u64, 0).into())
            }
        }

        /// Compute the top `TotalSelected` candidates in the CandidatePool and return
        /// a vec of their AccountIds (in the order of selection)
        pub fn compute_top_candidates() -> Vec<T::AccountId> {
            let mut candidates = <CandidatePool<T>>::get().0;
            // order candidates by stake (least to greatest so requires `rev()`)
            candidates.sort_by(|a, b| a.amount.cmp(&b.amount));
            let top_n = <TotalSelected<T>>::get() as usize;
            // choose the top TotalSelected qualified candidates, ordered by stake
            let mut collators = candidates
                .into_iter()
                .rev()
                .take(top_n)
                .filter(|x| x.amount >= <MinCollatorStake<T>>::get())
                .map(|x| x.owner)
                .collect::<Vec<T::AccountId>>();
            collators.sort();
            collators
        }

        /// Best as in most cumulatively supported in terms of stake
        /// Returns [collator_count, nomination_count, total staked]
        pub fn select_top_candidates(now: EraIndex) -> (u32, u32, BalanceOf<T>) {
            let (mut collator_count, mut nomination_count, mut total) =
                (0u32, 0u32, BalanceOf::<T>::zero());
            // choose the top TotalSelected qualified candidates, ordered by stake
            let collators = Self::compute_top_candidates();
            if collators.is_empty() {
                // SELECTION FAILED TO SELECT >=1 COLLATOR => select collators from previous era
                let last_era = now.saturating_sub(1u32);
                let mut total_per_candidate: BTreeMap<T::AccountId, BalanceOf<T>> = BTreeMap::new();
                // set this era AtStake to last era AtStake
                for (account, snapshot) in <AtStake<T>>::iter_prefix(last_era) {
                    collator_count = collator_count.saturating_add(1u32);
                    nomination_count =
                        nomination_count.saturating_add(snapshot.nominations.len() as u32);
                    total = total.saturating_add(snapshot.total);
                    total_per_candidate.insert(account.clone(), snapshot.total);
                    <AtStake<T>>::insert(now, account, snapshot);
                }
                // `SelectedCandidates` remains unchanged from last era
                // emit CollatorChosen event for tools that use this event
                for candidate in <SelectedCandidates<T>>::get() {
                    let snapshot_total = total_per_candidate
                        .get(&candidate)
                        .expect("all selected candidates have snapshots");
                    Self::deposit_event(Event::CollatorChosen {
                        era: now,
                        collator_account: candidate,
                        total_exposed_amount: *snapshot_total,
                    })
                }
                return (collator_count, nomination_count, total)
            }

            // snapshot exposure for era for weighting reward distribution
            for account in collators.iter() {
                let state = <CandidateInfo<T>>::get(account)
                    .expect("all members of CandidateQ must be candidates");

                collator_count = collator_count.saturating_add(1u32);
                nomination_count = nomination_count.saturating_add(state.nomination_count);
                total = total.saturating_add(state.total_counted);
                let CountedNominations { uncounted_stake, rewardable_nominations } =
                    Self::get_rewardable_nominators(&account);
                let total_counted = state.total_counted.saturating_sub(uncounted_stake);

                let snapshot = CollatorSnapshot {
                    bond: state.bond,
                    nominations: rewardable_nominations,
                    total: total_counted,
                };
                <AtStake<T>>::insert(now, account, snapshot);
                Self::deposit_event(Event::CollatorChosen {
                    era: now,
                    collator_account: account.clone(),
                    total_exposed_amount: state.total_counted,
                });
            }
            // insert canonical collator set
            <SelectedCandidates<T>>::put(
                BoundedVec::try_from(collators)
                    .expect("subset of collators is always less than or equal to max candidates"),
            );
            (collator_count, nomination_count, total)
        }

        /// Apply the nominator intent for revoke and decrease in order to build the
        /// effective list of nominators with their intended bond amount.
        ///
        /// This will:
        /// - if [NominationChange::Revoke] is outstanding, set the bond amount to 0.
        /// - if [NominationChange::Decrease] is outstanding, subtract the bond by specified amount.
        /// - else, do nothing
        ///
        /// The intended bond amounts will be used while calculating rewards.
        fn get_rewardable_nominators(collator: &T::AccountId) -> CountedNominations<T> {
            let requests = <NominationScheduledRequests<T>>::get(collator)
                .into_iter()
                .map(|x| (x.nominator, x.action))
                .collect::<BTreeMap<_, _>>();
            let mut uncounted_stake = BalanceOf::<T>::zero();
            let rewardable_nominations_vec = <TopNominations<T>>::get(collator)
                .expect("all members of CandidateQ must be candidates")
                .nominations
                .into_iter()
                .map(|mut bond| {
                    bond.amount = match requests.get(&bond.owner) {
                        None => bond.amount,
                        Some(NominationAction::Revoke(_)) => {
                            log::warn!(
                                "reward for nominator '{:?}' set to zero due to pending \
								revoke request",
                                bond.owner
                            );
                            uncounted_stake = uncounted_stake.saturating_add(bond.amount);
                            BalanceOf::<T>::zero()
                        },
                        Some(NominationAction::Decrease(amount)) => {
                            log::warn!(
                                "reward for nominator '{:?}' reduced by set amount due to pending \
								decrease request",
                                bond.owner
                            );
                            uncounted_stake = uncounted_stake.saturating_add(*amount);
                            bond.amount.saturating_sub(*amount)
                        },
                    };

                    bond
                })
                .collect();
            let rewardable_nominations = BoundedVec::truncate_from(rewardable_nominations_vec);
            CountedNominations { uncounted_stake, rewardable_nominations }
        }

        /// The account ID of the staking reward_pot.
        /// This actually does computation. If you need to keep using it, then make sure you cache
        /// the value and only call this once.
        pub fn compute_reward_pot_account_id() -> T::AccountId {
            T::RewardPotId::get().into_account_truncating()
        }

        /// The total amount of funds stored in this pallet
        pub fn reward_pot() -> BalanceOf<T> {
            // Must never be less than 0 but better be safe.
            T::Currency::free_balance(&Self::compute_reward_pot_account_id())
                .saturating_sub(T::Currency::minimum_balance())
        }

        pub fn update_collator_payout(
            payout_era: EraIndex,
            total_staked: BalanceOf<T>,
            payout: DelayedPayout<BalanceOf<T>>,
            total_points: RewardPoint,
            current_collator_scores: BoundedVec<CollatorScore<T::AccountId>, CollatorMaxScores>,
        ) {
            let collator_payout_period = Self::growth_period_info();
            let staking_reward_paid_in_era = payout.total_staking_reward;

            if Self::is_new_growth_period(&payout_era, &collator_payout_period) {
                <GrowthPeriod<T>>::mutate(|info| {
                    info.start_era_index = payout_era;
                    info.index = info.index.saturating_add(1);
                });

                let new_growth_period = Self::growth_period_info().index;
                let mut new_payout_info = GrowthInfo::new(payout_era);
                new_payout_info.number_of_accumulations = 1u32;
                new_payout_info.total_stake_accumulated = total_staked;
                new_payout_info.total_staker_reward = staking_reward_paid_in_era;
                new_payout_info.total_points = total_points;
                new_payout_info.collator_scores = current_collator_scores;

                <Growth<T>>::insert(new_growth_period, new_payout_info);

                Self::trigger_outstanding_growths(&(new_growth_period - 1));
            } else {
                Self::accumulate_payout_for_period(
                    collator_payout_period.index,
                    total_staked,
                    staking_reward_paid_in_era,
                    total_points,
                    current_collator_scores,
                );
            };
        }

        fn is_new_growth_period(
            era_index: &EraIndex,
            collator_payout_period: &GrowthPeriodInfo,
        ) -> bool {
            return collator_payout_period.index == 0 ||
                era_index - collator_payout_period.start_era_index >=
                    T::ErasPerGrowthPeriod::get()
        }

        fn accumulate_payout_for_period(
            growth_index: GrowthPeriodIndex,
            total_staked: BalanceOf<T>,
            staking_reward_paid_in_era: BalanceOf<T>,
            total_points: RewardPoint,
            current_collator_scores: BoundedVec<CollatorScore<T::AccountId>, CollatorMaxScores>,
        ) {
            <Growth<T>>::mutate(growth_index, |info| {
                info.number_of_accumulations = info.number_of_accumulations.saturating_add(1);
                info.total_stake_accumulated =
                    info.total_stake_accumulated.saturating_add(total_staked);
                info.total_staker_reward =
                    info.total_staker_reward.saturating_add(staking_reward_paid_in_era);
                info.total_points = info.total_points.saturating_add(total_points);
                info.collator_scores =
                    Self::update_collator_scores(&info.collator_scores, current_collator_scores);
            });
        }

        fn update_collator_scores(
            existing_collator_scores: &BoundedVec<CollatorScore<T::AccountId>, CollatorMaxScores>,
            current_collator_scores: BoundedVec<CollatorScore<T::AccountId>, CollatorMaxScores>,
        ) -> BoundedVec<CollatorScore<T::AccountId>, CollatorMaxScores> {
            let mut current_scores = existing_collator_scores
                .into_iter()
                .map(|current_score| (current_score.collator.clone(), current_score.points.clone()))
                .collect::<BTreeMap<_, _>>();

            current_collator_scores.into_iter().for_each(|new_score| {
                current_scores
                    .entry(new_score.collator)
                    .and_modify(|points| {
                        *points = points.saturating_add(new_score.points);
                    })
                    .or_insert(new_score.points);
            });

            return BoundedVec::truncate_from(
                current_scores
                    .into_iter()
                    .map(|(acc, pts)| CollatorScore::new(acc, pts))
                    .collect(),
            )
        }

        pub fn payout_collators(amount: BalanceOf<T>, growth_period: u32) -> DispatchResult {
            // The only validation we do is checking for replays, for everything else we trust T1.
            ensure!(
                <ProcessedGrowthPeriods<T>>::contains_key(growth_period) == false,
                Error::<T>::GrowthAlreadyProcessed
            );

            let mut imbalance: PositiveImbalanceOf<T> = PositiveImbalanceOf::<T>::zero();
            let mut pay =
                |collator_address: T::AccountId, amount: BalanceOf<T>| -> DispatchResult {
                    match T::Currency::deposit_into_existing(&collator_address, amount) {
                        Ok(amount_paid) => {
                            Self::deposit_event(Event::CollatorPaid {
                                account: collator_address,
                                amount: amount_paid.peek(),
                                period: growth_period,
                            });

                            imbalance.subsume(amount_paid);
                            return Ok(())
                        },
                        Err(e) => {
                            log::error!(
                                "💔💔 Error paying {:?} AVT to collator {:?}: {:?}",
                                amount,
                                collator_address,
                                e
                            );
                            return Err(Error::<T>::ErrorPayingCollator.into())
                        },
                    }
                };

            if <Growth<T>>::contains_key(growth_period) {
                // get the list of candidates that earned points from `growth_period`
                let growth_info = <Growth<T>>::get(growth_period);
                for collator_data in growth_info.collator_scores {
                    let percent =
                        Perbill::from_rational(collator_data.points, growth_info.total_points);
                    pay(collator_data.collator, percent * amount)?;
                }

                // Tidy up state
                <Growth<T>>::remove(growth_period);
                <ProcessedGrowthPeriods<T>>::insert(growth_period, ());
            } else {
                // use current candidates because there is no way of knowing who they were
                let collators = <SelectedCandidates<T>>::get();
                let number_of_collators = collators.len() as u32;
                for collator in collators.into_iter() {
                    let percent = Perbill::from_rational(1u32, number_of_collators);
                    pay(collator, percent * amount)?;
                }

                <ProcessedGrowthPeriods<T>>::insert(growth_period, ());
            }

            // Let the runtime know that we finished paying collators and we may have some amount
            // left.
            let dust_amount: BalanceOf<T> = amount.saturating_sub(imbalance.peek());

            // drop the imbalance to increase total issuance
            drop(imbalance);

            if dust_amount > BalanceOf::<T>::zero() {
                T::CollatorPayoutDustHandler::handle_dust(dust_amount);
            }

            Ok(())
        }

        pub fn collator_should_get_dust(
            dust: BalanceOf<T>,
            number_of_collators: u64,
            index: u64,
        ) -> bool {
            if dust.is_zero() {
                return false
            }

            let block_number: u64 =
                TryInto::<u64>::try_into(<frame_system::Pallet<T>>::block_number())
                    .unwrap_or_else(|_| 0u64);

            let chosen_collator_index = block_number % number_of_collators;

            return index == chosen_collator_index
        }

        pub fn identify_collators_to_withdraw_from(
            nominator: &T::AccountId,
            total_reduction: BalanceOf<T>,
        ) -> Result<(Vec<StakeInfo<T::AccountId, BalanceOf<T>>>, BalanceOf<T>), Error<T>> {
            let state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
            let net_total_bonded = state.total().saturating_sub(state.less_total);
            // Make sure the nominator has enough to unbond and stay above the min requirement
            ensure!(
                net_total_bonded >= Self::min_total_nominator_stake() + total_reduction,
                Error::<T>::NominatorBondBelowMin
            );

            // Desired balance on each collator after nominator reduces its stake
            let target_average_amount = Perbill::from_rational(1, state.nominations.0.len() as u32) *
                (net_total_bonded.saturating_sub(total_reduction));

            // Make sure each nominator will have at least required min amount
            ensure!(
                target_average_amount >= T::MinNominationPerCollator::get(),
                Error::<T>::NominationBelowMin
            );

            // The remaining amount the nominator wants to withdraw
            let mut outstanding_withdrawal = total_reduction;
            let mut payers: Vec<StakeInfo<T::AccountId, BalanceOf<T>>> = vec![];

            for bond in state.nominations.0.into_iter() {
                let amount_to_withdraw =
                    outstanding_withdrawal.min(bond.amount.saturating_sub(target_average_amount));

                if bond.amount >= amount_to_withdraw {
                    outstanding_withdrawal -= amount_to_withdraw;

                    payers.push(StakeInfo::new(
                        bond.owner,
                        bond.amount - (amount_to_withdraw + T::MinNominationPerCollator::get()),
                        amount_to_withdraw,
                    ));
                }

                if outstanding_withdrawal.is_zero() {
                    // exit early
                    break
                }
            }

            return Ok((payers, outstanding_withdrawal))
        }

        pub fn split_and_nominate(
            nominator: &T::AccountId,
            targets: Vec<<T::Lookup as StaticLookup>::Source>,
            amount: BalanceOf<T>,
        ) -> DispatchResultWithPostInfo {
            let num_collators = targets.len() as u32;
            let min_total_stake = Self::min_total_nominator_stake() * num_collators.into();

            ensure!(amount >= min_total_stake.into(), Error::<T>::NominatorBondBelowMin);
            ensure!(
                Self::get_nominator_stakable_free_balance(nominator) >= amount,
                Error::<T>::InsufficientBalance
            );

            let mut nomination_count = 0;
            if let Some(nominator_state) = <NominatorState<T>>::get(nominator) {
                nomination_count = nominator_state.nominations.0.len() as u32;
            }

            let amount_per_collator = Perbill::from_rational(1, num_collators) * amount;
            let dust = amount.saturating_sub(amount_per_collator * num_collators.into());
            let mut remaining_amount_to_nominate = amount;

            // This is only possible because we won't have more than 20 collators. If that changes,
            // we should not use a loop here.
            for (index, target) in targets.into_iter().enumerate() {
                let collator = T::Lookup::lookup(target)?;
                let collator_state =
                    <CandidateInfo<T>>::get(&collator).ok_or(Error::<T>::CandidateDNE)?;

                let mut actual_amount = amount_per_collator;
                if Self::collator_should_get_dust(dust, num_collators.into(), index as u64) {
                    actual_amount = amount_per_collator + dust;
                }

                // make sure we don't nominate more than what the user asked
                actual_amount = remaining_amount_to_nominate.min(actual_amount);

                Self::call_nominate(
                    nominator,
                    collator,
                    actual_amount,
                    collator_state.nomination_count,
                    nomination_count,
                )?;

                remaining_amount_to_nominate -= actual_amount;
                nomination_count += 1;
            }

            Ok(().into())
        }

        pub fn trigger_outstanding_growths(latest_period: &u32) {
            let periods_to_process = Self::get_untriggered_growths(*latest_period);

            for growth_period in periods_to_process.iter() {
                if <Growth<T>>::contains_key(growth_period) {
                    let growth_info = <Growth<T>>::get(growth_period);

                    if <ProcessedGrowthPeriods<T>>::contains_key(growth_period) ||
                        growth_info.tx_id.is_some() ||
                        growth_info.triggered.is_some()
                    {
                        log::warn!("Growth for period {:?} is already processed. Tx id: {:?}, triggered: {:?}", growth_period, growth_info.tx_id, growth_info.triggered);
                        continue
                    }

                    if growth_info.number_of_accumulations == 0u32 ||
                        growth_info.total_stake_accumulated == 0u32.into() ||
                        growth_info.total_staker_reward == 0u32.into()
                    {
                        log::warn!("Growth for period {:?} will be 0, skipping it.", growth_period);
                        <LastTriggeredGrowthPeriod<T>>::put(growth_period);
                        <Growth<T>>::mutate(growth_period, |growth| {
                            growth.tx_id = Some(0u32);
                        });

                        continue
                    }

                    let result = Self::trigger_growth_on_t1(&growth_period, growth_info);
                    if result.is_err() {
                        log::error!(
                            "💔 Error triggering growth for period {:?}. {:?}",
                            growth_period,
                            result
                        );
                    }
                }
            }
        }

        pub fn trigger_growth_on_t1(
            growth_period: &u32,
            growth_info: GrowthInfo<T::AccountId, BalanceOf<T>>,
        ) -> Result<(), DispatchError> {
            let rewards_in_period_128 = TryInto::<u128>::try_into(growth_info.total_staker_reward)
                .map_err(|_| DispatchError::Other(Error::<T>::ErrorConvertingBalance.into()))?;

            let average_staked_in_period_128 = TryInto::<u128>::try_into(
                growth_info.total_stake_accumulated / growth_info.number_of_accumulations.into(),
            )
            .map_err(|_| DispatchError::Other(Error::<T>::ErrorConvertingBalance.into()))?;

            let function_name: &[u8] = BridgeContractMethod::TriggerGrowth.name_as_bytes();
            let params = vec![
                (b"uint256".to_vec(), format!("{}", rewards_in_period_128).as_bytes().to_vec()),
                (
                    b"uint256".to_vec(),
                    format!("{}", average_staked_in_period_128).as_bytes().to_vec(),
                ),
                (b"uint32".to_vec(), format!("{}", growth_period).as_bytes().to_vec()),
            ];
            let tx_id = T::BridgeInterface::publish(function_name, &params, PALLET_ID.to_vec())
                .map_err(|e| DispatchError::Other(e.into()))?;

            <LastTriggeredGrowthPeriod<T>>::put(growth_period);
            <PublishedGrowth<T>>::insert(tx_id, growth_period);
            <Growth<T>>::mutate(growth_period, |growth| {
                growth.tx_id = Some(tx_id.into());
            });

            return Ok(())
        }

        pub fn get_untriggered_growths(current_period: u32) -> Vec<u32> {
            let starting_period = Self::last_triggered_growth_period() + 1;
            return (starting_period..=current_period).take(MAX_GROWTHS_TO_PROCESS).collect()
        }

        pub fn try_get_growth_data(
            growth_period: &u32,
        ) -> Result<GrowthInfo<T::AccountId, BalanceOf<T>>, Error<T>> {
            if <Growth<T>>::contains_key(growth_period) {
                return Ok(<Growth<T>>::get(growth_period))
            }

            Err(Error::<T>::GrowthDataNotFound)?
        }
    }

    /// Keep track of number of authored blocks per authority, uncles are counted as well since
    /// they're a valid proof of being online.
    impl<T: Config> pallet_authorship::EventHandler<T::AccountId, BlockNumberFor<T>> for Pallet<T> {
        /// Add reward points to block authors:
        /// * 20 points to the block producer for producing a block in the chain
        fn note_author(author: T::AccountId) {
            let now = <Era<T>>::get().current;
            let score_plus_20 = <AwardedPts<T>>::get(now, &author).saturating_add(20);
            <AwardedPts<T>>::insert(now, author, score_plus_20);
            <Points<T>>::mutate(now, |x| *x = x.saturating_add(20));

            frame_system::Pallet::<T>::register_extra_weight_unchecked(
                <T as Config>::WeightInfo::note_author(),
                DispatchClass::Mandatory,
            );
        }
    }
    impl<T: Config> OnGrowthLiftedHandler<BalanceOf<T>> for Pallet<T> {
        fn on_growth_lifted(amount: BalanceOf<T>, growth_period: u32) -> DispatchResult {
            return Self::payout_collators(amount, growth_period)
        }
    }
}

impl<T: Config> BridgeInterfaceNotification for Pallet<T> {
    fn process_result(tx_id: u32, caller_id: Vec<u8>, succeeded: bool) -> DispatchResult {
        // The tx_id might not be relevant for this pallet so we must not error if we don't know it.
        if caller_id == PALLET_ID.to_vec() && <PublishedGrowth<T>>::contains_key(tx_id) {
            let growth_period = <PublishedGrowth<T>>::get(tx_id);
            <Growth<T>>::mutate(growth_period, |growth| growth.triggered = Some(succeeded));
        }

        Ok(())
    }
}

/// [`TypedGet`] implementaion to get the AccountId of the StakingPot.
pub struct StakingPotAccountId<R>(PhantomData<R>);
impl<R> TypedGet for StakingPotAccountId<R>
where
    R: crate::Config,
{
    type Type = <R as frame_system::Config>::AccountId;
    fn get() -> Self::Type {
        <crate::Pallet<R>>::compute_reward_pot_account_id()
    }
}
