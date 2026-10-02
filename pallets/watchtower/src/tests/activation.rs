// Copyright 2026 Aventus DAO.

//! Tests for decoupled activation of queued internal proposals: finalising a vote no longer
//! activates the next proposal; `activate_next_proposal` (submitted by the OCW) does.

#![cfg(test)]

use crate::{mock::*, *};
use codec::Decode;
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::{traits::ValidateUnsigned, DispatchError};

fn submit_internal(ref_byte: u8) -> ProposalId {
    let context = Context { external_ref: H256::repeat_byte(ref_byte), ..Context::default() };
    let proposal = context.build_internal_request(vec![ref_byte]);
    assert_ok!(Watchtower::submit_proposal(None, proposal));
    ExternalRef::<TestRuntime>::get(&context.external_ref)
}

/// Threshold is 50% of 10 watchtowers, so 5 votes in favour finalise the proposal.
fn finalise_by_consensus(proposal_id: ProposalId) {
    for voter in [watchtower_1(), watchtower_2(), watchtower_3(), watchtower_4(), watchtower_5()] {
        assert_ok!(Watchtower::vote(RawOrigin::Signed(voter).into(), proposal_id, true));
    }
    assert_eq!(
        ProposalStatus::<TestRuntime>::get(proposal_id),
        ProposalStatusEnum::Resolved { passed: true }
    );
}

fn queue_head() -> Option<ProposalId> {
    Watchtower::peek_front_id().expect("queue is not corrupt")
}

fn activate(proposal_id: ProposalId) -> DispatchResult {
    Watchtower::activate_next_proposal(RawOrigin::None.into(), proposal_id)
}

fn activation_call(proposal_id: ProposalId) -> crate::Call<TestRuntime> {
    crate::Call::activate_next_proposal { proposal_id }
}

fn validate(source: TransactionSource, proposal_id: ProposalId) -> TransactionValidity {
    <Watchtower as ValidateUnsigned>::validate_unsigned(source, &activation_call(proposal_id))
}

fn pop_tx_from_mempool(pool_state: &Arc<RwLock<PoolState>>) -> Extrinsic {
    let tx = pool_state.write().transactions.pop().unwrap();
    Extrinsic::decode(&mut &*tx).unwrap()
}

mod finalising_a_vote {
    use super::*;

    #[test]
    fn does_not_activate_the_next_proposal() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(p1));
            assert_eq!(ProposalStatus::<TestRuntime>::get(p2), ProposalStatusEnum::Queued);

            finalise_by_consensus(p1);

            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), None);
            assert_eq!(ProposalStatus::<TestRuntime>::get(p2), ProposalStatusEnum::Queued);
            assert_eq!(queue_head(), Some(p2));
            assert!(ProposalsToRemove::<TestRuntime>::contains_key(p1));
            // Only p1 reached the hooks (on submission); p2 has not been activated.
            assert_eq!(proposals_submitted_to_hooks(), vec![p1]);
        });
    }

    #[test]
    fn on_expiry_does_not_activate_the_next_proposal() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);

            let target_block =
                MinVotingPeriod::<TestRuntime>::get().saturated_into::<u32>() + 10u32;
            roll_forward(target_block.into());

            assert_eq!(ProposalStatus::<TestRuntime>::get(p1), ProposalStatusEnum::Expired);
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), None);
            assert_eq!(ProposalStatus::<TestRuntime>::get(p2), ProposalStatusEnum::Queued);
            assert_eq!(queue_head(), Some(p2));
        });
    }
}

mod activate_next_proposal {
    use super::*;

    #[test]
    fn works() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);
            finalise_by_consensus(p1);

            roll_forward(1);
            let now = System::block_number();

            assert_ok!(activate(p2));

            let proposal = Proposals::<TestRuntime>::get(p2).unwrap();
            assert_eq!(ProposalStatus::<TestRuntime>::get(p2), ProposalStatusEnum::Active);
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(p2));
            assert_eq!(proposal.end_at, Some(now + proposal.vote_duration as u64));
            assert_eq!(queue_head(), None);
            assert_eq!(proposals_submitted_to_hooks(), vec![p1, p2]);
            System::assert_last_event(Event::ProposalActivated { proposal_id: p2 }.into());
        });
    }

    #[test]
    fn preserves_fifo_order() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);
            finalise_by_consensus(p1);
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), None);

            // Nothing is active, but p2 is waiting: p3 must queue behind it, not jump ahead.
            let p3 = submit_internal(3);
            assert_eq!(ProposalStatus::<TestRuntime>::get(p3), ProposalStatusEnum::Queued);
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), None);

            assert_noop!(activate(p3), Error::<TestRuntime>::ProposalNotNextInQueue);
            assert_ok!(activate(p2));
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(p2));

            finalise_by_consensus(p2);
            assert_ok!(activate(p3));
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(p3));
            assert_eq!(queue_head(), None);
        });
    }

    #[test]
    fn cancels_the_proposal_if_the_hook_fails() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);
            let p3 = submit_internal(3);
            finalise_by_consensus(p1);

            set_hook_failure(true);
            // Must not error, otherwise the tx would roll back and the OCW would retry forever.
            assert_ok!(activate(p2));
            set_hook_failure(false);

            assert_eq!(ProposalStatus::<TestRuntime>::get(p2), ProposalStatusEnum::Cancelled);
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), None);
            assert!(ProposalsToRemove::<TestRuntime>::contains_key(p2));
            // The queue has moved on.
            assert_eq!(queue_head(), Some(p3));
            System::assert_last_event(
                Event::VotingEnded {
                    proposal_id: p2,
                    external_ref: H256::repeat_byte(2),
                    consensus_result: ProposalStatusEnum::Cancelled,
                }
                .into(),
            );

            assert_ok!(activate(p3));
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(p3));
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_signed() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let p1 = submit_internal(1);
                let p2 = submit_internal(2);
                finalise_by_consensus(p1);

                assert_noop!(
                    Watchtower::activate_next_proposal(
                        RawOrigin::Signed(watchtower_1()).into(),
                        p2
                    ),
                    DispatchError::BadOrigin
                );
            });
        }

        #[test]
        fn proposal_is_not_at_the_head_of_the_queue() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let p1 = submit_internal(1);
                let _p2 = submit_internal(2);
                let p3 = submit_internal(3);
                finalise_by_consensus(p1);

                assert_noop!(activate(p3), Error::<TestRuntime>::ProposalNotNextInQueue);
            });
        }

        #[test]
        fn queue_is_empty() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let p1 = submit_internal(1);
                finalise_by_consensus(p1);

                assert_noop!(activate(p1), Error::<TestRuntime>::ProposalNotNextInQueue);
            });
        }

        #[test]
        fn a_proposal_is_still_active() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let p1 = submit_internal(1);
                let p2 = submit_internal(2);
                assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(p1));

                assert_noop!(activate(p2), Error::<TestRuntime>::ProposalAlreadyActive);
            });
        }
    }
}

mod validate_unsigned {
    use super::*;

    #[test]
    fn accepts_local_and_in_block_copies_when_state_matches() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);
            finalise_by_consensus(p1);

            for source in [TransactionSource::Local, TransactionSource::InBlock] {
                let validity = validate(source, p2).expect("valid");
                assert_eq!(validity.longevity, 64);
                assert_eq!(validity.propagate, false);
                assert_eq!(validity.requires, Vec::<Vec<u8>>::new());
                assert_eq!(
                    validity.provides,
                    vec![("wt_activateProposal", (WATCHTOWER_ACTIVATE_PROPOSAL_CONTEXT, p2))
                        .encode()]
                );
            }
        });
    }

    #[test]
    fn rejects_external_copies_even_when_state_matches() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);
            finalise_by_consensus(p1);

            assert_eq!(validate(TransactionSource::External, p2), InvalidTransaction::Call.into());
        });
    }

    #[test]
    fn is_stale_while_a_proposal_is_active() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let _p1 = submit_internal(1);
            let p2 = submit_internal(2);

            assert_eq!(validate(TransactionSource::Local, p2), InvalidTransaction::Stale.into());
        });
    }

    #[test]
    fn rejects_an_id_that_is_not_the_queue_head() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let _p2 = submit_internal(2);
            let p3 = submit_internal(3);
            finalise_by_consensus(p1);

            assert_eq!(
                validate(TransactionSource::Local, p3),
                InvalidTransaction::Custom(ACTIVATE_PROPOSAL_NOT_VALID).into()
            );
        });
    }

    #[test]
    fn rejects_when_the_queue_is_empty() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            finalise_by_consensus(p1);

            assert_eq!(
                validate(TransactionSource::Local, p1),
                InvalidTransaction::Custom(ACTIVATE_PROPOSAL_NOT_VALID).into()
            );
        });
    }
}

mod offchain_worker {
    use super::*;

    #[test]
    fn submits_an_activation_for_the_queue_head() {
        let (mut ext, pool_state, _) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);
            finalise_by_consensus(p1);

            Watchtower::offchain_worker(System::block_number());

            assert_eq!(pool_state.read().transactions.len(), 1);
            let tx = pop_tx_from_mempool(&pool_state);
            assert_eq!(tx.function, RuntimeCall::Watchtower(activation_call(p2)));

            // The tx is dispatchable as produced.
            assert_ok!(tx.function.dispatch(RawOrigin::None.into()));
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(p2));
            assert_eq!(ProposalStatus::<TestRuntime>::get(p2), ProposalStatusEnum::Active);
        });
    }

    #[test]
    fn submits_nothing_while_a_proposal_is_active() {
        let (mut ext, pool_state, _) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            let _p1 = submit_internal(1);
            let _p2 = submit_internal(2);

            Watchtower::offchain_worker(System::block_number());

            assert!(pool_state.read().transactions.is_empty());
        });
    }

    #[test]
    fn submits_nothing_when_the_queue_is_empty() {
        let (mut ext, pool_state, _) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            finalise_by_consensus(p1);

            Watchtower::offchain_worker(System::block_number());

            assert!(pool_state.read().transactions.is_empty());
        });
    }

    #[test]
    fn runs_at_most_once_per_block() {
        let (mut ext, pool_state, _) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            let p1 = submit_internal(1);
            let p2 = submit_internal(2);
            finalise_by_consensus(p1);

            let block = System::block_number();
            Watchtower::offchain_worker(block);
            assert_eq!(pool_state.read().transactions.len(), 1);

            // Same block again: the ocw lock stops a second submission.
            Watchtower::offchain_worker(block);
            assert_eq!(pool_state.read().transactions.len(), 1);

            // A new block submits again (the earlier copy is still unincluded in this test).
            Watchtower::offchain_worker(block + 1);
            assert_eq!(pool_state.read().transactions.len(), 2);
            let tx = pop_tx_from_mempool(&pool_state);
            assert_eq!(tx.function, RuntimeCall::Watchtower(activation_call(p2)));
        });
    }
}
