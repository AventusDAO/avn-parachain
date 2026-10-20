#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use core::marker::PhantomData;
use scale_info::TypeInfo;
use sp_core::H256;
use sp_runtime::{traits::Member, Debug, DispatchResult, Perbill};

pub type ProposalId = H256;

#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq, TypeInfo, DecodeWithMemTracking)]
pub enum RawPayload {
    /// Small proposals that can fit safely in the runtime
    Inline(Vec<u8>),

    /// A link to off-chain proposal data (e.g. IPFS hash)
    Uri(Vec<u8>),
}

#[derive(
    Encode, Decode, Debug, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub enum ProposalSource {
    /// External proposals created by other users. These require manual review and voting.
    External,
    /// Proposals created by other pallets. These can be voted on automatically by the pallet.
    Internal(ProposalType),
}

#[derive(
    Encode, Decode, Debug, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub enum ProposalType {
    Summary,
    Anchor,
    Governance,
    Other(u8),
}

#[derive(
    Encode, Decode, Debug, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub enum ProposalStatusEnum {
    Queued,
    Active,
    Resolved { passed: bool },
    Cancelled,
    Expired,
    Unknown,
}

/// How a proposal is decided.
///
/// Either side holding `threshold` of the eligible voters always resolves the proposal early,
/// whatever the rule. The rule says what happens when the voting period ends first.
#[derive(
    Encode,
    Decode,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
    Default,
)]
pub enum DecisionRule {
    /// Report `Expired` and let the consumer decide what that means. The summary pallet treats
    /// it as "no objection in time, accepted". Legacy default for internal proposals.
    #[default]
    ExpireUnresolved,
    /// `Resolved { passed: in_favors > againsts }` over the votes actually cast. Legacy
    /// behaviour of external proposals.
    SimpleMajorityOnExpiry,
    /// `Resolved { passed: false }`.
    RejectOnExpiry,
}

//implement default for ProposalStatusEnum to be Unknown
impl Default for ProposalStatusEnum {
    fn default() -> Self {
        ProposalStatusEnum::Unknown
    }
}

#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq, TypeInfo, DecodeWithMemTracking)]
pub struct ProposalRequest {
    pub title: Vec<u8>,
    pub payload: RawPayload,
    pub threshold: Perbill,
    pub source: ProposalSource,
    pub decision_rule: DecisionRule,
    /// A unique ref provided by the proposer. Used when sending notifications about this proposal.
    pub external_ref: H256,
    pub created_at: u32,
    pub vote_duration: Option<u32>,
    /// Number of nodes to randomly select to vote on this proposal. `None` means every node
    /// votes. Only valid for internal proposals.
    pub committee_size: Option<u32>,
}

// Interface for other pallets to interact with the watchtower pallet
pub trait WatchtowerInterface {
    type AccountId;

    fn submit_proposal(
        proposer: Option<Self::AccountId>,
        proposal: ProposalRequest,
    ) -> DispatchResult;

    fn get_proposal_status(proposal_id: ProposalId) -> ProposalStatusEnum;
    fn get_proposer(proposal_id: ProposalId) -> Option<Self::AccountId>;
    /// Smallest committee a proposal may request.
    fn min_committee_size() -> u32;
    /// Largest committee a proposal may request.
    fn max_committee_size() -> u32;
    /// Fails if a committee of `size` nodes could not be selected right now (node index still
    /// backfilling, or fewer than `min_committee_size` nodes registered). Lets a consumer
    /// refuse a configuration that would leave its proposals stuck in the queue.
    fn ensure_committee_ready(size: u32) -> DispatchResult;

    /// Benchmark setup for consumers: registers enough nodes that `ensure_committee_ready(size)`
    /// passes. Implementations without real nodes need not override it.
    #[cfg(feature = "runtime-benchmarks")]
    fn setup_nodes_for_benchmark(_size: u32) {}
}

// A simple no-op implementation of the WatchtowerInterface trait
pub struct NoopWatchtower<AccountId>(PhantomData<AccountId>);
impl<AccountId> WatchtowerInterface for NoopWatchtower<AccountId>
where
    AccountId: Member + MaxEncodedLen + TypeInfo + Eq + core::fmt::Debug,
{
    type AccountId = AccountId;

    fn submit_proposal(_a: Option<Self::AccountId>, _p: ProposalRequest) -> DispatchResult {
        Ok(())
    }

    fn get_proposal_status(_id: ProposalId) -> ProposalStatusEnum {
        ProposalStatusEnum::Unknown
    }

    fn get_proposer(_id: ProposalId) -> Option<Self::AccountId> {
        None
    }

    fn min_committee_size() -> u32 {
        0
    }

    fn max_committee_size() -> u32 {
        u32::MAX
    }

    fn ensure_committee_ready(_size: u32) -> DispatchResult {
        Ok(())
    }
}

pub trait WatchtowerHooks<P> {
    /// Called when Watchtower raises an alert/notification.
    fn on_proposal_submitted(proposal_id: ProposalId, proposal: P) -> DispatchResult;
    fn on_voting_completed(
        proposal_id: ProposalId,
        external_ref: &H256,
        result: &ProposalStatusEnum,
    );
    fn on_cancelled(proposal_id: ProposalId, external_ref: &H256);
}

#[impl_trait_for_tuples::impl_for_tuples(30)]
impl<P: Clone> WatchtowerHooks<P> for Tuple {
    /// Stops at the first member that rejects the proposal and returns its error, so the
    /// watchtower pallet can refuse (on submission) or cancel (on deferred activation) a
    /// proposal that a consumer cannot process. Members before the failing one have already run.
    fn on_proposal_submitted(proposal_id: ProposalId, proposal: P) -> DispatchResult {
        for_tuples!( #( Tuple::on_proposal_submitted(proposal_id, proposal.clone())?; )* );
        Ok(())
    }

    fn on_voting_completed(
        proposal_id: ProposalId,
        external_ref: &H256,
        result: &ProposalStatusEnum,
    ) {
        for_tuples!( #( Tuple::on_voting_completed(proposal_id, external_ref, result); )* );
    }

    fn on_cancelled(proposal_id: ProposalId, external_ref: &H256) {
        for_tuples!( #( Tuple::on_cancelled(proposal_id, external_ref); )* );
    }
}
