// Copyright 2026 Aventus DAO Ltd

//! # Watchtower floor oracle: per chain checkpoints
//!
//! Before any floor price logic can exist, every node has to observe an external chain
//! (Ethereum, Base, ...) at the same point in time. This pallet keeps one agreed **checkpoint**
//! per external chain: a block number, the parachain timestamp when agreement completed and the
//! id of the round that produced it.
//!
//! - Root seeds the initial block number for each chain with [`Pallet::seed_chain`].
//! - Consumers ask for a checkpoint through [`CheckpointOracle::request_checkpoint`]. If the stored
//!   checkpoint is younger than the chain's freshness limit it is returned as is. Otherwise a
//!   voting round starts, or the running one is joined, and the caller gets `Pending`.
//! - Collators vote their own finalised view of the chain with [`Pallet::submit_checkpoint_vote`],
//!   produced by their offchain worker (see [`checkpoint`]). The value is picked with the same
//!   supermajority / quorum selection that pallet-eth-bridge uses for its latest block. The
//!   external service rounds the chain head down to a fixed interval after a confirmation depth so
//!   every collator votes the same number; a round therefore settles within roughly one such
//!   interval of lag plus a few parachain blocks. A chain no collator can reach gets no votes: its
//!   rounds time out and restart forever, visible as repeated `CheckpointRoundRestarted`.
//! - A checkpoint is written exactly once per round and never changed. A later round writes a new
//!   checkpoint under a new round id.
//!
//! There is no background polling, no block rounding and no height advancement. Freshness is
//! measured as time since agreement, not time since the external chain last advanced.
//!
//! # Oracle rounds
//!
//! On top of the checkpoints, the pallet runs two independent request channels with the same
//! shape: a request pins the current checkpoint of the channel's chain, opens a watchtower
//! `Choice` proposal, and watchtower nodes report what they observed at that pinned point. The
//! report is the vote; the first observation reaching the proposal's threshold wins.
//!
//! - **Floor** ([`floor`]): the Punks floor on Ethereum, stored in `LastFinalisedFloor`.
//! - **Venue** ([`venue`]): the raw state of the Exotic perpetuals market for Punks on the venue
//!   chain (Base), stored in `LastFinalisedVenue`.
//!
//! The contracts each channel reads, and the chain they live on, are root configured
//! (`FloorMarketConfig`, `VenueMarketConfig`) so mainnet and test networks differ only in
//! storage. A round snapshots its market when it starts.
//!
//! Both channels share the requester, the round parameters and the committee model. Their
//! proposals are serialised by the watchtower pallet, which runs one internal proposal at a
//! time, so a round requested while the other channel's round is active waits in the queue.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::{format, string::String};

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::{
    dispatch::{DispatchResult, DispatchResultWithPostInfo},
    pallet_prelude::*,
    traits::UnixTime,
    BoundedBTreeMap,
};
use frame_system::{
    ensure_none, ensure_root, ensure_signed,
    offchain::{CreateBare, CreateTransactionBase, SubmitTransaction},
    pallet_prelude::*,
};
pub use pallet_avn::{self as avn, MAX_VALIDATOR_ACCOUNTS};
use pallet_watchtower::NodesInterface;
use sp_avn_common::{
    bounds::MaximumValidatorsBound,
    constants::context::SUBMIT_CHECKPOINT_VOTE_CONTEXT,
    event_types::Validator,
    quorum::{select_with_quorum, QuorumSelection},
    QuorumPolicy,
};
use sp_core::H256;
pub use sp_floor_oracle::{
    CountryCode, FinalisedFloorRound, FinalisedVenueRound, FloorMarket, FloorObservation,
    VenueMarket, VenueObservation, FLOOR_ORACLE_REPORT_CONTEXT, FLOOR_SERVICE_PATH,
    NODE_COUNTRY_SERVICE_PATH, UNKNOWN_COUNTRY, VENUE_ORACLE_REPORT_CONTEXT, VENUE_SERVICE_PATH,
};
use sp_runtime::{
    traits::{Dispatchable, Saturating},
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
        ValidTransaction,
    },
    DispatchError, Perbill, RuntimeAppPublic,
};
use sp_std::prelude::*;
pub use sp_watchtower::{
    Checkpoint, CheckpointOracle, CheckpointRequest, ExternalChainId, ProposalId,
    ProposalStatusEnum, RoundId, VoteFilter, WatchtowerHooks,
};

pub mod default_weights;
pub use default_weights::WeightInfo;
pub mod checkpoint;
pub mod floor;
pub use floor::FloorRound;
pub mod ocw;
pub mod venue;
pub use venue::VenueRound;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[cfg(test)]
#[path = "tests/checkpoint_ocw_tests.rs"]
mod checkpoint_ocw_tests;
#[cfg(test)]
#[path = "tests/floor_tests.rs"]
mod floor_tests;
#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "tests/venue_tests.rs"]
mod venue_tests;

pub use pallet::*;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

pub type AVN<T> = avn::Pallet<T>;
pub type Author<T> =
    Validator<<T as avn::Config>::AuthorityId, <T as frame_system::Config>::AccountId>;

/// `InvalidTransaction::Custom` codes returned by `validate_unsigned`.
pub const VOTE_NO_MATCHING_ROUND: u8 = 1;
pub const VOTE_ALREADY_CAST: u8 = 2;
pub const VOTE_BAD_SIGNATURE: u8 = 3;
pub const VOTE_BEHIND_CHECKPOINT: u8 = 4;
pub const FLOOR_REPORT_NOT_VALID: u8 = 10;
pub const VENUE_REPORT_NOT_VALID: u8 = 11;

/// State of the checkpoint voting round that is running for one chain.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct CheckpointRoundState<AccountId: Ord, BlockNumber> {
    pub round_id: RoundId,
    /// Parachain block the round started at. Used for the lazy timeout check.
    pub started_at: BlockNumber,
    /// Finalised block number each collator reported.
    pub votes: BoundedBTreeMap<AccountId, u32, MaximumValidatorsBound>,
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config:
        frame_system::Config
        + avn::Config
        + pallet_watchtower::Config
        + CreateTransactionBase<Call<Self>>
        + CreateBare<Call<Self>>
    {
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + From<Call<Self>>;

        /// Share of the committee that must report the same observation.
        #[pallet::constant]
        type OracleRoundThreshold: Get<Perbill>;

        /// Voting period of an oracle round (floor or venue), in blocks.
        #[pallet::constant]
        type OracleVoteDuration: Get<u32>;

        /// A finalised floor younger than this, in seconds, answers a request directly.
        #[pallet::constant]
        type MaxFloorAge: Get<u64>;

        /// A finalised venue observation whose pinned block is younger than this, in seconds,
        /// answers a request directly.
        #[pallet::constant]
        type MaxVenueAge: Get<u64>;

        /// Source of the parachain timestamp stored in a checkpoint.
        type TimeProvider: UnixTime;

        /// Quorum numbers used to settle a round.
        type Quorum: QuorumPolicy;

        /// A round older than this, in parachain blocks, is restarted by the next request.
        #[pallet::constant]
        type RoundTimeoutBlocks: Get<BlockNumberFor<Self>>;

        type WeightInfo: WeightInfo;
    }

    /// Freshness limit per chain, in seconds. A chain is registered when it has an entry here.
    #[pallet::storage]
    #[pallet::getter(fn max_checkpoint_age)]
    pub type MaxCheckpointAge<T: Config> =
        StorageMap<_, Blake2_128Concat, ExternalChainId, u64, OptionQuery>;

    /// The latest agreed checkpoint per chain. This is what nodes and other pallets read.
    #[pallet::storage]
    #[pallet::getter(fn checkpoints)]
    pub type Checkpoints<T: Config> =
        StorageMap<_, Blake2_128Concat, ExternalChainId, Checkpoint, OptionQuery>;

    /// The checkpoint voting round running per chain, if any.
    #[pallet::storage]
    #[pallet::getter(fn checkpoint_round)]
    pub type CheckpointRound<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        ExternalChainId,
        CheckpointRoundState<T::AccountId, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// Last round id handed out. Round ids are unique across chains; 0 marks a seeded value.
    #[pallet::storage]
    #[pallet::getter(fn last_checkpoint_round_id)]
    pub type LastCheckpointRoundId<T: Config> = StorageValue<_, RoundId, ValueQuery>;

    /// The Punks market floor rounds read. Set by root; a round snapshots it when it starts.
    #[pallet::storage]
    #[pallet::getter(fn floor_market)]
    pub type FloorMarketConfig<T: Config> = StorageValue<_, FloorMarket, OptionQuery>;

    /// The floor round collecting reports, if any. Only one runs at a time.
    #[pallet::storage]
    #[pallet::getter(fn active_floor_round)]
    pub type ActiveFloorRound<T: Config> =
        StorageValue<_, FloorRound<BlockNumberFor<T>>, OptionQuery>;

    /// Hash of the observation each watchtower reported, per floor round.
    #[pallet::storage]
    #[pallet::getter(fn floor_reports)]
    pub type FloorReports<T: Config> =
        StorageDoubleMap<_, Twox64Concat, u64, Blake2_128Concat, T::AccountId, H256, OptionQuery>;

    /// The distinct observations reported per floor round, by hash. The winning hash is looked
    /// up here when the round settles.
    #[pallet::storage]
    #[pallet::getter(fn floor_observations)]
    pub type FloorObservations<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        u64,
        Blake2_128Concat,
        H256,
        FloorObservation,
        OptionQuery,
    >;

    /// Last floor round id handed out.
    #[pallet::storage]
    #[pallet::getter(fn last_floor_round_id)]
    pub type LastFloorRoundId<T: Config> = StorageValue<_, u64, ValueQuery>;

    /// A floor request is waiting for the checkpoint round on `FloorChainId` to settle.
    #[pallet::storage]
    #[pallet::getter(fn pending_floor_request)]
    pub type PendingFloorRequest<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// The most recent floor the watchtowers agreed on.
    #[pallet::storage]
    #[pallet::getter(fn last_finalised_floor)]
    pub type LastFinalisedFloor<T: Config> = StorageValue<_, FinalisedFloorRound, OptionQuery>;

    /// Watchtowers sampled per oracle round (floor or venue). Unset lets every watchtower
    /// report. Set by root; applies to rounds started afterwards.
    #[pallet::storage]
    #[pallet::getter(fn oracle_committee_size)]
    pub type OracleCommitteeSize<T: Config> = StorageValue<_, u32, OptionQuery>;

    /// The account allowed to request floor prices and venue state besides root. Set by root.
    #[pallet::storage]
    #[pallet::getter(fn oracle_requester)]
    pub type OracleRequester<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    // ------------------------------------------------------------------ venue channel

    /// The Exotic market venue rounds read. Set by root; a round snapshots it when it starts.
    #[pallet::storage]
    #[pallet::getter(fn venue_market)]
    pub type VenueMarketConfig<T: Config> = StorageValue<_, VenueMarket, OptionQuery>;

    /// The venue round collecting reports, if any. Only one runs at a time.
    #[pallet::storage]
    #[pallet::getter(fn active_venue_round)]
    pub type ActiveVenueRound<T: Config> =
        StorageValue<_, VenueRound<BlockNumberFor<T>>, OptionQuery>;

    /// Hash of the observation each watchtower reported, per venue round.
    #[pallet::storage]
    #[pallet::getter(fn venue_reports)]
    pub type VenueReports<T: Config> =
        StorageDoubleMap<_, Twox64Concat, u64, Blake2_128Concat, T::AccountId, H256, OptionQuery>;

    /// The distinct observations reported per venue round, by hash.
    #[pallet::storage]
    #[pallet::getter(fn venue_observations)]
    pub type VenueObservations<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        u64,
        Blake2_128Concat,
        H256,
        VenueObservation,
        OptionQuery,
    >;

    /// Last venue round id handed out.
    #[pallet::storage]
    #[pallet::getter(fn last_venue_round_id)]
    pub type LastVenueRoundId<T: Config> = StorageValue<_, u64, ValueQuery>;

    /// A venue request is waiting for the checkpoint round on the market's chain to settle.
    #[pallet::storage]
    #[pallet::getter(fn pending_venue_request)]
    pub type PendingVenueRequest<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// The most recent venue state the watchtowers agreed on.
    #[pallet::storage]
    #[pallet::getter(fn last_finalised_venue)]
    pub type LastFinalisedVenue<T: Config> = StorageValue<_, FinalisedVenueRound, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// The freshness limit of a chain was set.
        FreshnessLimitSet { chain_id: ExternalChainId, max_checkpoint_age_secs: u64 },
        /// Root wrote the initial checkpoint of a chain.
        CheckpointSeeded { chain_id: ExternalChainId, block_number: u32, agreed_at: u64 },
        /// A voting round started because no fresh checkpoint exists.
        CheckpointRoundStarted { chain_id: ExternalChainId, round_id: RoundId },
        /// A round ran past the timeout and was replaced by a new one.
        CheckpointRoundRestarted {
            chain_id: ExternalChainId,
            old_round_id: RoundId,
            new_round_id: RoundId,
        },
        /// Collators agreed on a checkpoint.
        CheckpointAgreed {
            chain_id: ExternalChainId,
            round_id: RoundId,
            block_number: u32,
            agreed_at: u64,
        },
        /// Outcome of a root `request_checkpoint` call.
        CheckpointRequestServed { chain_id: ExternalChainId, ready: bool, round_id: RoundId },
        /// A request was answered from a floor that is still within `MaxFloorAge`.
        FloorPriceServed { round_id: u64, floor_wei: u128 },
        /// A request joined the round already running.
        FloorRoundInProgress { round_id: u64 },
        /// No fresh checkpoint exists; the request waits for the checkpoint round to settle.
        FloorRequestWaitingForCheckpoint { checkpoint_round_id: RoundId },
        /// A floor round opened as a watchtower proposal.
        FloorRoundStarted {
            round_id: u64,
            proposal_id: ProposalId,
            pinned_ts: u64,
            pinned_block: u32,
            market: FloorMarket,
        },
        /// Root changed the floor market. `None` disables floor requests.
        FloorMarketSet { market: Option<FloorMarket> },
        /// Root changed the committee size of oracle rounds. `None` lets every watchtower
        /// report. Rounds already started keep their committee.
        OracleCommitteeSizeSet { size: Option<u32> },
        /// The watchtower pallet refused the proposal; the request was dropped.
        FloorRoundStartFailed { error: DispatchError },
        /// A watchtower reported its observation; `hash` is its answer on the proposal and
        /// `country` the self-reported location of the node, for display.
        FloorReportSubmitted {
            round_id: u64,
            watchtower: T::AccountId,
            hash: H256,
            country: CountryCode,
        },
        /// The watchtowers agreed on a floor.
        FloorRoundFinalised {
            round_id: u64,
            floor_wei: u128,
            agreeing: u32,
            reports: u32,
            required: u32,
        },
        /// The round ended without agreement.
        FloorRoundFailed { round_id: u64, result: ProposalStatusEnum },
        /// Root changed the privileged oracle requester. `None` leaves root as the only requester.
        OracleRequesterSet { who: Option<T::AccountId> },
        /// Root changed the venue market. `None` disables venue requests.
        VenueMarketSet { market: Option<VenueMarket> },
        /// A request was answered from a venue state whose pinned block is within `MaxVenueAge`.
        VenueStateServed { round_id: u64, pinned_block: u64 },
        /// A request joined the venue round already running.
        VenueRoundInProgress { round_id: u64 },
        /// No fresh checkpoint exists for the venue chain; the request waits for the round.
        VenueRequestWaitingForCheckpoint { checkpoint_round_id: RoundId },
        /// A venue round opened as a watchtower proposal.
        VenueRoundStarted {
            round_id: u64,
            proposal_id: ProposalId,
            pinned_block: u32,
            market: VenueMarket,
        },
        /// The watchtower pallet refused the venue proposal; the request was dropped.
        VenueRoundStartFailed { error: DispatchError },
        /// A watchtower reported its venue observation; `hash` is its answer on the proposal
        /// and `country` the self-reported location of the node, for display.
        VenueReportSubmitted {
            round_id: u64,
            watchtower: T::AccountId,
            hash: H256,
            country: CountryCode,
        },
        /// The watchtowers agreed on the venue state. Raw contract values, 18 decimals.
        VenueRoundFinalised {
            round_id: u64,
            pinned_block: u64,
            spot_e18: u128,
            oracle_e18: u128,
            oi_notional_e18: u128,
            agreeing: u32,
            reports: u32,
            required: u32,
        },
        /// The venue round ended without agreement.
        VenueRoundFailed { round_id: u64, result: ProposalStatusEnum },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The chain has not been seeded by root.
        ChainNotRegistered,
        /// The freshness limit must be greater than zero.
        InvalidFreshnessLimit,
        /// No voting round is running for this chain.
        NoActiveRound,
        /// The vote names a round that is not the running one.
        RoundMismatch,
        /// The author already voted in this round.
        DuplicateVote,
        /// The author is not a current validator.
        NotAValidator,
        /// The voted height is below the current checkpoint. Checkpoints never move backwards.
        BlockBehindCheckpoint,
        /// The round cannot hold any more votes.
        TooManyVotes,
        /// No floor round is collecting reports.
        NoActiveFloorRound,
        /// The report names a round that is not the running one.
        FloorRoundMismatch,
        /// The round's watchtower proposal is not active yet, so there is no committee.
        FloorProposalNotActive,
        /// The reporter is not an authorized watchtower in the round's committee.
        NotInFloorCommittee,
        /// The watchtower already reported in this round.
        DuplicateFloorReport,
        /// The observation fails the plausibility checks.
        ImplausibleObservation,
        /// The observation was not taken at the round's pinned block.
        ObservationBlockMismatch,
        /// The voter has not reported an observation for this round.
        NoFloorReport,
        /// A vote on a floor proposal that is not the hash the voter reported.
        VoteContradictsReport,
        /// Floor proposals are decided by reports; yes/no votes are not accepted.
        FloorVotesAreReports,
        /// The report signature does not verify against the watchtower's signing key.
        FloorReportBadSignature,
        /// Only root or the account set by `set_oracle_requester` may request oracle data.
        UnauthorizedOracleRequest,
        /// No floor market is configured; root must call `set_floor_market` first.
        FloorMarketNotSet,
        /// The committee size is below the watchtower pallet's minimum or above its maximum.
        CommitteeSizeOutOfBounds,
        /// The committee size exceeds the number of authorized watchtowers; such a proposal
        /// could never activate and would block the queue.
        CommitteeSizeExceedsWatchtowers,
        /// No venue market is configured; root must call `set_venue_market` first.
        VenueMarketNotSet,
        /// No venue round is collecting reports.
        NoActiveVenueRound,
        /// The report names a venue round that is not the running one.
        VenueRoundMismatch,
        /// The venue round's watchtower proposal is not active yet, so there is no committee.
        VenueProposalNotActive,
        /// The reporter is not an authorized watchtower in the venue round's committee.
        NotInVenueCommittee,
        /// The watchtower already reported in this venue round.
        DuplicateVenueReport,
        /// The venue observation fails the plausibility checks.
        ImplausibleVenueObservation,
        /// The venue observation was not taken at the round's pinned block.
        VenueObservationBlockMismatch,
        /// The voter has not reported a venue observation for this round.
        NoVenueReport,
        /// A vote on a venue proposal that is not the hash the voter reported.
        VenueVoteContradictsReport,
        /// Venue proposals are decided by reports; yes/no votes are not accepted.
        VenueVotesAreReports,
        /// The venue report signature does not verify against the watchtower's signing key.
        VenueReportBadSignature,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Register a chain and, if it has no checkpoint yet, seed its initial block number.
        ///
        /// Calling it again on a seeded chain only updates the freshness limit.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::seed_chain())]
        pub fn seed_chain(
            origin: OriginFor<T>,
            chain_id: ExternalChainId,
            initial_block_number: u32,
            max_checkpoint_age_secs: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(max_checkpoint_age_secs > 0, Error::<T>::InvalidFreshnessLimit);

            MaxCheckpointAge::<T>::insert(chain_id, max_checkpoint_age_secs);
            Self::deposit_event(Event::<T>::FreshnessLimitSet {
                chain_id,
                max_checkpoint_age_secs,
            });

            if !Checkpoints::<T>::contains_key(chain_id) {
                let agreed_at = Self::now_secs();
                Checkpoints::<T>::insert(
                    chain_id,
                    Checkpoint { block_number: initial_block_number, agreed_at, round_id: 0 },
                );
                Self::deposit_event(Event::<T>::CheckpointSeeded {
                    chain_id,
                    block_number: initial_block_number,
                    agreed_at,
                });
            }

            Ok(())
        }

        /// Ask for a checkpoint the same way another pallet would. Meant for operations and
        /// testing; the result is reported as an event.
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::request_checkpoint())]
        pub fn request_checkpoint(
            origin: OriginFor<T>,
            chain_id: ExternalChainId,
        ) -> DispatchResult {
            ensure_root(origin)?;

            let (ready, round_id) = match Self::do_request_checkpoint(chain_id)? {
                CheckpointRequest::Ready(checkpoint) => (true, checkpoint.round_id),
                CheckpointRequest::Pending { round_id, .. } => (false, round_id),
            };
            Self::deposit_event(Event::<T>::CheckpointRequestServed { chain_id, ready, round_id });

            Ok(())
        }

        /// Ask for the current Punks floor. Root or the privileged requester only. Answers from
        /// the last finalised floor when it is fresh, joins the running round, or opens a new
        /// one (waiting for a checkpoint first if needed). The outcome is reported as an event.
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::request_floor_price())]
        pub fn request_floor_price(origin: OriginFor<T>) -> DispatchResult {
            Self::ensure_oracle_requester(origin)?;
            Self::do_request_floor_price()
        }

        /// Root sets (or with `None` clears) the account allowed to request floor prices.
        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::set_oracle_requester())]
        pub fn set_oracle_requester(
            origin: OriginFor<T>,
            who: Option<T::AccountId>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            match &who {
                Some(account) => OracleRequester::<T>::put(account),
                None => OracleRequester::<T>::kill(),
            }
            Self::deposit_event(Event::<T>::OracleRequesterSet { who });
            Ok(())
        }

        /// A watchtower's observation of the floor for the running round. `country` is the
        /// node's self-reported location, only echoed in the event.
        ///
        /// Unsigned; the watchtower's node key signs [`Pallet::report_signing_payload`].
        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::submit_floor_report())]
        pub fn submit_floor_report(
            origin: OriginFor<T>,
            round_id: u64,
            observation: FloorObservation,
            country: CountryCode,
            watchtower: T::AccountId,
            _signature: <T::SignerId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;
            Self::do_submit_floor_report(round_id, observation, country, watchtower)
        }

        /// Ask for the current state of the Exotic venue. Root or the privileged requester
        /// only. Answers from the last finalised state when it is fresh, joins the running
        /// round, or opens a new one (waiting for a checkpoint first if needed). The outcome is
        /// reported as an event.
        #[pallet::call_index(6)]
        #[pallet::weight(<T as Config>::WeightInfo::request_venue_state())]
        pub fn request_venue_state(origin: OriginFor<T>) -> DispatchResult {
            Self::ensure_oracle_requester(origin)?;
            Self::do_request_venue_state()
        }

        /// A watchtower's observation of the venue for the running round. `country` is the
        /// node's self-reported location, only echoed in the event.
        ///
        /// Unsigned; the watchtower's node key signs [`Pallet::venue_report_signing_payload`].
        #[pallet::call_index(7)]
        #[pallet::weight(<T as Config>::WeightInfo::submit_venue_report())]
        pub fn submit_venue_report(
            origin: OriginFor<T>,
            round_id: u64,
            observation: VenueObservation,
            country: CountryCode,
            watchtower: T::AccountId,
            _signature: <T::SignerId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;
            Self::do_submit_venue_report(round_id, observation, country, watchtower)
        }

        /// Root sets (or with `None` clears) the Exotic market venue rounds read. A running
        /// round keeps the market it started with.
        #[pallet::call_index(8)]
        #[pallet::weight(<T as Config>::WeightInfo::set_venue_market())]
        pub fn set_venue_market(
            origin: OriginFor<T>,
            market: Option<VenueMarket>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            match market {
                Some(market) => VenueMarketConfig::<T>::put(market),
                None => VenueMarketConfig::<T>::kill(),
            }
            Self::deposit_event(Event::<T>::VenueMarketSet { market });
            Ok(())
        }

        /// Root sets (or with `None` clears) the Punks market floor rounds read. A running
        /// round keeps the market it started with.
        #[pallet::call_index(9)]
        #[pallet::weight(<T as Config>::WeightInfo::set_floor_market())]
        pub fn set_floor_market(
            origin: OriginFor<T>,
            market: Option<FloorMarket>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            match market {
                Some(market) => FloorMarketConfig::<T>::put(market),
                None => FloorMarketConfig::<T>::kill(),
            }
            Self::deposit_event(Event::<T>::FloorMarketSet { market });
            Ok(())
        }

        /// Root sets how many watchtowers are sampled per oracle round, or with `None` lets
        /// every watchtower report. Bounded by the watchtower pallet's committee limits and
        /// by the number of authorized watchtowers. Rounds already started are unaffected.
        #[pallet::call_index(10)]
        #[pallet::weight(<T as Config>::WeightInfo::set_oracle_committee_size())]
        pub fn set_oracle_committee_size(
            origin: OriginFor<T>,
            size: Option<u32>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            match size {
                Some(size) => {
                    ensure!(
                        size >= T::MinCommitteeSize::get() && size <= T::MaxCommitteeSize::get(),
                        Error::<T>::CommitteeSizeOutOfBounds
                    );
                    ensure!(
                        size <= T::Watchtowers::get_authorized_watchtowers_count(),
                        Error::<T>::CommitteeSizeExceedsWatchtowers
                    );
                    OracleCommitteeSize::<T>::put(size)
                },
                None => OracleCommitteeSize::<T>::kill(),
            }
            Self::deposit_event(Event::<T>::OracleCommitteeSizeSet { size });
            Ok(())
        }

        /// A collator's view of the finalised block number of `chain_id` for round `round_id`.
        ///
        /// Unsigned; the author's AVN key signs [`Pallet::vote_signing_payload`].
        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::submit_checkpoint_vote(MAX_VALIDATOR_ACCOUNTS).max(
            <T as Config>::WeightInfo::submit_checkpoint_vote_with_quorum(MAX_VALIDATOR_ACCOUNTS)
        ))]
        pub fn submit_checkpoint_vote(
            origin: OriginFor<T>,
            chain_id: ExternalChainId,
            round_id: RoundId,
            author: Author<T>,
            block_number: u32,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;

            let mut round = CheckpointRound::<T>::get(chain_id).ok_or(Error::<T>::NoActiveRound)?;
            ensure!(round.round_id == round_id, Error::<T>::RoundMismatch);
            ensure!(
                Self::is_at_or_above_checkpoint(chain_id, block_number),
                Error::<T>::BlockBehindCheckpoint
            );

            let validators = AVN::<T>::validators();
            ensure!(
                validators.iter().any(|v| v.account_id == author.account_id),
                Error::<T>::NotAValidator
            );

            // Votes from accounts that left the validator set no longer count, so the vote
            // count stays consistent with the quorum numbers of the current set.
            round
                .votes
                .retain(|account, _| validators.iter().any(|v| v.account_id == *account));

            ensure!(!round.votes.contains_key(&author.account_id), Error::<T>::DuplicateVote);
            round
                .votes
                .try_insert(author.account_id, block_number)
                .map_err(|_| Error::<T>::TooManyVotes)?;

            let mut buckets: Vec<(u32, usize)> = Vec::new();
            for (_, voted_block) in round.votes.iter() {
                match buckets.iter_mut().find(|(block, _)| block == voted_block) {
                    Some(bucket) => bucket.1 += 1,
                    None => buckets.push((*voted_block, 1)),
                }
            }

            let selection = select_with_quorum(
                &buckets,
                T::Quorum::get_supermajority_quorum() as usize,
                T::Quorum::get_quorum() as usize,
            );

            let validators_count = validators.len() as u32;
            match selection {
                QuorumSelection::Selected(agreed_block) => {
                    let agreed_at = Self::now_secs();
                    Checkpoints::<T>::insert(
                        chain_id,
                        Checkpoint { block_number: agreed_block, agreed_at, round_id },
                    );
                    CheckpointRound::<T>::remove(chain_id);
                    Self::deposit_event(Event::<T>::CheckpointAgreed {
                        chain_id,
                        round_id,
                        block_number: agreed_block,
                        agreed_at,
                    });
                    Self::on_checkpoint_agreed(
                        chain_id,
                        &Checkpoint { block_number: agreed_block, agreed_at, round_id },
                    );
                    Ok(Some(<T as Config>::WeightInfo::submit_checkpoint_vote_with_quorum(
                        validators_count,
                    ))
                    .into())
                },
                QuorumSelection::BelowThreshold | QuorumSelection::Unresolved => {
                    if selection == QuorumSelection::Unresolved {
                        log::warn!(
                            "💔 Checkpoint round {:?} for chain {:?} has enough votes but no quorum",
                            round_id,
                            chain_id
                        );
                    }
                    CheckpointRound::<T>::insert(chain_id, round);
                    Ok(Some(<T as Config>::WeightInfo::submit_checkpoint_vote(validators_count))
                        .into())
                },
            }
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            let reduce_priority: TransactionPriority = TransactionPriority::from(1000u64);

            match call {
                Call::submit_checkpoint_vote {
                    chain_id,
                    round_id,
                    author,
                    block_number,
                    signature,
                } => {
                    let round = match CheckpointRound::<T>::get(chain_id) {
                        Some(round) if round.round_id == *round_id => round,
                        _ => return InvalidTransaction::Custom(VOTE_NO_MATCHING_ROUND).into(),
                    };
                    if round.votes.contains_key(&author.account_id) {
                        return InvalidTransaction::Custom(VOTE_ALREADY_CAST).into()
                    }
                    if !Self::is_at_or_above_checkpoint(*chain_id, *block_number) {
                        return InvalidTransaction::Custom(VOTE_BEHIND_CHECKPOINT).into()
                    }

                    let payload = Self::vote_signing_payload(
                        *chain_id,
                        *round_id,
                        &author.account_id,
                        *block_number,
                    );
                    if !Self::vote_signature_is_valid(&payload, author, signature) {
                        return InvalidTransaction::Custom(VOTE_BAD_SIGNATURE).into()
                    }

                    ValidTransaction::with_tag_prefix("WatchtowerFloorOracleVote")
                        .and_provides((chain_id, round_id, author.account_id.clone()))
                        .priority(TransactionPriority::max_value() - reduce_priority)
                        .longevity(64_u64)
                        .propagate(true)
                        .build()
                },
                Call::submit_floor_report {
                    round_id,
                    observation,
                    country,
                    watchtower,
                    signature,
                } => {
                    if Self::validate_floor_report(
                        *round_id,
                        observation,
                        country,
                        watchtower,
                        signature,
                    )
                    .is_err()
                    {
                        return InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID).into()
                    }

                    ValidTransaction::with_tag_prefix("WatchtowerFloorOracleReport")
                        .and_provides((round_id, watchtower.clone()))
                        .priority(TransactionPriority::max_value() - reduce_priority)
                        .longevity(64_u64)
                        .propagate(true)
                        .build()
                },
                Call::submit_venue_report {
                    round_id,
                    observation,
                    country,
                    watchtower,
                    signature,
                } => {
                    if Self::validate_venue_report(
                        *round_id,
                        observation,
                        country,
                        watchtower,
                        signature,
                    )
                    .is_err()
                    {
                        return InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID).into()
                    }
                    ValidTransaction::with_tag_prefix("WatchtowerVenueOracleReport")
                        .and_provides((round_id, watchtower.clone()))
                        .priority(TransactionPriority::max_value() - reduce_priority)
                        .longevity(64_u64)
                        .propagate(true)
                        .build()
                },
                _ => InvalidTransaction::Call.into(),
            }
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn offchain_worker(now: BlockNumberFor<T>) {
            // Checkpoint votes first: a vote in this block can unblock a channel next block.
            Self::checkpoint_offchain_worker(now);
            Self::floor_offchain_worker(now);
            Self::venue_offchain_worker(now);
        }
    }

    impl<T: Config> Pallet<T> {
        /// Root, or a signed origin equal to the stored privileged requester.
        fn ensure_oracle_requester(origin: OriginFor<T>) -> DispatchResult {
            if ensure_root(origin.clone()).is_ok() {
                return Ok(())
            }
            let who = ensure_signed(origin)?;
            ensure!(
                OracleRequester::<T>::get() == Some(who),
                Error::<T>::UnauthorizedOracleRequest
            );
            Ok(())
        }

        /// Bytes an author signs for a checkpoint vote. Shared by validation, tests and the
        /// offchain worker so they cannot drift apart.
        pub fn vote_signing_payload(
            chain_id: ExternalChainId,
            round_id: RoundId,
            account_id: &T::AccountId,
            block_number: u32,
        ) -> Vec<u8> {
            (SUBMIT_CHECKPOINT_VOTE_CONTEXT, chain_id, round_id, account_id, block_number).encode()
        }

        /// Verify a vote signature against the key registered for the author, not the key sent
        /// in the call.
        pub fn vote_signature_is_valid(
            payload: &[u8],
            author: &Author<T>,
            signature: &<T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> bool {
            match AVN::<T>::try_get_validator(&author.account_id) {
                Some(validator) => validator.key.verify(&payload, signature),
                None => false,
            }
        }

        /// A vote may reconfirm the current checkpoint height but never go below it. Together
        /// with the selection rule (smallest value with enough votes) this guarantees that an
        /// agreed checkpoint never moves backwards.
        pub fn is_at_or_above_checkpoint(chain_id: ExternalChainId, block_number: u32) -> bool {
            match Checkpoints::<T>::get(chain_id) {
                Some(checkpoint) => block_number >= checkpoint.block_number,
                None => true,
            }
        }

        /// Rounds that are currently collecting votes.
        pub fn active_rounds() -> Vec<(ExternalChainId, RoundId)> {
            CheckpointRound::<T>::iter()
                .map(|(chain_id, round)| (chain_id, round.round_id))
                .collect()
        }

        /// Whether `account_id` already voted in the running round `round_id` of `chain_id`.
        pub fn author_has_voted(
            chain_id: ExternalChainId,
            round_id: RoundId,
            account_id: &T::AccountId,
        ) -> bool {
            match CheckpointRound::<T>::get(chain_id) {
                Some(round) if round.round_id == round_id => round.votes.contains_key(account_id),
                _ => false,
            }
        }

        pub fn do_request_checkpoint(
            chain_id: ExternalChainId,
        ) -> Result<CheckpointRequest<BlockNumberFor<T>>, DispatchError> {
            let max_age =
                MaxCheckpointAge::<T>::get(chain_id).ok_or(Error::<T>::ChainNotRegistered)?;
            let now_block = <frame_system::Pallet<T>>::block_number();
            let stale = Checkpoints::<T>::get(chain_id);

            if let Some(round) = CheckpointRound::<T>::get(chain_id) {
                if now_block.saturating_sub(round.started_at) <= T::RoundTimeoutBlocks::get() {
                    // Requests arriving while a round runs share its result.
                    return Ok(CheckpointRequest::Pending {
                        round_id: round.round_id,
                        started_at: round.started_at,
                        stale,
                    })
                }

                let new_round = Self::start_round(chain_id, now_block);
                Self::deposit_event(Event::<T>::CheckpointRoundRestarted {
                    chain_id,
                    old_round_id: round.round_id,
                    new_round_id: new_round.round_id,
                });
                return Ok(CheckpointRequest::Pending {
                    round_id: new_round.round_id,
                    started_at: new_round.started_at,
                    stale,
                })
            }

            if let Some(checkpoint) = &stale {
                if Self::now_secs().saturating_sub(checkpoint.agreed_at) <= max_age {
                    return Ok(CheckpointRequest::Ready(checkpoint.clone()))
                }
            }

            let round = Self::start_round(chain_id, now_block);
            Self::deposit_event(Event::<T>::CheckpointRoundStarted {
                chain_id,
                round_id: round.round_id,
            });
            Ok(CheckpointRequest::Pending {
                round_id: round.round_id,
                started_at: round.started_at,
                stale,
            })
        }

        fn start_round(
            chain_id: ExternalChainId,
            now_block: BlockNumberFor<T>,
        ) -> CheckpointRoundState<T::AccountId, BlockNumberFor<T>> {
            let round_id = LastCheckpointRoundId::<T>::mutate(|id| {
                *id = id.saturating_add(1);
                *id
            });
            let round =
                CheckpointRoundState { round_id, started_at: now_block, votes: Default::default() };
            CheckpointRound::<T>::insert(chain_id, round.clone());
            round
        }

        pub(crate) fn now_secs() -> u64 {
            T::TimeProvider::now().as_secs()
        }

        /// Fan-out to every channel waiting for a checkpoint on `chain_id`. Each channel checks
        /// its own chain and pending flag, so both may start a round on one agreement.
        pub(crate) fn on_checkpoint_agreed(chain_id: ExternalChainId, checkpoint: &Checkpoint) {
            Self::on_checkpoint_agreed_floor(chain_id, checkpoint);
            Self::on_checkpoint_agreed_venue(chain_id, checkpoint);
        }
    }

    impl<T: Config> VoteFilter<T::AccountId> for Pallet<T> {
        /// Votes on an oracle proposal must be the hash the voter reported on that channel;
        /// yes/no votes are refused. Other proposals are not filtered.
        fn ensure_can_vote(
            proposal_id: ProposalId,
            voter: &T::AccountId,
            vote: &sp_watchtower::VoteChoice,
        ) -> DispatchResult {
            if let Some(result) = Self::floor_vote_filter(proposal_id, voter, vote) {
                return result
            }
            if let Some(result) = Self::venue_vote_filter(proposal_id, voter, vote) {
                return result
            }
            Ok(())
        }
    }

    impl<T: Config> WatchtowerHooks<pallet_watchtower::Proposal<T>> for Pallet<T> {
        fn on_proposal_submitted(
            _proposal_id: ProposalId,
            _proposal: pallet_watchtower::Proposal<T>,
        ) -> DispatchResult {
            // Oracle proposals are created by this pallet; nothing to prepare on activation.
            Ok(())
        }

        /// Matched by proposal id, not by the active slot: the watchtower also cancels queued
        /// proposals that were never active, and the round must still be closed then.
        fn on_voting_completed(
            proposal_id: ProposalId,
            _external_ref: &H256,
            result: &ProposalStatusEnum,
        ) {
            if !Self::finish_floor_if_matches(proposal_id, result) {
                Self::finish_venue_if_matches(proposal_id, result);
            }
        }

        fn on_cancelled(proposal_id: ProposalId, _external_ref: &H256) {
            let result = ProposalStatusEnum::Cancelled;
            if !Self::finish_floor_if_matches(proposal_id, &result) {
                Self::finish_venue_if_matches(proposal_id, &result);
            }
        }
    }

    impl<T: Config> CheckpointOracle<BlockNumberFor<T>> for Pallet<T> {
        fn request_checkpoint(
            chain_id: ExternalChainId,
        ) -> Result<CheckpointRequest<BlockNumberFor<T>>, DispatchError> {
            Self::do_request_checkpoint(chain_id)
        }

        fn checkpoint(chain_id: ExternalChainId) -> Option<Checkpoint> {
            Checkpoints::<T>::get(chain_id)
        }
    }
}
