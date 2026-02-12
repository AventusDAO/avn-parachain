#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::{dispatch::DispatchResult, ensure};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_application_crypto::RuntimeAppPublic;
use sp_avn_common::{
    bounds::{MaximumValidatorsBound, VotingSessionIdBound},
    event_types::Validator,
};
use sp_runtime::{
    scale_info::TypeInfo,
    traits::{Member, Zero},
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionValidity, ValidTransaction,
    },
    BoundedVec, DispatchError,
};
use sp_std::prelude::*;

use super::{Config, Error};
use crate::Pallet as AVN;

pub const APPROVE_VOTE_IS_NOT_VALID: u8 = 2;
pub const REJECT_VOTE_IS_NOT_VALID: u8 = 3;
pub const VOTE_SESSION_IS_NOT_VALID: u8 = 4;
pub const VOTING_SESSION_DATA_IS_NOT_FOUND: u8 = 5;
pub const APPROVE_VOTE: bool = true;
pub const REJECT_VOTE: bool = false;

#[derive(PartialEq, Eq, Clone, Encode, Decode, Debug, TypeInfo, MaxEncodedLen)]
pub struct VotingSessionData<AccountId, BlockNumber> {
    // TODO convert to BoundedVec
    /// The unique identifier for this voting session
    pub voting_session_id: BoundedVec<u8, VotingSessionIdBound>,
    /// The number of approval votes that are needed to reach an outcome.
    pub threshold: u32,
    /// The current set of voters that approved it.
    pub ayes: BoundedVec<AccountId, MaximumValidatorsBound>,
    /// The current set of voters that rejected it.
    pub nays: BoundedVec<AccountId, MaximumValidatorsBound>,
    /// The hard end time of this vote.
    pub end_of_voting_period: BlockNumber,
    /// The block number this session was created on
    pub created_at_block: BlockNumber,
}

// AccountId cannot be defaulted by the `Default` derive macro anymore so we
// need to provide a manual implementation
impl<AccountId, BlockNumber: Zero> Default for VotingSessionData<AccountId, BlockNumber> {
    fn default() -> Self {
        Self {
            voting_session_id: BoundedVec::default(),
            threshold: 0u32,
            ayes: BoundedVec::default(),
            nays: BoundedVec::default(),
            end_of_voting_period: Zero::zero(),
            created_at_block: Zero::zero(),
        }
    }
}

impl<AccountId: Member, BlockNumber: Member> VotingSessionData<AccountId, BlockNumber> {
    pub fn new(
        session_id: BoundedVec<u8, VotingSessionIdBound>,
        threshold: u32,
        end_of_voting_period: BlockNumber,
        created_at_block: BlockNumber,
    ) -> Self {
        return VotingSessionData::<AccountId, BlockNumber> {
            voting_session_id: session_id,
            threshold,
            ayes: BoundedVec::default(),
            nays: BoundedVec::default(),
            end_of_voting_period,
            created_at_block,
        };
    }

    pub fn has_outcome(&self) -> bool {
        let threshold: usize = self.threshold as usize;
        return self.ayes.len() >= threshold || self.nays.len() >= threshold;
    }

    // The vote has been accepted (positive votes surpass or match the negative ones)
    pub fn is_approved(&self) -> bool {
        return self.ayes.len() >= self.threshold as usize;
    }

    // The voter has already cast a vote for this vote subject in this session
    pub fn has_voted(&self, voter: &AccountId) -> bool {
        return self.ayes.contains(voter) || self.nays.contains(voter);
    }
}

pub trait VotingSessionManager<AccountId, BlockNumber> {
    fn cast_vote_context(&self) -> &'static [u8];

    fn end_voting_period_context(&self) -> &'static [u8];

    fn state(&self) -> Result<VotingSessionData<AccountId, BlockNumber>, DispatchError>;

    // The voting session has been created and the vote subject is in the correct state (for
    // example, pending a vote)
    fn is_valid(&self) -> bool;

    // The session is valid (is_valid) AND has not ended yet
    fn is_active(&self) -> bool;

    fn record_approve_vote(&self, voter: AccountId) -> DispatchResult;

    fn record_reject_vote(&self, voter: AccountId) -> DispatchResult;

    fn end_voting_session(&self, sender: AccountId) -> DispatchResult;
}

pub fn process_approve_vote<T: Config>(
    voting_session: &Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>>,
    voter: T::AccountId,
) -> DispatchResult {
    validate_vote::<T>(voting_session, &voter)?;
    voting_session.record_approve_vote(voter.clone())?;
    end_voting_if_outcome_reached::<T>(voting_session, voter)?;
    Ok(())
}

pub fn validate_vote<T: Config>(
    voting_session: &Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>>,
    voter: &T::AccountId,
) -> DispatchResult {
    ensure!(AVN::<T>::is_validator(voter), Error::<T>::NotAValidator);
    ensure!(voting_session.is_active(), Error::<T>::InvalidVote);
    ensure!(!voting_session.state()?.has_voted(voter), Error::<T>::DuplicateVote);
    Ok(())
}

pub fn process_reject_vote<T: Config>(
    voting_session: &Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>>,
    voter: T::AccountId,
) -> DispatchResult {
    validate_vote::<T>(voting_session, &voter)?;
    voting_session.record_reject_vote(voter.clone())?;
    end_voting_if_outcome_reached::<T>(voting_session, voter)?;
    Ok(())
}

fn end_voting_if_outcome_reached<T: Config>(
    voting_session: &Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>>,
    voter: T::AccountId,
) -> DispatchResult {
    if voting_session.state()?.has_outcome() && voting_session.is_active() {
        voting_session.end_voting_session(voter)?;
    }

    Ok(())
}

pub fn end_voting_period_validate_unsigned<T: Config>(
    voting_session: &Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>>,
    validator: &Validator<T::AuthorityId, T::AccountId>,
    signature: &<T::AuthorityId as RuntimeAppPublic>::Signature,
) -> TransactionValidity {
    if !voting_session.is_valid() {
        return InvalidTransaction::Custom(VOTE_SESSION_IS_NOT_VALID).into();
    }

    let voting_session_data = voting_session.state();
    if voting_session_data.is_err() {
        return InvalidTransaction::Custom(VOTING_SESSION_DATA_IS_NOT_FOUND).into();
    }

    let voting_session_id =
        voting_session_data.expect("voting session data is ok").voting_session_id;

    if !AVN::<T>::signature_is_valid(
        &(voting_session.end_voting_period_context(), voting_session_id.clone()),
        &validator,
        signature,
    ) {
        return InvalidTransaction::BadProof.into();
    };

    return ValidTransaction::with_tag_prefix("vote")
        .priority(TransactionPriority::max_value())
        .and_provides(vec![(
            voting_session.end_voting_period_context(),
            voting_session_id,
            validator,
        )
            .encode()])
        .longevity(64_u64)
        .propagate(true)
        .build();
}

/// Unlike SR signatures, `_eth_signature` is not validated here and MUST be validated in the public
/// dispatch method being called. ECDSA sig validation is different because we cannot raise an
/// offence (and mutate storage) from validate_unsigned.
pub fn approve_vote_validate_unsigned<T: Config>(
    voting_session: &Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>>,
    validator: &Validator<T::AuthorityId, T::AccountId>,
    signature: &<T::AuthorityId as RuntimeAppPublic>::Signature,
) -> TransactionValidity {
    if validate_vote::<T>(&voting_session, &validator.account_id).is_err() {
        return InvalidTransaction::Custom(APPROVE_VOTE_IS_NOT_VALID).into();
    }

    let voting_session_data = voting_session.state();
    if voting_session_data.is_err() {
        return InvalidTransaction::Custom(VOTING_SESSION_DATA_IS_NOT_FOUND).into();
    }

    let voting_session_id =
        voting_session_data.expect("voting session data is ok").voting_session_id;

    if !AVN::<T>::signature_is_valid(
        &(voting_session.cast_vote_context(), &voting_session_id, APPROVE_VOTE),
        &validator,
        signature,
    ) {
        return InvalidTransaction::BadProof.into();
    };

    return ValidTransaction::with_tag_prefix("vote")
        .priority(TransactionPriority::max_value())
        .and_provides(vec![(
            voting_session.cast_vote_context(),
            voting_session_id,
            APPROVE_VOTE,
            validator,
        )
            .encode()])
        .longevity(64_u64)
        .propagate(true)
        .build();
}

pub fn reject_vote_validate_unsigned<T: Config>(
    voting_session: &Box<dyn VotingSessionManager<T::AccountId, BlockNumberFor<T>>>,
    validator: &Validator<T::AuthorityId, T::AccountId>,
    signature: &<T::AuthorityId as RuntimeAppPublic>::Signature,
) -> TransactionValidity {
    // TODO: Check if we can end the vote here
    if validate_vote::<T>(voting_session, &validator.account_id).is_err() {
        return InvalidTransaction::Custom(REJECT_VOTE_IS_NOT_VALID).into();
    }

    let voting_session_data = voting_session.state();
    if voting_session_data.is_err() {
        return InvalidTransaction::Custom(VOTING_SESSION_DATA_IS_NOT_FOUND).into();
    }

    let voting_session_id =
        voting_session_data.expect("voting session data is ok").voting_session_id;

    if !AVN::<T>::signature_is_valid(
        &(voting_session.cast_vote_context(), &voting_session_id, REJECT_VOTE),
        &validator,
        signature,
    ) {
        return InvalidTransaction::BadProof.into();
    };

    return ValidTransaction::with_tag_prefix("vote")
        .priority(TransactionPriority::max_value())
        .and_provides(vec![(
            voting_session.cast_vote_context(),
            voting_session_id,
            REJECT_VOTE,
            validator,
        )
            .encode()])
        .longevity(64_u64)
        .propagate(true)
        .build();
}
