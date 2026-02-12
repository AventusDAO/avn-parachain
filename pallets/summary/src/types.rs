use crate::*;

use codec::DecodeWithMemTracking;
pub use sp_runtime::{
    traits::{AtLeast32Bit, Hash},
    Perbill, SaturatedConversion,
};

#[derive(Encode, Decode, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct RootData<AccountId> {
    pub root_hash: H256,
    pub added_by: Option<AccountId>,
    pub is_validated: bool, // This is set to true when 2/3 of validators approve it
    pub is_finalised: bool, /* This is set to true when EthEvents confirms Tier1 has received
                             * the root */
    pub tx_id: Option<EthereumId>, /* This is the TransacionId that will be used to
                                    * submit
                                    * the tx */
}

impl<AccountId> RootData<AccountId> {
    pub fn new(root_hash: H256, added_by: AccountId, transaction_id: Option<EthereumId>) -> Self {
        return RootData::<AccountId> {
            root_hash,
            added_by: Some(added_by),
            is_validated: false,
            is_finalised: false,
            tx_id: transaction_id,
        };
    }
}

impl<AccountId> Default for RootData<AccountId> {
    fn default() -> Self {
        Self {
            root_hash: H256::zero(),
            added_by: None,
            is_validated: false,
            is_finalised: false,
            tx_id: None,
        }
    }
}

#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
pub enum ExternalValidationEnum {
    Unknown,
    ValidationInProgress,
    PendingAdminReview,
    Accepted,
    Rejected,
}

impl Default for ExternalValidationEnum {
    fn default() -> Self {
        ExternalValidationEnum::Unknown
    }
}

#[derive(
    Encode, Decode, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub struct ExternalValidationData {
    pub proposal_id: ProposalId,
    pub external_ref: H256,
    pub proposal_status: ProposalStatusEnum,
}

impl ExternalValidationData {
    pub fn new(
        proposal_id: ProposalId,
        external_ref: H256,
        proposal_status: ProposalStatusEnum,
    ) -> Self {
        Self { proposal_id, external_ref, proposal_status }
    }
}

#[derive(Encode, Decode, TypeInfo, Debug, Clone, PartialEq, DecodeWithMemTracking)]
pub enum AdminConfig<BlockNumber> {
    ExternalValidationThreshold(u32),
    SchedulePeriod(BlockNumber),
    VotingPeriod(BlockNumber),
}
