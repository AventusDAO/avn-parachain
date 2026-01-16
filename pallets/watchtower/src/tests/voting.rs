//Copyright 2025 Truth Network.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_core::Pair;
pub use test_case::test_case;

fn sign_vote(voter: TestAccount, msg: &[u8]) -> Signature {
    voter.key_pair().sign(msg).into()
}

fn create_and_submit_proposal(payload: RawPayload, source: ProposalSource) -> (ProposalId, bool) {
    let context = Context::default();
    let proposal = context.build_request(payload, source.clone());
    let is_internal;
    if let ProposalSource::Internal(_) = source {
        assert_ok!(Watchtower::submit_proposal(None, proposal));
        is_internal = true;
    } else {
        assert_ok!(Watchtower::submit_external_proposal(
            RawOrigin::Signed(watchtower_owner_1()).into(),
            proposal
        ));
        is_internal = false;
    }
    (ExternalRef::<TestRuntime>::get(&context.external_ref), is_internal)
}

mod voting_on_proposals {
    use super::{test_case, *};

    #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
    #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
    fn works(payload: RawPayload, source: ProposalSource) {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_request(payload, source.clone());
            let voter;
            let vote_weight;
            if let ProposalSource::Internal(_) = source {
                assert_ok!(Watchtower::submit_proposal(None, proposal));
                vote_weight = 1;
                voter = watchtower_1();
            } else {
                assert_ok!(Watchtower::submit_external_proposal(
                    RawOrigin::Signed(watchtower_owner_1()).into(),
                    proposal
                ));

                vote_weight = <mock::TestRuntime as pallet::Config>::Watchtowers::get_watchtower_voting_weight(&watchtower_owner_1());
                voter = watchtower_owner_1();
            }

            let in_favor = true;
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert_ok!(Watchtower::vote(RawOrigin::Signed(voter).into(), proposal_id, in_favor));

            //Verify state and events
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert!(Votes::<TestRuntime>::contains_key(&proposal_id));
            assert!(Voters::<TestRuntime>::contains_key(&proposal_id, &voter));
            let votes = Votes::<TestRuntime>::get(&proposal_id);
            assert_eq!(votes.in_favors, 1 * vote_weight);
            assert_eq!(votes.againsts, 0);

            System::assert_last_event(
                Event::VoteSubmitted { proposal_id, voter: voter.clone(), in_favor, vote_weight }
                    .into(),
            );
        });
    }

    #[test]
    fn works_with_unsigned() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_internal_request(b"test".to_vec());
            let in_favor = true;
            let voter = watchtower_1();
            let vote_weight = 1;
            assert_ok!(Watchtower::submit_proposal(None, proposal));

            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            let signature = sign_vote(
                get_default_voter(),
                &(WATCHTOWER_UNSIGNED_VOTE_CONTEXT, proposal_id, in_favor, &voter).encode(),
            );
            assert_ok!(Watchtower::unsigned_vote(
                RawOrigin::None.into(),
                proposal_id,
                in_favor,
                voter.clone(),
                signature.into()
            ));

            //Verify state and events
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert!(Votes::<TestRuntime>::contains_key(&proposal_id));
            assert!(Voters::<TestRuntime>::contains_key(&proposal_id, &voter));
            let votes = Votes::<TestRuntime>::get(&proposal_id);
            assert_eq!(votes.in_favors, 1 * vote_weight);
            assert_eq!(votes.againsts, 0);

            System::assert_last_event(
                Event::VoteSubmitted { proposal_id, voter: voter.clone(), in_favor, vote_weight }
                    .into(),
            );
        });
    }

    mod fails_when {
        use super::{test_case, *};

        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
        fn voter_has_already_voted(payload: RawPayload, source: ProposalSource) {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (proposal_id, is_internal) =
                    create_and_submit_proposal(payload, source.clone());
                let voter = if is_internal { watchtower_1() } else { watchtower_owner_1() };

                let in_favor = true;

                assert_ok!(Watchtower::vote(
                    RawOrigin::Signed(voter.clone()).into(),
                    proposal_id,
                    in_favor
                ));
                // Vote again
                assert_noop!(
                    Watchtower::vote(RawOrigin::Signed(voter).into(), proposal_id, in_favor),
                    Error::<TestRuntime>::AlreadyVoted
                );
            });
        }

        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External, ProposalStatusEnum::Queued; "external_queued")]
        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External, ProposalStatusEnum::Resolved { passed: true }; "external_resolved_passed")]
        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External, ProposalStatusEnum::Resolved { passed: false }; "external_resolved_failed")]
        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External, ProposalStatusEnum::Cancelled; "external_cancelled")]
        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External, ProposalStatusEnum::Expired; "external_expired")]
        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External, ProposalStatusEnum::Unknown; "external_unknown")]
        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary), ProposalStatusEnum::Queued; "internal_queued")]
        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary), ProposalStatusEnum::Resolved { passed: true }; "internal_resolved_passed")]
        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary), ProposalStatusEnum::Resolved { passed: false }; "internal_resolved_failed")]
        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary), ProposalStatusEnum::Cancelled; "internal_cancelled")]
        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary), ProposalStatusEnum::Expired; "internal_expired")]
        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary), ProposalStatusEnum::Unknown; "internal_unknown")]
        fn invalid_proposal_state(
            payload: RawPayload,
            source: ProposalSource,
            state: ProposalStatusEnum,
        ) {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (proposal_id, is_internal) =
                    create_and_submit_proposal(payload, source.clone());
                let voter = if is_internal { watchtower_1() } else { watchtower_owner_1() };
                let in_favor = true;

                // Set status
                ProposalStatus::<TestRuntime>::insert(proposal_id, state);

                assert_noop!(
                    Watchtower::vote(RawOrigin::Signed(voter).into(), proposal_id, in_favor),
                    Error::<TestRuntime>::ProposalNotActive
                );
            });
        }

        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
        fn unauthorised_voter(payload: RawPayload, source: ProposalSource) {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (proposal_id, _) = create_and_submit_proposal(payload, source.clone());

                let random_user = random_user();
                let in_favor = true;

                assert_noop!(
                    Watchtower::vote(RawOrigin::Signed(random_user).into(), proposal_id, in_favor),
                    Error::<TestRuntime>::UnauthorizedVoter
                );
            });
        }

        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
        fn nodes_vote_on_external_proposals(payload: RawPayload, source: ProposalSource) {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (proposal_id, _) = create_and_submit_proposal(payload, source.clone());
                let voter = watchtower_1(); // A node account

                let in_favor = true;

                assert_noop!(
                    Watchtower::vote(RawOrigin::Signed(voter).into(), proposal_id, in_favor),
                    Error::<TestRuntime>::UnauthorizedVoter
                );
            });
        }

        #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
        fn node_owner_vote_on_internal_proposals(payload: RawPayload, source: ProposalSource) {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (proposal_id, _) = create_and_submit_proposal(payload, source.clone());
                let voter = watchtower_owner_1(); // A node owner account

                let in_favor = true;

                assert_noop!(
                    Watchtower::vote(RawOrigin::Signed(voter).into(), proposal_id, in_favor),
                    Error::<TestRuntime>::UnauthorizedVoter
                );
            });
        }
    }
}

mod proposal_lifecycle {
    use super::*;

    #[test]
    fn consensus_can_be_reached_for_internal_proposals() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_internal_request(b"test".to_vec());
            assert_ok!(Watchtower::submit_proposal(None, proposal));
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);

            // 1st vote - in favor
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_1()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_2()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_3()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_4()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_5()).into(),
                proposal_id,
                false
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_6()).into(),
                proposal_id,
                true
            ));

            // Verify state and events
            let votes = Votes::<TestRuntime>::get(&proposal_id);
            assert_eq!(votes.in_favors, 5);
            assert_eq!(votes.againsts, 1);

            let expected_status = ProposalStatusEnum::Resolved { passed: true };
            System::assert_last_event(
                Event::VotingEnded {
                    proposal_id,
                    external_ref: context.external_ref,
                    consensus_result: expected_status.clone(),
                }
                .into(),
            );
            assert_eq!(ProposalStatus::<TestRuntime>::get(proposal_id), expected_status);
        });
    }

    #[test]
    fn consensus_can_be_reached_for_external_proposals() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_external_request(b"test".to_vec());
            assert_ok!(Watchtower::submit_proposal(None, proposal));
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);

            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_owner_3()).into(),
                proposal_id,
                false
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_owner_1()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_owner_2()).into(),
                proposal_id,
                false
            ));

            let owner_1_weight =
                <mock::TestRuntime as pallet::Config>::Watchtowers::get_watchtower_voting_weight(
                    &watchtower_owner_1(),
                );
            let owner_2_weight =
                <mock::TestRuntime as pallet::Config>::Watchtowers::get_watchtower_voting_weight(
                    &watchtower_owner_2(),
                );
            let owner_3_weight =
                <mock::TestRuntime as pallet::Config>::Watchtowers::get_watchtower_voting_weight(
                    &watchtower_owner_3(),
                );

            // Verify state and events
            let votes = Votes::<TestRuntime>::get(&proposal_id);
            assert_eq!(votes.in_favors, owner_1_weight);
            assert_eq!(votes.againsts, owner_2_weight + owner_3_weight);

            let expected_status = ProposalStatusEnum::Resolved { passed: false };
            System::assert_last_event(
                Event::VotingEnded {
                    proposal_id,
                    external_ref: context.external_ref,
                    consensus_result: expected_status.clone(),
                }
                .into(),
            );
            assert_eq!(ProposalStatus::<TestRuntime>::get(proposal_id), expected_status);
        });
    }

    #[test]
    fn threshold_is_respected() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let mut context = Context::default();
            context.threshold = Perbill::from_percent(80); // Set high threshold
            let proposal = context.build_internal_request(b"test".to_vec());
            assert_ok!(Watchtower::submit_proposal(None, proposal));
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);

            // 1st vote - in favor
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_1()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_2()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_3()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_4()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_5()).into(),
                proposal_id,
                false
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_6()).into(),
                proposal_id,
                true
            ));
            // Additional votes are needed because the threshold is higher than 50%
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_7()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_8()).into(),
                proposal_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_9()).into(),
                proposal_id,
                true
            ));

            // Verify state and events
            let votes = Votes::<TestRuntime>::get(&proposal_id);
            assert_eq!(votes.in_favors, 8);
            assert_eq!(votes.againsts, 1);

            let expected_status = ProposalStatusEnum::Resolved { passed: true };
            System::assert_last_event(
                Event::VotingEnded {
                    proposal_id,
                    external_ref: context.external_ref,
                    consensus_result: expected_status.clone(),
                }
                .into(),
            );
            assert_eq!(ProposalStatus::<TestRuntime>::get(proposal_id), expected_status);
        });
    }

    #[test]
    fn expired_internal_proposals_are_removed_automatically() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_internal_request(b"test".to_vec());
            assert_ok!(Watchtower::submit_proposal(None, proposal));
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);

            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_1()).into(),
                proposal_id,
                true
            ));
            // Proposal is still active
            assert_eq!(ProposalStatus::<TestRuntime>::get(proposal_id), ProposalStatusEnum::Active);

            let target_block =
                MinVotingPeriod::<TestRuntime>::get().saturated_into::<u32>() + 10u32;
            roll_forward(target_block.into());

            // Verify state and events
            assert_eq!(Votes::<TestRuntime>::contains_key(proposal_id), false);
            assert_eq!(Voters::<TestRuntime>::contains_key(proposal_id, watchtower_1()), false);
            assert_eq!(Proposals::<TestRuntime>::contains_key(proposal_id), false);

            assert_eq!(
                ProposalStatus::<TestRuntime>::get(proposal_id),
                ProposalStatusEnum::Expired
            );
            System::assert_last_event(Event::ProposalCleaned { proposal_id }.into());
        });
    }

    #[test]
    fn votes_can_finalise_expired_proposals() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_external_request(b"test".to_vec());
            assert_ok!(Watchtower::submit_proposal(None, proposal));
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);

            let target_block =
                MinVotingPeriod::<TestRuntime>::get().saturated_into::<u32>() + 10u32;
            roll_forward(target_block.into());

            // External requests are not automatically cleaned up, a vote can still finalise it
            assert_eq!(ProposalStatus::<TestRuntime>::get(proposal_id), ProposalStatusEnum::Active);

            // This should finalise the proposal
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_owner_1()).into(),
                proposal_id,
                true
            ));

            // Verify state and events
            assert_eq!(ProposalsToRemove::<TestRuntime>::contains_key(proposal_id), true);
            assert_eq!(Votes::<TestRuntime>::contains_key(proposal_id), false);
            assert_eq!(
                Voters::<TestRuntime>::contains_key(proposal_id, watchtower_owner_1()),
                false
            );
            // Proposal is still in storage
            assert_eq!(ProposalsToRemove::<TestRuntime>::contains_key(proposal_id), true);

            let expected_status = ProposalStatusEnum::Resolved { passed: false };
            System::assert_last_event(
                Event::VotingEnded {
                    proposal_id,
                    external_ref: context.external_ref,
                    consensus_result: expected_status.clone(),
                }
                .into(),
            );
            assert_eq!(ProposalStatus::<TestRuntime>::get(proposal_id), expected_status);

            // Trigger cleanup after the proposal is finalised
            roll_forward(10u32.into());

            // Proposal has been cleaned up
            assert_eq!(ProposalsToRemove::<TestRuntime>::contains_key(proposal_id), false);
            System::assert_last_event(Event::ProposalCleaned { proposal_id }.into());
        });
    }
}
