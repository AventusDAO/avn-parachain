// Copyright 2026 Aventus DAO.

//! Trusted owners voting on committee proposals: they fill seats the committee left empty once
//! the trusted voting window opens, and vote at most once per proposal.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::{pallet_prelude::BlockNumberFor, RawOrigin};
use sp_core::Pair;
use sp_runtime::{
    traits::ValidateUnsigned,
    transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidity},
};

const ANSWER: H256 = H256::repeat_byte(0xaa);

/// Every node owns itself unless listed in `grouped`, as `(owner, nodes)`. Committee selection
/// also treats every node as its own owner, so the two views agree for nodes not listed.
fn set_owners(grouped: Vec<(AccountId, Vec<AccountId>)>) {
    let grouped_nodes: Vec<AccountId> =
        grouped.iter().flat_map(|(_, nodes)| nodes.clone()).collect();
    let mut owners: Vec<(AccountId, Vec<AccountId>)> = default_watchtowers()
        .into_iter()
        .filter(|node| !grouped_nodes.contains(node))
        .map(|node| (node, vec![node]))
        .collect();
    owners.extend(grouped);
    set_node_owners(owners);
}

fn trust(owners: Vec<AccountId>) {
    assert_ok!(Watchtower::add_trusted_owners(
        RawOrigin::Root.into(),
        owners.try_into().expect("within MAX_TRUSTED_OWNERS")
    ));
}

fn set_start(start: Option<Perbill>) {
    assert_ok!(Watchtower::set_admin_config(
        RawOrigin::Root.into(),
        AdminConfig::TrustedVotingStart(start)
    ));
}

/// Submits and activates an internal proposal with a committee of `size`. Returns the proposal,
/// its committee and the nodes outside it.
fn activated(
    size: u32,
    threshold: Perbill,
    voting: VotingKind,
) -> (ProposalId, Vec<AccountId>, Vec<AccountId>) {
    let context = Context {
        external_ref: H256::repeat_byte(1),
        committee_size: Some(size),
        threshold,
        voting,
        ..Context::default()
    };
    assert_ok!(Watchtower::submit_proposal(None, context.build_internal_request(vec![1])));
    let id = ExternalRef::<TestRuntime>::get(&context.external_ref);
    assert_ok!(Watchtower::activate_next_proposal(RawOrigin::None.into(), id));

    let committee: Vec<AccountId> = ProposalCommittee::<TestRuntime>::iter_key_prefix(id).collect();
    let others = default_watchtowers().into_iter().filter(|n| !committee.contains(n)).collect();
    (id, committee, others)
}

/// The first block trusted owners may vote on `id` with a start of 50%.
fn opens_at(id: ProposalId) -> BlockNumberFor<TestRuntime> {
    let proposal = Proposals::<TestRuntime>::get(id).expect("proposal");
    let duration = proposal.vote_duration;
    proposal.end_at.expect("active") - duration as u64 +
        Perbill::from_percent(50).mul_ceil(duration) as u64
}

fn open_window(id: ProposalId) {
    System::set_block_number(opens_at(id));
}

fn vote_call(id: ProposalId, voter: &AccountId, in_favor: bool) -> crate::Call<TestRuntime> {
    let payload = Watchtower::unsigned_vote_signing_payload(
        id,
        &VoteChoice::Binary(in_favor),
        &TEST_COUNTRY,
        voter,
    );
    let signature: Signature = node_signer(voter).key_pair().sign(&payload).into();
    crate::Call::unsigned_vote {
        proposal_id: id,
        in_favor,
        country: TEST_COUNTRY,
        watchtower: *voter,
        signature: signature.into(),
    }
}

fn pool(call: &crate::Call<TestRuntime>) -> TransactionValidity {
    <Watchtower as ValidateUnsigned>::validate_unsigned(TransactionSource::External, call)
}

fn choice_vote(voter: &AccountId, id: ProposalId, choice: H256) -> DispatchResultWithPostInfo {
    let payload = Watchtower::unsigned_vote_signing_payload(
        id,
        &VoteChoice::Choice(choice),
        &TEST_COUNTRY,
        voter,
    );
    let signature: Signature = node_signer(voter).key_pair().sign(&payload).into();
    Watchtower::unsigned_vote_choice(
        RawOrigin::None.into(),
        id,
        choice,
        TEST_COUNTRY,
        *voter,
        signature.into(),
    )
}

fn status(id: ProposalId) -> ProposalStatusEnum {
    ProposalStatus::<TestRuntime>::get(id)
}

/// A committee of 3 deciding at 50% (2 votes), the first outsider trusted and the window at
/// 50%. Returns the proposal, its committee and the trusted node.
fn with_trusted_outsider() -> (ProposalId, Vec<AccountId>, AccountId) {
    set_owners(vec![]);
    let (id, committee, others) = activated(3, Perbill::from_percent(50), VotingKind::Binary);
    let trusted = others[0];
    trust(vec![trusted]);
    set_start(Some(Perbill::from_percent(50)));
    (id, committee, trusted)
}

mod window {
    use super::*;

    #[test]
    fn opens_at_the_configured_share_of_the_voting_period() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, _, trusted) = with_trusted_outsider();

            System::set_block_number(opens_at(id) - 1);
            assert!(!Watchtower::can_vote(id, &trusted));
            assert_noop!(node_vote(trusted, id, true), Error::<TestRuntime>::TrustedVotingNotOpen);
            assert_eq!(
                pool(&vote_call(id, &trusted, true)),
                InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID).into()
            );

            open_window(id);
            assert!(Watchtower::can_vote(id, &trusted));
            assert_ok!(pool(&vote_call(id, &trusted, true)));
            assert_ok!(node_vote(trusted, id, true));
        });
    }

    #[test]
    fn never_opens_while_the_start_is_unset() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, _, trusted) = with_trusted_outsider();
            set_start(None);

            open_window(id);
            assert!(!Watchtower::can_vote(id, &trusted));
            assert_noop!(node_vote(trusted, id, true), Error::<TestRuntime>::TrustedVotingNotOpen);
        });
    }
}

mod eligibility {
    use super::*;

    #[test]
    fn outsiders_that_are_not_trusted_still_cannot_vote() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            let (id, _, others) = activated(3, Perbill::from_percent(50), VotingKind::Binary);
            trust(vec![others[0]]);
            set_start(Some(Perbill::from_percent(50)));
            open_window(id);

            assert!(!Watchtower::can_vote(id, &others[1]));
            assert_noop!(
                node_vote(others[1], id, true),
                Error::<TestRuntime>::NotInCommitteeOrTrusted
            );
        });
    }

    #[test]
    fn a_removed_owner_cannot_vote() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, _, trusted) = with_trusted_outsider();
            assert_ok!(Watchtower::remove_trusted_owners(
                RawOrigin::Root.into(),
                vec![trusted].try_into().unwrap()
            ));
            open_window(id);

            assert_noop!(
                node_vote(trusted, id, true),
                Error::<TestRuntime>::NotInCommitteeOrTrusted
            );
        });
    }

    #[test]
    fn an_owner_votes_only_through_nodes_they_still_own() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, _, trusted) = with_trusted_outsider();
            // The node changes hands; the trusted owner keeps no node.
            set_owners(vec![(watchtower_owner_2(), vec![trusted])]);
            open_window(id);

            assert_noop!(
                node_vote(trusted, id, true),
                Error::<TestRuntime>::NotInCommitteeOrTrusted
            );
        });
    }

    #[test]
    fn proposals_without_a_committee_are_unaffected() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            let context = Context { external_ref: H256::repeat_byte(1), ..Context::default() };
            assert_ok!(Watchtower::submit_proposal(None, context.build_internal_request(vec![1])));
            let id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert_ok!(Watchtower::activate_next_proposal(RawOrigin::None.into(), id));
            let node = default_watchtowers()[0];
            trust(vec![node]);
            set_start(Some(Perbill::from_percent(50)));

            // Every node votes from the start, and nothing is recorded for trusted owners.
            assert_eq!(Watchtower::voter_seat(id, &node).ok(), Some(VoterSeat::Open));
            assert_ok!(node_vote(node, id, true));
            assert!(!TrustedVoters::<TestRuntime>::contains_key(id, node));
        });
    }
}

mod filling_seats {
    use super::*;

    #[test]
    fn a_trusted_vote_counts_towards_the_committee_threshold() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, committee, trusted) = with_trusted_outsider();
            assert_ok!(node_vote(committee[0], id, true));

            open_window(id);
            assert_eq!(
                Watchtower::voter_seat(id, &trusted).ok(),
                Some(VoterSeat::Trusted { owner: trusted })
            );
            assert_ok!(node_vote(trusted, id, true));

            // 2 of the committee's 3 seats in favour.
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: true });
            assert!(TrustedVoters::<TestRuntime>::contains_key(id, trusted));
            System::assert_has_event(
                Event::TrustedVoteCast { proposal_id: id, owner: trusted, node: trusted }.into(),
            );
        });
    }

    #[test]
    fn trusted_owners_can_decide_for_a_silent_committee() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            let (id, _, others) = activated(3, Perbill::from_percent(50), VotingKind::Binary);
            trust(vec![others[0], others[1]]);
            set_start(Some(Perbill::from_percent(50)));
            open_window(id);

            assert_ok!(node_vote(others[0], id, false));
            assert_ok!(node_vote(others[1], id, false));
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: false });
        });
    }

    #[test]
    fn no_vote_is_accepted_once_every_seat_is_taken() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            // 100% of 3: nothing decides before every seat is taken.
            let (id, committee, others) = activated(3, Perbill::one(), VotingKind::Binary);
            trust(vec![others[0], others[1]]);
            set_start(Some(Perbill::from_percent(50)));

            assert_ok!(node_vote(committee[0], id, true));
            assert_ok!(node_vote(committee[1], id, false));
            open_window(id);
            assert_ok!(node_vote(others[0], id, true));

            // 3 votes on a committee of 3: neither another trusted owner nor the member who has
            // not voted yet gets in.
            for node in [others[1], committee[2]] {
                assert!(!Watchtower::can_vote(id, &node));
                assert_noop!(node_vote(node, id, true), Error::<TestRuntime>::NoEmptySeat);
            }
            assert_eq!(status(id), ProposalStatusEnum::Active);
        });
    }

    #[test]
    fn a_choice_proposal_fails_early_once_the_seats_left_cannot_make_a_winner() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            // 100% of 3: every seat must back the same answer.
            let (id, committee, others) = activated(3, Perbill::one(), VotingKind::Choice);
            trust(vec![others[0], others[1]]);
            set_start(Some(Perbill::from_percent(50)));

            assert_ok!(choice_vote(&committee[0], id, ANSWER));
            open_window(id);
            assert_ok!(choice_vote(&others[0], id, H256::repeat_byte(0xbb)));

            // One seat is left, so no answer can reach 3. The trusted vote took a seat, so the
            // count of seats left is exact and failing now cannot cut off a winning vote.
            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: false });
            assert_noop!(
                choice_vote(&committee[1], id, ANSWER),
                Error::<TestRuntime>::ProposalNotActive
            );
        });
    }

    #[test]
    fn work_for_choice_proposals() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            let (id, committee, others) =
                activated(3, Perbill::from_percent(50), VotingKind::Choice);
            trust(vec![others[0], others[1]]);
            set_start(Some(Perbill::from_percent(50)));

            assert_ok!(choice_vote(&committee[0], id, ANSWER));
            open_window(id);
            assert_ok!(choice_vote(&others[0], id, ANSWER));

            assert_eq!(status(id), ProposalStatusEnum::Resolved { passed: true });
            assert_eq!(WinningChoice::<TestRuntime>::get(id), Some(ANSWER));
            assert!(TrustedVoters::<TestRuntime>::contains_key(id, others[0]));
        });
    }

    #[test]
    fn trusted_voters_are_cleaned_up_with_the_proposal() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, committee, trusted) = with_trusted_outsider();
            assert_ok!(node_vote(committee[0], id, true));
            open_window(id);
            assert_ok!(node_vote(trusted, id, true));
            assert!(ProposalsToRemove::<TestRuntime>::contains_key(id));

            roll_forward(2);

            assert!(!TrustedVoters::<TestRuntime>::contains_key(id, trusted));
            assert!(!Proposals::<TestRuntime>::contains_key(id));
        });
    }
}

mod one_vote_per_owner {
    use super::*;

    #[test]
    fn across_the_owner_s_nodes() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            let (id, _, others) = activated(3, Perbill::from_percent(50), VotingKind::Binary);
            let owner = watchtower_owner_1();
            set_owners(vec![(owner, vec![others[0], others[1]])]);
            trust(vec![owner]);
            set_start(Some(Perbill::from_percent(50)));
            open_window(id);

            // In the pool both nodes share the owner's tag, so only one is kept.
            let first = pool(&vote_call(id, &others[0], true)).expect("valid");
            let second = pool(&vote_call(id, &others[1], true)).expect("valid");
            assert_ne!(first.provides[0], second.provides[0]);
            assert_eq!(first.provides[1], second.provides[1]);

            assert_ok!(node_vote(others[0], id, true));
            assert!(!Watchtower::can_vote(id, &others[1]));
            assert_noop!(node_vote(others[1], id, true), Error::<TestRuntime>::AlreadyVoted);
        });
    }

    #[test]
    fn a_committee_vote_uses_up_the_owner_s_trusted_vote() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            let (id, committee, others) =
                activated(3, Perbill::from_percent(50), VotingKind::Binary);
            let owner = watchtower_owner_1();
            set_owners(vec![(owner, vec![committee[0], others[0]])]);
            trust(vec![owner]);
            set_start(Some(Perbill::from_percent(50)));

            assert_eq!(
                Watchtower::voter_seat(id, &committee[0]).ok(),
                Some(VoterSeat::TrustedCommittee { owner })
            );
            assert_ok!(node_vote(committee[0], id, true));
            assert!(TrustedVoters::<TestRuntime>::contains_key(id, owner));

            open_window(id);
            assert_noop!(node_vote(others[0], id, true), Error::<TestRuntime>::AlreadyVoted);
        });
    }

    #[test]
    fn a_trusted_vote_uses_up_the_owner_s_committee_seat() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_owners(vec![]);
            let (id, committee, others) =
                activated(3, Perbill::from_percent(50), VotingKind::Binary);
            let owner = watchtower_owner_1();
            set_owners(vec![(owner, vec![committee[0], others[0]])]);
            trust(vec![owner]);
            set_start(Some(Perbill::from_percent(50)));
            open_window(id);

            assert_ok!(node_vote(others[0], id, true));
            assert!(!Watchtower::can_vote(id, &committee[0]));
            assert_noop!(node_vote(committee[0], id, true), Error::<TestRuntime>::AlreadyVoted);
        });
    }

    #[test]
    fn members_whose_owner_is_not_trusted_leave_no_record() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, committee, _) = with_trusted_outsider();
            assert_eq!(Watchtower::voter_seat(id, &committee[0]).ok(), Some(VoterSeat::Committee));
            assert_ok!(node_vote(committee[0], id, true));
            assert!(TrustedVoters::<TestRuntime>::iter_prefix(id).next().is_none());
        });
    }
}
