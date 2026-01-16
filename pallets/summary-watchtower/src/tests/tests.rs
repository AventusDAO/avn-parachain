#![cfg(test)]

use crate::{mock::*, *};
use codec::Encode;
use frame_support::{assert_err, assert_ok, BoundedVec};
use pallet_watchtower::{Payload, Proposal};
use sp_core::H256;
use sp_runtime::Perbill;
use sp_watchtower::{DecisionRule, ProposalSource, ProposalType};

fn make_proposal(created_at: u64, root_id: RootId<u64>, root_hash: H256) -> Proposal<TestRuntime> {
    let payload_bytes = (root_id, root_hash).encode();

    Proposal {
        title: BoundedVec::try_from(b"Title".to_vec()).unwrap(),
        payload: Payload::Inline(BoundedVec::try_from(payload_bytes).unwrap()),
        threshold: Perbill::from_percent(50),
        source: ProposalSource::Internal(ProposalType::Summary),
        decision_rule: DecisionRule::SimpleMajority,
        external_ref: H256::repeat_byte(0xaa),
        proposer: None,
        created_at,
        vote_duration: 10,
        end_at: Some(100),
    }
}

mod process_new_proposal {
    use super::*;

    #[test]
    fn works() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            System::set_block_number(10);

            let range = RootRange::new(5u64, 8u64);
            let root_id = RootId::new(range, 1u64);
            let root_hash = H256::repeat_byte(0x42);
            let payload_bytes = (root_id, root_hash).encode();

            let title: BoundedVec<_, <TestRuntime as pallet_watchtower::Config>::MaxTitleLen> =
                BoundedVec::try_from(b"Summary root proposal".to_vec()).unwrap();
            let inline_payload: BoundedVec<
                _,
                <TestRuntime as pallet_watchtower::Config>::MaxInlineLen,
            > = BoundedVec::try_from(payload_bytes).unwrap();

            let proposal = Proposal {
                title,
                payload: Payload::Inline(inline_payload),
                threshold: Perbill::from_percent(60),
                source: ProposalSource::Internal(ProposalType::Summary),
                decision_rule: DecisionRule::SimpleMajority,
                external_ref: H256::repeat_byte(0xaa),
                proposer: None,
                created_at: 5u64,
                vote_duration: 10,
                end_at: None,
            };

            let proposal_id = H256::repeat_byte(0x11);
            assert_ok!(SummaryWatchtower::on_proposal_submitted(proposal_id, proposal));

            let expected_root = RootData { root_id, root_hash };
            assert_eq!(RootInfo::<TestRuntime>::get(), Some((proposal_id, expected_root)));

            System::assert_last_event(RuntimeEvent::SummaryWatchtower(
                Event::SummaryVerificationRequested { proposal_id, root_data: expected_root },
            ));
        });
    }

    #[test]
    fn handles_aborting() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            System::set_block_number(10);

            let range = RootRange::new(5u64, 8u64);
            let root_id = RootId::new(range, 1u64);
            let root_hash = H256::repeat_byte(0x42);
            let mut proposal = make_proposal(5u64, root_id, root_hash);

            let proposal_id = H256::repeat_byte(0x11);
            assert_ok!(SummaryWatchtower::on_proposal_submitted(proposal_id, proposal.clone()));

            let expected_root = RootData { root_id, root_hash };
            assert_eq!(RootInfo::<TestRuntime>::get(), Some((proposal_id, expected_root)));

            // Now add another proposal before this one if finalised
            proposal.external_ref = H256::repeat_byte(0xbb);
            let new_proposal_id = H256::repeat_byte(0x22);
            assert_ok!(SummaryWatchtower::on_proposal_submitted(new_proposal_id, proposal));

            System::assert_has_event(RuntimeEvent::SummaryWatchtower(
                Event::ProposalValidationReplaced {
                    aborted_proposal_id: proposal_id,
                    new_proposal_id,
                },
            ));

            System::assert_last_event(RuntimeEvent::SummaryWatchtower(
                Event::SummaryVerificationRequested {
                    proposal_id: new_proposal_id,
                    root_data: expected_root,
                },
            ));
        });
    }

    #[test]
    fn handles_external_source() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let current_block = 10u64;
            System::set_block_number(current_block);

            let range = RootRange::new(5u64, 8u64);
            let root_id = RootId::new(range, 1u64);
            let root_hash = H256::repeat_byte(0x42);
            let mut proposal = make_proposal(5u64, root_id, root_hash);
            proposal.source = ProposalSource::External;

            let proposal_id = H256::repeat_byte(0x22);
            assert_ok!(SummaryWatchtower::on_proposal_submitted(proposal_id, proposal));

            // External proposals are ignored
            assert_eq!(RootInfo::<TestRuntime>::get(), None);
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn range_end_not_finalised() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let current_block = 10u64;
                System::set_block_number(current_block);

                let bad_to_range = current_block + 1;

                let range = RootRange::new(8u64, bad_to_range);
                let root_id = RootId::new(range, 1u64);
                let root_hash = H256::repeat_byte(0x01);

                let proposal = make_proposal(5u64, root_id, root_hash);

                let proposal_id = H256::repeat_byte(0x22);
                assert_err!(
                    SummaryWatchtower::on_proposal_submitted(proposal_id, proposal),
                    Error::<TestRuntime>::InvalidSummaryProposal
                );
            });
        }

        #[test]
        fn from_less_than_to() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let current_block = 10u64;
                System::set_block_number(current_block);

                let range = RootRange::new(current_block - 1, current_block - 2);
                let root_id = RootId::new(range, 1u64);
                let root_hash = H256::repeat_byte(0x01);

                let proposal = make_proposal(5u64, root_id, root_hash);

                let proposal_id = H256::repeat_byte(0x22);
                assert_err!(
                    SummaryWatchtower::on_proposal_submitted(proposal_id, proposal),
                    Error::<TestRuntime>::InvalidSummaryProposal
                );
            });
        }

        #[test]
        fn payload_not_inline() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let current_block = 10u64;
                System::set_block_number(current_block);

                let range = RootRange::new(5u64, 8u64);
                let root_id = RootId::new(range, 1u64);
                let root_hash = H256::repeat_byte(0x42);

                let mut proposal = make_proposal(5u64, root_id, root_hash);
                proposal.payload =
                    Payload::Uri(BoundedVec::try_from(b"Bad payload".to_vec()).unwrap());
                let proposal_id = H256::repeat_byte(0x22);
                assert_err!(
                    SummaryWatchtower::on_proposal_submitted(proposal_id, proposal),
                    Error::<TestRuntime>::ExternalPayloadNotSupported
                );
            });
        }
    }
}

mod voting_tracking_tests {
    use super::*;

    #[test]
    fn vote_in_progress_tracks_deadline() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let proposal_id = H256::repeat_byte(0x33);
            let watchtower = watchtower_1();
            let submission_block: u64 = 5;

            assert_ok!(SummaryWatchtower::record_vote_submission(
                submission_block,
                proposal_id,
                watchtower.clone()
            ));

            assert_eq!(
                SummaryWatchtower::vote_in_progress(
                    proposal_id,
                    watchtower.clone(),
                    submission_block
                ),
                true
            );

            let deadline_block = submission_block + crate::BLOCK_INCLUSION_PERIOD as u64;
            assert_eq!(
                SummaryWatchtower::vote_in_progress(
                    proposal_id,
                    watchtower.clone(),
                    deadline_block
                ),
                true
            );

            assert_eq!(
                SummaryWatchtower::vote_in_progress(
                    proposal_id,
                    watchtower.clone(),
                    deadline_block + 1
                ),
                false
            );
        });
    }
}

mod on_voting {
    use super::*;

    #[test]
    fn completed_clears_vote_tracking() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            System::set_block_number(10);

            let range = RootRange::new(5u64, 8u64);
            let root_id = RootId::new(range, 1u64);
            let root_hash = H256::repeat_byte(0x42);

            let proposal = make_proposal(5u64, root_id, root_hash);

            let proposal_id = H256::repeat_byte(0x11);
            assert_ok!(SummaryWatchtower::on_proposal_submitted(proposal_id, proposal.clone()));

            let expected_root = RootData { root_id, root_hash };
            assert_eq!(RootInfo::<TestRuntime>::get(), Some((proposal_id, expected_root)));

            // Call on_voting_completed and ensure tracking is cleared
            SummaryWatchtower::on_voting_completed(
                proposal_id,
                &proposal.external_ref,
                &ProposalStatusEnum::Expired,
            );
            // assert its cleared
            assert_eq!(RootInfo::<TestRuntime>::get(), None);
        });
    }

    #[test]
    fn cancel_clears_vote_tracking() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            System::set_block_number(10);

            let range = RootRange::new(5u64, 8u64);
            let root_id = RootId::new(range, 1u64);
            let root_hash = H256::repeat_byte(0x42);

            let proposal = make_proposal(5u64, root_id, root_hash);

            let proposal_id = H256::repeat_byte(0x11);
            assert_ok!(SummaryWatchtower::on_proposal_submitted(proposal_id, proposal.clone()));

            let expected_root = RootData { root_id, root_hash };
            assert_eq!(RootInfo::<TestRuntime>::get(), Some((proposal_id, expected_root)));

            // Call cancelled and ensure tracking is cleared
            SummaryWatchtower::on_cancelled(proposal_id, &proposal.external_ref);
            // assert its cleared
            assert_eq!(RootInfo::<TestRuntime>::get(), None);
        });
    }
}

#[test]
fn ocw_response_validation_works() {
    ExtBuilder::build_default().as_externality().execute_with(|| {
        // Test valid hex response
        let valid_response =
            b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_vec();
        let result = SummaryWatchtower::validate_response(valid_response);
        assert!(result.is_ok());

        // Test invalid length response
        let invalid_length_response = b"0123456789abcdef".to_vec();
        let result = SummaryWatchtower::validate_response(invalid_length_response);
        assert!(result.is_err());

        // Test invalid hex response
        let invalid_hex_response =
            b"gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg".to_vec();
        let result = SummaryWatchtower::validate_response(invalid_hex_response);
        assert!(result.is_err());

        // Test non-UTF8 response
        let non_utf8_response = vec![0xFF; 64];
        let result = SummaryWatchtower::validate_response(non_utf8_response);
        assert!(result.is_err());
    });
}
