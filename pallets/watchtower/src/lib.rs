#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
    vec,
};

use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
    dispatch::DispatchResult, pallet_prelude::*, traits::IsSubType, weights::WeightMeter,
};
use frame_system::{
    offchain::{CreateBare, CreateTransactionBase, SubmitTransaction},
    pallet_prelude::*,
};
use sp_avn_common::ocw_lock::{self as OcwLock};
pub use sp_avn_common::{verify_signature, InnerCallValidator, Proof};
use sp_core::{MaxEncodedLen, H256};
pub use sp_runtime::{
    traits::{AtLeast32Bit, Dispatchable, ValidateUnsigned},
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
        ValidTransaction,
    },
    Perbill, SaturatedConversion,
};
use sp_runtime::{
    traits::{IdentifyAccount, Verify},
    RuntimeAppPublic, Saturating,
};
use sp_std::prelude::*;
pub use sp_watchtower::*;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);
pub const DEFAULT_VOTING_PERIOD_BLOCKS: u32 = 100;
pub const WATCHTOWER_UNSIGNED_VOTE_CONTEXT: &'static [u8] = b"wt_unsigned_vote";
pub const WATCHTOWER_FINALISE_PROPOSAL_CONTEXT: &'static [u8] = b"wt_finalise_proposal";
pub const WATCHTOWER_ACTIVATE_PROPOSAL_CONTEXT: &'static [u8] = b"wt_activate_proposal";
pub const UNSIGNED_VOTE_NOT_VALID: u8 = 2;
pub const ACTIVATE_PROPOSAL_NOT_VALID: u8 = 3;
/// The queue head requests a committee that cannot be sampled yet (index backfilling).
/// Activation is deferred, not degraded. A head with too few seats is not deferred: activation
/// cancels it.
pub const COMMITTEE_NOT_READY: u8 = 4;
/// Offchain worker lock id used to make sure activation is attempted at most once per block.
pub const ACTIVATION_OCW_ID: &'static [u8] = b"watchtower_activation";

pub mod proxy;
pub mod types;
pub mod vote;
pub use types::*;
pub mod queue;
pub use queue::*;
pub mod committee;
pub use committee::*;
pub mod migration;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub mod default_weights;
pub use default_weights::WeightInfo;

#[cfg(test)]
#[path = "tests/activation.rs"]
mod activation;
#[cfg(test)]
#[path = "tests/add_proposal.rs"]
mod add_proposal;
#[cfg(test)]
#[path = "tests/admin.rs"]
mod admin;
#[cfg(test)]
#[path = "tests/choice_voting.rs"]
mod choice_voting;
#[cfg(test)]
#[path = "tests/committee.rs"]
mod committee_tests;
#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/voting.rs"]
mod voting;

pub use pallet::*;

/// Most `Voters` (and, separately, `ProposalCommittee`) entries of one proposal that `on_idle`
/// removes per block.
pub const CLEANUP_PAGE_SIZE: u32 = 250;
/// Offchain local storage key caching the node's country, with when it was fetched.
const NODE_COUNTRY_CACHE_KEY: &[u8] = b"watchtower::node_country";
/// How long a cached country is used before it is fetched again, in milliseconds.
const NODE_COUNTRY_CACHE_MS: u64 = 60 * 60 * 1000;

/// The node's own country as its external service reports it, for the country carried by its
/// votes. Best effort: any failure yields [`UNKNOWN_COUNTRY`] and never blocks a vote. For the
/// offchain workers of pallets built on the watchtower, which hold the AVN service access.
///
/// A known country is cached in offchain local storage and reused for an hour, so votes do not
/// each call the service. After that it is fetched again; if that fails, the last known country
/// is kept for another hour rather than asked for on every vote. Unknown is never cached: the
/// service already backs off failed lookups, and the next vote picks the country up as soon as it
/// resolves. A node that changes its country (e.g. `AVN_NODE_COUNTRY`) shows the new one within the
/// hour.
pub fn fetch_node_country<T: pallet_avn::Config>() -> CountryCode {
    let now = sp_io::offchain::timestamp().unix_millis();
    let cache = sp_runtime::offchain::storage::StorageValueRef::persistent(NODE_COUNTRY_CACHE_KEY);
    let cached = cache.get::<(CountryCode, u64)>().ok().flatten();
    if let Some((country, fetched_at)) = cached {
        // A clock that went backwards counts as expired rather than fresh.
        if fetched_at <= now && now - fetched_at < NODE_COUNTRY_CACHE_MS {
            return country
        }
    }

    let fetched =
        pallet_avn::Pallet::<T>::get_data_from_service(String::from(NODE_COUNTRY_SERVICE_PATH))
            .ok()
            .and_then(|body| String::from_utf8(body).ok())
            .and_then(|text| {
                sp_avn_common::http_data_codec::decode_from_http_data::<CountryCode>(&text).ok()
            })
            .map(sanitise_country);
    match (fetched, cached) {
        (Some(country), _) if country != UNKNOWN_COUNTRY => {
            cache.set(&(country, now));
            country
        },
        (_, Some((country, _))) => {
            log::debug!("Watchtower OCW: node country refresh failed, keeping the last known");
            cache.set(&(country, now));
            country
        },
        _ => {
            log::debug!("Watchtower OCW: node country unavailable, reporting unknown");
            UNKNOWN_COUNTRY
        },
    }
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config:
        CreateTransactionBase<Call<Self>> + CreateBare<Call<Self>> + frame_system::Config
    {
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + IsSubType<Call<Self>>
            + From<Call<Self>>;

        /// Access control for “external” (non-pallet-originated) proposals.
        type ExternalProposerOrigin: EnsureOrigin<
            Self::RuntimeOrigin,
            Success = Option<Self::AccountId>,
        >;

        /// The SignerId type used in Watchtowers
        type SignerId: Member + Parameter + sp_runtime::RuntimeAppPublic + Ord + MaxEncodedLen;

        /// A type that can be used to verify signatures
        type Public: IdentifyAccount<AccountId = Self::AccountId>;

        /// The signature type used by accounts/transactions.
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;

        /// Interface for accessing registered watchtowers
        type Watchtowers: NodesInterface<Self::AccountId, Self::SignerId>;

        /// Hooks for other pallets to implement custom logic on certain events
        type WatchtowerHooks: WatchtowerHooks<Proposal<Self>>;

        /// Consulted before a vote on an internal proposal is counted. `()` accepts every vote.
        type VoteFilter: VoteFilter<Self::AccountId>;

        /// Weight information for extrinsics in this pallet
        type WeightInfo: WeightInfo;

        /// The lifetime (in blocks) of a signed transaction.
        #[pallet::constant]
        type SignedTxLifetime: Get<u32>;

        /// Maximum proposal title length
        #[pallet::constant]
        type MaxTitleLen: Get<u32>;

        /// Maximum length of inline proposal data
        #[pallet::constant]
        type MaxInlineLen: Get<u32>;

        /// Maximum length of URI for proposals
        #[pallet::constant]
        type MaxUriLen: Get<u32>;

        /// Maximum length of Internal proposals
        #[pallet::constant]
        type MaxInternalProposalLen: Get<u32>;

        /// Smallest committee a proposal may request. Stops a handful of nodes deciding a
        /// proposal.
        #[pallet::constant]
        type MinCommitteeSize: Get<u32>;

        /// Largest committee a proposal may request. Bounds the activation weight.
        #[pallet::constant]
        type MaxCommitteeSize: Get<u32>;

        /// Seed source for committee selection.
        type Randomness: frame_support::traits::Randomness<Self::Hash, BlockNumberFor<Self>>;

        /// Populates the node provider and the consumers so benchmarks measure real storage.
        #[cfg(feature = "runtime-benchmarks")]
        type BenchmarkHelper: BenchmarkHelper<Self::AccountId>;
    }

    #[pallet::type_value]
    pub fn DefaultVotingPeriod<T: Config>() -> BlockNumberFor<T> {
        DEFAULT_VOTING_PERIOD_BLOCKS.into()
    }

    #[pallet::storage]
    pub type MinVotingPeriod<T: Config> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery, DefaultVotingPeriod<T>>;

    #[pallet::storage]
    #[pallet::getter(fn id_by_external_ref)]
    pub type ExternalRef<T: Config> = StorageMap<_, Blake2_128Concat, H256, ProposalId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn proposals)]
    pub type Proposals<T: Config> =
        StorageMap<_, Blake2_128Concat, ProposalId, Proposal<T>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn proposal_status)]
    pub type ProposalStatus<T: Config> =
        StorageMap<_, Blake2_128Concat, ProposalId, ProposalStatusEnum, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn votes)]
    pub type Votes<T: Config> = StorageMap<_, Blake2_128Concat, ProposalId, Vote, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn voters)]
    pub type Voters<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        ProposalId,
        Blake2_128Concat,
        T::AccountId, // Voter
        bool,         // voted in_favor or against
        ValueQuery,
    >;

    /// Nodes selected to vote on a proposal. Only populated for proposals with a committee.
    #[pallet::storage]
    pub type ProposalCommittee<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        ProposalId,
        Blake2_128Concat,
        T::AccountId, // Committee member
        (),
        OptionQuery,
    >;

    /// Effective committee size, fixed at activation. Present iff the proposal has a committee.
    /// It is the threshold denominator and a cheap "has committee" flag.
    #[pallet::storage]
    pub type ProposalCommitteeSize<T: Config> =
        StorageMap<_, Blake2_128Concat, ProposalId, u32, OptionQuery>;

    /// The currently active internal proposal being voted on, if any
    #[pallet::storage]
    pub type ActiveInternalProposal<T: Config> = StorageValue<_, ProposalId, OptionQuery>;

    /// How each proposal is voted on. Absent means `Binary`, so existing proposals need no
    /// migration.
    #[pallet::storage]
    pub type VotingKindOf<T: Config> =
        StorageMap<_, Blake2_128Concat, ProposalId, VotingKind, ValueQuery>;

    /// Votes per answer for `Choice` proposals.
    #[pallet::storage]
    #[pallet::getter(fn choice_tally)]
    pub type ChoiceTally<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, ProposalId, Blake2_128Concat, H256, u32, ValueQuery>;

    /// Running totals per `Choice` proposal, so the decision reads one entry however many
    /// answers were given.
    #[pallet::storage]
    pub type ChoiceStandings<T: Config> =
        StorageMap<_, Blake2_128Concat, ProposalId, ChoiceStanding, ValueQuery>;

    /// The answer a `Choice` proposal settled on. Readable from `on_voting_completed`.
    #[pallet::storage]
    #[pallet::getter(fn winning_choice)]
    pub type WinningChoice<T: Config> =
        StorageMap<_, Blake2_128Concat, ProposalId, H256, OptionQuery>;

    #[pallet::storage] // ring slots: physical index -> item id
    pub type InternalProposalQueue<T: Config> =
        StorageMap<_, Blake2_128Concat, (QueueId, u32), ProposalId, OptionQuery>;

    #[pallet::storage] // next to pop
    pub type Head<T: Config> = StorageValue<_, u64, ValueQuery>;

    #[pallet::storage] // next free slot to push
    pub type Tail<T: Config> = StorageValue<_, u64, ValueQuery>;

    /// Completed or Expired proposals that need to be removed from storage.
    #[pallet::storage]
    pub type ProposalsToRemove<T: Config> =
        StorageMap<_, Blake2_128Concat, ProposalId, (), OptionQuery>;

    /// The account that is able to submit proposals
    #[pallet::storage]
    pub type AdminAccount<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new proposal has been submitted
        ProposalSubmitted {
            proposal_id: ProposalId,
            external_ref: H256,
            status: ProposalStatusEnum,
        },
        /// A vote has been cast on a proposal. `country` is the self-reported location of the
        /// node that voted, for display; `None` for votes by watchtower owners, which are
        /// accounts rather than nodes.
        VoteSubmitted {
            voter: T::AccountId,
            proposal_id: ProposalId,
            in_favor: bool,
            vote_weight: u32,
            country: Option<CountryCode>,
        },
        /// A vote naming an answer was counted on a `Choice` proposal. `country` as in
        /// `VoteSubmitted`.
        ChoiceVoteSubmitted {
            voter: T::AccountId,
            proposal_id: ProposalId,
            choice: H256,
            country: Option<CountryCode>,
        },
        /// Consensus has been reached on a proposal
        VotingEnded {
            proposal_id: ProposalId,
            external_ref: H256,
            consensus_result: ProposalStatusEnum,
            /// The answer a passed `Choice` proposal settled on.
            winning_choice: Option<H256>,
        },
        /// A completed or expired proposal has been cleaned from storage
        ProposalCleaned { proposal_id: ProposalId },
        /// Minimum voting period has been updated
        MinVotingPeriodSet { new_period: BlockNumberFor<T> },
        /// Admin account has been updated
        AdminAccountSet { new_admin: Option<T::AccountId> },
        /// A queued internal proposal has become the active proposal
        ProposalActivated { proposal_id: ProposalId },
        /// The id at the head of the queue had no proposal data and was skipped
        ProposalActivationSkipped { proposal_id: ProposalId },
        /// A committee of `size` nodes was selected to vote on the proposal
        CommitteeSelected { proposal_id: ProposalId, size: u32 },
        /// The committee could not be built (too few seats or a corrupt node index); the proposal
        /// was cancelled
        CommitteeSelectionFailed { proposal_id: ProposalId, error: DispatchError },
        /// The queue head was moved one place back by an admin; `promoted` is the new head
        QueueHeadDemoted { demoted: ProposalId, promoted: ProposalId },
        /// The queue head was cancelled by an admin without being activated
        QueueHeadCancelled { proposal_id: ProposalId },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The title is too large
        InvalidTitle,
        /// The payload is too large for inline storage
        InvalidInlinePayload,
        /// The payload URI is too large
        InvalidUri,
        /// The proposal is not valid
        InvalidProposal,
        /// The proposal source is not valid for the chosen extrinsic
        InvalidProposalSource,
        /// A proposal with the same external_ref already exists
        DuplicateExternalRef,
        /// A proposal with the same id already exists
        DuplicateProposal,
        /// Inner proposal queue is full
        InnerProposalQueueFull,
        /// Inner proposal queue is corrupt
        QueueCorruptState,
        /// Inner proposal queue is empty
        QueueEmpty,
        /// The signature on the call has expired
        SignedTransactionExpired,
        /// The sender of the signed tx is not the same as the signer in the proof
        SenderIsNotSigner,
        /// The proof on the call is not valid
        UnauthorizedSignedTransaction,
        /// The proposal was not found
        ProposalNotFound,
        /// The voter is not an authorized watchtower
        UnauthorizedVoter,
        /// A yes/no vote on a `Choice` proposal, or an answer on a yes/no one.
        WrongVoteKind,
        /// The proposal is not currently active
        ProposalNotActive,
        /// The voter has already voted
        AlreadyVoted,
        /// The signing key of the voter could not be found
        VoterSigningKeyNotFound,
        /// The signature on the unsigned transaction is not valid
        UnauthorizedUnsignedTransaction,
        /// The voting period for the proposal has not yet ended
        ProposalVotingPeriodNotEnded,
        /// The voting period is shorter than the minimum allowed
        VotingPeriodTooShort,
        /// The proposal state doesn't match the active proposal state
        CorruptedState,
        /// This proposal cannot be voted on with an unsigned transaction
        InvalidProposalForUnsignedVote,
        /// Admin account is not set
        AdminAccountNotSet,
        /// There is already an active internal proposal
        ProposalAlreadyActive,
        /// The proposal is not at the head of the internal proposal queue
        ProposalNotNextInQueue,
        /// The voter is not in the committee selected for this proposal
        NotInCommittee,
        /// The requested committee is smaller than `MinCommitteeSize`
        CommitteeSizeTooSmall,
        /// The requested committee is larger than `MaxCommitteeSize`
        CommitteeSizeTooLarge,
        /// The dense node index is still being backfilled; a committee cannot be sampled yet
        NodeIndexNotReady,
        /// The requested committee is larger than the seats the node owners can fill (one per
        /// owner)
        NotEnoughSeatsForCommittee,
        /// The dense node or owner index is inconsistent (hole, duplicate or empty owner)
        NodeIndexCorrupt,
        /// The internal proposal queue needs at least two entries for this operation
        QueueTooShort,
        /// Choice voting is only available for internal proposals
        ChoiceVotingInternalOnly,
        /// Internal proposals are voted on by their committee nodes with `unsigned_vote` or
        /// `unsigned_vote_choice`, not with a signed vote
        SignedVoteOnInternalProposal,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        // We don't want external users to add internal proposals to avoid
        // DOSing the internal proposal queue.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::submit_external_proposal())]
        pub fn submit_external_proposal(
            origin: OriginFor<T>,
            proposal: ProposalRequest,
        ) -> DispatchResult {
            let proposer = T::ExternalProposerOrigin::ensure_origin(origin)?;
            ensure!(
                matches!(proposal.source, ProposalSource::External),
                Error::<T>::InvalidProposalSource
            );

            Self::add_proposal(proposer, proposal)?;
            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::signed_submit_external_proposal())]
        pub fn signed_submit_external_proposal(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            proposal: ProposalRequest,
            block_number: BlockNumberFor<T>,
        ) -> DispatchResult {
            let proposer = T::ExternalProposerOrigin::ensure_origin(origin)?;
            ensure!(
                matches!(proposal.source, ProposalSource::External),
                Error::<T>::InvalidProposalSource
            );
            ensure!(proposer == Some(proof.signer.clone()), Error::<T>::SenderIsNotSigner);
            ensure!(
                block_number.saturating_add(T::SignedTxLifetime::get().into()) >
                    frame_system::Pallet::<T>::block_number(),
                Error::<T>::SignedTransactionExpired
            );

            // Create and verify the signed payload
            let signed_payload = Self::encode_signed_submit_external_proposal_params(
                &proof.relayer,
                &proposal,
                &block_number,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload).is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            Self::add_proposal(proposer, proposal)?;
            Ok(())
        }

        /// A node owner votes yes or no on an external proposal. The vote counts once per node
        /// the owner holds and carries no country. Internal proposals are refused: their
        /// committee nodes vote with `unsigned_vote` or `unsigned_vote_choice`.
        #[pallet::call_index(2)]
        #[pallet::weight(
            <T as Config>::WeightInfo::vote()
            .max(<T as Config>::WeightInfo::vote_end_proposal())
        )]
        pub fn vote(
            origin: OriginFor<T>,
            proposal_id: ProposalId,
            in_favor: bool,
        ) -> DispatchResultWithPostInfo {
            let owner = ensure_signed(origin)?;
            let finalised = Self::process_owner_vote(&owner, proposal_id, in_favor)?;

            if finalised {
                Ok(Some(<T as Config>::WeightInfo::vote_end_proposal()).into())
            } else {
                Ok(Some(<T as Config>::WeightInfo::vote()).into())
            }
        }

        /// `vote` relayed on the owner's behalf: the owner signs the vote offchain, together with
        /// the relayer and `block_number`, and the proxy dispatches it with the owner as origin.
        /// The signature must match and `block_number` be within `SignedTxLifetime` of now. The
        /// vote is then counted as in `vote`.
        #[pallet::call_index(3)]
        #[pallet::weight(
            <T as Config>::WeightInfo::signed_vote()
            .max(<T as Config>::WeightInfo::signed_vote_end_proposal())
        )]
        pub fn signed_vote(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            proposal_id: ProposalId,
            in_favor: bool,
            block_number: BlockNumberFor<T>,
        ) -> DispatchResultWithPostInfo {
            let owner = ensure_signed(origin)?;
            ensure!(owner == proof.signer, Error::<T>::SenderIsNotSigner);
            ensure!(
                block_number.saturating_add(T::SignedTxLifetime::get().into()) >
                    frame_system::Pallet::<T>::block_number(),
                Error::<T>::SignedTransactionExpired
            );

            // Create and verify the signed payload
            let signed_payload = Self::encode_signed_submit_vote_params(
                &proof.relayer,
                &proposal_id,
                &in_favor,
                &block_number,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload).is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            let finalised = Self::process_owner_vote(&owner, proposal_id, in_favor)?;

            if finalised {
                Ok(Some(<T as Config>::WeightInfo::signed_vote_end_proposal()).into())
            } else {
                Ok(Some(<T as Config>::WeightInfo::signed_vote()).into())
            }
        }

        #[pallet::call_index(4)]
        #[pallet::weight(
            <T as Config>::WeightInfo::unsigned_vote()
            .max(<T as Config>::WeightInfo::unsigned_vote_end_proposal())
        )]
        /// A watchtower node votes yes or no on the active internal proposal. `country` is the
        /// node's self-reported location, shown in `VoteSubmitted`. Signed by the node key over
        /// [`Pallet::unsigned_vote_signing_payload`].
        pub fn unsigned_vote(
            origin: OriginFor<T>,
            proposal_id: ProposalId,
            in_favor: bool,
            country: CountryCode,
            watchtower: T::AccountId,
            signature: <T::SignerId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;

            let vote = VoteChoice::Binary(in_favor);
            Self::validate_unsigned_vote(
                proposal_id,
                &vote,
                &country,
                watchtower.clone(),
                signature,
            )?;

            let finalised =
                Self::process_vote_choice(&watchtower, proposal_id, vote, Some(country))?;

            if finalised {
                Ok(Some(<T as Config>::WeightInfo::unsigned_vote_end_proposal()).into())
            } else {
                Ok(Some(<T as Config>::WeightInfo::unsigned_vote()).into())
            }
        }

        /// A watchtower names its answer on a `Choice` proposal. `country` as in
        /// `unsigned_vote`. Signed by the node key over [`Pallet::unsigned_vote_signing_payload`].
        #[pallet::call_index(10)]
        #[pallet::weight(
            <T as Config>::WeightInfo::unsigned_vote_choice()
            .max(<T as Config>::WeightInfo::unsigned_vote_choice_end_proposal())
        )]
        pub fn unsigned_vote_choice(
            origin: OriginFor<T>,
            proposal_id: ProposalId,
            choice: H256,
            country: CountryCode,
            watchtower: T::AccountId,
            signature: <T::SignerId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;

            let vote = VoteChoice::Choice(choice);
            Self::validate_unsigned_vote(
                proposal_id,
                &vote,
                &country,
                watchtower.clone(),
                signature,
            )?;

            let finalised =
                Self::process_vote_choice(&watchtower, proposal_id, vote, Some(country))?;

            if finalised {
                Ok(Some(<T as Config>::WeightInfo::unsigned_vote_choice_end_proposal()).into())
            } else {
                Ok(Some(<T as Config>::WeightInfo::unsigned_vote_choice()).into())
            }
        }

        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::finalise_proposal())]
        pub fn finalise_proposal(origin: OriginFor<T>, proposal_id: ProposalId) -> DispatchResult {
            // Anyone can call this to finalise voting
            ensure_signed(origin)?;

            let proposal = Proposals::<T>::get(proposal_id).ok_or(Error::<T>::ProposalNotFound)?;
            ensure!(
                ProposalStatus::<T>::get(proposal_id) == ProposalStatusEnum::Active,
                Error::<T>::ProposalNotActive
            );
            let current_block = <frame_system::Pallet<T>>::block_number();
            ensure!(
                Self::proposal_expired(current_block, &proposal),
                Error::<T>::ProposalVotingPeriodNotEnded
            );

            Self::finalise_expired_voting(proposal_id, &proposal)?;

            Ok(())
        }

        /// Set admin configurations
        #[pallet::call_index(6)]
        #[pallet::weight(<T as Config>::WeightInfo::set_admin_config_voting())]
        pub fn set_admin_config(
            origin: OriginFor<T>,
            config: AdminConfig<BlockNumberFor<T>, T::AccountId>,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            match config {
                AdminConfig::MinVotingPeriod(period) => {
                    <MinVotingPeriod<T>>::mutate(|p| *p = period);
                    Self::deposit_event(Event::MinVotingPeriodSet { new_period: period });
                    return Ok(Some(<T as Config>::WeightInfo::set_admin_config_voting()).into())
                },
                AdminConfig::AdminAccount(admin_account) => {
                    <AdminAccount<T>>::mutate(|a| *a = admin_account.clone());
                    Self::deposit_event(Event::AdminAccountSet { new_admin: admin_account });
                    return Ok(Some(<T as Config>::WeightInfo::set_admin_config_account()).into())
                },
            }
        }

        /// Activate the internal proposal at the head of the queue.
        ///
        /// Unsigned. Submitted by the offchain worker of collators once the previous internal
        /// proposal has been finalised. `validate_unsigned` only accepts locally produced
        /// copies, so this cannot be submitted via RPC or gossip.
        ///
        /// This is the only place internal proposals are activated, so it also carries the
        /// committee selection cost. The weight is charged for `MaxCommitteeSize` members and
        /// refunded to the actual committee size.
        #[pallet::call_index(7)]
        #[pallet::weight(Pallet::<T>::activation_weight(T::MaxCommitteeSize::get()))]
        pub fn activate_next_proposal(
            origin: OriginFor<T>,
            proposal_id: ProposalId,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;
            ensure!(
                Self::peek_front_id()? == Some(proposal_id),
                Error::<T>::ProposalNotNextInQueue
            );

            // Refund from the size selection actually ran for, not from storage: a failed
            // selection rolls `ProposalCommitteeSize` back but its reads were still done.
            let committee_size = Self::activate_next_proposal_inner()?;
            Ok(Some(Self::activation_weight(committee_size)).into())
        }

        /// Move the proposal at the head of the internal queue one place back, so the proposal
        /// behind it is activated first.
        ///
        /// Root only. Unblocks a head whose committee cannot be selected yet (for example it
        /// requests a committee while the node index is still being backfilled) when a proposal
        /// behind it could proceed. Needs at least two queued proposals.
        #[pallet::call_index(8)]
        #[pallet::weight(<T as Config>::WeightInfo::demote_queue_head())]
        pub fn demote_queue_head(origin: OriginFor<T>, proposal_id: ProposalId) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                Self::peek_front_id()? == Some(proposal_id),
                Error::<T>::ProposalNotNextInQueue
            );

            let (demoted, promoted) = Self::demote_head()?;
            Self::deposit_event(Event::QueueHeadDemoted { demoted, promoted });

            Ok(())
        }

        /// Cancel the proposal at the head of the internal queue without activating it.
        ///
        /// Root only. The consumer is told through `on_voting_completed` with `Cancelled` (the
        /// summary pallet sends the root to admin review) and the queue moves on. Meant for a
        /// head that cannot be activated, for example one whose committee waits for a node index
        /// backfill that is not progressing. The active proposal, if any, is untouched.
        #[pallet::call_index(9)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel_queue_head())]
        pub fn cancel_queue_head(origin: OriginFor<T>, proposal_id: ProposalId) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                Self::peek_front_id()? == Some(proposal_id),
                Error::<T>::ProposalNotNextInQueue
            );

            let proposal_id = Self::dequeue()?;
            let proposal = Proposals::<T>::get(proposal_id).ok_or(Error::<T>::ProposalNotFound)?;
            Self::finalise_voting(proposal_id, &proposal, ProposalStatusEnum::Cancelled)?;
            Self::deposit_event(Event::QueueHeadCancelled { proposal_id });

            Ok(())
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        fn validate_unsigned(source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            let reduce_priority: TransactionPriority = TransactionPriority::from(1000u64);

            match call {
                Call::activate_next_proposal { proposal_id } => {
                    // No signature: the call carries no privileged data. Its only argument must
                    // match the queue head and it only runs when nothing is active, so a forged
                    // copy is either rejected or does exactly what the legitimate one does.
                    // `Local` is only produced by this node's own OCW (RPC and gossip are
                    // `External`); `InBlock` is needed so other nodes can import the block.
                    match source {
                        TransactionSource::Local | TransactionSource::InBlock => {},
                        _ => return InvalidTransaction::Call.into(),
                    }

                    if ActiveInternalProposal::<T>::get().is_some() {
                        return InvalidTransaction::Stale.into()
                    }

                    match Self::peek_front_id() {
                        Ok(Some(head)) if head == *proposal_id => {},
                        _ => return InvalidTransaction::Custom(ACTIVATE_PROPOSAL_NOT_VALID).into(),
                    }

                    // A head that must wait for its committee stays queued. Keep the activation
                    // out of the block rather than include a failing tx.
                    if Self::head_committee_ready(*proposal_id).is_err() {
                        return InvalidTransaction::Custom(COMMITTEE_NOT_READY).into()
                    }

                    ValidTransaction::with_tag_prefix("wt_activateProposal")
                        .priority(TransactionPriority::max_value() - reduce_priority)
                        .and_provides((WATCHTOWER_ACTIVATE_PROPOSAL_CONTEXT, proposal_id))
                        .longevity(64_u64)
                        // Every collator submits its own copy locally; nothing is gossiped.
                        .propagate(false)
                        .build()
                },
                Call::unsigned_vote { proposal_id, in_favor, country, watchtower, signature } => {
                    // Fail early if vote is invalid. This avoids DDos attacks with invalid votes
                    if let Err(_) = Self::validate_unsigned_vote(
                        *proposal_id,
                        &VoteChoice::Binary(*in_favor),
                        country,
                        watchtower.clone(),
                        signature.clone(),
                    ) {
                        return InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID).into()
                    }

                    ValidTransaction::with_tag_prefix("wt_unsignedVote")
                        .priority(TransactionPriority::max_value() - reduce_priority)
                        .and_provides((watchtower, proposal_id))
                        .longevity(64_u64)
                        .propagate(true)
                        .build()
                },
                Call::unsigned_vote_choice {
                    proposal_id,
                    choice,
                    country,
                    watchtower,
                    signature,
                } => {
                    if let Err(_) = Self::validate_unsigned_vote(
                        *proposal_id,
                        &VoteChoice::Choice(*choice),
                        country,
                        watchtower.clone(),
                        signature.clone(),
                    ) {
                        return InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID).into()
                    }

                    ValidTransaction::with_tag_prefix("wt_unsignedVote")
                        .priority(TransactionPriority::max_value() - reduce_priority)
                        .and_provides((watchtower, proposal_id))
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
        fn on_idle(n: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            Self::cleanup_proposals(n, remaining_weight)
        }

        /// Submits `activate_next_proposal` from every collator when there is no active
        /// internal proposal but the queue is not empty. The tx is local-only, so whichever
        /// collator authors the next block includes its own copy and the rest become stale.
        fn offchain_worker(now: BlockNumberFor<T>) {
            // Cheapest check first: a host call with no storage access. Watchtower nodes are
            // not validators and exit here.
            if !sp_io::offchain::is_validator() {
                return
            }

            // One state read.
            if ActiveInternalProposal::<T>::get().is_some() {
                return
            }

            // Three state reads (Tail, Head, slot).
            let proposal_id = match Self::peek_front_id() {
                Ok(Some(id)) => id,
                Ok(None) => return,
                Err(e) => {
                    log::error!("🪲 Watchtower activation OCW: queue in corrupt state: {:?}", e);
                    return
                },
            };

            // The head may request a committee the node index cannot provide yet (backfill in
            // progress). `validate_unsigned` would reject the tx anyway; skip it here and say
            // why. Root can `demote_queue_head` to let the proposal behind it go first.
            if let Err(e) = Self::head_committee_ready(proposal_id) {
                log::warn!(
                    "⚠️ Watchtower activation OCW: proposal {:?} cannot be activated yet: {:?}",
                    proposal_id,
                    e
                );
                return
            }

            // Writes to offchain storage, so it comes after the read-only checks. At most one
            // attempt per block, even if the OCW re-runs for the same height.
            if OcwLock::record_block_run(now, ACTIVATION_OCW_ID.to_vec()).is_err() {
                return
            }

            let xt = T::create_bare(Call::<T>::activate_next_proposal { proposal_id }.into());
            if let Err(e) = SubmitTransaction::<T, Call<T>>::submit_transaction(xt) {
                // Usually means an identical copy is already in the local pool.
                log::debug!(
                    "Watchtower activation OCW: could not submit activation for {:?}: {:?}",
                    proposal_id,
                    e
                );
            }
        }
    }

    impl<T: Config> Pallet<T> {
        // Make sure this function returns an error if admin account is not set
        // If you change the return type, make sure to update `EnsureExternalProposerOrRoot`
        pub fn proposal_admin() -> Result<T::AccountId, Error<T>> {
            Ok(<AdminAccount<T>>::get().ok_or(Error::<T>::AdminAccountNotSet)?)
        }

        /// Worst case of the two `activate_next_proposal` outcomes for a committee of `k`.
        fn activation_weight(k: u32) -> Weight {
            <T as Config>::WeightInfo::activate_next_proposal(k)
                .max(<T as Config>::WeightInfo::activate_next_proposal_hook_fails(k))
        }

        /// Whether `proposal_id` (expected to be the queue head) can go through activation now.
        /// It must wait only while its committee cannot be sampled yet: a head with too few
        /// seats is let through so that activation cancels it. Proposals without a committee,
        /// or without data, are always ready.
        pub(crate) fn head_committee_ready(proposal_id: ProposalId) -> Result<(), Error<T>> {
            match Proposals::<T>::get(proposal_id) {
                Some(proposal) => match Self::committee_ready_for(&proposal) {
                    Ok(_) | Err(Error::<T>::NotEnoughSeatsForCommittee) => Ok(()),
                    Err(e) => Err(e),
                },
                None => Ok(()),
            }
        }

        fn add_proposal(
            proposer: Option<T::AccountId>,
            proposal_request: ProposalRequest,
        ) -> DispatchResult {
            let current_block = <frame_system::Pallet<T>>::block_number();
            let voting = proposal_request.voting;
            // Proposal is validated before creating it.
            let mut proposal = to_proposal::<T>(proposal_request, proposer, current_block)?;

            let external_ref = proposal.external_ref;
            ensure!(
                !ExternalRef::<T>::contains_key(external_ref),
                Error::<T>::DuplicateExternalRef
            );

            let proposal_id = proposal.generate_id();
            ensure!(!Proposals::<T>::contains_key(proposal_id), Error::<T>::DuplicateProposal);

            let status: ProposalStatusEnum;
            if let ProposalSource::Internal(_) = proposal.source {
                // Internal proposals are always queued. Activation, including committee
                // selection, happens in `activate_next_proposal` (submitted by the collator
                // OCW) so its weight never lands inside the submitting extrinsic.
                Self::enqueue(proposal_id)?;
                status = ProposalStatusEnum::Queued;
            } else {
                // External proposals never have a committee, so this cannot fail.
                Self::activate_proposal(proposal_id, &mut proposal, current_block, None)?;
                status = ProposalStatusEnum::Active;
            }

            ProposalStatus::<T>::insert(proposal_id, &status);
            Proposals::<T>::insert(proposal_id, &proposal);
            ExternalRef::<T>::insert(external_ref, proposal_id);
            if voting != VotingKind::Binary {
                VotingKindOf::<T>::insert(proposal_id, voting);
            }

            if status == ProposalStatusEnum::Active {
                T::WatchtowerHooks::on_proposal_submitted(proposal_id, proposal)?;
            }

            Self::deposit_event(Event::ProposalSubmitted { proposal_id, external_ref, status });

            Ok(())
        }

        /// A yes/no vote by a node owner on an external proposal (`vote`, `signed_vote`). Owners
        /// are accounts, not nodes, so the vote carries no country.
        fn process_owner_vote(
            owner: &T::AccountId,
            proposal_id: ProposalId,
            in_favor: bool,
        ) -> Result<bool, DispatchError> {
            let proposal = Proposals::<T>::get(proposal_id).ok_or(Error::<T>::ProposalNotFound)?;
            ensure!(
                matches!(proposal.source, ProposalSource::External),
                Error::<T>::SignedVoteOnInternalProposal
            );
            Self::process_vote_choice(owner, proposal_id, VoteChoice::Binary(in_favor), None)
        }

        fn kind_of(vote: &VoteChoice) -> VotingKind {
            match vote {
                VoteChoice::Binary(_) => VotingKind::Binary,
                VoteChoice::Choice(_) => VotingKind::Choice,
            }
        }

        fn process_vote_choice(
            voter: &T::AccountId,
            proposal_id: ProposalId,
            vote: VoteChoice,
            country: Option<CountryCode>,
        ) -> Result<bool, DispatchError> {
            let proposal = Proposals::<T>::get(proposal_id).ok_or(Error::<T>::ProposalNotFound)?;
            ensure!(
                VotingKindOf::<T>::get(proposal_id) == Self::kind_of(&vote),
                Error::<T>::WrongVoteKind
            );
            ensure!(
                ProposalStatus::<T>::get(proposal_id) == ProposalStatusEnum::Active,
                Error::<T>::ProposalNotActive
            );

            // Do this before validating vote uniqueness. Unsigned votes from a node that already
            // voted never get here (`validate_unsigned_vote` refuses them), so for those the
            // expired proposal is finalised by `finalise_proposal` or `on_idle` instead.
            let current_block = <frame_system::Pallet<T>>::block_number();
            if Self::proposal_expired(current_block, &proposal) {
                // Voting ended but we haven't finalised it yet
                Self::finalise_expired_voting(proposal_id, &proposal)?;
                return Ok(true)
            }

            ensure!(!Voters::<T>::contains_key(proposal_id, voter), Error::<T>::AlreadyVoted);

            let vote_weight;
            match proposal.source {
                ProposalSource::Internal(_) => {
                    ensure!(
                        T::Watchtowers::is_authorized_watchtower(voter),
                        Error::<T>::UnauthorizedVoter
                    );
                    ensure!(
                        Self::is_committee_member(proposal_id, voter),
                        Error::<T>::NotInCommittee
                    );
                    T::VoteFilter::ensure_can_vote(proposal_id, voter, &vote)?;

                    // This should not happen but just in case (defensive programming)
                    ensure!(
                        ActiveInternalProposal::<T>::get() == Some(proposal_id),
                        Error::<T>::CorruptedState
                    );

                    vote_weight = 1;
                },
                ProposalSource::External => {
                    ensure!(
                        T::Watchtowers::is_watchtower_owner(voter),
                        Error::<T>::UnauthorizedVoter
                    );

                    vote_weight = T::Watchtowers::get_watchtower_voting_weight(voter);
                    // This should not happen but just in case
                    ensure!(vote_weight > 0, Error::<T>::UnauthorizedVoter);
                },
            };

            match vote {
                VoteChoice::Binary(in_favor) => {
                    Voters::<T>::insert(proposal_id, voter, in_favor);
                    Votes::<T>::mutate(proposal_id, |tally| {
                        if in_favor {
                            tally.in_favors = tally.in_favors.saturating_add(vote_weight);
                        } else {
                            tally.againsts = tally.againsts.saturating_add(vote_weight);
                        }
                    });
                    Self::deposit_event(Event::VoteSubmitted {
                        voter: voter.clone(),
                        proposal_id,
                        in_favor,
                        vote_weight,
                        country: country.map(sanitise_country),
                    });
                },
                VoteChoice::Choice(choice) => {
                    Voters::<T>::insert(proposal_id, voter, true);
                    let tally = ChoiceTally::<T>::mutate(proposal_id, choice, |tally| {
                        *tally = tally.saturating_add(vote_weight);
                        *tally
                    });
                    ChoiceStandings::<T>::mutate(proposal_id, |standing| {
                        standing.record(choice, tally, vote_weight)
                    });
                    Self::deposit_event(Event::ChoiceVoteSubmitted {
                        voter: voter.clone(),
                        proposal_id,
                        choice,
                        country: country.map(sanitise_country),
                    });
                },
            }

            // Expiry was handled above, so only an early decision is possible here.
            if let Some(result) = Self::decide(proposal_id, &proposal, false) {
                // Consensus has been reached, finalise voting
                Self::finalise_voting(proposal_id, &proposal, result)?;
                return Ok(true)
            }

            Ok(false)
        }

        fn cleanup_proposals(now: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(remaining_weight);

            // Check if the active proposal has expired and finalise it if needed. An active
            // proposal that has not expired is left alone and does NOT block the cleanup
            // below: the two jobs are independent.
            if meter
                .try_consume(<T as Config>::WeightInfo::active_proposal_expiry_status())
                .is_err()
            {
                return meter.consumed()
            }

            if let Some((proposal_id, active_proposal, expired)) =
                Self::active_proposal_expiry_status(now)
            {
                if expired {
                    if meter
                        .try_consume(<T as Config>::WeightInfo::finalise_expired_voting())
                        .is_err()
                    {
                        return meter.consumed()
                    }
                    Self::finalise_expired_voting(proposal_id, &active_proposal).unwrap_or_else(
                        |e| {
                            log::error!(
                                "🪲 Failed to finalise active proposal {}: {:?}",
                                proposal_id,
                                e
                            );
                        },
                    );
                }
            };

            // Now remove any completed proposals. Every storage access below is paid for, in
            // both weight components, before it happens: the fixed part (head read, emptiness
            // check, final removal) is reserved here and each page of entries is sized to
            // what the meter can still afford.
            if meter
                .try_consume(<T as Config>::WeightInfo::cleanup_finished_proposal())
                .is_err()
            {
                return meter.consumed()
            }

            let Some(proposal_id) = Self::next_proposal_to_remove() else {
                // Nothing to clean
                return meter.consumed();
            };

            let page =
                Self::affordable_entries(&meter, <T as Config>::WeightInfo::cleanup_voters_page);
            if page > 0 {
                let removed = Self::remove_voters_page(proposal_id, page);
                meter.consume(<T as Config>::WeightInfo::cleanup_voters_page(removed));
            }

            let page =
                Self::affordable_entries(&meter, <T as Config>::WeightInfo::cleanup_committee_page);
            if page > 0 {
                let removed = Self::remove_committee_page(proposal_id, page);
                meter.consume(<T as Config>::WeightInfo::cleanup_committee_page(removed));
            }

            let page = Self::affordable_entries(
                &meter,
                <T as Config>::WeightInfo::cleanup_choice_tally_page,
            );
            if page > 0 {
                let removed = Self::remove_choice_tally_page(proposal_id, page);
                meter.consume(<T as Config>::WeightInfo::cleanup_choice_tally_page(removed));
            }

            Self::remove_proposal_if_cleaned(proposal_id);

            meter.consumed()
        }

        /// Largest page of at most `CLEANUP_PAGE_SIZE` entries whose `page_weight` fits in
        /// `meter`, or 0 if not even an empty page does. Generated weights are linear in the
        /// page size, so `page_weight(n)` for any `n` up to the result fits too.
        fn affordable_entries(meter: &WeightMeter, page_weight: fn(u32) -> Weight) -> u32 {
            let base = page_weight(0);
            let per_entry = page_weight(1).saturating_sub(base);
            let Some(spare) = meter.remaining().checked_sub(&base) else { return 0 };
            spare
                .checked_div_per_component(&per_entry)
                .unwrap_or(CLEANUP_PAGE_SIZE as u64)
                .min(CLEANUP_PAGE_SIZE as u64) as u32
        }

        /// Oldest proposal waiting to have its data removed.
        pub(crate) fn next_proposal_to_remove() -> Option<ProposalId> {
            ProposalsToRemove::<T>::iter_keys().next()
        }

        /// Removes at most `max` `Voters` entries of `proposal_id`, collecting the keys first
        /// so nothing is deleted while iterating. Returns how many were removed.
        pub(crate) fn remove_voters_page(proposal_id: ProposalId, max: u32) -> u32 {
            let voters: Vec<T::AccountId> =
                Voters::<T>::iter_key_prefix(proposal_id).take(max as usize).collect();
            for who in &voters {
                Voters::<T>::remove(proposal_id, who);
            }
            voters.len() as u32
        }

        /// Same as `remove_voters_page` for the committee members.
        pub(crate) fn remove_committee_page(proposal_id: ProposalId, max: u32) -> u32 {
            let members: Vec<T::AccountId> = ProposalCommittee::<T>::iter_key_prefix(proposal_id)
                .take(max as usize)
                .collect();
            for who in &members {
                ProposalCommittee::<T>::remove(proposal_id, who);
            }
            members.len() as u32
        }

        /// Same as `remove_voters_page` for the per-answer tallies of a `Choice` proposal.
        pub(crate) fn remove_choice_tally_page(proposal_id: ProposalId, max: u32) -> u32 {
            let answers: Vec<H256> =
                ChoiceTally::<T>::iter_key_prefix(proposal_id).take(max as usize).collect();
            for answer in &answers {
                ChoiceTally::<T>::remove(proposal_id, answer);
            }
            answers.len() as u32
        }

        /// Removes the proposal and its remaining data once no voters, no committee members
        /// and no tallies are left. Returns true if it was removed.
        pub(crate) fn remove_proposal_if_cleaned(proposal_id: ProposalId) -> bool {
            if Voters::<T>::iter_prefix(proposal_id).next().is_some() ||
                ProposalCommittee::<T>::iter_prefix(proposal_id).next().is_some() ||
                ChoiceTally::<T>::iter_prefix(proposal_id).next().is_some()
            {
                return false
            }

            Proposals::<T>::remove(proposal_id);
            Votes::<T>::remove(proposal_id);
            ProposalCommitteeSize::<T>::remove(proposal_id);
            VotingKindOf::<T>::remove(proposal_id);
            ChoiceStandings::<T>::remove(proposal_id);
            WinningChoice::<T>::remove(proposal_id);
            ProposalsToRemove::<T>::remove(proposal_id);

            Self::deposit_event(Event::ProposalCleaned { proposal_id });
            true
        }

        /// Bytes a watchtower node signs for an unsigned vote. Shared by validation and every
        /// offchain worker that votes, so they cannot drift apart. The country is signed so
        /// a relayer cannot change it.
        pub fn unsigned_vote_signing_payload(
            proposal_id: ProposalId,
            vote: &VoteChoice,
            country: &CountryCode,
            watchtower: &T::AccountId,
        ) -> Vec<u8> {
            // The enum's tag keeps a yes/no vote and an answer apart.
            (WATCHTOWER_UNSIGNED_VOTE_CONTEXT, proposal_id, vote, country, watchtower).encode()
        }

        fn validate_unsigned_vote(
            proposal_id: ProposalId,
            vote: &VoteChoice,
            country: &CountryCode,
            watchtower: T::AccountId,
            signature: <T::SignerId as RuntimeAppPublic>::Signature,
        ) -> Result<(), DispatchError> {
            // Only Active internal proposals can be voted on with unsigned txs
            ensure!(
                ActiveInternalProposal::<T>::get() == Some(proposal_id),
                Error::<T>::InvalidProposalForUnsignedVote
            );
            ensure!(
                VotingKindOf::<T>::get(proposal_id) == Self::kind_of(vote),
                Error::<T>::WrongVoteKind
            );
            // Reject non-members before the signature check so they never reach the pool.
            ensure!(
                Self::is_committee_member(proposal_id, &watchtower),
                Error::<T>::NotInCommittee
            );
            // Same for votes a consumer cannot accept. Before the uniqueness check, so a vote
            // the consumer refuses gets the consumer's reason.
            T::VoteFilter::ensure_can_vote(proposal_id, &watchtower, vote)?;
            // A vote already counted must not re-enter the pool: once the pool forgets the
            // included copy, anyone could re-gossip the signed bytes and have them included
            // for free, only to fail in dispatch. This also means a node that already voted
            // cannot finalise an expired proposal by voting again; `finalise_proposal` and
            // `on_idle` do that.
            ensure!(!Voters::<T>::contains_key(proposal_id, &watchtower), Error::<T>::AlreadyVoted);

            let voter_signing_key = match T::Watchtowers::get_node_signing_key(&watchtower) {
                Some(key) => key,
                None => return Err(Error::<T>::VoterSigningKeyNotFound.into()),
            };

            let payload =
                Self::unsigned_vote_signing_payload(proposal_id, vote, country, &watchtower);
            ensure!(
                voter_signing_key.verify(&payload, &signature),
                Error::<T>::UnauthorizedUnsignedTransaction
            );

            Ok(())
        }
    }

    impl<T: Config> WatchtowerInterface for Pallet<T> {
        type AccountId = T::AccountId;

        fn get_proposal_status(proposal_id: ProposalId) -> ProposalStatusEnum {
            ProposalStatus::<T>::get(proposal_id)
        }

        fn get_proposer(proposal_id: ProposalId) -> Option<Self::AccountId> {
            Proposals::<T>::get(proposal_id)?.proposer
        }

        fn submit_proposal(
            proposer: Option<Self::AccountId>,
            proposal: ProposalRequest,
        ) -> DispatchResult {
            Self::add_proposal(proposer, proposal)
        }

        fn min_committee_size() -> u32 {
            T::MinCommitteeSize::get()
        }

        fn max_committee_size() -> u32 {
            T::MaxCommitteeSize::get()
        }

        fn ensure_committee_ready(size: u32) -> DispatchResult {
            Self::committee_ready(size).map_err(Into::into)
        }

        #[cfg(feature = "runtime-benchmarks")]
        fn setup_nodes_for_benchmark(size: u32) {
            T::BenchmarkHelper::setup_nodes(size);
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
}
