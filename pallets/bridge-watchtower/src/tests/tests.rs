#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_ok, BoundedVec};
use pallet_watchtower::Payload;
use sp_runtime::Perbill;

fn make_proposal(source: ProposalSource) -> Proposal<TestRuntime> {
    Proposal {
        title: BoundedVec::try_from(b"Title".to_vec()).unwrap(),
        payload: Payload::Inline(BoundedVec::try_from(b"payload".to_vec()).unwrap()),
        threshold: Perbill::from_percent(50),
        source,
        decision_rule: DecisionRule::ExpireUnresolved,
        external_ref: H256::repeat_byte(0xaa),
        proposer: None,
        created_at: 1,
        vote_duration: 10,
        end_at: None,
        committee_size: None,
    }
}

#[test]
fn hooks_accept_any_proposal() {
    ExtBuilder::build_default().as_externality().execute_with(|| {
        let proposal_id = H256::repeat_byte(0x01);

        for source in [
            ProposalSource::Internal(ProposalType::Summary),
            ProposalSource::Internal(ProposalType::Other(1)),
            ProposalSource::External,
        ] {
            assert_ok!(BridgeWatchtower::on_proposal_submitted(proposal_id, make_proposal(source)));
        }

        BridgeWatchtower::on_voting_completed(
            proposal_id,
            &H256::repeat_byte(0xaa),
            &ProposalStatusEnum::Resolved { passed: true },
        );
        BridgeWatchtower::on_cancelled(proposal_id, &H256::repeat_byte(0xaa));
    });
}
