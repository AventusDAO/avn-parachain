use crate::*;
use frame_support::{CloneNoBound, EqNoBound, PartialEqNoBound, RuntimeDebugNoBound};

#[derive(
    Encode,
    Decode,
    RuntimeDebugNoBound,
    CloneNoBound,
    PartialEqNoBound,
    EqNoBound,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub enum Payload<T: Config> {
    /// Small proposals that can fit safely in the runtime
    Inline(BoundedVec<u8, T::MaxInlineLen>),

    /// A link to off-chain proposal data (e.g. IPFS hash)
    Uri(BoundedVec<u8, T::MaxUriLen>),
}

pub fn to_proposal<T: Config>(
    request: ProposalRequest,
    proposer: Option<T::AccountId>,
    current_block: BlockNumberFor<T>,
) -> Result<Proposal<T>, Error<T>> {
    let min_vote_duration = MinVotingPeriod::<T>::get().saturated_into::<u32>();
    let vote_duration: u32 = request.vote_duration.unwrap_or(min_vote_duration);
    if vote_duration < min_vote_duration {
        return Err(Error::<T>::VotingPeriodTooShort)
    }

    let proposal = Proposal {
        title: BoundedVec::try_from(request.title).map_err(|_| Error::<T>::InvalidTitle)?,
        payload: to_payload(request.payload)?,
        threshold: request.threshold,
        source: request.source,
        decision_rule: request.decision_rule,
        external_ref: request.external_ref,
        proposer,
        created_at: BlockNumberFor::<T>::from(request.created_at),
        vote_duration,
        // This gets updated when the proposal is activated
        end_at: None,
    };

    if !proposal.is_valid(current_block) {
        return Err(Error::<T>::InvalidProposal)
    }

    Ok(proposal)
}

pub fn to_payload<T: Config>(raw: RawPayload) -> Result<Payload<T>, Error<T>> {
    match raw {
        RawPayload::Inline(data) => {
            let bounded =
                BoundedVec::try_from(data).map_err(|_| Error::<T>::InvalidInlinePayload)?;
            Ok(Payload::Inline(bounded))
        },
        RawPayload::Uri(data) => {
            let bounded = BoundedVec::try_from(data).map_err(|_| Error::<T>::InvalidUri)?;
            Ok(Payload::Uri(bounded))
        },
    }
}

#[derive(
    Encode,
    Decode,
    RuntimeDebugNoBound,
    CloneNoBound,
    PartialEqNoBound,
    EqNoBound,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct Proposal<T: Config> {
    pub title: BoundedVec<u8, T::MaxTitleLen>,
    pub payload: Payload<T>,
    pub threshold: Perbill,
    pub source: ProposalSource,
    pub decision_rule: DecisionRule,
    /// A unique ref provided by the proposer. Used when sending notifications about this proposal.
    pub external_ref: H256,
    // Internal proposer or SUDO. SUDO does not have an account id hence Option
    pub proposer: Option<T::AccountId>,
    pub created_at: BlockNumberFor<T>,
    pub vote_duration: u32,
    pub end_at: Option<BlockNumberFor<T>>,
}

impl<T: Config> Proposal<T> {
    pub fn generate_id(&self) -> ProposalId {
        // External ref is unique globally, so we can use it to generate a unique id
        let data = (self.external_ref, self.created_at, self.vote_duration).encode();
        let hash = sp_io::hashing::blake2_256(&data);
        ProposalId::from(hash)
    }

    pub fn is_valid(&self, current_block: BlockNumberFor<T>) -> bool {
        let base_is_valid = !self.title.is_empty() &&
            self.external_ref != H256::zero() &&
            self.vote_duration >= MinVotingPeriod::<T>::get().saturated_into::<u32>() &&
            self.threshold <= Perbill::one() &&
            self.created_at <= current_block;

        let payload_valid = match &self.payload {
            Payload::Inline(data) =>
                !data.is_empty() && matches!(self.source, ProposalSource::Internal(_)),
            Payload::Uri(data) =>
                !data.is_empty() && matches!(self.source, ProposalSource::External),
        };

        base_is_valid && payload_valid
    }
}

pub trait NodesInterface<AccountId, SignerId> {
    /// Check if the given account is an authorized watchtower
    fn is_authorized_watchtower(who: &AccountId) -> bool;

    /// Check if the given account owns watchtower nodes
    fn is_watchtower_owner(who: &AccountId) -> bool;

    /// Get the count of authorized watchtowers without fetching the full list
    fn get_authorized_watchtowers_count() -> u32;

    /// Get the voting weight of a given watchtower
    fn get_watchtower_voting_weight(who: &AccountId) -> u32;

    /// Get the signing key for a given watchtower account
    fn get_node_signing_key(node: &AccountId) -> Option<SignerId>;

    /// Get a local watchtower account and its signing key, if available on this node
    fn get_node_from_local_signing_keys() -> Option<(AccountId, SignerId)>;
}

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    RuntimeDebug,
    Clone,
    PartialEq,
    Eq,
    TypeInfo,
    MaxEncodedLen,
    Default,
)]
pub struct Vote {
    pub in_favors: u32,
    pub againsts: u32,
}

#[derive(Encode, Decode, TypeInfo, Debug, Clone, PartialEq, DecodeWithMemTracking)]
pub enum AdminConfig<BlockNumber, AccountId> {
    MinVotingPeriod(BlockNumber),
    AdminAccount(Option<AccountId>),
}
