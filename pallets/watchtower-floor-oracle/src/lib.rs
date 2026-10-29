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
//!   supermajority / quorum selection that pallet-eth-bridge uses for its latest block, so votes
//!   need not match: once two thirds are in, the round settles on the lowest value about a third of
//!   the voters reached. The external service votes the head minus a confirmation depth, so a round
//!   settles a few parachain blocks after it starts. A chain no collator can reach gets no votes:
//!   its rounds time out and restart forever, visible as repeated `CheckpointRoundRestarted`.
//! - A checkpoint is written exactly once per round and never changed. A later round writes a new
//!   checkpoint under a new round id.
//! - Votes are bounded above when root has set the chain's fastest block time: a vote may run ahead
//!   of the checkpoint only by the blocks the chain can have produced since it was agreed, plus
//!   `CheckpointVoteSlackSecs` worth (see [`Pallet::max_checkpoint_vote`]). Bad votes can then push
//!   a checkpoint at most that far past the real chain, which catches up on its own. Collators cap
//!   their own vote at the bound, so a checkpoint left behind the chain still catches up, by at
//!   most the bound per round.
//!
//! There is no background polling, no block rounding and no height advancement. Freshness is
//! measured as time since agreement, not time since the external chain last advanced.
//!
//! # Oracle rounds
//!
//! On top of the checkpoints, the pallet runs two independent request channels with the same
//! shape: a request pins a point on the channel's chain, opens a watchtower `Choice` proposal,
//! and watchtower nodes report what they observed at that pinned point. The report is the vote;
//! the first observation reaching the proposal's threshold wins.
//!
//! - **Floor** ([`floor`]): the Punks floor on Ethereum, stored in `LastFinalisedFloor`. Pinned to
//!   the chain's checkpoint.
//! - **Venue** ([`venue`]): the raw state of the Exotic perpetuals market for Punks on the venue
//!   chain (Base), stored in `LastFinalisedVenue`. Pinned to a target timestamp set when the
//!   round's proposal activates, not to a checkpoint, so the state is as fresh as possible.
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

use codec::{Decode, Encode, FullCodec, MaxEncodedLen};
use frame_support::{
    dispatch::{DispatchResult, DispatchResultWithPostInfo},
    pallet_prelude::*,
    storage::IterableStorageDoubleMap,
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
    FinalisedFloorRound, FinalisedVenueRound, FloorMarket, FloorObservation, VenueMarket,
    VenueObservation, FLOOR_ORACLE_REPORT_CONTEXT, FLOOR_SERVICE_PATH, VENUE_ORACLE_REPORT_CONTEXT,
    VENUE_SERVICE_PATH,
};
use sp_runtime::{
    traits::{Dispatchable, Saturating, Zero},
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
        ValidTransaction,
    },
    DispatchError, Perbill, RuntimeAppPublic, SaturatedConversion,
};
use sp_std::prelude::*;
use sp_watchtower::sanitise_country;
pub use sp_watchtower::{
    Checkpoint, CheckpointOracle, CheckpointRequest, CountryCode, ExternalChainId, ProposalId,
    ProposalStatusEnum, RoundId, VoteFilter, WatchtowerHooks, NODE_COUNTRY_SERVICE_PATH,
    UNKNOWN_COUNTRY,
};

pub mod default_weights;
pub use default_weights::WeightInfo;
pub mod checkpoint;
pub mod floor;
pub use floor::{FloorRound, FloorRoundKind};
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
pub const VOTE_TOO_FAR_AHEAD: u8 = 5;
pub const FLOOR_REPORT_NOT_VALID: u8 = 10;
pub const VENUE_REPORT_NOT_VALID: u8 = 11;

/// Most reports, and most observations, `on_idle` deletes for a finished round in one block.
pub const ROUND_CLEANUP_PAGE_SIZE: u32 = 250;

/// The two oracle channels, as the key of a finished round awaiting cleanup.
#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum OracleChannel {
    Floor,
    Venue,
}

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

        /// A finalised floor whose Ethereum block is younger than this, in seconds, answers a
        /// request directly.
        #[pallet::constant]
        type MaxFloorAge: Get<u64>;

        /// Oldest checkpoint, in seconds since it was agreed, a floor round pins. An older one
        /// starts a new checkpoint round, even if the chain's own freshness limit still accepts
        /// it. A floor is older than its checkpoint by the confirmation depth (about two minutes
        /// on Ethereum), so this must stay well below `MaxFloorAge`, or a new floor could
        /// already be stale when it finalises.
        #[pallet::constant]
        type MaxFloorCheckpointAge: Get<u64>;

        /// For this many seconds after a forced round ends, finalised or failed,
        /// `force_floor_round` behaves as `request_floor_price`. Bounds how often the requester
        /// can open forced rounds. A forced round also pins a checkpoint no older than this.
        #[pallet::constant]
        type MinForcedFloorAge: Get<u64>;

        /// A finalised venue observation whose external block timestamp is younger than this, in
        /// seconds, answers a request directly.
        #[pallet::constant]
        type MaxVenueAge: Get<u64>;

        /// How far behind the parachain time a venue round reads, in seconds. The target
        /// timestamp is set when the round's proposal activates, this far back, so that by the
        /// time watchtowers read it the target block is a few venue chain blocks deep.
        #[pallet::constant]
        type VenueTargetLag: Get<u64>;

        /// Source of the parachain timestamp stored in a checkpoint.
        type TimeProvider: UnixTime;

        /// Quorum numbers used to settle a round.
        type Quorum: QuorumPolicy;

        /// A round older than this, in parachain blocks, is restarted by the next request.
        #[pallet::constant]
        type RoundTimeoutBlocks: Get<BlockNumberFor<Self>>;

        /// How far, in seconds of the chain's fastest block production, a checkpoint vote may
        /// run ahead of the time elapsed since the checkpoint was agreed. Covers honest drift;
        /// it is also about how long the chain needs to overtake a checkpoint bad votes pushed
        /// to the limit. Only applies to chains with a `MinExternalBlockTimeMs`.
        #[pallet::constant]
        type CheckpointVoteSlackSecs: Get<u64>;

        type WeightInfo: WeightInfo;
    }

    /// Freshness limit per chain, in seconds. A chain is registered when it has an entry here.
    #[pallet::storage]
    #[pallet::getter(fn max_checkpoint_age)]
    pub type MaxCheckpointAge<T: Config> =
        StorageMap<_, Blake2_128Concat, ExternalChainId, u64, OptionQuery>;

    /// Shortest time between two blocks of a chain, in milliseconds. Bounds how far a checkpoint
    /// vote may run ahead (see [`Pallet::max_checkpoint_vote`]). No entry: votes are not bounded
    /// above, as on a local dev chain that mines on demand.
    #[pallet::storage]
    #[pallet::getter(fn min_external_block_time_ms)]
    pub type MinExternalBlockTimeMs<T: Config> =
        StorageMap<_, Blake2_128Concat, ExternalChainId, u32, OptionQuery>;

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

    /// A floor request is waiting for the checkpoint round on the market's chain to settle, and
    /// the kind of round it starts then.
    #[pallet::storage]
    #[pallet::getter(fn pending_floor_request)]
    pub type PendingFloorRequest<T: Config> = StorageValue<_, FloorRoundKind, OptionQuery>;

    /// Parachain time, in seconds, the last forced floor round ended, finalised or failed.
    #[pallet::storage]
    #[pallet::getter(fn last_forced_round_ended_at)]
    pub type LastForcedRoundEndedAt<T: Config> = StorageValue<_, u64, OptionQuery>;

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

    /// The most recent venue state the watchtowers agreed on.
    #[pallet::storage]
    #[pallet::getter(fn last_finalised_venue)]
    pub type LastFinalisedVenue<T: Config> = StorageValue<_, FinalisedVenueRound, OptionQuery>;

    /// Rounds that have ended but still have reports or observations stored. `on_idle` deletes
    /// them a page at a time, so the report or block that ends a round does not pay for it.
    #[pallet::storage]
    pub type RoundsToClean<T: Config> =
        StorageMap<_, Twox64Concat, (OracleChannel, u64), (), OptionQuery>;

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
            checkpoint_agreed_at_sec: u64,
            external_block: u32,
            market: FloorMarket,
        },
        /// The round that just started is forced: `force_floor_round` asked for it, directly or
        /// through a request that waited for a checkpoint. Follows its `FloorRoundStarted`.
        FloorRoundForced { round_id: u64 },
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
        /// The watchtowers agreed on a floor. Voting can outlast `MaxFloorAge`, so the floor may
        /// already be stale: its age runs from `external_block_time_sec`, as the cache's does.
        FloorRoundFinalised {
            round_id: u64,
            external_block_time_sec: u64,
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
        /// A request was answered from a venue state whose external block timestamp is within
        /// `MaxVenueAge`.
        VenueStateServed { round_id: u64, external_block: u64 },
        /// A request joined the venue round already running.
        VenueRoundInProgress { round_id: u64 },
        /// A venue round opened as a watchtower proposal. It reads nothing until the proposal
        /// activates.
        VenueRoundStarted { round_id: u64, proposal_id: ProposalId, market: VenueMarket },
        /// The venue round's proposal activated; watchtowers read the venue chain at the latest
        /// block whose timestamp is at most `target_external_block_time_sec`.
        VenueRoundActivated { round_id: u64, target_external_block_time_sec: u64 },
        /// A watchtower reported its venue observation; `hash` is its answer on the proposal
        /// and `country` the self-reported location of the node, for display.
        VenueReportSubmitted {
            round_id: u64,
            watchtower: T::AccountId,
            hash: H256,
            country: CountryCode,
        },
        /// The watchtowers agreed on the venue state. Raw contract values, 18 decimals. Voting
        /// can outlast `MaxVenueAge`, so the state may already be stale: its age runs from
        /// `external_block_time_sec`, as the cache's does.
        VenueRoundFinalised {
            round_id: u64,
            external_block: u64,
            external_block_time_sec: u64,
            spot_e18: u128,
            oracle_e18: u128,
            oi_notional_e18: u128,
            agreeing: u32,
            reports: u32,
            required: u32,
        },
        /// The venue round ended without agreement.
        VenueRoundFailed { round_id: u64, result: ProposalStatusEnum },
        /// Root replaced a chain's checkpoint and dropped its running round.
        CheckpointForced { chain_id: ExternalChainId, block_number: u32, agreed_at: u64 },
        /// The fastest block time of a chain was set, or cleared (`None`: votes unbounded).
        ExternalBlockTimeSet { chain_id: ExternalChainId, min_external_block_time_ms: Option<u32> },
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
        /// The voted height is below the current checkpoint. Votes never move a checkpoint
        /// backwards; only root can, with `force_set_checkpoint`.
        BlockBehindCheckpoint,
        /// The round cannot hold any more votes.
        TooManyVotes,
        /// No floor round is collecting reports.
        NoActiveFloorRound,
        /// The report names a round that is not the running one.
        FloorRoundMismatch,
        /// The round's watchtower proposal is not active yet (no committee), or its voting
        /// period has ended and it awaits finalisation.
        FloorProposalNotActive,
        /// The reporter is not an authorized watchtower in the round's committee.
        NotInFloorCommittee,
        /// The watchtower already reported in this round.
        DuplicateFloorReport,
        /// The observation fails the plausibility checks.
        ImplausibleObservation,
        /// The observation was not taken at the round's external block.
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
        /// The venue round's watchtower proposal is not active yet (no committee), or its voting
        /// period has ended and it awaits finalisation.
        VenueProposalNotActive,
        /// The reporter is not an authorized watchtower in the venue round's committee.
        NotInVenueCommittee,
        /// The watchtower already reported in this venue round.
        DuplicateVenueReport,
        /// The venue observation fails the plausibility checks.
        ImplausibleVenueObservation,
        /// The venue observation was not taken at the round's target timestamp.
        VenueObservationOffTarget,
        /// A venue market's target tolerance must be above zero, and with `VenueTargetLag` at
        /// most `MaxVenueAge`.
        InvalidVenueTargetTolerance,
        /// The voter has not reported a venue observation for this round.
        NoVenueReport,
        /// A vote on a venue proposal that is not the hash the voter reported.
        VenueVoteContradictsReport,
        /// Venue proposals are decided by reports; yes/no votes are not accepted.
        VenueVotesAreReports,
        /// The venue report signature does not verify against the watchtower's signing key.
        VenueReportBadSignature,
        /// The voted height is further ahead of the checkpoint than the chain can have moved
        /// since it was agreed (see `max_checkpoint_vote`).
        BlockTooFarAhead,
        /// A block time must be greater than zero.
        InvalidBlockTime,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Register a chain and, if it has no checkpoint yet, seed its initial block number.
        ///
        /// `min_external_block_time_ms` is the chain's fastest block time, which bounds how far
        /// checkpoint votes may run ahead; `None` leaves votes unbounded. Seed with a recent
        /// block: with a block time set, a checkpoint behind the chain catches up only by the
        /// bound per round.
        ///
        /// Calling it again on a seeded chain only updates the freshness limit and block time.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::seed_chain())]
        pub fn seed_chain(
            origin: OriginFor<T>,
            chain_id: ExternalChainId,
            initial_block_number: u32,
            max_checkpoint_age_secs: u64,
            min_external_block_time_ms: Option<u32>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(max_checkpoint_age_secs > 0, Error::<T>::InvalidFreshnessLimit);
            Self::do_set_external_block_time(chain_id, min_external_block_time_ms)?;

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

        /// Replace the checkpoint of a seeded chain, in either direction. The recovery path for
        /// a wrong checkpoint: votes below it are refused, so one agreed too high (or left above
        /// a chain that was reset) would stop the chain's checkpoints for good. Drops the
        /// running round, whose votes were cast against the old value, and hands the new
        /// checkpoint to whatever waits on one, as an agreed checkpoint would be.
        ///
        /// Use a recent finalised block: forced floor rounds only pin young checkpoints, and with
        /// a block time set, a checkpoint behind the chain catches up only by the vote bound per
        /// round.
        #[pallet::call_index(12)]
        #[pallet::weight(<T as Config>::WeightInfo::force_set_checkpoint())]
        pub fn force_set_checkpoint(
            origin: OriginFor<T>,
            chain_id: ExternalChainId,
            block_number: u32,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(Checkpoints::<T>::contains_key(chain_id), Error::<T>::ChainNotRegistered);

            CheckpointRound::<T>::remove(chain_id);
            let agreed_at = Self::now_secs();
            let checkpoint = Checkpoint { block_number, agreed_at, round_id: 0 };
            Checkpoints::<T>::insert(chain_id, &checkpoint);
            Self::deposit_event(Event::<T>::CheckpointForced { chain_id, block_number, agreed_at });
            Self::on_checkpoint_agreed(chain_id, &checkpoint);
            Ok(())
        }

        /// Set or clear the fastest block time of a registered chain, in milliseconds. It bounds
        /// how far checkpoint votes may run ahead (see [`Pallet::max_checkpoint_vote`]); `None`
        /// removes the bound. Takes effect for votes from the next block, on the running round
        /// too.
        #[pallet::call_index(13)]
        #[pallet::weight(<T as Config>::WeightInfo::set_external_block_time())]
        pub fn set_external_block_time(
            origin: OriginFor<T>,
            chain_id: ExternalChainId,
            min_external_block_time_ms: Option<u32>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(MaxCheckpointAge::<T>::contains_key(chain_id), Error::<T>::ChainNotRegistered);
            Self::do_set_external_block_time(chain_id, min_external_block_time_ms)
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

        /// Ask for a new Punks floor, whatever the cache holds. For a requester that suspects
        /// the cached floor is stale. Root or the privileged requester only. Joins the running
        /// round if there is one. Within `MinForcedFloorAge` of the last forced round ending it
        /// behaves as `request_floor_price`.
        #[pallet::call_index(11)]
        #[pallet::weight(<T as Config>::WeightInfo::force_floor_round())]
        pub fn force_floor_round(origin: OriginFor<T>) -> DispatchResult {
            Self::ensure_oracle_requester(origin)?;
            Self::do_force_floor_round()
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
        /// round, or opens a new one. The outcome is reported as an event.
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
                Some(market) => {
                    // A sanity bound only. Root picks the tolerance: just above the venue
                    // chain's block time (6 for Base). An observation can be `VenueTargetLag +
                    // tolerance` older than activation, and voting adds a block or two on top,
                    // so a tolerance near this bound yields state that is already past
                    // `MaxVenueAge` when it finalises. The bound only rules out settings that
                    // could never produce fresh state.
                    ensure!(
                        market.target_tolerance_secs > 0 &&
                            market
                                .target_tolerance_secs
                                .saturating_add(T::VenueTargetLag::get()) <=
                                T::MaxVenueAge::get(),
                        Error::<T>::InvalidVenueTargetTolerance
                    );
                    VenueMarketConfig::<T>::put(market)
                },
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
        /// by the number of authorized watchtowers, and refused while the watchtower cannot
        /// build such a committee (index backfilling, more seats than node owners). Rounds
        /// already started are unaffected.
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
                    // Refuse a size the watchtower cannot serve (node index still backfilling,
                    // more seats than node owners): the next oracle proposal would sit at the
                    // head of the internal queue, holding back every summary and anchor
                    // proposal behind it, or be cancelled. Surfaces the watchtower's own error,
                    // as the summary pallet does.
                    <pallet_watchtower::Pallet<T> as sp_watchtower::WatchtowerInterface>::ensure_committee_ready(size)?;
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
            ensure!(
                Self::is_within_vote_bound(chain_id, block_number),
                Error::<T>::BlockTooFarAhead
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
                    if !Self::is_within_vote_bound(*chain_id, *block_number) {
                        return InvalidTransaction::Custom(VOTE_TOO_FAR_AHEAD).into()
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
        fn integrity_test() {
            assert!(
                T::MaxFloorCheckpointAge::get() < T::MaxFloorAge::get(),
                "MaxFloorCheckpointAge must be below MaxFloorAge, or new floors start out stale"
            );
            assert!(
                T::MinForcedFloorAge::get() > 0,
                "MinForcedFloorAge must be above zero, or forced rounds can run back to back"
            );
            assert!(
                T::VenueTargetLag::get() < T::MaxVenueAge::get(),
                "VenueTargetLag must be below MaxVenueAge, or no venue state is ever fresh"
            );
        }

        fn on_idle(_now: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            Self::cleanup_finished_rounds(remaining_weight)
        }

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
            (
                SUBMIT_CHECKPOINT_VOTE_CONTEXT,
                Self::genesis_hash(),
                chain_id,
                round_id,
                account_id,
                block_number,
            )
                .encode()
        }

        /// Ties a signed payload to this chain. Unsigned calls get no `CheckGenesis`, and round
        /// ids restart at 1 on every deployment, so without it a signature made on one chain
        /// could verify on another where the same key is registered. frame_system never prunes
        /// block 0's hash, so it is in every node's current state.
        pub fn genesis_hash() -> T::Hash {
            frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero())
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

        /// Highest block a checkpoint vote on `chain_id` may name now: the checkpoint plus the
        /// blocks the chain can have produced, at its fastest block time, since the checkpoint
        /// was agreed and in `CheckpointVoteSlackSecs` more. `None`, for no bound, when the chain
        /// has no block time set or no checkpoint.
        ///
        /// Honest votes stay under it: they trail the chain's head, and the checkpoint trailed
        /// it too when agreed. Bad votes cannot carry a checkpoint more than about the slack
        /// past the real chain, so a checkpoint agreed too high is overtaken within about the
        /// slack instead of stopping the chain until root steps in.
        pub fn max_checkpoint_vote(chain_id: ExternalChainId) -> Option<u32> {
            let block_time_ms = MinExternalBlockTimeMs::<T>::get(chain_id)?;
            let checkpoint = Checkpoints::<T>::get(chain_id)?;
            let secs = Self::now_secs()
                .saturating_sub(checkpoint.agreed_at)
                .saturating_add(T::CheckpointVoteSlackSecs::get());
            let blocks = secs.saturating_mul(1_000) / u64::from(block_time_ms.max(1));
            Some(checkpoint.block_number.saturating_add(blocks.saturated_into::<u32>()))
        }

        pub fn is_within_vote_bound(chain_id: ExternalChainId, block_number: u32) -> bool {
            Self::max_checkpoint_vote(chain_id).map_or(true, |max| block_number <= max)
        }

        fn do_set_external_block_time(
            chain_id: ExternalChainId,
            min_external_block_time_ms: Option<u32>,
        ) -> DispatchResult {
            ensure!(min_external_block_time_ms != Some(0), Error::<T>::InvalidBlockTime);
            MinExternalBlockTimeMs::<T>::set(chain_id, min_external_block_time_ms);
            Self::deposit_event(Event::<T>::ExternalBlockTimeSet {
                chain_id,
                min_external_block_time_ms,
            });
            Ok(())
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
            Self::do_request_checkpoint_within(chain_id, None, None)
        }

        /// As [`Pallet::do_request_checkpoint`], but a stored checkpoint only counts as ready
        /// when it is also no older than `max_age` and above block `above_block`, if given.
        /// Lets a channel ask for a fresher checkpoint than the chain's own limit, or for one
        /// past a block it already read.
        pub(crate) fn do_request_checkpoint_within(
            chain_id: ExternalChainId,
            max_age: Option<u64>,
            above_block: Option<u32>,
        ) -> Result<CheckpointRequest<BlockNumberFor<T>>, DispatchError> {
            let chain_max_age =
                MaxCheckpointAge::<T>::get(chain_id).ok_or(Error::<T>::ChainNotRegistered)?;
            let max_age = max_age.map_or(chain_max_age, |max_age| max_age.min(chain_max_age));
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
                if Self::now_secs().saturating_sub(checkpoint.agreed_at) <= max_age &&
                    above_block.map_or(true, |above| checkpoint.block_number > above)
                {
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

        /// The oracle proposal is the active one and still within its voting period. A report
        /// after `end_at` would be stored and announced, but the watchtower would only finalise
        /// the proposal and never count it; `finalise_proposal` and `on_idle` close it instead.
        pub(crate) fn oracle_proposal_open(proposal_id: ProposalId) -> bool {
            if pallet_watchtower::ActiveInternalProposal::<T>::get() != Some(proposal_id) {
                return false
            }
            let now = <frame_system::Pallet<T>>::block_number();
            pallet_watchtower::Proposals::<T>::get(proposal_id).map_or(false, |proposal| {
                !pallet_watchtower::Pallet::<T>::proposal_expired(now, &proposal)
            })
        }

        pub(crate) fn now_secs() -> u64 {
            T::TimeProvider::now().as_secs()
        }

        /// Deletes one page of a finished round's rows, sized to what `remaining_weight` can
        /// pay for. Generated weights are linear in the page size, so `cleanup_round_page(n)`
        /// for any `n` up to the computed page fits.
        pub(crate) fn cleanup_finished_rounds(remaining_weight: Weight) -> Weight {
            let base = <T as Config>::WeightInfo::cleanup_round_page(0);
            let per_row = <T as Config>::WeightInfo::cleanup_round_page(1).saturating_sub(base);
            let Some(spare) = remaining_weight.checked_sub(&base) else { return Weight::zero() };
            let page = spare
                .checked_div_per_component(&per_row)
                .unwrap_or(ROUND_CLEANUP_PAGE_SIZE as u64)
                .min(ROUND_CLEANUP_PAGE_SIZE as u64) as u32;
            if page == 0 {
                return Weight::zero()
            }

            let Some((channel, round_id)) = Self::next_round_to_clean() else {
                return T::DbWeight::get().reads(1)
            };
            let removed = Self::cleanup_round_page(channel, round_id, page);
            <T as Config>::WeightInfo::cleanup_round_page(removed)
        }

        /// Some finished round still holding rows.
        pub(crate) fn next_round_to_clean() -> Option<(OracleChannel, u64)> {
            RoundsToClean::<T>::iter_keys().next()
        }

        /// Removes up to `max` reports and up to `max` observations of a finished round, and
        /// forgets the round once both are gone. Returns the larger of the two counts, which is
        /// what the weight is charged for.
        ///
        /// Not `clear_prefix`: it still goes through the old `kill_prefix` host function, which
        /// counts only rows already in the database, so rows written this block go uncharged.
        pub(crate) fn cleanup_round_page(channel: OracleChannel, round_id: u64, max: u32) -> u32 {
            let ((reports, reports_left), (observations, observations_left)) = match channel {
                OracleChannel::Floor => (
                    Self::remove_round_rows::<FloorReports<T>, _, _>(round_id, max),
                    Self::remove_round_rows::<FloorObservations<T>, _, _>(round_id, max),
                ),
                OracleChannel::Venue => (
                    Self::remove_round_rows::<VenueReports<T>, _, _>(round_id, max),
                    Self::remove_round_rows::<VenueObservations<T>, _, _>(round_id, max),
                ),
            };
            if !reports_left && !observations_left {
                RoundsToClean::<T>::remove((channel, round_id));
            }
            reports.max(observations)
        }

        /// Removes up to `max` rows of `round_id` from `Map`, returning how many it removed and
        /// whether any are left. Keys are collected first so nothing is deleted while iterating;
        /// one key past the page tells whether the round is done.
        fn remove_round_rows<Map, K2, V>(round_id: u64, max: u32) -> (u32, bool)
        where
            Map: IterableStorageDoubleMap<u64, K2, V>,
            K2: FullCodec,
            V: FullCodec,
        {
            let mut keys: Vec<K2> = Map::iter_key_prefix(round_id).take(max as usize + 1).collect();
            let left = keys.len() > max as usize;
            keys.truncate(max as usize);
            for key in &keys {
                Map::remove(round_id, key);
            }
            (keys.len() as u32, left)
        }

        /// Hands an agreed checkpoint to the channels pinned to checkpoints (only the floor;
        /// the venue channel pins a timestamp instead).
        pub(crate) fn on_checkpoint_agreed(chain_id: ExternalChainId, checkpoint: &Checkpoint) {
            Self::on_checkpoint_agreed_floor(chain_id, checkpoint);
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
        /// Called when an internal proposal activates. A venue round fixes its target
        /// timestamp here rather than when it was requested, so time spent in the queue does
        /// not age the state it reads.
        fn on_proposal_submitted(
            proposal_id: ProposalId,
            _proposal: pallet_watchtower::Proposal<T>,
        ) -> DispatchResult {
            Self::on_venue_proposal_activated(proposal_id);
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
