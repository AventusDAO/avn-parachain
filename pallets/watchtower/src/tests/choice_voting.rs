// Copyright 2026 Aventus DAO.

//! `Choice` proposals: every voter names an answer, the first answer reaching the threshold
//! wins.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_core::Pair;
use sp_runtime::{
    traits::ValidateUnsigned,
    transaction_validity::{InvalidTransaction, TransactionSource},
};

const A: H256 = H256::repeat_byte(0xaa);
const B: H256 = H256::repeat_byte(0xbb);

/// The ten mock watchtowers, in index order.
fn watchtowers() -> Vec<AccountId> {
    default_watchtowers()
}

fn signer_for(account: &AccountId) -> TestAccount {
    if get_default_voter().account_id() == *account {
        return get_default_voter()
    }
    for i in 2..=10u8 {
        if TestAccount::new([10 + i; 32]).account_id() == *account {
            return TestAccount::new([i; 32])
        }
    }
    panic!("not a mock watchtower: {:?}", account);
}

fn choice_call(
    proposal_id: ProposalId,
    voter: &AccountId,
    choice: H256,
) -> crate::Call<TestRuntime> {
    let payload = Watchtower::unsigned_vote_signing_payload(
        proposal_id,
        &VoteChoice::Choice(choice),
        &TEST_COUNTRY,
        voter,
    );
    let signature: Signature = signer_for(voter).key_pair().sign(&payload).into();
    crate::Call::unsigned_vote_choice {
        proposal_id,
        choice,
        country: TEST_COUNTRY,
        watchtower: *voter,
        signature: signature.into(),
    }
}

fn vote_choice(
    voter: &AccountId,
    proposal_id: ProposalId,
    choice: H256,
) -> DispatchResultWithPostInfo {
    match choice_call(proposal_id, voter, choice) {
        crate::Call::unsigned_vote_choice {
            proposal_id,
            choice,
            country,
            watchtower,
            signature,
        } => Watchtower::unsigned_vote_choice(
            RawOrigin::None.into(),
            proposal_id,
            choice,
            country,
            watchtower,
            signature,
        ),
        _ => unreachable!(),
    }
}

fn validate(call: &crate::Call<TestRuntime>) -> Result<(), InvalidTransaction> {
    <Watchtower as ValidateUnsigned>::validate_unsigned(TransactionSource::External, call)
        .map(|_| ())
        .map_err(|e| match e {
            sp_runtime::transaction_validity::TransactionValidityError::Invalid(i) => i,
            other => panic!("unexpected validity error {:?}", other),
        })
}

/// Submits and activates an internal proposal of the given kind and decision rule. The default
/// context has threshold 50%, no committee: 5 of the 10 watchtowers decide.
fn active_proposal(voting: VotingKind, decision_rule: DecisionRule) -> ProposalId {
    let context = Context { voting, decision_rule: Some(decision_rule), ..Default::default() };
    assert_ok!(Watchtower::submit_proposal(None, context.build_internal_request(b"x".to_vec())));
    activate_head()
}

fn status(id: ProposalId) -> ProposalStatusEnum {
    ProposalStatus::<TestRuntime>::get(id)
}

mod kinds {
    use super::*;

    #[test]
    fn binary_votes_are_rejected_on_choice_proposals() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();
            assert_eq!(VotingKindOf::<TestRuntime>::get(id), VotingKind::Choice);

            assert_noop!(
                Watchtower::vote(RawOrigin::Signed(wt[0]).into(), id, true),
                Error::<TestRuntime>::WrongVoteKind
            );
            let payload = Watchtower::unsigned_vote_signing_payload(
                id,
                &VoteChoice::Binary(true),
                &TEST_COUNTRY,
                &wt[0],
            );
            let signature: Signature = signer_for(&wt[0]).key_pair().sign(&payload).into();
            let call = crate::Call::unsigned_vote {
                proposal_id: id,
                in_favor: true,
                country: TEST_COUNTRY,
                watchtower: wt[0],
                signature: signature.into(),
            };
            assert_eq!(validate(&call), Err(InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID)));
            assert_noop!(
                Watchtower::unsigned_vote(
                    RawOrigin::None.into(),
                    id,
                    true,
                    TEST_COUNTRY,
                    wt[0],
                    match call {
                        crate::Call::unsigned_vote { signature, .. } => signature,
                        _ => unreachable!(),
                    }
                ),
                Error::<TestRuntime>::WrongVoteKind
            );
        });
    }

    #[test]
    fn choice_votes_are_rejected_on_binary_proposals() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Binary, DecisionRule::ExpireUnresolved);
            let wt = watchtowers();
            assert!(!VotingKindOf::<TestRuntime>::contains_key(id), "binary is the default");

            assert_eq!(
                validate(&choice_call(id, &wt[0], A)),
                Err(InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID))
            );
            assert_noop!(vote_choice(&wt[0], id, A), Error::<TestRuntime>::WrongVoteKind);
            // Yes/no still works.
            assert_ok!(Watchtower::vote(RawOrigin::Signed(wt[0]).into(), id, true));
        });
    }
}

mod tally {
    use super::*;

    #[test]
    fn first_answer_to_reach_the_threshold_wins() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            reset_hook_state();
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();

            // A, B, A, B, A, A: A reaches 4, not yet 5.
            for (i, choice) in [A, B, A, B, A, A].iter().enumerate() {
                assert_ok!(validate(&choice_call(id, &wt[i], *choice)));
                assert_ok!(vote_choice(&wt[i], id, *choice));
                assert!(Voters::<TestRuntime>::contains_key(id, wt[i]));
                System::assert_last_event(
                    Event::ChoiceVoteSubmitted {
                        voter: wt[i],
                        proposal_id: id,
                        choice: *choice,
                        country: Some(TEST_COUNTRY),
                    }
                    .into(),
                );
            }
            assert_eq!(Watchtower::choice_tally(id, A), 4);
            assert_eq!(Watchtower::choice_tally(id, B), 2);
            assert_eq!(status(id), ProposalStatusEnum::Active);
            assert_eq!(Watchtower::winning_choice(id), None);

            // The fifth A decides it.
            assert_ok!(vote_choice(&wt[6], id, A));
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: true });
            assert_eq!(Watchtower::winning_choice(id), Some(A));
            assert_eq!(
                completed_votes(),
                vec![(id, ProposalStatusEnum::Resolved { passed: true })]
            );
            System::assert_has_event(
                Event::VotingEnded {
                    proposal_id: id,
                    external_ref: Context::default().external_ref,
                    consensus_result: ProposalStatusEnum::Resolved { passed: true },
                    winning_choice: Some(A),
                }
                .into(),
            );

            // Late votes are refused.
            assert_noop!(
                vote_choice(&wt[7], id, A),
                Error::<TestRuntime>::InvalidProposalForUnsignedVote
            );
        });
    }

    #[test]
    fn a_lone_first_answer_cannot_block_the_majority() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();

            // A malicious first voter names B; five honest voters name A.
            assert_ok!(vote_choice(&wt[0], id, B));
            for i in 1..=5 {
                assert_ok!(vote_choice(&wt[i], id, A));
            }
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: true });
            assert_eq!(Watchtower::winning_choice(id), Some(A));
        });
    }

    #[test]
    fn fails_early_once_no_answer_can_reach_the_threshold() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();

            // Needed 5 of 10. After six distinct answers the leader has 1 and 4 voters remain:
            // still possible. After the seventh, 1 + 3 < 5: hopeless.
            for i in 0..6 {
                assert_ok!(vote_choice(&wt[i], id, H256::repeat_byte(i as u8 + 1)));
                assert_eq!(status(id), ProposalStatusEnum::Active, "vote {}", i);
            }
            assert_ok!(vote_choice(&wt[6], id, H256::repeat_byte(7)));
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: false });
            assert_eq!(Watchtower::winning_choice(id), None);
        });
    }

    #[test]
    fn standing_tracks_the_leader_ties_and_votes_cast() {
        let mut standing = ChoiceStanding::default();
        standing.record(A, 1, 1);
        assert_eq!(
            standing,
            ChoiceStanding { cast: 1, leader: Some(A), leader_tally: 1, tied: false }
        );

        // B draws level: A stays leader, the tie is flagged.
        standing.record(B, 1, 1);
        assert_eq!(
            standing,
            ChoiceStanding { cast: 2, leader: Some(A), leader_tally: 1, tied: true }
        );

        // B pulls ahead.
        standing.record(B, 2, 1);
        assert_eq!(
            standing,
            ChoiceStanding { cast: 3, leader: Some(B), leader_tally: 2, tied: false }
        );

        // A third answer below the leader changes only the count.
        standing.record(H256::repeat_byte(0xcc), 1, 1);
        assert_eq!(
            standing,
            ChoiceStanding { cast: 4, leader: Some(B), leader_tally: 2, tied: false }
        );

        // A draws level again, then the leader breaks the tie.
        standing.record(A, 2, 1);
        assert!(standing.tied);
        standing.record(B, 3, 1);
        assert_eq!(
            standing,
            ChoiceStanding { cast: 6, leader: Some(B), leader_tally: 3, tied: false }
        );
    }

    #[test]
    fn standing_matches_the_tallies() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();
            let c = H256::repeat_byte(0xcc);

            for (i, choice) in [A, B, c, B, A].iter().enumerate() {
                assert_ok!(vote_choice(&wt[i], id, *choice));
            }
            // A and B both have 2; B got there first, so it leads and the tie is flagged.
            assert_eq!(
                ChoiceStandings::<TestRuntime>::get(id),
                ChoiceStanding { cast: 5, leader: Some(B), leader_tally: 2, tied: true }
            );
            assert_eq!(Watchtower::choice_tally(id, A), 2);
            assert_eq!(Watchtower::choice_tally(id, B), 2);
            assert_eq!(status(id), ProposalStatusEnum::Active);
        });
    }

    #[test]
    fn committee_size_is_the_denominator() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let context = Context {
                voting: VotingKind::Choice,
                decision_rule: Some(DecisionRule::RejectOnExpiry),
                committee_size: Some(4),
                ..Default::default()
            };
            assert_ok!(Watchtower::submit_proposal(
                None,
                context.build_internal_request(b"x".to_vec())
            ));
            let id = activate_head();
            let members: Vec<AccountId> = watchtowers()
                .into_iter()
                .filter(|w| Watchtower::is_committee_member(id, w))
                .collect();
            assert_eq!(members.len(), 4);

            // 50% of 4 is 2.
            assert_ok!(vote_choice(&members[0], id, A));
            assert_eq!(status(id), ProposalStatusEnum::Active);
            assert_ok!(vote_choice(&members[1], id, A));
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: true });
        });
    }
}

mod expiry {
    use super::*;

    /// Ends the voting period and finalises through the public call, so the outcome is still
    /// in storage (`on_idle` would clean it up in the same block).
    fn expire(id: ProposalId) {
        let end = Proposals::<TestRuntime>::get(id).unwrap().end_at.unwrap();
        System::set_block_number(end);
        assert_ok!(Watchtower::finalise_proposal(RawOrigin::Signed(watchtowers()[0]).into(), id));
        assert_ne!(status(id), ProposalStatusEnum::Active);
    }

    #[test]
    fn reject_on_expiry_fails() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();
            assert_ok!(vote_choice(&wt[0], id, A));
            assert_ok!(vote_choice(&wt[1], id, A));

            expire(id);
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: false });
            assert_eq!(Watchtower::winning_choice(id), None);
        });
    }

    #[test]
    fn expire_unresolved_expires() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::ExpireUnresolved);
            let wt = watchtowers();
            assert_ok!(vote_choice(&wt[0], id, A));

            expire(id);
            assert_eq!(status(id), ProposalStatusEnum::Expired);
        });
    }

    #[test]
    fn simple_majority_picks_the_plurality_and_fails_ties() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::SimpleMajorityOnExpiry);
            let wt = watchtowers();
            assert_ok!(vote_choice(&wt[0], id, A));
            assert_ok!(vote_choice(&wt[1], id, A));
            assert_ok!(vote_choice(&wt[2], id, B));
            expire(id);
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: true });
            assert_eq!(Watchtower::winning_choice(id), Some(A));
            System::assert_has_event(
                Event::VotingEnded {
                    proposal_id: id,
                    external_ref: Context::default().external_ref,
                    consensus_result: ProposalStatusEnum::Resolved { passed: true },
                    winning_choice: Some(A),
                }
                .into(),
            );
        });

        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::SimpleMajorityOnExpiry);
            let wt = watchtowers();
            assert_ok!(vote_choice(&wt[0], id, A));
            assert_ok!(vote_choice(&wt[1], id, B));
            expire(id);
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: false });
            assert_eq!(Watchtower::winning_choice(id), None);
        });
    }
}

mod cleanup {
    use super::*;

    #[test]
    fn tallies_and_outcome_are_removed_with_the_proposal() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();
            assert_ok!(vote_choice(&wt[0], id, B));
            for i in 1..=5 {
                assert_ok!(vote_choice(&wt[i], id, A));
            }
            assert!(ProposalsToRemove::<TestRuntime>::contains_key(id));

            roll_forward(3);

            assert!(!Proposals::<TestRuntime>::contains_key(id));
            assert_eq!(ChoiceTally::<TestRuntime>::iter_prefix(id).count(), 0);
            assert!(!ChoiceStandings::<TestRuntime>::contains_key(id));
            assert!(!VotingKindOf::<TestRuntime>::contains_key(id));
            assert!(!WinningChoice::<TestRuntime>::contains_key(id));
            assert_eq!(Voters::<TestRuntime>::iter_prefix(id).count(), 0);
            System::assert_has_event(Event::ProposalCleaned { proposal_id: id }.into());
        });
    }
}

mod filter {
    use super::*;

    #[test]
    fn filter_sees_the_answer_and_can_reject_it() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            reset_hook_state();
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();

            assert_ok!(vote_choice(&wt[0], id, A));
            assert!(filtered_votes().contains(&(id, wt[0], VoteChoice::Choice(A))));

            reject_choice_vote(wt[1], B);
            assert_eq!(
                validate(&choice_call(id, &wt[1], B)),
                Err(InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID))
            );
            assert_noop!(vote_choice(&wt[1], id, B), VOTE_FILTER_REJECTED);
            // The same voter may still name the answer the filter accepts.
            assert_ok!(vote_choice(&wt[1], id, A));
        });
    }

    #[test]
    fn bad_signatures_never_reach_the_pool() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let id = active_proposal(VotingKind::Choice, DecisionRule::RejectOnExpiry);
            let wt = watchtowers();

            // Signed for answer A, submitted for B.
            let forged = match choice_call(id, &wt[0], A) {
                crate::Call::unsigned_vote_choice { signature, .. } =>
                    crate::Call::unsigned_vote_choice {
                        proposal_id: id,
                        choice: B,
                        country: TEST_COUNTRY,
                        watchtower: wt[0],
                        signature,
                    },
                _ => unreachable!(),
            };
            assert_eq!(validate(&forged), Err(InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID)));
            assert_ok!(validate(&choice_call(id, &wt[0], A)));
        });
    }
}
