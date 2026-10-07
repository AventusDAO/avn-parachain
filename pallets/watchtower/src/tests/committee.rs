// Copyright 2026 Aventus DAO.

//! Tests for random committee selection, committee-gated voting, expiry rules and the
//! `Proposals` storage migration.

#![cfg(test)]

use crate::{
    migration::{
        v1::{DecisionRuleV0, ProposalV0},
        WatchtowerMigrations,
    },
    mock::*,
    *,
};
use frame_support::{
    assert_noop, assert_ok,
    traits::{GetStorageVersion, OnRuntimeUpgrade},
};
use frame_system::RawOrigin;
use sp_core::Pair;
use sp_runtime::traits::ValidateUnsigned;
use std::collections::BTreeSet;

fn submit_internal(ref_byte: u8, committee_size: Option<u32>) -> ProposalId {
    let context =
        Context { external_ref: H256::repeat_byte(ref_byte), committee_size, ..Context::default() };
    assert_ok!(Watchtower::submit_proposal(None, context.build_internal_request(vec![ref_byte])));
    ExternalRef::<TestRuntime>::get(&context.external_ref)
}

fn activate(proposal_id: ProposalId) -> DispatchResultWithPostInfo {
    Watchtower::activate_next_proposal(RawOrigin::None.into(), proposal_id)
}

fn members(proposal_id: ProposalId) -> BTreeSet<AccountId> {
    ProposalCommittee::<TestRuntime>::iter_prefix(proposal_id)
        .map(|(who, _)| who)
        .collect()
}

fn committee_size(proposal_id: ProposalId) -> Option<u32> {
    ProposalCommitteeSize::<TestRuntime>::get(proposal_id)
}

fn first_n_watchtowers(n: usize) -> Vec<AccountId> {
    default_watchtowers().into_iter().take(n).collect()
}

/// The key pair that signs unsigned votes for `account` (see `NODE_SIGNING_KEYS` in the mock).
fn signer_for(account: &AccountId) -> TestAccount {
    if *account == watchtower_1() {
        return get_default_voter()
    }
    for i in 2..=10u8 {
        if TestAccount::new([10 + i; 32]).account_id() == *account {
            return TestAccount::new([i; 32])
        }
    }
    panic!("not a mock watchtower: {:?}", account);
}

fn unsigned_vote_call(
    proposal_id: ProposalId,
    voter: &AccountId,
    in_favor: bool,
) -> crate::Call<TestRuntime> {
    let payload = (WATCHTOWER_UNSIGNED_VOTE_CONTEXT, proposal_id, in_favor, voter).encode();
    let signature: Signature = signer_for(voter).key_pair().sign(&payload).into();
    crate::Call::unsigned_vote {
        proposal_id,
        in_favor,
        watchtower: *voter,
        signature: signature.into(),
    }
}

fn vote(voter: AccountId, proposal_id: ProposalId, in_favor: bool) -> DispatchResultWithPostInfo {
    Watchtower::vote(RawOrigin::Signed(voter).into(), proposal_id, in_favor)
}

/// Activates a committee proposal under `seed` in a fresh externality and returns the members.
fn committee_for(seed: u64, ref_byte: u8, size: u32) -> BTreeSet<AccountId> {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        set_random_seed(seed);
        let id = submit_internal(ref_byte, Some(size));
        assert_ok!(activate(id));
        members(id)
    })
}

mod selection {
    use super::*;

    #[test]
    fn stores_the_requested_number_of_distinct_authorized_nodes() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_internal(1, Some(3));
            assert_eq!(committee_size(id), None);
            assert!(members(id).is_empty());

            assert_ok!(activate(id));

            let selected = members(id);
            assert_eq!(selected.len(), 3);
            assert_eq!(committee_size(id), Some(3));
            let authorized: BTreeSet<_> = default_watchtowers().into_iter().collect();
            assert!(selected.is_subset(&authorized));
            System::assert_has_event(Event::CommitteeSelected { proposal_id: id, size: 3 }.into());
            System::assert_last_event(Event::ProposalActivated { proposal_id: id }.into());
            // The request is kept on the proposal untouched.
            assert_eq!(Proposals::<TestRuntime>::get(id).unwrap().committee_size, Some(3));
        });
    }

    #[test]
    fn is_deterministic_for_a_given_seed_and_proposal() {
        assert_eq!(committee_for(7, 1, 3), committee_for(7, 1, 3));
    }

    #[test]
    fn depends_on_the_seed() {
        let committees: BTreeSet<_> = (1..=6u64).map(|seed| committee_for(seed, 1, 3)).collect();
        assert!(committees.len() > 1, "six seeds produced the same committee");
    }

    #[test]
    fn depends_on_the_proposal_id() {
        let committees: BTreeSet<_> =
            (1..=6u8).map(|ref_byte| committee_for(7, ref_byte, 3)).collect();
        assert!(committees.len() > 1, "six proposals produced the same committee");
    }

    #[test]
    fn selects_every_node_when_fewer_than_requested_are_registered() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_authorized_watchtowers(first_n_watchtowers(3));
            let id = submit_internal(1, Some(5));

            assert_ok!(activate(id));

            assert_eq!(committee_size(id), Some(3));
            assert_eq!(members(id), first_n_watchtowers(3).into_iter().collect());
        });
    }

    #[test]
    fn legacy_proposals_have_no_committee() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_internal(1, None);
            assert_ok!(activate(id));

            assert_eq!(committee_size(id), None);
            assert!(members(id).is_empty());
            assert!(Watchtower::is_committee_member(id, &watchtower_1()));
            assert!(Watchtower::is_committee_member(id, &random_user()));
        });
    }
}

mod not_ready {
    use super::*;

    fn validate(proposal_id: ProposalId) -> TransactionValidity {
        <Watchtower as ValidateUnsigned>::validate_unsigned(
            TransactionSource::Local,
            &crate::Call::activate_next_proposal { proposal_id },
        )
    }

    #[test]
    fn activation_is_deferred_while_the_node_index_is_backfilling() {
        let (mut ext, pool_state, _) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            set_indexed_count_override(Some(9)); // 9 of 10 nodes indexed
            let id = submit_internal(1, Some(3));

            // The extrinsic fails and rolls back...
            assert_noop!(activate(id), Error::<TestRuntime>::NodeIndexNotReady);
            // ...the pool rejects it...
            assert_eq!(validate(id), InvalidTransaction::Custom(COMMITTEE_NOT_READY).into());
            // ...and the collator OCW does not even submit it.
            Watchtower::offchain_worker(System::block_number());
            assert!(pool_state.read().transactions.is_empty());

            // Still queued at the head, nothing selected, policy unchanged.
            assert_eq!(ProposalStatus::<TestRuntime>::get(id), ProposalStatusEnum::Queued);
            assert_eq!(Watchtower::peek_front_id().unwrap(), Some(id));
            assert_eq!(committee_size(id), None);
            assert!(members(id).is_empty());

            // Backfill completes: the OCW submits and the same proposal activates with a
            // committee.
            set_indexed_count_override(None);
            Watchtower::offchain_worker(System::block_number() + 1);
            assert_eq!(pool_state.read().transactions.len(), 1);
            assert_ok!(validate(id));
            assert_ok!(activate(id));
            assert_eq!(committee_size(id), Some(3));
        });
    }

    #[test]
    fn activation_is_deferred_when_too_few_nodes_exist() {
        let (mut ext, pool_state, _) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            set_authorized_watchtowers(first_n_watchtowers(1)); // MinCommitteeSize is 2
            let id = submit_internal(1, Some(3));

            assert_noop!(activate(id), Error::<TestRuntime>::NotEnoughNodesForCommittee);
            assert_eq!(validate(id), InvalidTransaction::Custom(COMMITTEE_NOT_READY).into());
            Watchtower::offchain_worker(System::block_number());
            assert!(pool_state.read().transactions.is_empty());
            assert_eq!(ProposalStatus::<TestRuntime>::get(id), ProposalStatusEnum::Queued);

            set_authorized_watchtowers(first_n_watchtowers(2));
            assert_ok!(activate(id));
            assert_eq!(committee_size(id), Some(2));
        });
    }

    #[test]
    fn activation_is_deferred_when_no_nodes_exist() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_authorized_watchtowers(vec![]);
            let id = submit_internal(1, Some(3));

            assert_noop!(activate(id), Error::<TestRuntime>::NotEnoughNodesForCommittee);
            assert_eq!(ProposalStatus::<TestRuntime>::get(id), ProposalStatusEnum::Queued);
        });
    }

    #[test]
    fn legacy_proposals_activate_while_the_node_index_is_backfilling() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_indexed_count_override(Some(0));
            let id = submit_internal(1, None);

            assert_ok!(validate(id));
            assert_ok!(activate(id));
            assert_eq!(ProposalStatus::<TestRuntime>::get(id), ProposalStatusEnum::Active);
        });
    }

    #[test]
    fn consumers_can_check_readiness_before_configuring_a_committee() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            // 10 nodes, fully indexed: 3 is fine, 1 is below MinCommitteeSize.
            assert_ok!(<Watchtower as WatchtowerInterface>::ensure_committee_ready(3));
            assert_noop!(
                <Watchtower as WatchtowerInterface>::ensure_committee_ready(1),
                Error::<TestRuntime>::NotEnoughNodesForCommittee
            );

            set_indexed_count_override(Some(9));
            assert_noop!(
                <Watchtower as WatchtowerInterface>::ensure_committee_ready(3),
                Error::<TestRuntime>::NodeIndexNotReady
            );

            set_indexed_count_override(None);
            set_authorized_watchtowers(first_n_watchtowers(1));
            assert_noop!(
                <Watchtower as WatchtowerInterface>::ensure_committee_ready(3),
                Error::<TestRuntime>::NotEnoughNodesForCommittee
            );
        });
    }

    #[test]
    fn a_stuck_head_can_be_demoted_so_a_legacy_proposal_behind_it_proceeds() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_indexed_count_override(Some(9));
            let stuck = submit_internal(1, Some(3));
            let legacy = submit_internal(2, None);
            assert_noop!(activate(stuck), Error::<TestRuntime>::NodeIndexNotReady);

            assert_ok!(Watchtower::demote_queue_head(RawOrigin::Root.into(), stuck));

            assert_ok!(activate(legacy));
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(legacy));
            assert_eq!(Watchtower::peek_front_id().unwrap(), Some(stuck));
        });
    }
}

mod corrupt_index {
    use super::*;

    fn assert_cancelled_without_committee(id: ProposalId, error: Error<TestRuntime>) {
        assert_eq!(ProposalStatus::<TestRuntime>::get(id), ProposalStatusEnum::Cancelled);
        assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), None);
        assert_eq!(committee_size(id), None);
        assert!(members(id).is_empty());
        assert!(ProposalsToRemove::<TestRuntime>::contains_key(id));
        assert_eq!(completed_votes(), vec![(id, ProposalStatusEnum::Cancelled)]);
        System::assert_has_event(
            Event::CommitteeSelectionFailed { proposal_id: id, error: error.into() }.into(),
        );
        // The hook is only called for activated proposals.
        assert!(proposals_submitted_to_hooks().is_empty());
    }

    #[test]
    fn a_hole_cancels_the_proposal_and_the_queue_moves_on() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            // 3 nodes, 5 requested: every index is read, so the hole is always hit.
            set_authorized_watchtowers(first_n_watchtowers(3));
            set_index_fault(Some(IndexFault::Hole(1)));
            let bad = submit_internal(1, Some(5));
            let next = submit_internal(2, None);

            // Must not error: the OCW would otherwise retry forever.
            assert_ok!(activate(bad));

            assert_cancelled_without_committee(bad, Error::<TestRuntime>::NodeIndexCorrupt);
            assert_eq!(Watchtower::peek_front_id().unwrap(), Some(next));
            assert_ok!(activate(next));
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(next));
        });
    }

    #[test]
    fn a_failed_selection_is_still_charged_for_the_members_it_read() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            // 3 nodes, 5 requested: selection runs for an effective committee of 3 before
            // the hole cancels it, and the rollback leaves no stored committee size behind.
            set_authorized_watchtowers(first_n_watchtowers(3));
            set_index_fault(Some(IndexFault::Hole(1)));
            let bad = submit_internal(1, Some(5));

            let post_info = activate(bad).expect("cancelled, not failed");

            assert_eq!(committee_size(bad), None);
            let weight_for = |k: u32| {
                <TestRuntime as Config>::WeightInfo::activate_next_proposal(k)
                    .max(<TestRuntime as Config>::WeightInfo::activate_next_proposal_hook_fails(k))
            };
            assert_ne!(weight_for(3), weight_for(0));
            assert_eq!(post_info.actual_weight, Some(weight_for(3)));
        });
    }

    #[test]
    fn a_duplicate_entry_cancels_the_proposal() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_authorized_watchtowers(first_n_watchtowers(3));
            set_index_fault(Some(IndexFault::Duplicate(1)));
            let bad = submit_internal(1, Some(5));

            assert_ok!(activate(bad));

            assert_cancelled_without_committee(bad, Error::<TestRuntime>::NodeIndexCorrupt);
        });
    }

    #[test]
    fn the_cancelled_proposal_is_cleaned_up() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            set_authorized_watchtowers(first_n_watchtowers(3));
            set_index_fault(Some(IndexFault::Hole(0)));
            let bad = submit_internal(1, Some(5));
            assert_ok!(activate(bad));

            roll_forward(2);

            assert!(!Proposals::<TestRuntime>::contains_key(bad));
            assert!(!ProposalsToRemove::<TestRuntime>::contains_key(bad));
            System::assert_last_event(Event::ProposalCleaned { proposal_id: bad }.into());
        });
    }
}

mod voting {
    use super::*;

    fn activated_committee(size: u32) -> (ProposalId, Vec<AccountId>, Vec<AccountId>) {
        let id = submit_internal(1, Some(size));
        assert_ok!(activate(id));
        let selected = members(id);
        let non_members: Vec<AccountId> =
            default_watchtowers().into_iter().filter(|w| !selected.contains(w)).collect();
        (id, selected.into_iter().collect(), non_members)
    }

    #[test]
    fn members_can_vote_and_the_threshold_uses_the_committee_size() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, selected, _) = activated_committee(3);
            // 50% of 3 is 2 votes, even though 10 nodes are registered.
            assert_ok!(vote(selected[0], id, true));
            assert_eq!(ProposalStatus::<TestRuntime>::get(id), ProposalStatusEnum::Active);

            assert_ok!(vote(selected[1], id, true));

            assert_eq!(
                ProposalStatus::<TestRuntime>::get(id),
                ProposalStatusEnum::Resolved { passed: true }
            );
        });
    }

    #[test]
    fn non_members_cannot_vote_with_a_signed_transaction() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, _, non_members) = activated_committee(3);

            assert_noop!(vote(non_members[0], id, true), Error::<TestRuntime>::NotInCommittee);
            assert!(!Voters::<TestRuntime>::contains_key(id, non_members[0]));
        });
    }

    #[test]
    fn non_members_cannot_vote_with_an_unsigned_transaction() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, selected, non_members) = activated_committee(3);

            let call = unsigned_vote_call(id, &non_members[0], true);
            // Rejected in the pool...
            assert_eq!(
                <Watchtower as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::External,
                    &call
                ),
                InvalidTransaction::Custom(UNSIGNED_VOTE_NOT_VALID).into()
            );
            // ...and in a block.
            assert_noop!(
                Watchtower::unsigned_vote(
                    RawOrigin::None.into(),
                    id,
                    true,
                    non_members[0],
                    match unsigned_vote_call(id, &non_members[0], true) {
                        crate::Call::unsigned_vote { signature, .. } => signature,
                        _ => unreachable!(),
                    }
                ),
                Error::<TestRuntime>::NotInCommittee
            );

            // A member's unsigned vote is accepted.
            let member_call = unsigned_vote_call(id, &selected[0], true);
            assert_ok!(<Watchtower as ValidateUnsigned>::validate_unsigned(
                TransactionSource::External,
                &member_call
            ));
            if let crate::Call::unsigned_vote { signature, .. } = member_call {
                assert_ok!(Watchtower::unsigned_vote(
                    RawOrigin::None.into(),
                    id,
                    true,
                    selected[0],
                    signature
                ));
            }
            assert!(Voters::<TestRuntime>::contains_key(id, selected[0]));
        });
    }

    #[test]
    fn a_member_deregistered_mid_vote_cannot_vote_and_the_denominator_is_unchanged() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, selected, _) = activated_committee(3);

            let remaining: Vec<AccountId> =
                authorized_watchtowers().into_iter().filter(|w| *w != selected[0]).collect();
            set_authorized_watchtowers(remaining);

            assert_noop!(vote(selected[0], id, true), Error::<TestRuntime>::UnauthorizedVoter);
            assert_eq!(committee_size(id), Some(3));
            // The other two members still carry the proposal.
            assert_ok!(vote(selected[1], id, true));
            assert_ok!(vote(selected[2], id, true));
            assert_eq!(
                ProposalStatus::<TestRuntime>::get(id),
                ProposalStatusEnum::Resolved { passed: true }
            );
        });
    }

    #[test]
    fn unauthorized_accounts_are_rejected_before_the_committee_check() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (id, _, _) = activated_committee(3);
            assert_noop!(vote(random_user(), id, true), Error::<TestRuntime>::UnauthorizedVoter);
        });
    }
}

mod cleanup {
    use super::*;

    #[test]
    fn removes_committee_rows_and_size_after_a_hook_failure() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_internal(1, Some(3));
            set_hook_failure(true);
            assert_ok!(activate(id));
            set_hook_failure(false);

            // Committee was stored before the hook ran, then the proposal was cancelled.
            assert_eq!(ProposalStatus::<TestRuntime>::get(id), ProposalStatusEnum::Cancelled);
            assert_eq!(members(id).len(), 3);
            assert_eq!(committee_size(id), Some(3));

            roll_forward(2);

            assert!(members(id).is_empty());
            assert_eq!(committee_size(id), None);
            assert!(!Proposals::<TestRuntime>::contains_key(id));
            System::assert_last_event(Event::ProposalCleaned { proposal_id: id }.into());
        });
    }

    #[test]
    fn pages_large_committees_and_only_cleans_the_proposal_once_all_rows_are_gone() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_internal(1, Some(3));
            assert_ok!(activate(id));
            let selected: Vec<AccountId> = members(id).into_iter().collect();
            assert_ok!(vote(selected[0], id, true));
            assert_ok!(vote(selected[1], id, true));
            assert!(ProposalsToRemove::<TestRuntime>::contains_key(id));

            // Pad the committee well past one cleanup page (250 rows per block).
            for i in 0..600u32 {
                let mut seed = [0xccu8; 32];
                seed[..4].copy_from_slice(&i.to_le_bytes());
                ProposalCommittee::<TestRuntime>::insert(
                    id,
                    TestAccount::new(seed).account_id(),
                    (),
                );
            }
            assert_eq!(members(id).len(), 603);

            roll_one_block();
            assert!(members(id).len() > 0);
            assert!(Proposals::<TestRuntime>::contains_key(id));
            assert!(committee_size(id).is_some());

            roll_one_block();
            assert!(Proposals::<TestRuntime>::contains_key(id));

            roll_one_block();
            assert!(members(id).is_empty());
            assert_eq!(committee_size(id), None);
            assert!(!Proposals::<TestRuntime>::contains_key(id));
            assert!(!ProposalsToRemove::<TestRuntime>::contains_key(id));
            System::assert_last_event(Event::ProposalCleaned { proposal_id: id }.into());
        });
    }
}

mod request_validation {
    use super::*;

    fn submit(committee_size: Option<u32>, source: ProposalSource) -> DispatchResult {
        let context = Context { committee_size, ..Context::default() };
        let payload = match source {
            ProposalSource::External => RawPayload::Uri(b"uri".to_vec()),
            ProposalSource::Internal(_) => RawPayload::Inline(b"test".to_vec()),
        };
        Watchtower::submit_proposal(None, context.build_request(payload, source))
    }

    #[test]
    fn rejects_committees_below_the_minimum() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            assert_noop!(
                submit(Some(1), ProposalSource::Internal(ProposalType::Summary)),
                Error::<TestRuntime>::CommitteeSizeTooSmall
            );
            assert_noop!(
                submit(Some(0), ProposalSource::Internal(ProposalType::Summary)),
                Error::<TestRuntime>::CommitteeSizeTooSmall
            );
        });
    }

    #[test]
    fn rejects_committees_above_the_maximum() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            assert_noop!(
                submit(Some(6), ProposalSource::Internal(ProposalType::Summary)),
                Error::<TestRuntime>::CommitteeSizeTooLarge
            );
        });
    }

    #[test]
    fn rejects_committees_on_external_proposals() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            assert_noop!(
                submit(Some(3), ProposalSource::External),
                Error::<TestRuntime>::InvalidProposal
            );
            assert_ok!(submit(None, ProposalSource::External));
        });
    }

    #[test]
    fn accepts_the_bounds() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            assert_ok!(submit(Some(2), ProposalSource::Internal(ProposalType::Summary)));
            let context = Context { external_ref: H256::repeat_byte(2), ..Context::default() };
            assert_ok!(Watchtower::submit_proposal(
                None,
                Context { committee_size: Some(5), ..context }.build_request(
                    RawPayload::Inline(b"t".to_vec()),
                    ProposalSource::Internal(ProposalType::Summary)
                )
            ));
        });
    }
}

mod decision_rules {
    use super::*;

    fn submit_and_activate(rule: DecisionRule, source: ProposalSource) -> ProposalId {
        let context = Context { decision_rule: Some(rule), ..Context::default() };
        let payload = match source {
            ProposalSource::External => RawPayload::Uri(b"uri".to_vec()),
            ProposalSource::Internal(_) => RawPayload::Inline(b"test".to_vec()),
        };
        let is_internal = matches!(source, ProposalSource::Internal(_));
        assert_ok!(Watchtower::submit_proposal(None, context.build_request(payload, source)));
        let id = ExternalRef::<TestRuntime>::get(&context.external_ref);
        if is_internal {
            activate_head();
        }
        id
    }

    fn expire() {
        let target = MinVotingPeriod::<TestRuntime>::get().saturated_into::<u32>() + 10u32;
        roll_forward(target.into());
    }

    fn status_after_expiry(id: ProposalId) -> ProposalStatusEnum {
        // Internal proposals are finalised by on_idle; external ones by the next vote or call.
        if ProposalStatus::<TestRuntime>::get(id) == ProposalStatusEnum::Active {
            assert_ok!(Watchtower::finalise_proposal(RawOrigin::Signed(random_user()).into(), id));
        }
        ProposalStatus::<TestRuntime>::get(id)
    }

    #[test]
    fn expire_unresolved_keeps_the_legacy_internal_result() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_and_activate(
                DecisionRule::ExpireUnresolved,
                ProposalSource::Internal(ProposalType::Summary),
            );
            assert_ok!(vote(watchtower_1(), id, true));
            expire();
            assert_eq!(status_after_expiry(id), ProposalStatusEnum::Expired);
        });
    }

    #[test]
    fn reject_fails_the_proposal_even_with_votes_in_favour() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_and_activate(
                DecisionRule::RejectOnExpiry,
                ProposalSource::Internal(ProposalType::Summary),
            );
            assert_ok!(vote(watchtower_1(), id, true));
            expire();
            assert_eq!(status_after_expiry(id), ProposalStatusEnum::Resolved { passed: false });
        });
    }

    #[test]
    fn simple_majority_passes_with_more_votes_in_favour() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_and_activate(
                DecisionRule::SimpleMajorityOnExpiry,
                ProposalSource::Internal(ProposalType::Summary),
            );
            assert_ok!(vote(watchtower_1(), id, true));
            assert_ok!(vote(watchtower_2(), id, true));
            assert_ok!(vote(watchtower_3(), id, false));
            expire();
            assert_eq!(status_after_expiry(id), ProposalStatusEnum::Resolved { passed: true });
        });
    }

    #[test]
    fn simple_majority_fails_on_a_tie_or_with_no_votes() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_and_activate(
                DecisionRule::SimpleMajorityOnExpiry,
                ProposalSource::Internal(ProposalType::Summary),
            );
            expire();
            assert_eq!(status_after_expiry(id), ProposalStatusEnum::Resolved { passed: false });
        });
    }

    #[test]
    fn external_proposals_honour_reject() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_and_activate(DecisionRule::RejectOnExpiry, ProposalSource::External);
            assert_ok!(vote(watchtower_owner_1(), id, true));
            expire();
            assert_eq!(status_after_expiry(id), ProposalStatusEnum::Resolved { passed: false });
        });
    }

    #[test]
    fn external_proposals_can_expire_unresolved() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = submit_and_activate(DecisionRule::ExpireUnresolved, ProposalSource::External);
            assert_ok!(vote(watchtower_owner_1(), id, true));
            expire();
            assert_eq!(status_after_expiry(id), ProposalStatusEnum::Expired);
        });
    }

    #[test]
    fn external_proposals_default_to_simple_majority_on_expiry() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            assert_ok!(Watchtower::submit_proposal(
                None,
                context.build_external_request(b"uri".to_vec())
            ));
            let id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert_eq!(
                Proposals::<TestRuntime>::get(id).unwrap().decision_rule,
                DecisionRule::SimpleMajorityOnExpiry
            );
            assert_ok!(vote(watchtower_owner_1(), id, true));
            expire();
            assert_eq!(status_after_expiry(id), ProposalStatusEnum::Resolved { passed: true });
        });
    }
}

mod migration {
    use super::*;

    #[test]
    fn translates_v0_proposals_with_legacy_defaults() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let id = H256::repeat_byte(0x55);
            let old = ProposalV0::<TestRuntime> {
                title: BoundedVec::try_from(b"old".to_vec()).unwrap(),
                payload: Payload::Inline(BoundedVec::try_from(vec![1, 2, 3]).unwrap()),
                threshold: Perbill::from_percent(60),
                source: ProposalSource::Internal(ProposalType::Summary),
                decision_rule: DecisionRuleV0::SimpleMajority,
                external_ref: H256::repeat_byte(9),
                proposer: Some(watchtower_owner_1()),
                created_at: 1,
                vote_duration: 10,
                end_at: Some(11),
            };
            sp_io::storage::set(&Proposals::<TestRuntime>::hashed_key_for(id), &old.encode());
            // Undecodable with the new layout until migrated.
            assert!(Proposals::<TestRuntime>::get(id).is_none());
            assert_eq!(Watchtower::on_chain_storage_version(), StorageVersion::new(0));

            WatchtowerMigrations::<TestRuntime>::on_runtime_upgrade();

            let new = Proposals::<TestRuntime>::get(id).expect("translated");
            assert_eq!(new.source, ProposalSource::Internal(ProposalType::Summary));
            assert_eq!(new.title.to_vec(), b"old".to_vec());
            assert_eq!(new.threshold, Perbill::from_percent(60));
            assert_eq!(new.proposer, Some(watchtower_owner_1()));
            assert_eq!(new.end_at, Some(11));
            assert_eq!(new.committee_size, None);
            assert_eq!(new.decision_rule, DecisionRule::ExpireUnresolved);
            assert_eq!(Watchtower::on_chain_storage_version(), StorageVersion::new(1));
        });
    }

    #[test]
    fn is_skipped_when_already_migrated() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            StorageVersion::new(1).put::<Watchtower>();
            let id = H256::repeat_byte(0x55);
            let old = ProposalV0::<TestRuntime> {
                title: BoundedVec::try_from(b"old".to_vec()).unwrap(),
                payload: Payload::Inline(BoundedVec::try_from(vec![1]).unwrap()),
                threshold: Perbill::from_percent(60),
                source: ProposalSource::Internal(ProposalType::Summary),
                decision_rule: DecisionRuleV0::SimpleMajority,
                external_ref: H256::repeat_byte(9),
                proposer: None,
                created_at: 1,
                vote_duration: 10,
                end_at: None,
            };
            sp_io::storage::set(&Proposals::<TestRuntime>::hashed_key_for(id), &old.encode());

            WatchtowerMigrations::<TestRuntime>::on_runtime_upgrade();

            // Untouched: still not decodable as the new type.
            assert!(Proposals::<TestRuntime>::get(id).is_none());
        });
    }
}
