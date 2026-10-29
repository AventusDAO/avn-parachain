// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_core::Pair;
pub use test_case::test_case;

fn sign_vote(voter: TestAccount, msg: &[u8]) -> Signature {
    voter.key_pair().sign(msg).into()
}

/// Votes the way the proposal's source allows: a node's unsigned vote on an internal proposal,
/// an owner's signed vote on an external one.
fn cast_vote(
    voter: AccountId,
    proposal_id: ProposalId,
    in_favor: bool,
    is_internal: bool,
) -> DispatchResultWithPostInfo {
    if is_internal {
        node_vote(voter, proposal_id, in_favor)
    } else {
        owner_vote(voter, proposal_id, in_favor)
    }
}

fn create_and_submit_proposal(payload: RawPayload, source: ProposalSource) -> (ProposalId, bool) {
    let context = Context::default();
    let proposal = context.build_request(payload, source.clone());
    let is_internal;
    if let ProposalSource::Internal(_) = source {
        assert_ok!(Watchtower::submit_proposal(None, proposal));
        activate_head();
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
            let country;
            if let ProposalSource::Internal(_) = source {
                assert_ok!(Watchtower::submit_proposal(None, proposal));
                activate_head();
                vote_weight = 1;
                voter = watchtower_1();
                country = Some(TEST_COUNTRY);
            } else {
                assert_ok!(Watchtower::submit_external_proposal(
                    RawOrigin::Signed(watchtower_owner_1()).into(),
                    proposal
                ));

                vote_weight = <mock::TestRuntime as pallet::Config>::Watchtowers::get_watchtower_voting_weight(&watchtower_owner_1());
                voter = watchtower_owner_1();
                country = None;
            }

            let in_favor = true;
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert_ok!(cast_vote(voter, proposal_id, in_favor, country.is_some()));

            //Verify state and events
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert!(Votes::<TestRuntime>::contains_key(&proposal_id));
            assert!(Voters::<TestRuntime>::contains_key(&proposal_id, &voter));
            let votes = Votes::<TestRuntime>::get(&proposal_id);
            assert_eq!(votes.in_favors, 1 * vote_weight);
            assert_eq!(votes.againsts, 0);

            System::assert_last_event(
                Event::VoteSubmitted {
                    proposal_id,
                    voter: voter.clone(),
                    in_favor,
                    vote_weight,
                    country,
                }
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
            activate_head();

            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            let signature = sign_vote(
                get_default_voter(),
                &Watchtower::unsigned_vote_signing_payload(
                    proposal_id,
                    &VoteChoice::Binary(in_favor),
                    &TEST_COUNTRY,
                    &voter,
                ),
            );
            assert_ok!(Watchtower::unsigned_vote(
                RawOrigin::None.into(),
                proposal_id,
                in_favor,
                TEST_COUNTRY,
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
                Event::VoteSubmitted {
                    proposal_id,
                    voter: voter.clone(),
                    in_favor,
                    vote_weight,
                    country: Some(TEST_COUNTRY),
                }
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

                assert_ok!(cast_vote(voter, proposal_id, in_favor, is_internal));
                // Vote again
                assert_noop!(
                    cast_vote(voter, proposal_id, in_favor, is_internal),
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
                    cast_vote(voter, proposal_id, in_favor, is_internal),
                    Error::<TestRuntime>::ProposalNotActive
                );
            });
        }

        #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
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

        #[test]
        fn signed_votes_on_internal_proposals() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (proposal_id, _) = create_and_submit_proposal(
                    RawPayload::Inline(b"test".to_vec()),
                    ProposalSource::Internal(ProposalType::Summary),
                );

                // Nodes, owners and anyone else alike: nodes vote with `unsigned_vote` only.
                for voter in [watchtower_1(), watchtower_owner_1(), random_user()] {
                    assert_noop!(
                        owner_vote(voter, proposal_id, true),
                        Error::<TestRuntime>::SignedVoteOnInternalProposal
                    );
                }
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
            activate_head();
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);

            // 1st vote - in favor
            assert_ok!(node_vote(watchtower_1(), proposal_id, true));
            assert_ok!(node_vote(watchtower_2(), proposal_id, true));
            assert_ok!(node_vote(watchtower_3(), proposal_id, true));
            assert_ok!(node_vote(watchtower_4(), proposal_id, true));
            assert_ok!(node_vote(watchtower_5(), proposal_id, false));
            assert_ok!(node_vote(watchtower_6(), proposal_id, true));

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
                    winning_choice: None,
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
                    winning_choice: None,
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
            activate_head();
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);

            // 1st vote - in favor
            assert_ok!(node_vote(watchtower_1(), proposal_id, true));
            assert_ok!(node_vote(watchtower_2(), proposal_id, true));
            assert_ok!(node_vote(watchtower_3(), proposal_id, true));
            assert_ok!(node_vote(watchtower_4(), proposal_id, true));
            assert_ok!(node_vote(watchtower_5(), proposal_id, false));
            assert_ok!(node_vote(watchtower_6(), proposal_id, true));
            // Additional votes are needed because the threshold is higher than 50%
            assert_ok!(node_vote(watchtower_7(), proposal_id, true));
            assert_ok!(node_vote(watchtower_8(), proposal_id, true));
            assert_ok!(node_vote(watchtower_9(), proposal_id, true));

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
                    winning_choice: None,
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
            activate_head();
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);

            assert_ok!(node_vote(watchtower_1(), proposal_id, true));
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
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), None);
            System::assert_last_event(Event::ProposalCleaned { proposal_id }.into());
        });
    }

    #[test]
    fn cleanup_runs_while_an_internal_proposal_is_active() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            // An external proposal that will be finalised and need cleaning up.
            let external_context = Context::default();
            let external_proposal = external_context.build_external_request(b"test".to_vec());
            assert_ok!(Watchtower::submit_proposal(None, external_proposal));
            let external_id = ExternalRef::<TestRuntime>::get(&external_context.external_ref);

            // An internal proposal that stays active for the whole test.
            let internal_context = Context {
                external_ref: H256::repeat_byte(2),
                vote_duration: Some(1_000),
                ..Context::default()
            };
            let internal_proposal = internal_context.build_internal_request(b"test".to_vec());
            assert_ok!(Watchtower::submit_proposal(None, internal_proposal));
            activate_head();
            let internal_id = ExternalRef::<TestRuntime>::get(&internal_context.external_ref);
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(internal_id));

            // Two owners with weight 3 each reach the 50% threshold of 10.
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_owner_1()).into(),
                external_id,
                true
            ));
            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_owner_2()).into(),
                external_id,
                true
            ));
            assert_eq!(
                ProposalStatus::<TestRuntime>::get(external_id),
                ProposalStatusEnum::Resolved { passed: true }
            );
            assert!(ProposalsToRemove::<TestRuntime>::contains_key(external_id));

            roll_forward(2);

            // The internal proposal is still active and not expired...
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(internal_id));
            assert_eq!(ProposalStatus::<TestRuntime>::get(internal_id), ProposalStatusEnum::Active);
            // ...and that did not stop the external proposal being cleaned up.
            assert!(!ProposalsToRemove::<TestRuntime>::contains_key(external_id));
            assert!(!Proposals::<TestRuntime>::contains_key(external_id));
            assert!(!Votes::<TestRuntime>::contains_key(external_id));
            assert!(!Voters::<TestRuntime>::contains_key(external_id, watchtower_owner_1()));
            System::assert_last_event(Event::ProposalCleaned { proposal_id: external_id }.into());
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
                    winning_choice: None,
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

mod vote_filter {
    use super::*;
    use sp_runtime::{
        traits::ValidateUnsigned,
        transaction_validity::{InvalidTransaction, TransactionSource},
    };

    fn unsigned_vote_call(proposal_id: ProposalId, in_favor: bool) -> crate::Call<TestRuntime> {
        let voter = watchtower_1();
        let signature = sign_vote(
            get_default_voter(),
            &Watchtower::unsigned_vote_signing_payload(
                proposal_id,
                &VoteChoice::Binary(in_favor),
                &TEST_COUNTRY,
                &voter,
            ),
        );
        crate::Call::unsigned_vote {
            proposal_id,
            in_favor,
            country: TEST_COUNTRY,
            watchtower: voter,
            signature: signature.into(),
        }
    }

    fn internal_proposal() -> ProposalId {
        create_and_submit_proposal(
            RawPayload::Inline(b"test".to_vec()),
            ProposalSource::Internal(ProposalType::Summary),
        )
        .0
    }

    #[test]
    fn rejected_votes_never_reach_the_pool() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            reset_hook_state();
            reject_vote(watchtower_1(), true);
            let proposal_id = internal_proposal();

            assert_eq!(
                <Watchtower as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::External,
                    &unsigned_vote_call(proposal_id, true)
                ),
                InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID).into()
            );
            // The same voter may still cast the vote the filter accepts.
            assert_ok!(<Watchtower as ValidateUnsigned>::validate_unsigned(
                TransactionSource::External,
                &unsigned_vote_call(proposal_id, false)
            ));
        });
    }

    #[test]
    fn rejected_votes_fail_on_unsigned_dispatch() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            reset_hook_state();
            reject_vote(watchtower_1(), true);
            let proposal_id = internal_proposal();

            let signature = match unsigned_vote_call(proposal_id, true) {
                crate::Call::unsigned_vote { signature, .. } => signature,
                _ => unreachable!(),
            };
            assert_noop!(
                Watchtower::unsigned_vote(
                    RawOrigin::None.into(),
                    proposal_id,
                    true,
                    TEST_COUNTRY,
                    watchtower_1(),
                    signature
                ),
                VOTE_FILTER_REJECTED
            );
            assert!(!Voters::<TestRuntime>::contains_key(proposal_id, watchtower_1()));
        });
    }

    /// A vote that was already counted is refused by the pool, so re-gossiping its signed
    /// bytes cannot get it included again.
    #[test]
    fn a_counted_vote_cannot_be_replayed() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let proposal_id = internal_proposal();
            let call = unsigned_vote_call(proposal_id, true);
            assert_ok!(<Watchtower as ValidateUnsigned>::validate_unsigned(
                TransactionSource::External,
                &call
            ));
            let signature = match &call {
                crate::Call::unsigned_vote { signature, .. } => signature.clone(),
                _ => unreachable!(),
            };
            assert_ok!(Watchtower::unsigned_vote(
                RawOrigin::None.into(),
                proposal_id,
                true,
                TEST_COUNTRY,
                watchtower_1(),
                signature
            ));
            assert!(Voters::<TestRuntime>::contains_key(proposal_id, watchtower_1()));

            assert_eq!(
                <Watchtower as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::External,
                    &call
                ),
                InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID).into()
            );
        });
    }

    /// A node may sign any two bytes as its country; events only ever show letters or `??`.
    #[test]
    fn a_junk_country_is_shown_as_unknown() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let proposal_id = internal_proposal();
            let voter = watchtower_1();
            let junk: CountryCode = [0xFF, 0x00];
            let signature = sign_vote(
                get_default_voter(),
                &Watchtower::unsigned_vote_signing_payload(
                    proposal_id,
                    &VoteChoice::Binary(true),
                    &junk,
                    &voter,
                ),
            );
            assert_ok!(Watchtower::unsigned_vote(
                RawOrigin::None.into(),
                proposal_id,
                true,
                junk,
                voter,
                signature.into()
            ));
            System::assert_has_event(
                Event::VoteSubmitted {
                    proposal_id,
                    voter,
                    in_favor: true,
                    vote_weight: 1,
                    country: Some(UNKNOWN_COUNTRY),
                }
                .into(),
            );
        });
    }

    /// The country is signed with the vote, so whoever relays it cannot change it.
    #[test]
    fn a_relayed_vote_with_another_country_is_rejected() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let proposal_id = internal_proposal();
            let tampered = match unsigned_vote_call(proposal_id, true) {
                crate::Call::unsigned_vote {
                    proposal_id, in_favor, watchtower, signature, ..
                } => crate::Call::unsigned_vote {
                    proposal_id,
                    in_favor,
                    country: *b"US",
                    watchtower,
                    signature,
                },
                _ => unreachable!(),
            };
            assert_eq!(
                <Watchtower as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::External,
                    &tampered
                ),
                InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID).into()
            );
            assert_ok!(<Watchtower as ValidateUnsigned>::validate_unsigned(
                TransactionSource::External,
                &unsigned_vote_call(proposal_id, true)
            ));
        });
    }

    #[test]
    fn external_proposals_are_not_filtered() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            reset_hook_state();
            reject_vote(watchtower_owner_1(), true);
            let (proposal_id, _) = create_and_submit_proposal(
                RawPayload::Uri(b"test".to_vec()),
                ProposalSource::External,
            );

            assert_ok!(Watchtower::vote(
                RawOrigin::Signed(watchtower_owner_1()).into(),
                proposal_id,
                true
            ));
            assert!(Voters::<TestRuntime>::contains_key(proposal_id, watchtower_owner_1()));
        });
    }
}
