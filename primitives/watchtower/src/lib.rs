#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use core::marker::PhantomData;
use scale_info::TypeInfo;
use sp_core::H256;
use sp_runtime::{traits::Member, DispatchResult, Perbill, RuntimeDebug};

pub type ProposalId = H256;

#[derive(Encode, Decode, RuntimeDebug, Clone, PartialEq, Eq, TypeInfo, DecodeWithMemTracking)]
pub enum RawPayload {
    /// Small proposals that can fit safely in the runtime
    Inline(Vec<u8>),

    /// A link to off-chain proposal data (e.g. IPFS hash)
    Uri(Vec<u8>),
}

#[derive(
    Encode,
    Decode,
    RuntimeDebug,
    Clone,
    PartialEq,
    Eq,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
pub enum ProposalSource {
    /// External proposals created by other users. These require manual review and voting.
    External,
    /// Proposals created by other pallets. These can be voted on automatically by the pallet.
    Internal(ProposalType),
}

#[derive(
    Encode,
    Decode,
    RuntimeDebug,
    Clone,
    PartialEq,
    Eq,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
pub enum ProposalType {
    Summary,
    Anchor,
    Governance,
    Other(u8),
}

#[derive(
    Encode,
    Decode,
    RuntimeDebug,
    Clone,
    PartialEq,
    Eq,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
pub enum ProposalStatusEnum {
    Queued,
    Active,
    Resolved { passed: bool },
    Cancelled,
    Expired,
    Unknown,
}

#[derive(
    Encode,
    Decode,
    RuntimeDebug,
    Clone,
    PartialEq,
    Eq,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
pub enum DecisionRule {
    /// Yes > No to win
    SimpleMajority,
}

//implement default for ProposalStatusEnum to be Unknown
impl Default for ProposalStatusEnum {
    fn default() -> Self {
        ProposalStatusEnum::Unknown
    }
}

#[derive(Encode, Decode, RuntimeDebug, Clone, PartialEq, Eq, TypeInfo, DecodeWithMemTracking)]
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
    fn on_proposal_submitted(proposal_id: ProposalId, proposal: P) -> DispatchResult {
        for_tuples!( #( Tuple::on_proposal_submitted(proposal_id, proposal.clone()); )* );
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
