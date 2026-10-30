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

mod incoming_events {
    use super::*;
    use sp_avn_common::event_types::{AvtLowerClaimedData, EthEventId, ValidEvents};

    fn eth_event(event_type: ValidEvents, event_data: EventData) -> EthEvent {
        EthEvent {
            event_id: EthEventId {
                signature: event_type.signature(),
                transaction_hash: H256::repeat_byte(0xee),
            },
            event_data,
        }
    }

    fn lower_locked_data() -> LowerLockedData {
        LowerLockedData {
            lower_id: 7,
            t1_recipient: H160::repeat_byte(0x22),
            token_contract: H160::repeat_byte(0x11),
            amount: U256::MAX,
            t2_sender: H256::repeat_byte(0xaa),
            unlock_time: 1_700_000_000,
        }
    }

    #[test]
    fn lower_locked_event_is_detected() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let data = lower_locked_data();
            let event =
                eth_event(ValidEvents::LowerLocked, EventData::LogLowerLocked(data.clone()));

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&event));

            System::assert_last_event(RuntimeEvent::BridgeWatchtower(
                crate::Event::LowerLockDetected {
                    lower_id: data.lower_id,
                    t1_recipient: data.t1_recipient,
                    token_contract: data.token_contract,
                    amount: data.amount,
                    t2_sender: data.t2_sender,
                    unlock_time: data.unlock_time,
                    eth_tx_hash: H256::repeat_byte(0xee),
                },
            ));
        });
    }

    #[test]
    fn other_events_are_ignored() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let event = eth_event(
                ValidEvents::AvtLowerClaimed,
                EventData::LogLowerClaimed(AvtLowerClaimedData { lower_id: 7 }),
            );

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&event));

            assert!(System::events().is_empty());
        });
    }

    #[test]
    fn transaction_results_are_accepted() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_ok!(BridgeWatchtower::process_result(1, b"other_pallet".to_vec(), true));
            assert!(System::events().is_empty());
        });
    }
}
