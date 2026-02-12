// Copyright 2019-2022 PureStake Inc.
// This file is part of Moonbeam.

// Moonbeam is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonbeam is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonbeam.  If not, see <http://www.gnu.org/licenses/>.

//! # Staking Pallet Unit Tests
//! The unit tests are organized by the call they test. The order matches the order
//! of the calls in the `lib.rs`.
//! 1. Root
//! 2. Public (Collator, Nominator)
//! 3. Miscellaneous Property-Based Tests
use crate::{
    assert_eq_events, assert_eq_last_events, assert_event_emitted, assert_last_event,
    assert_tail_eq,
    mock::{
        roll_one_block, roll_to, roll_to_era_begin, roll_to_era_end, set_author, set_reward_pot,
        AccountId, Balances, ExtBuilder, MaxNominationsPerNominator, ParachainStaking,
        RuntimeEvent as MetaEvent, RuntimeOrigin as Origin, Test, TestAccount,
    },
    nomination_requests::{CancelledScheduledRequest, NominationAction, ScheduledRequest},
    AtStake, CollatorSnapshot, CollatorStatus, Error, Event, NominationScheduledRequests,
    NominatorAdded, NOMINATOR_LOCK_ID,
};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::{traits::Zero, BoundedVec, DispatchError, ModuleError};

// ~~ ROOT ~~

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id();
}

#[test]
fn invalid_root_origin_fails() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::set_total_selected(Origin::signed(to_acc_id(45)), 6u32),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_noop!(
            ParachainStaking::set_blocks_per_era(Origin::signed(to_acc_id(45)), 3u32),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}

// SET TOTAL SELECTED

#[test]
fn set_total_selected_event_emits_correctly() {
    ExtBuilder::default().build().execute_with(|| {
        // before we can bump total_selected we must bump the blocks per era
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 6u32));
        assert_ok!(ParachainStaking::set_total_selected(Origin::root(), 6u32));
        assert_last_event!(MetaEvent::ParachainStaking(Event::TotalSelectedSet {
            old: 5u32,
            new: 6u32
        }));
    });
}

#[test]
fn set_total_selected_fails_if_above_blocks_per_era() {
    ExtBuilder::default().build().execute_with(|| {
        assert_eq!(ParachainStaking::era().length, 5); // test relies on this
        assert_noop!(
            ParachainStaking::set_total_selected(Origin::root(), 6u32),
            Error::<Test>::EraLengthMustBeAtLeastTotalSelectedCollators,
        );
    });
}

#[test]
fn set_total_selected_passes_if_equal_to_blocks_per_era() {
    ExtBuilder::default().build().execute_with(|| {
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 10u32));
        assert_ok!(ParachainStaking::set_total_selected(Origin::root(), 10u32));
    });
}

#[test]
fn set_total_selected_passes_if_below_blocks_per_era() {
    ExtBuilder::default().build().execute_with(|| {
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 10u32));
        assert_ok!(ParachainStaking::set_total_selected(Origin::root(), 9u32));
    });
}

#[test]
fn set_blocks_per_era_fails_if_below_total_selected() {
    ExtBuilder::default().build().execute_with(|| {
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 20u32));
        assert_ok!(ParachainStaking::set_total_selected(Origin::root(), 15u32));
        assert_noop!(
            ParachainStaking::set_blocks_per_era(Origin::root(), 14u32),
            Error::<Test>::EraLengthMustBeAtLeastTotalSelectedCollators,
        );
    });
}

#[test]
fn set_blocks_per_era_passes_if_equal_to_total_selected() {
    ExtBuilder::default().build().execute_with(|| {
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 10u32));
        assert_ok!(ParachainStaking::set_total_selected(Origin::root(), 9u32));
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 9u32));
    });
}

#[test]
fn set_blocks_per_era_passes_if_above_total_selected() {
    ExtBuilder::default().build().execute_with(|| {
        assert_eq!(ParachainStaking::era().length, 5); // test relies on this
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 6u32));
    });
}

#[test]
fn set_total_selected_storage_updates_correctly() {
    ExtBuilder::default().build().execute_with(|| {
        // era length must be >= total_selected, so update that first
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 10u32));

        assert_eq!(ParachainStaking::total_selected(), 5u32);
        assert_ok!(ParachainStaking::set_total_selected(Origin::root(), 6u32));
        assert_eq!(ParachainStaking::total_selected(), 6u32);
    });
}

#[test]
fn cannot_set_total_selected_to_current_total_selected() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::set_total_selected(Origin::root(), 5u32),
            Error::<Test>::NoWritingSameValue
        );
    });
}

#[test]
fn cannot_set_total_selected_below_module_min() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::set_total_selected(Origin::root(), 4u32),
            Error::<Test>::CannotSetBelowMin
        );
    });
}

// SET BLOCKS PER ERA

#[test]
fn set_blocks_per_era_event_emits_correctly() {
    ExtBuilder::default().build().execute_with(|| {
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 6u32));
        assert_last_event!(MetaEvent::ParachainStaking(Event::BlocksPerEraSet {
            current_era: 1,
            first_block: 0,
            old: 5,
            new: 6,
        }));
    });
}

#[test]
fn set_blocks_per_era_storage_updates_correctly() {
    ExtBuilder::default().build().execute_with(|| {
        assert_eq!(ParachainStaking::era().length, 5);
        assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 6u32));
        assert_eq!(ParachainStaking::era().length, 6);
    });
}

#[test]
fn cannot_set_blocks_per_era_below_module_min() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::set_blocks_per_era(Origin::root(), 2u32),
            Error::<Test>::CannotSetBelowMin
        );
    });
}

#[test]
fn cannot_set_blocks_per_era_to_current_blocks_per_era() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::set_blocks_per_era(Origin::root(), 5u32),
            Error::<Test>::NoWritingSameValue
        );
    });
}

#[test]
fn era_immediately_jumps_if_current_duration_exceeds_new_blocks_per_era() {
    ExtBuilder::default()
        .with_balances(vec![(to_acc_id(1), 20)])
        .with_candidates(vec![(to_acc_id(1), 20)])
        .build()
        .execute_with(|| {
            // we can't lower the blocks per era because it must be above the number of collators,
            // and we can't lower the number of collators because it must be above
            // MinSelectedCandidates. so we first raise blocks per era, then lower it.
            assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 10u32));

            roll_to(17);
            assert_last_event!(MetaEvent::ParachainStaking(Event::NewEra {
                starting_block: 10,
                era: 2,
                selected_collators_number: 1,
                total_balance: 20
            }));
            assert_ok!(ParachainStaking::set_blocks_per_era(Origin::root(), 5u32));
            roll_to(18);
            assert_last_event!(MetaEvent::ParachainStaking(Event::NewEra {
                starting_block: 18,
                era: 3,
                selected_collators_number: 1,
                total_balance: 20
            }));
        });
}

// ~~ PUBLIC ~~

// JOIN CANDIDATES

#[test]
fn join_candidates_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default().with_balances(vec![(account_id, 10)]).build().execute_with(|| {
        assert_ok!(ParachainStaking::join_candidates(Origin::signed(account_id), 10u128, 0u32));
        assert_last_event!(MetaEvent::ParachainStaking(Event::JoinedCollatorCandidates {
            account: account_id,
            amount_locked: 10u128,
            new_total_amt_locked: 10u128,
        }));
    });
}

#[test]
fn join_candidates_reserves_balance() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default().with_balances(vec![(account_id, 10)]).build().execute_with(|| {
        assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 10);
        assert_ok!(ParachainStaking::join_candidates(Origin::signed(account_id), 10u128, 0u32));
        assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 0);
    });
}

#[test]
fn join_candidates_increases_total_staked() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default().with_balances(vec![(account_id, 10)]).build().execute_with(|| {
        assert_eq!(ParachainStaking::total(), 0);
        assert_ok!(ParachainStaking::join_candidates(Origin::signed(account_id), 10u128, 0u32));
        assert_eq!(ParachainStaking::total(), 10);
    });
}

#[test]
fn join_candidates_creates_candidate_state() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default().with_balances(vec![(account_id, 10)]).build().execute_with(|| {
        assert!(ParachainStaking::candidate_info(account_id).is_none());
        assert_ok!(ParachainStaking::join_candidates(Origin::signed(account_id), 10u128, 0u32));
        let candidate_state =
            ParachainStaking::candidate_info(account_id).expect("just joined => exists");
        assert_eq!(candidate_state.bond, 10u128);
    });
}

#[test]
fn join_candidates_adds_to_candidate_pool() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default().with_balances(vec![(account_id, 10)]).build().execute_with(|| {
        assert!(ParachainStaking::candidate_pool().0.is_empty());
        assert_ok!(ParachainStaking::join_candidates(Origin::signed(account_id), 10u128, 0u32));
        let candidate_pool = ParachainStaking::candidate_pool();
        assert_eq!(candidate_pool.0[0].owner, account_id);
        assert_eq!(candidate_pool.0[0].amount, 10);
    });
}

#[test]
fn cannot_join_candidates_if_candidate() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 1000)])
        .with_candidates(vec![(account_id, 500)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::join_candidates(Origin::signed(account_id), 11u128, 100u32),
                Error::<Test>::CandidateExists
            );
        });
}

#[test]
fn cannot_join_candidates_if_nominator() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 50), (account_id_2, 20)])
        .with_candidates(vec![(account_id, 50)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::join_candidates(Origin::signed(account_id_2), 10u128, 1u32),
                Error::<Test>::NominatorExists
            );
        });
}

#[test]
fn cannot_join_candidates_without_min_bond() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default().with_balances(vec![(account_id, 1000)]).build().execute_with(|| {
        assert_noop!(
            ParachainStaking::join_candidates(Origin::signed(account_id), 9u128, 100u32),
            Error::<Test>::CandidateBondBelowMin
        );
    });
}

#[test]
fn cannot_join_candidates_with_more_than_available_balance() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default().with_balances(vec![(account_id, 500)]).build().execute_with(|| {
        assert_noop!(
            ParachainStaking::join_candidates(Origin::signed(account_id), 501u128, 100u32),
            DispatchError::Module(ModuleError {
                index: 2,
                error: [6, 0, 0, 0],
                message: Some("InsufficientBalance")
            })
        );
    });
}

#[test]
fn insufficient_join_candidates_weight_hint_fails() {
    ExtBuilder::default()
        .with_balances(vec![
            (to_acc_id(1), 20),
            (to_acc_id(2), 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
            (to_acc_id(6), 20),
        ])
        .with_candidates(vec![
            (to_acc_id(1), 20),
            (to_acc_id(2), 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
        ])
        .build()
        .execute_with(|| {
            for i in 0..5 {
                assert_noop!(
                    ParachainStaking::join_candidates(Origin::signed(to_acc_id(6)), 20, i),
                    Error::<Test>::TooLowCandidateCountWeightHintJoinCandidates
                );
            }
        });
}

#[test]
fn sufficient_join_candidates_weight_hint_succeeds() {
    ExtBuilder::default()
        .with_balances(vec![
            (to_acc_id(1), 20),
            (to_acc_id(2), 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
            (to_acc_id(6), 20),
            (to_acc_id(7), 20),
            (to_acc_id(8), 20),
            (to_acc_id(9), 20),
        ])
        .with_candidates(vec![
            (to_acc_id(1), 20),
            (to_acc_id(2), 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
        ])
        .build()
        .execute_with(|| {
            let mut count = 5u32;
            for i in 6..10 {
                assert_ok!(ParachainStaking::join_candidates(
                    Origin::signed(to_acc_id(i)),
                    20,
                    count
                ));
                count += 1u32;
            }
        });
}

// SCHEDULE LEAVE CANDIDATES

#[test]
fn leave_candidates_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateScheduledExit {
                exit_allowed_era: 1,
                candidate: account_id,
                scheduled_exit: 3
            }));
        });
}

#[test]
fn leave_candidates_removes_candidate_from_candidate_pool() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::candidate_pool().0.len(), 1);
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            assert!(ParachainStaking::candidate_pool().0.is_empty());
        });
}

#[test]
fn cannot_leave_candidates_if_not_candidate() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::schedule_leave_candidates(Origin::signed(to_acc_id(1)), 1u32),
            Error::<Test>::CandidateDNE
        );
    });
}

#[test]
fn cannot_leave_candidates_if_already_leaving_candidates() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            assert_noop!(
                ParachainStaking::schedule_leave_candidates(Origin::signed(account_id), 1u32),
                Error::<Test>::CandidateAlreadyLeaving
            );
        });
}

#[test]
fn insufficient_leave_candidates_weight_hint_fails() {
    ExtBuilder::default()
        .with_balances(vec![
            (to_acc_id(1), 20),
            (to_acc_id(2), 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
        ])
        .with_candidates(vec![
            (to_acc_id(1), 20),
            (to_acc_id(2), 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
        ])
        .build()
        .execute_with(|| {
            for i in 1..6 {
                assert_noop!(
                    ParachainStaking::schedule_leave_candidates(Origin::signed(to_acc_id(i)), 4u32),
                    Error::<Test>::TooLowCandidateCountToLeaveCandidates
                );
            }
        });
}

#[test]
fn sufficient_leave_candidates_weight_hint_succeeds() {
    ExtBuilder::default()
        .with_balances(vec![
            (to_acc_id(1), 20),
            (to_acc_id(2), 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
        ])
        .with_candidates(vec![
            (to_acc_id(1), 20),
            (to_acc_id(2), 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
        ])
        .build()
        .execute_with(|| {
            let mut count = 5u32;
            for i in 1..6 {
                assert_ok!(ParachainStaking::schedule_leave_candidates(
                    Origin::signed(to_acc_id(i)),
                    count
                ));
                count -= 1u32;
            }
        });
}

// EXECUTE LEAVE CANDIDATES

#[test]
fn execute_leave_candidates_emits_event() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id),
                account_id,
                0
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateLeft {
                ex_candidate: account_id,
                unlocked_amount: 10,
                new_total_amt_locked: 0
            }));
        });
}

#[test]
fn execute_leave_candidates_callable_by_any_signed() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(to_acc_id(2)),
                account_id,
                0
            ));
        });
}

#[test]
fn execute_leave_candidates_requires_correct_weight_hint() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 10),
            (to_acc_id(2), 10),
            (to_acc_id(3), 10),
            (to_acc_id(4), 10),
        ])
        .with_candidates(vec![(account_id, 10)])
        .with_nominations(vec![
            (to_acc_id(2), account_id, 10),
            (to_acc_id(3), account_id, 10),
            (to_acc_id(4), account_id, 10),
        ])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            roll_to(10);
            for i in 0..3 {
                assert_noop!(
                    ParachainStaking::execute_leave_candidates(
                        Origin::signed(account_id),
                        account_id,
                        i
                    ),
                    Error::<Test>::TooLowCandidateNominationCountToLeaveCandidates
                );
            }
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(to_acc_id(2)),
                account_id,
                3
            ));
        });
}

#[test]
fn execute_leave_candidates_unreserves_balance() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 0);
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id),
                account_id,
                0
            ));
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 10);
        });
}

#[test]
fn execute_leave_candidates_decreases_total_staked() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::total(), 10);
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id),
                account_id,
                0
            ));
            assert_eq!(ParachainStaking::total(), 0);
        });
}

#[test]
fn execute_leave_candidates_removes_candidate_state() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            // candidate state is not immediately removed
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("just left => still exists");
            assert_eq!(candidate_state.bond, 10u128);
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id),
                account_id,
                0
            ));
            assert!(ParachainStaking::candidate_info(account_id).is_none());
        });
}

#[test]
fn execute_leave_candidates_removes_pending_nomination_requests() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 10)])
        .with_nominations(vec![(account_id_2, account_id, 15)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_2),
                account_id,
                5
            ));
            let state = ParachainStaking::nomination_scheduled_requests(&account_id);
            assert_eq!(
                state,
                BoundedVec::<
                    ScheduledRequest<sp_core::sr25519::Public, u128>,
                    MaxNominationsPerNominator,
                >::truncate_from(vec![ScheduledRequest {
                    nominator: account_id_2,
                    when_executable: 3,
                    action: NominationAction::Decrease(5),
                }]),
            );
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            // candidate state is not immediately removed
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("just left => still exists");
            assert_eq!(candidate_state.bond, 10u128);
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id),
                account_id,
                1
            ));
            assert!(ParachainStaking::candidate_info(account_id).is_none());
            assert!(
                !ParachainStaking::nomination_scheduled_requests(&account_id)
                    .iter()
                    .any(|x| x.nominator == account_id_2),
                "nomination request not removed"
            );
            assert!(
                !<NominationScheduledRequests<Test>>::contains_key(&account_id),
                "the key was not removed from storage"
            );
        });
}

#[test]
fn cannot_execute_leave_candidates_before_delay() {
    let account_id = to_acc_id(1u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            assert_noop!(
                ParachainStaking::execute_leave_candidates(
                    Origin::signed(account_id_3),
                    account_id,
                    0
                ),
                Error::<Test>::CandidateCannotLeaveYet
            );
            roll_to(9);
            assert_noop!(
                ParachainStaking::execute_leave_candidates(
                    Origin::signed(account_id_3),
                    account_id,
                    0
                ),
                Error::<Test>::CandidateCannotLeaveYet
            );
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id_3),
                account_id,
                0
            ));
        });
}

// CANCEL LEAVE CANDIDATES

#[test]
fn cancel_leave_candidates_emits_event() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            assert_ok!(ParachainStaking::cancel_leave_candidates(Origin::signed(account_id), 1));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CancelledCandidateExit {
                candidate: account_id
            }));
        });
}

#[test]
fn cancel_leave_candidates_updates_candidate_state() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            assert_ok!(ParachainStaking::cancel_leave_candidates(Origin::signed(account_id), 1));
            let candidate = ParachainStaking::candidate_info(&account_id)
                .expect("just cancelled leave so exists");
            assert!(candidate.is_active());
        });
}

#[test]
fn cancel_leave_candidates_adds_to_candidate_pool() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10)])
        .with_candidates(vec![(account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id),
                1u32
            ));
            assert_ok!(ParachainStaking::cancel_leave_candidates(Origin::signed(account_id), 1));
            assert_eq!(ParachainStaking::candidate_pool().0[0].owner, account_id);
            assert_eq!(ParachainStaking::candidate_pool().0[0].amount, 10);
        });
}

// GO OFFLINE

#[test]
fn go_offline_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::go_offline(Origin::signed(account_id)));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateWentOffline {
                candidate: account_id
            }));
        });
}

#[test]
fn go_offline_removes_candidate_from_candidate_pool() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::candidate_pool().0.len(), 1);
            assert_ok!(ParachainStaking::go_offline(Origin::signed(account_id)));
            assert!(ParachainStaking::candidate_pool().0.is_empty());
        });
}

#[test]
fn go_offline_updates_candidate_state_to_idle() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("is active candidate");
            assert_eq!(candidate_state.status, CollatorStatus::Active);
            assert_ok!(ParachainStaking::go_offline(Origin::signed(account_id)));
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("is candidate, just offline");
            assert_eq!(candidate_state.status, CollatorStatus::Idle);
        });
}

#[test]
fn cannot_go_offline_if_not_candidate() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::go_offline(Origin::signed(to_acc_id(3))),
            Error::<Test>::CandidateDNE
        );
    });
}

#[test]
fn cannot_go_offline_if_already_offline() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::go_offline(Origin::signed(account_id)));
            assert_noop!(
                ParachainStaking::go_offline(Origin::signed(account_id)),
                Error::<Test>::AlreadyOffline
            );
        });
}

// GO ONLINE

#[test]
fn go_online_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::go_offline(Origin::signed(account_id)));
            assert_ok!(ParachainStaking::go_online(Origin::signed(account_id)));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateBackOnline {
                candidate: account_id
            }));
        });
}

#[test]
fn go_online_adds_to_candidate_pool() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::go_offline(Origin::signed(account_id)));
            assert!(ParachainStaking::candidate_pool().0.is_empty());
            assert_ok!(ParachainStaking::go_online(Origin::signed(account_id)));
            assert_eq!(ParachainStaking::candidate_pool().0[0].owner, account_id);
            assert_eq!(ParachainStaking::candidate_pool().0[0].amount, 20);
        });
}

#[test]
fn go_online_storage_updates_candidate_state() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::go_offline(Origin::signed(account_id)));
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("offline still exists");
            assert_eq!(candidate_state.status, CollatorStatus::Idle);
            assert_ok!(ParachainStaking::go_online(Origin::signed(account_id)));
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("online so exists");
            assert_eq!(candidate_state.status, CollatorStatus::Active);
        });
}

#[test]
fn cannot_go_online_if_not_candidate() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::go_online(Origin::signed(to_acc_id(3))),
            Error::<Test>::CandidateDNE
        );
    });
}

#[test]
fn cannot_go_online_if_already_online() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::go_online(Origin::signed(account_id)),
                Error::<Test>::AlreadyActive
            );
        });
}

#[test]
fn cannot_go_online_if_leaving() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(Origin::signed(account_id), 1));
            assert_noop!(
                ParachainStaking::go_online(Origin::signed(account_id)),
                Error::<Test>::CannotGoOnlineIfLeaving
            );
        });
}

// CANCEL CANDIDATE BOND LESS REQUEST

#[test]
fn cancel_candidate_unbond_emits_event() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_candidate_unbond(Origin::signed(account_id), 10));
            assert_ok!(ParachainStaking::cancel_candidate_unbond(Origin::signed(account_id)));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CancelledCandidateBondLess {
                candidate: account_id,
                amount: 10,
                execute_era: 3,
            }));
        });
}

#[test]
fn cancel_candidate_unbond_updates_candidate_state() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_candidate_unbond(Origin::signed(account_id), 10));
            assert_ok!(ParachainStaking::cancel_candidate_unbond(Origin::signed(account_id)));
            assert!(ParachainStaking::candidate_info(&account_id).unwrap().request.is_none());
        });
}

#[test]
fn only_candidate_can_cancel_candidate_unbond_request() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_candidate_unbond(Origin::signed(account_id), 10));
            assert_noop!(
                ParachainStaking::cancel_candidate_unbond(Origin::signed(to_acc_id(2))),
                Error::<Test>::CandidateDNE
            );
        });
}

// SCHEDULE LEAVE NOMINATORS

#[test]
fn schedule_leave_nominators_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominatorExitScheduled {
                era: 1,
                nominator: account_id_2,
                scheduled_exit: 3
            }));
        });
}

#[test]
fn cannot_schedule_leave_nominators_if_already_leaving() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            assert_noop!(
                ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)),
                Error::<Test>::NominatorAlreadyLeaving
            );
        });
}

#[test]
fn cannot_schedule_leave_nominators_if_not_nominator() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)),
                Error::<Test>::NominatorDNE
            );
        });
}

// EXECUTE LEAVE NOMINATORS

#[test]
fn execute_leave_nominators_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_2),
                account_id_2,
                1
            ));
            assert_event_emitted!(Event::NominatorLeft {
                nominator: account_id_2,
                unstaked_amount: 10
            });
        });
}

#[test]
fn execute_leave_nominators_unreserves_balance() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 00);
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_2),
                account_id_2,
                1
            ));
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 10);
            assert_eq!(crate::mock::query_lock_amount(account_id_2, NOMINATOR_LOCK_ID), None);
        });
}

#[test]
fn execute_leave_nominators_decreases_total_staked() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::total(), 40);
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_2),
                account_id_2,
                1
            ));
            assert_eq!(ParachainStaking::total(), 30);
        });
}

#[test]
fn execute_leave_nominators_removes_nominator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert!(ParachainStaking::nominator_state(account_id_2).is_some());
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_2),
                account_id_2,
                1
            ));
            assert!(ParachainStaking::nominator_state(account_id_2).is_none());
        });
}

#[test]
fn execute_leave_nominators_removes_pending_nomination_requests() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 10), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 10)])
        .with_nominations(vec![(account_id_2, account_id, 15)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_2),
                account_id,
                5
            ));
            let state = ParachainStaking::nomination_scheduled_requests(&account_id);
            assert_eq!(
                state,
                BoundedVec::<
                    ScheduledRequest<sp_core::sr25519::Public, u128>,
                    MaxNominationsPerNominator,
                >::truncate_from(vec![ScheduledRequest {
                    nominator: account_id_2,
                    when_executable: 3,
                    action: NominationAction::Decrease(5),
                }]),
            );
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_2),
                account_id_2,
                1
            ));
            assert!(ParachainStaking::nominator_state(account_id_2).is_none());
            assert!(
                !ParachainStaking::nomination_scheduled_requests(&account_id)
                    .iter()
                    .any(|x| x.nominator == account_id_2),
                "nomination request not removed"
            )
        });
}

#[test]
fn execute_leave_nominators_removes_nominations_from_collator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 100),
            (account_id_2, 20),
            (account_id_3, 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
        ])
        .with_candidates(vec![
            (account_id_2, 20),
            (account_id_3, 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
        ])
        .with_nominations(vec![
            (account_id, account_id_2, 10),
            (account_id, account_id_3, 10),
            (account_id, to_acc_id(4), 10),
            (account_id, to_acc_id(5), 10),
        ])
        .build()
        .execute_with(|| {
            for i in 2..6 {
                let candidate_state = ParachainStaking::candidate_info(to_acc_id(i))
                    .expect("initialized in ext builder");
                assert_eq!(candidate_state.total_counted, 30);
                let top_nominations = ParachainStaking::top_nominations(to_acc_id(i))
                    .expect("initialized in ext builder");
                assert_eq!(top_nominations.nominations[0].owner, account_id);
                assert_eq!(top_nominations.nominations[0].amount, 10);
                assert_eq!(top_nominations.total, 10);
            }
            assert_eq!(
                ParachainStaking::nominator_state(account_id).unwrap().nominations.0.len(),
                4usize
            );
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id)));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id),
                account_id,
                10
            ));
            for i in 2..6 {
                let candidate_state = ParachainStaking::candidate_info(to_acc_id(i))
                    .expect("initialized in ext builder");
                assert_eq!(candidate_state.total_counted, 20);
                let top_nominations = ParachainStaking::top_nominations(to_acc_id(i))
                    .expect("initialized in ext builder");
                assert!(top_nominations.nominations.is_empty());
            }
        });
}

#[test]
fn cannot_execute_leave_nominators_before_delay() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            assert_noop!(
                ParachainStaking::execute_leave_nominators(
                    Origin::signed(account_id_2),
                    account_id_2,
                    1
                ),
                Error::<Test>::NominatorCannotLeaveYet
            );
            // can execute after delay
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_2),
                account_id_2,
                1
            ));
        });
}

#[test]
fn cannot_execute_leave_nominators_if_single_nomination_revoke_manually_cancelled() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (account_id_3, 30)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            assert_ok!(ParachainStaking::cancel_nomination_request(
                Origin::signed(account_id_2),
                account_id_3
            ));
            roll_to(10);
            assert_noop!(
                ParachainStaking::execute_leave_nominators(
                    Origin::signed(account_id_2),
                    account_id_2,
                    2
                ),
                Error::<Test>::NominatorNotLeaving
            );
            // can execute after manually scheduling revoke, and the era delay after which
            // all revokes can be executed
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id_3
            ));
            roll_to(20);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_2),
                account_id_2,
                2
            ));
        });
}

#[test]
fn insufficient_execute_leave_nominators_weight_hint_fails() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
            (to_acc_id(6), 20),
        ])
        .with_candidates(vec![(account_id, 20)])
        .with_nominations(vec![
            (to_acc_id(3), account_id, 10),
            (to_acc_id(4), account_id, 10),
            (to_acc_id(5), account_id, 10),
            (to_acc_id(6), account_id, 10),
        ])
        .build()
        .execute_with(|| {
            for i in 3..7 {
                assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(to_acc_id(
                    i
                ))));
            }
            roll_to(10);
            for i in 3..7 {
                assert_noop!(
                    ParachainStaking::execute_leave_nominators(
                        Origin::signed(to_acc_id(i)),
                        to_acc_id(i),
                        0
                    ),
                    Error::<Test>::TooLowNominationCountToLeaveNominators
                );
            }
        });
}

#[test]
fn sufficient_execute_leave_nominators_weight_hint_succeeds() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
            (to_acc_id(6), 20),
        ])
        .with_candidates(vec![(account_id, 20)])
        .with_nominations(vec![
            (to_acc_id(3), account_id, 10),
            (to_acc_id(4), account_id, 10),
            (to_acc_id(5), account_id, 10),
            (to_acc_id(6), account_id, 10),
        ])
        .build()
        .execute_with(|| {
            for i in 3..7 {
                assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(to_acc_id(
                    i
                ))));
            }
            roll_to(10);
            for i in 3..7 {
                assert_ok!(ParachainStaking::execute_leave_nominators(
                    Origin::signed(to_acc_id(i)),
                    to_acc_id(i),
                    1
                ));
            }
        });
}

// CANCEL LEAVE NOMINATORS

#[test]
fn cancel_leave_nominators_emits_correct_event() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            assert_ok!(ParachainStaking::cancel_leave_nominators(Origin::signed(account_id_2)));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominatorExitCancelled {
                nominator: account_id_2
            }));
        });
}

#[test]
fn cannot_cancel_leave_nominators_if_single_nomination_revoke_manually_cancelled() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (account_id_3, 30)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            assert_ok!(ParachainStaking::cancel_nomination_request(
                Origin::signed(account_id_2),
                account_id_3
            ));
            roll_to(10);
            assert_noop!(
                ParachainStaking::cancel_leave_nominators(Origin::signed(account_id_2)),
                Error::<Test>::NominatorNotLeaving
            );
            // can execute after manually scheduling revoke, without waiting for era delay after
            // which all revokes can be executed
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id_3
            ));
            assert_ok!(ParachainStaking::cancel_leave_nominators(Origin::signed(account_id_2)));
        });
}

// CANCEL PENDING NOMINATION REQUEST
// 1. CANCEL REVOKE NOMINATION

#[test]
fn cancel_revoke_nomination_emits_correct_event() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id
            ));
            assert_ok!(ParachainStaking::cancel_nomination_request(
                Origin::signed(account_id_2),
                account_id
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CancelledNominationRequest {
                nominator: account_id_2,
                collator: account_id,
                cancelled_request: CancelledScheduledRequest {
                    when_executable: 3,
                    action: NominationAction::Revoke(10),
                },
            }));
        });
}

#[test]
fn cancel_revoke_nomination_updates_nominator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id
            ));
            let state = ParachainStaking::nomination_scheduled_requests(&account_id);
            assert_eq!(
                state,
                vec![ScheduledRequest {
                    nominator: account_id_2,
                    when_executable: 3,
                    action: NominationAction::Revoke(10),
                }],
            );
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                10
            );
            assert_ok!(ParachainStaking::cancel_nomination_request(
                Origin::signed(account_id_2),
                account_id
            ));
            assert!(!ParachainStaking::nomination_scheduled_requests(&account_id)
                .iter()
                .any(|x| x.nominator == account_id_2));
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                0
            );
        });
}

// 2. CANCEL NOMINATOR BOND LESS

#[test]
fn cancel_nominator_unbond_correct_event() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 15)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_2),
                account_id,
                5
            ));
            assert_ok!(ParachainStaking::cancel_nomination_request(
                Origin::signed(account_id_2),
                account_id
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CancelledNominationRequest {
                nominator: account_id_2,
                collator: account_id,
                cancelled_request: CancelledScheduledRequest {
                    when_executable: 3,
                    action: NominationAction::Decrease(5),
                },
            }));
        });
}

#[test]
fn cancel_nominator_unbond_updates_nominator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 15)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_2),
                account_id,
                5
            ));
            let state = ParachainStaking::nomination_scheduled_requests(&account_id);
            assert_eq!(
                state,
                vec![ScheduledRequest {
                    nominator: account_id_2,
                    when_executable: 3,
                    action: NominationAction::Decrease(5),
                }],
            );
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                5
            );
            assert_ok!(ParachainStaking::cancel_nomination_request(
                Origin::signed(account_id_2),
                account_id
            ));
            assert!(!ParachainStaking::nomination_scheduled_requests(&account_id)
                .iter()
                .any(|x| x.nominator == account_id_2));
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                0
            );
        });
}

// ~~ PROPERTY-BASED TESTS ~~

#[test]
fn nominator_schedule_revocation_total() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 40),
            (account_id_3, 20),
            (account_id_4, 20),
            (to_acc_id(5), 20),
        ])
        .with_candidates(vec![
            (account_id, 20),
            (account_id_3, 20),
            (account_id_4, 20),
            (to_acc_id(5), 20),
        ])
        .with_nominations(vec![
            (account_id_2, account_id, 10),
            (account_id_2, account_id_3, 10),
            (account_id_2, account_id_4, 10),
        ])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id
            ));
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                10
            );
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                0
            );
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_2),
                to_acc_id(5),
                10,
                0,
                2
            ));
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id_3
            ));
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id_4
            ));
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                20,
            );
            roll_to(20);
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_2),
                account_id_2,
                account_id_3
            ));
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                10,
            );
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_2),
                account_id_2,
                account_id_4
            ));
            assert_eq!(
                ParachainStaking::nominator_state(&account_id_2)
                    .map(|x| x.less_total)
                    .expect("nominator state must exist"),
                0
            );
        });
}

#[test]
fn parachain_flow_reserve_matches_config() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    let account_id_5 = to_acc_id(5u64);
    let account_id_6 = to_acc_id(6u64);
    let account_id_7 = to_acc_id(7u64);
    let account_id_10 = to_acc_id(10u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 100),
            (account_id_2, 100),
            (account_id_3, 100),
            (account_id_4, 100),
            (account_id_5, 100),
            (account_id_6, 100),
            (account_id_7, 100),
            (to_acc_id(8), 100),
            (to_acc_id(9), 100),
            (account_id_10, 100),
            (to_acc_id(11), 1),
        ])
        .with_candidates(vec![
            (account_id, 20),
            (account_id_2, 20),
            (account_id_3, 20),
            (account_id_4, 20),
            (account_id_5, 10),
        ])
        .with_nominations(vec![
            (account_id_6, account_id, 10),
            (account_id_7, account_id, 10),
            (to_acc_id(8), account_id_2, 10),
            (to_acc_id(9), account_id_2, 10),
            (account_id_10, account_id, 10),
        ])
        .build()
        .execute_with(|| {
            assert_eq!(Balances::free_balance(&to_acc_id(11)), 1);
            roll_to(8);
            // chooses top TotalSelectedCandidates (5), in order
            let mut expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
            ];
            assert_eq_events!(expected.clone());
            // ~ set block author as 1 for all blocks this era
            set_author(2, account_id, 100);
            // We now payout from a central pot so we need to fund it
            set_reward_pot(50);
            roll_to(16);
            // distribute total issuance to collator 1 and its nominators 6, 7, 19
            let mut new = vec![
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 10,
                    era: 3,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 15,
                    era: 4,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded {
                    /*Explanation of how reward is computed:
                        Total staked = 50
                        Total reward to be paid = 50
                        Collator stake = 20

                        collator gets 40% ([collator stake] * 100 / [total staked]) of the [total reward] = 20 (40 * 50 / 100)
                        Total 20
                    */
                    account: account_id,
                    rewards: 20,
                },
                Event::Rewarded {
                    /*Explanation of how reward is computed:
                        Total staked = 50
                        Total reward to be paid = 50
                        Nominator stake = 10

                        nominator gets 20% ([nominator stake] * 100 / [total staked]) of 50 ([total reward]) = 10
                        Total 10
                    */
                    account: account_id_6,
                    rewards: 10,
                },
                Event::Rewarded { account: account_id_7, rewards: 10 },
                Event::Rewarded { account: account_id_10, rewards: 10 },
            ];
            expected.append(&mut new);
            assert_eq_events!(expected.clone());
            // ~ set block author as 1 for all blocks this era
            set_author(3, account_id, 100);
            set_author(4, account_id, 100);
            set_author(5, account_id, 100);
            // 1. ensure nominators are paid for 2 eras after they leave
            assert_noop!(
                ParachainStaking::schedule_leave_nominators(Origin::signed(to_acc_id(66))),
                Error::<Test>::NominatorDNE
            );
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_6)));
            // fast forward to block in which nominator 6 exit executes. Doing it in 2 steps so we
            // can reset the reward pot
            set_reward_pot(55);
            roll_to(20);

            set_reward_pot(56);
            roll_to(25);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_6),
                account_id_6,
                10
            ));
            set_reward_pot(58);
            roll_to(30);
            let mut new2 = vec![
                Event::NominatorExitScheduled {
                    era: 4,
                    nominator: account_id_6,
                    scheduled_exit: 6,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 20,
                    era: 5,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 22 },
                Event::Rewarded { account: account_id_6, rewards: 11 },
                Event::Rewarded { account: account_id_7, rewards: 11 },
                Event::Rewarded { account: account_id_10, rewards: 11 },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 25,
                    era: 6,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 22 },
                Event::Rewarded { account: account_id_6, rewards: 11 },
                Event::Rewarded { account: account_id_7, rewards: 11 },
                Event::Rewarded { account: account_id_10, rewards: 11 },
                Event::NominatorLeftCandidate {
                    nominator: account_id_6,
                    candidate: account_id,
                    unstaked_amount: 10,
                    total_candidate_staked: 40,
                },
                Event::NominatorLeft { nominator: account_id_6, unstaked_amount: 10 },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 30,
                    era: 7,
                    selected_collators_number: 5,
                    total_balance: 130,
                },
                Event::Rewarded { account: account_id, rewards: 29 },
                Event::Rewarded { account: account_id_7, rewards: 14 },
                Event::Rewarded { account: account_id_10, rewards: 14 },
            ];
            expected.append(&mut new2);
            assert_eq_events!(expected.clone());
            // 6 won't be paid for this era because they left already
            set_author(6, account_id, 100);
            set_reward_pot(61);
            roll_to(35);
            // keep paying 6
            let mut new3 = vec![
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 35,
                    era: 8,
                    selected_collators_number: 5,
                    total_balance: 130,
                },
                Event::Rewarded { account: account_id, rewards: 30 },
                Event::Rewarded { account: account_id_7, rewards: 15 },
                Event::Rewarded { account: account_id_10, rewards: 15 },
            ];
            expected.append(&mut new3);
            assert_eq_events!(expected.clone());
            set_author(7, account_id, 100);
            set_reward_pot(64);
            roll_to(40);
            // no more paying 6
            let mut new4 = vec![
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 40,
                    era: 9,
                    selected_collators_number: 5,
                    total_balance: 130,
                },
                Event::Rewarded { account: account_id, rewards: 32 },
                Event::Rewarded { account: account_id_7, rewards: 16 },
                Event::Rewarded { account: account_id_10, rewards: 16 },
            ];
            expected.append(&mut new4);
            assert_eq_events!(expected.clone());
            set_author(8, account_id, 100);
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(to_acc_id(8)),
                account_id,
                10,
                10,
                10
            ));
            set_reward_pot(67);
            roll_to(45);
            // new nomination is not rewarded yet
            let mut new5 = vec![
                Event::Nomination {
                    nominator: to_acc_id(8),
                    locked_amount: 10,
                    candidate: account_id,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 50 },
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 45,
                    era: 10,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 33 },
                Event::Rewarded { account: account_id_7, rewards: 17 },
                Event::Rewarded { account: account_id_10, rewards: 17 },
            ];
            expected.append(&mut new5);
            assert_eq_events!(expected.clone());
            set_author(9, account_id, 100);
            set_author(10, account_id, 100);
            set_reward_pot(70);
            roll_to(50);
            // new nomination is still not rewarded yet
            let mut new6 = vec![
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 50,
                    era: 11,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 35 },
                Event::Rewarded { account: account_id_7, rewards: 17 },
                Event::Rewarded { account: account_id_10, rewards: 17 },
            ];
            expected.append(&mut new6);
            assert_eq_events!(expected.clone());
            set_reward_pot(75);
            roll_to(55);
            // new nomination is rewarded, 2 eras after joining (`RewardPaymentDelay` is 2)
            let mut new7 = vec![
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 55,
                    era: 12,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 30 },
                Event::Rewarded { account: account_id_7, rewards: 15 },
                Event::Rewarded { account: account_id_10, rewards: 15 },
                Event::Rewarded { account: to_acc_id(8), rewards: 15 },
            ];
            expected.append(&mut new7);
            assert_eq_events!(expected);
        });
}
// Working \^/ --------------------------
#[test]
fn rewards_matches_config() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    let account_id_5 = to_acc_id(5u64);
    let account_id_6 = to_acc_id(6u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 100),
            (account_id_2, 100),
            (account_id_3, 100),
            (account_id_4, 100),
            (account_id_5, 100),
            (account_id_6, 100),
        ])
        .with_candidates(vec![(account_id, 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_3, account_id, 10)])
        .build()
        .execute_with(|| {
            roll_to(8);
            // chooses top TotalSelectedCandidates (5), in order
            let mut expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 1,
                    total_balance: 40,
                },
            ];
            assert_eq_events!(expected.clone());
            assert_ok!(ParachainStaking::join_candidates(
                Origin::signed(account_id_4),
                20u128,
                100u32
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::JoinedCollatorCandidates {
                account: account_id_4,
                amount_locked: 20u128,
                new_total_amt_locked: 60u128,
            }));
            roll_to(9);
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_5),
                account_id_4,
                10,
                10,
                10
            ));
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_6),
                account_id_4,
                10,
                10,
                10
            ));
            roll_to(11);
            let mut new = vec![
                Event::JoinedCollatorCandidates {
                    account: account_id_4,
                    amount_locked: 20,
                    new_total_amt_locked: 60,
                },
                Event::Nomination {
                    nominator: account_id_5,
                    locked_amount: 10,
                    candidate: account_id_4,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 30 },
                },
                Event::Nomination {
                    nominator: account_id_6,
                    locked_amount: 10,
                    candidate: account_id_4,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 40 },
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_4,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 10,
                    era: 3,
                    selected_collators_number: 2,
                    total_balance: 80,
                },
            ];
            expected.append(&mut new);
            assert_eq_events!(expected.clone());
            // only reward author with id 4
            set_author(3, account_id_4, 100);
            set_reward_pot(30);
            roll_to(21);
            // 20% of 10 is commission + due_portion (0) = 2 + 4 = 6
            // all nominator payouts are 10-2 = 8 * stake_pct
            let mut new2 = vec![
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_4,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 15,
                    era: 4,
                    selected_collators_number: 2,
                    total_balance: 80,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_4,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 20,
                    era: 5,
                    selected_collators_number: 2,
                    total_balance: 80,
                },
                Event::Rewarded { account: account_id_4, rewards: 15 },
                Event::Rewarded { account: account_id_5, rewards: 7 },
                Event::Rewarded { account: account_id_6, rewards: 7 },
            ];
            expected.append(&mut new2);
            assert_eq_events!(expected);
        });
}

#[test]
fn collator_exit_executes_after_delay() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 1000),
            (account_id_2, 300),
            (to_acc_id(3), 100),
            (to_acc_id(4), 100),
            (to_acc_id(5), 100),
            (to_acc_id(6), 100),
            (to_acc_id(7), 100),
            (to_acc_id(8), 9),
            (to_acc_id(9), 4),
        ])
        .with_candidates(vec![(account_id, 500), (account_id_2, 200)])
        .with_nominations(vec![
            (to_acc_id(3), account_id, 100),
            (to_acc_id(4), account_id, 100),
            (to_acc_id(5), account_id_2, 100),
            (to_acc_id(6), account_id_2, 100),
        ])
        .build()
        .execute_with(|| {
            roll_to(11);
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id_2),
                2
            ));
            let info = ParachainStaking::candidate_info(&account_id_2).unwrap();
            assert_eq!(info.status, CollatorStatus::Leaving(5));
            roll_to(21);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id_2),
                account_id_2,
                2
            ));
            // we must exclude leaving collators from rewards while
            // holding them retroactively accountable for previous faults
            // (within the last T::SlashingWindow blocks)
            let expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 700,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_2,
                    total_exposed_amount: 400,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 2,
                    total_balance: 1100,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id,
                    total_exposed_amount: 700,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_2,
                    total_exposed_amount: 400,
                },
                Event::NewEra {
                    starting_block: 10,
                    era: 3,
                    selected_collators_number: 2,
                    total_balance: 1100,
                },
                Event::CandidateScheduledExit {
                    exit_allowed_era: 3,
                    candidate: account_id_2,
                    scheduled_exit: 5,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id,
                    total_exposed_amount: 700,
                },
                Event::NewEra {
                    starting_block: 15,
                    era: 4,
                    selected_collators_number: 1,
                    total_balance: 700,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id,
                    total_exposed_amount: 700,
                },
                Event::NewEra {
                    starting_block: 20,
                    era: 5,
                    selected_collators_number: 1,
                    total_balance: 700,
                },
                Event::CandidateLeft {
                    ex_candidate: account_id_2,
                    unlocked_amount: 400,
                    new_total_amt_locked: 700,
                },
            ];
            assert_eq_events!(expected);
        });
}

#[test]
fn collator_selection_chooses_top_candidates() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    let account_id_5 = to_acc_id(5u64);
    let account_id_6 = to_acc_id(6u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 1000),
            (account_id_2, 1000),
            (account_id_3, 1000),
            (account_id_4, 1000),
            (account_id_5, 1000),
            (account_id_6, 1000),
            (to_acc_id(7), 33),
            (to_acc_id(8), 33),
            (to_acc_id(9), 33),
        ])
        .with_candidates(vec![
            (account_id, 100),
            (account_id_2, 90),
            (account_id_3, 80),
            (account_id_4, 70),
            (account_id_5, 60),
            (account_id_6, 50),
        ])
        .build()
        .execute_with(|| {
            roll_to(8);
            // should choose top TotalSelectedCandidates (5), in order
            let expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
            ];
            assert_eq_events!(expected.clone());
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id_6),
                6
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateScheduledExit {
                exit_allowed_era: 2,
                candidate: account_id_6,
                scheduled_exit: 4
            }));
            roll_to(21);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id_6),
                account_id_6,
                0
            ));
            assert_ok!(ParachainStaking::join_candidates(
                Origin::signed(account_id_6),
                69u128,
                100u32
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::JoinedCollatorCandidates {
                account: account_id_6,
                amount_locked: 69u128,
                new_total_amt_locked: 469u128,
            }));
            roll_to(27);
            // should choose top TotalSelectedCandidates (5), in order
            let expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::CandidateScheduledExit {
                    exit_allowed_era: 2,
                    candidate: account_id_6,
                    scheduled_exit: 4,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 10,
                    era: 3,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 15,
                    era: 4,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 20,
                    era: 5,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::CandidateLeft {
                    ex_candidate: account_id_6,
                    unlocked_amount: 50,
                    new_total_amt_locked: 400,
                },
                Event::JoinedCollatorCandidates {
                    account: account_id_6,
                    amount_locked: 69,
                    new_total_amt_locked: 469,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_6,
                    total_exposed_amount: 69,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 25,
                    era: 6,
                    selected_collators_number: 5,
                    total_balance: 409,
                },
            ];
            assert_eq_events!(expected);
        });
}

#[test]
fn payout_distribution_to_solo_collators() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    let account_id_5 = to_acc_id(5u64);
    let account_id_6 = to_acc_id(6u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 1000),
            (account_id_2, 1000),
            (account_id_3, 1000),
            (account_id_4, 1000),
            (account_id_5, 1000),
            (to_acc_id(6), 1000),
            (to_acc_id(7), 33),
            (to_acc_id(8), 33),
            (to_acc_id(9), 33),
        ])
        .with_candidates(vec![
            (account_id, 100),
            (account_id_2, 90),
            (account_id_3, 80),
            (account_id_4, 70),
            (account_id_5, 60),
            (account_id_6, 50),
        ])
        .build()
        .execute_with(|| {
            roll_to(8);
            // should choose top TotalCandidatesSelected (5), in order
            let mut expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
            ];
            assert_eq_events!(expected.clone());
            // ~ set block author as 1 for all blocks this era
            set_author(2, account_id, 100);
            set_reward_pot(305);
            roll_to(16);
            // pay total issuance to 1
            let mut new = vec![
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 10,
                    era: 3,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 15,
                    era: 4,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::Rewarded { account: account_id, rewards: 305 },
            ];
            expected.append(&mut new);
            assert_eq_events!(expected.clone());
            // ~ set block author as 1 for 3 blocks this era
            set_author(4, account_id, 60);
            // ~ set block author as 2 for 2 blocks this era
            set_author(4, account_id_2, 40);
            set_reward_pot(320);
            roll_to(26);
            // pay 60% total issuance to 1 and 40% total issuance to 2
            let mut new1 = vec![
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 20,
                    era: 5,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 25,
                    era: 6,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::Rewarded { account: account_id, rewards: 192 },
                Event::Rewarded { account: account_id_2, rewards: 128 },
            ];
            expected.append(&mut new1);
            assert_eq_events!(expected.clone());
            // ~ each collator produces 1 block this era
            set_author(6, account_id, 20);
            set_author(6, account_id_2, 20);
            set_author(6, account_id_3, 20);
            set_author(6, account_id_4, 20);
            set_author(6, account_id_5, 20);
            set_reward_pot(336);
            roll_to(39);
            // pay 20% issuance for all collators
            let mut new2 = vec![
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 30,
                    era: 7,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_5,
                    total_exposed_amount: 60,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_3,
                    total_exposed_amount: 80,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_4,
                    total_exposed_amount: 70,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id,
                    total_exposed_amount: 100,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_2,
                    total_exposed_amount: 90,
                },
                Event::NewEra {
                    starting_block: 35,
                    era: 8,
                    selected_collators_number: 5,
                    total_balance: 400,
                },
                Event::Rewarded { account: account_id_5, rewards: 67 },
                Event::Rewarded { account: account_id, rewards: 67 },
                Event::Rewarded { account: account_id_2, rewards: 67 },
                Event::Rewarded { account: account_id_4, rewards: 67 },
                Event::Rewarded { account: account_id_3, rewards: 67 },
            ];
            expected.append(&mut new2);
            assert_eq_events!(expected);
            // check that distributing rewards clears awarded pts
            assert!(ParachainStaking::awarded_pts(1, account_id).is_zero());
            assert!(ParachainStaking::awarded_pts(4, account_id).is_zero());
            assert!(ParachainStaking::awarded_pts(4, account_id_2).is_zero());
            assert!(ParachainStaking::awarded_pts(6, account_id).is_zero());
            assert!(ParachainStaking::awarded_pts(6, account_id_2).is_zero());
            assert!(ParachainStaking::awarded_pts(6, account_id_3).is_zero());
            assert!(ParachainStaking::awarded_pts(6, account_id_4).is_zero());
            assert!(ParachainStaking::awarded_pts(6, account_id_5).is_zero());
        });
}

#[test]
fn multiple_nominations() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    let account_id_5 = to_acc_id(5u64);
    let account_id_6 = to_acc_id(6u64);
    let account_id_7 = to_acc_id(7u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 100),
            (account_id_2, 100),
            (account_id_3, 100),
            (account_id_4, 100),
            (account_id_5, 100),
            (account_id_6, 100),
            (account_id_7, 100),
            (to_acc_id(8), 100),
            (to_acc_id(9), 100),
            (to_acc_id(10), 100),
        ])
        .with_candidates(vec![
            (account_id, 20),
            (account_id_2, 20),
            (account_id_3, 20),
            (account_id_4, 20),
            (account_id_5, 10),
        ])
        .with_nominations(vec![
            (account_id_6, account_id, 10),
            (account_id_7, account_id, 10),
            (to_acc_id(8), account_id_2, 10),
            (to_acc_id(9), account_id_2, 10),
            (to_acc_id(10), account_id, 10),
        ])
        .build()
        .execute_with(|| {
            roll_to(8);
            // chooses top TotalSelectedCandidates (5), in order
            let mut expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
            ];
            assert_eq_events!(expected.clone());
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_6),
                account_id_2,
                10,
                10,
                10
            ));
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_6),
                account_id_3,
                10,
                10,
                10
            ));
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_6),
                account_id_4,
                10,
                10,
                10
            ));
            roll_to(16);
            let mut new = vec![
                Event::Nomination {
                    nominator: account_id_6,
                    locked_amount: 10,
                    candidate: account_id_2,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 50 },
                },
                Event::Nomination {
                    nominator: account_id_6,
                    locked_amount: 10,
                    candidate: account_id_3,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 30 },
                },
                Event::Nomination {
                    nominator: account_id_6,
                    locked_amount: 10,
                    candidate: account_id_4,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 30 },
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_3,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_4,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_2,
                    total_exposed_amount: 50,
                },
                Event::NewEra {
                    starting_block: 10,
                    era: 3,
                    selected_collators_number: 5,
                    total_balance: 170,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_3,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_4,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_2,
                    total_exposed_amount: 50,
                },
                Event::NewEra {
                    starting_block: 15,
                    era: 4,
                    selected_collators_number: 5,
                    total_balance: 170,
                },
            ];
            expected.append(&mut new);
            assert_eq_events!(expected.clone());
            roll_to(21);
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_7),
                account_id_2,
                80,
                10,
                10
            ));
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(to_acc_id(10)),
                account_id_2,
                10,
                10,
                10
            ),);
            roll_to(26);
            let mut new2 = vec![
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_3,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_4,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_2,
                    total_exposed_amount: 50,
                },
                Event::NewEra {
                    starting_block: 20,
                    era: 5,
                    selected_collators_number: 5,
                    total_balance: 170,
                },
                Event::Nomination {
                    nominator: account_id_7,
                    locked_amount: 80,
                    candidate: account_id_2,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 130 },
                },
                Event::Nomination {
                    nominator: to_acc_id(10),
                    locked_amount: 10,
                    candidate: account_id_2,
                    nominator_position: NominatorAdded::AddedToBottom,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_3,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_4,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_2,
                    total_exposed_amount: 130,
                },
                Event::NewEra {
                    starting_block: 25,
                    era: 6,
                    selected_collators_number: 5,
                    total_balance: 250,
                },
            ];
            expected.append(&mut new2);
            assert_eq_events!(expected.clone());
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id_2),
                5
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateScheduledExit {
                exit_allowed_era: 6,
                candidate: account_id_2,
                scheduled_exit: 8
            }));
            roll_to(31);
            let mut new3 = vec![
                Event::CandidateScheduledExit {
                    exit_allowed_era: 6,
                    candidate: account_id_2,
                    scheduled_exit: 8,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_3,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_4,
                    total_exposed_amount: 30,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::NewEra {
                    starting_block: 30,
                    era: 7,
                    selected_collators_number: 4,
                    total_balance: 120,
                },
            ];
            expected.append(&mut new3);
            assert_eq_events!(expected);
            // verify that nominations are removed after collator leaves, not before
            assert_eq!(ParachainStaking::nominator_state(account_id_7).unwrap().total, 90);
            assert_eq!(
                ParachainStaking::nominator_state(account_id_7).unwrap().nominations.0.len(),
                2usize
            );
            assert_eq!(ParachainStaking::nominator_state(account_id_6).unwrap().total, 40);
            assert_eq!(
                ParachainStaking::nominator_state(account_id_6).unwrap().nominations.0.len(),
                4usize
            );
            assert_eq!(Balances::locks(&account_id_6)[0].amount, 40);
            assert_eq!(Balances::locks(&account_id_7)[0].amount, 90);
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_6), 60);
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_7), 10);
            roll_to(40);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id_2),
                account_id_2,
                5
            ));
            assert_eq!(ParachainStaking::nominator_state(account_id_7).unwrap().total, 10);
            assert_eq!(ParachainStaking::nominator_state(account_id_6).unwrap().total, 30);
            assert_eq!(
                ParachainStaking::nominator_state(account_id_7).unwrap().nominations.0.len(),
                1usize
            );
            assert_eq!(
                ParachainStaking::nominator_state(account_id_6).unwrap().nominations.0.len(),
                3usize
            );
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_6), 70);
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_7), 90);
        });
}

#[test]
// The test verifies that the pending revoke request is removed by 2's exit so there is no dangling
// revoke request after 2 exits
fn execute_leave_candidate_removes_nominations() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 100),
            (account_id_2, 100),
            (account_id_3, 100),
            (account_id_4, 100),
        ])
        .with_candidates(vec![(account_id, 20), (account_id_2, 20)])
        .with_nominations(vec![
            (account_id_3, account_id, 10),
            (account_id_3, account_id_2, 10),
            (account_id_4, account_id, 10),
            (account_id_4, account_id_2, 10),
        ])
        .build()
        .execute_with(|| {
            // Verifies the revocation request is initially empty
            assert!(!ParachainStaking::nomination_scheduled_requests(&account_id_2)
                .iter()
                .any(|x| x.nominator == account_id_3));

            assert_ok!(ParachainStaking::schedule_leave_candidates(
                Origin::signed(account_id_2),
                2
            ));
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_3),
                account_id_2
            ));
            // Verifies the revocation request is present
            assert!(ParachainStaking::nomination_scheduled_requests(&account_id_2)
                .iter()
                .any(|x| x.nominator == account_id_3));

            roll_to(16);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                Origin::signed(account_id_2),
                account_id_2,
                2
            ));
            // Verifies the revocation request is again empty
            assert!(!ParachainStaking::nomination_scheduled_requests(&account_id_2)
                .iter()
                .any(|x| x.nominator == account_id_3));
        });
}

#[test]
fn payouts_follow_nomination_changes() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    let account_id_5 = to_acc_id(5u64);
    let account_id_6 = to_acc_id(6u64);
    let account_id_7 = to_acc_id(7u64);
    let account_id_10 = to_acc_id(10u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 100),
            (account_id_2, 100),
            (account_id_3, 100),
            (account_id_4, 100),
            (account_id_5, 100),
            (account_id_6, 100),
            (account_id_7, 100),
            (to_acc_id(8), 100),
            (to_acc_id(9), 100),
            (account_id_10, 100),
        ])
        .with_candidates(vec![
            (account_id, 20),
            (account_id_2, 20),
            (account_id_3, 20),
            (account_id_4, 20),
            (account_id_5, 10),
        ])
        .with_nominations(vec![
            (account_id_6, account_id, 10),
            (account_id_7, account_id, 10),
            (to_acc_id(8), account_id_2, 10),
            (to_acc_id(9), account_id_2, 10),
            (account_id_10, account_id, 10),
        ])
        .build()
        .execute_with(|| {
            roll_to(8);
            // chooses top TotalSelectedCandidates (5), in order
            let mut expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
            ];
            assert_eq_events!(expected.clone());
            // ~ set block author as 1 for all blocks this era
            set_author(2, account_id, 100);
            set_reward_pot(50);
            roll_to(16);
            // distribute total reward to collator 1 and its nominators 6, 7, 19
            let mut new = vec![
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 10,
                    era: 3,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 4,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 15,
                    era: 4,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 20 },
                Event::Rewarded { account: account_id_6, rewards: 10 },
                Event::Rewarded { account: account_id_7, rewards: 10 },
                Event::Rewarded { account: account_id_10, rewards: 10 },
            ];
            expected.append(&mut new);
            assert_eq_events!(expected.clone());
            // ~ set block author as 1 for all blocks this era
            set_author(3, account_id, 100);
            set_author(4, account_id, 100);
            set_author(5, account_id, 100);
            set_author(6, account_id, 100);
            // 1. ensure nominators are paid for 2 eras after they leave
            assert_noop!(
                ParachainStaking::schedule_leave_nominators(Origin::signed(to_acc_id(66))),
                Error::<Test>::NominatorDNE
            );
            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_6)));
            // fast forward to block in which nominator 6 exit executes
            set_reward_pot(52);
            roll_to(20);

            set_reward_pot(56);
            roll_to(25);

            assert_ok!(ParachainStaking::execute_leave_nominators(
                Origin::signed(account_id_6),
                account_id_6,
                10
            ));
            // keep paying 6
            let mut new2 = vec![
                Event::NominatorExitScheduled {
                    era: 4,
                    nominator: account_id_6,
                    scheduled_exit: 6,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 5,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 20,
                    era: 5,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 21 },
                Event::Rewarded { account: account_id_6, rewards: 10 },
                Event::Rewarded { account: account_id_7, rewards: 10 },
                Event::Rewarded { account: account_id_10, rewards: 10 },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 6,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 25,
                    era: 6,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 22 },
                Event::Rewarded { account: account_id_6, rewards: 11 },
                Event::Rewarded { account: account_id_7, rewards: 11 },
                Event::Rewarded { account: account_id_10, rewards: 11 },
                Event::NominatorLeftCandidate {
                    nominator: account_id_6,
                    candidate: account_id,
                    unstaked_amount: 10,
                    total_candidate_staked: 40,
                },
                Event::NominatorLeft { nominator: account_id_6, unstaked_amount: 10 },
            ];
            expected.append(&mut new2);
            assert_eq_events!(expected.clone());
            // 6 won't be paid for this era because they left already
            set_author(7, account_id, 100);
            set_reward_pot(58);
            roll_to(30);
            set_reward_pot(61);
            roll_to(35);
            // keep paying 6
            let mut new3 = vec![
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 7,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 30,
                    era: 7,
                    selected_collators_number: 5,
                    total_balance: 130,
                },
                Event::Rewarded { account: account_id, rewards: 29 },
                Event::Rewarded { account: account_id_7, rewards: 14 },
                Event::Rewarded { account: account_id_10, rewards: 14 },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 8,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 35,
                    era: 8,
                    selected_collators_number: 5,
                    total_balance: 130,
                },
                Event::Rewarded { account: account_id, rewards: 30 },
                Event::Rewarded { account: account_id_7, rewards: 15 },
                Event::Rewarded { account: account_id_10, rewards: 15 },
            ];
            expected.append(&mut new3);
            assert_eq_events!(expected.clone());
            set_author(8, account_id, 100);
            set_reward_pot(64);
            roll_to(40);
            // no more paying 6
            let mut new4 = vec![
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id,
                    total_exposed_amount: 40,
                },
                Event::CollatorChosen {
                    era: 9,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 40,
                    era: 9,
                    selected_collators_number: 5,
                    total_balance: 130,
                },
                Event::Rewarded { account: account_id, rewards: 32 },
                Event::Rewarded { account: account_id_7, rewards: 16 },
                Event::Rewarded { account: account_id_10, rewards: 16 },
            ];
            expected.append(&mut new4);
            assert_eq_events!(expected.clone());
            set_author(9, account_id, 100);
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(to_acc_id(8)),
                account_id,
                10,
                10,
                10
            ));
            set_reward_pot(67);
            roll_to(45);
            // new nomination is not rewarded yet
            let mut new5 = vec![
                Event::Nomination {
                    nominator: to_acc_id(8),
                    locked_amount: 10,
                    candidate: account_id,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 50 },
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 10,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 45,
                    era: 10,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 33 },
                Event::Rewarded { account: account_id_7, rewards: 17 },
                Event::Rewarded { account: account_id_10, rewards: 17 },
            ];
            expected.append(&mut new5);
            assert_eq_events!(expected.clone());
            set_author(10, account_id, 100);
            set_reward_pot(70);
            roll_to(50);
            // new nomination not rewarded yet
            let mut new6 = vec![
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 11,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 50,
                    era: 11,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 35 },
                Event::Rewarded { account: account_id_7, rewards: 17 },
                Event::Rewarded { account: account_id_10, rewards: 17 },
            ];
            expected.append(&mut new6);
            assert_eq_events!(expected.clone());
            set_reward_pot(75);
            roll_to(55);
            // new nomination is rewarded for first time
            // 2 eras after joining (`RewardPaymentDelay` = 2)
            let mut new7 = vec![
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id_5,
                    total_exposed_amount: 10,
                },
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id,
                    total_exposed_amount: 50,
                },
                Event::CollatorChosen {
                    era: 12,
                    collator_account: account_id_2,
                    total_exposed_amount: 40,
                },
                Event::NewEra {
                    starting_block: 55,
                    era: 12,
                    selected_collators_number: 5,
                    total_balance: 140,
                },
                Event::Rewarded { account: account_id, rewards: 30 },
                Event::Rewarded { account: account_id_7, rewards: 15 },
                Event::Rewarded { account: account_id_10, rewards: 15 },
                Event::Rewarded { account: to_acc_id(8), rewards: 15 },
            ];
            expected.append(&mut new7);
            assert_eq_events!(expected);
        });
}

#[test]
fn bottom_nominations_are_empty_when_top_nominations_not_full() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 10),
            (account_id_3, 10),
            (to_acc_id(4), 10),
            (to_acc_id(5), 10),
        ])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            // no top nominators => no bottom nominators
            let top_nominations = ParachainStaking::top_nominations(account_id).unwrap();
            let bottom_nominations = ParachainStaking::bottom_nominations(account_id).unwrap();
            assert!(top_nominations.nominations.is_empty());
            assert!(bottom_nominations.nominations.is_empty());
            // 1 nominator => 1 top nominator, 0 bottom nominators
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_2),
                account_id,
                10,
                10,
                10
            ));
            let top_nominations = ParachainStaking::top_nominations(account_id).unwrap();
            let bottom_nominations = ParachainStaking::bottom_nominations(account_id).unwrap();
            assert_eq!(top_nominations.nominations.len(), 1usize);
            assert!(bottom_nominations.nominations.is_empty());
            // 2 nominators => 2 top nominators, 0 bottom nominators
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(account_id_3),
                account_id,
                10,
                10,
                10
            ));
            let top_nominations = ParachainStaking::top_nominations(account_id).unwrap();
            let bottom_nominations = ParachainStaking::bottom_nominations(account_id).unwrap();
            assert_eq!(top_nominations.nominations.len(), 2usize);
            assert!(bottom_nominations.nominations.is_empty());
            // 3 nominators => 3 top nominators, 0 bottom nominators
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(to_acc_id(4)),
                account_id,
                10,
                10,
                10
            ));
            let top_nominations = ParachainStaking::top_nominations(account_id).unwrap();
            let bottom_nominations = ParachainStaking::bottom_nominations(account_id).unwrap();
            assert_eq!(top_nominations.nominations.len(), 3usize);
            assert!(bottom_nominations.nominations.is_empty());
            // 4 nominators => 4 top nominators, 0 bottom nominators
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(to_acc_id(5)),
                account_id,
                10,
                10,
                10
            ));
            let top_nominations = ParachainStaking::top_nominations(account_id).unwrap();
            let bottom_nominations = ParachainStaking::bottom_nominations(account_id).unwrap();
            assert_eq!(top_nominations.nominations.len(), 4usize);
            assert!(bottom_nominations.nominations.is_empty());
        });
}

#[test]
fn candidate_pool_updates_when_total_counted_changes() {
    let account_id = to_acc_id(1u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_9 = to_acc_id(9u64);
    let account_id_10 = to_acc_id(10u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_3, 19),
            (to_acc_id(4), 20),
            (to_acc_id(5), 21),
            (to_acc_id(6), 22),
            (to_acc_id(7), 15),
            (to_acc_id(8), 16),
            (account_id_9, 17),
            (account_id_10, 18),
        ])
        .with_candidates(vec![(account_id, 20)])
        .with_nominations(vec![
            (account_id_3, account_id, 11),
            (to_acc_id(4), account_id, 12),
            (to_acc_id(5), account_id, 13),
            (to_acc_id(6), account_id, 14),
            (to_acc_id(7), account_id, 15),
            (to_acc_id(8), account_id, 16),
            (account_id_9, account_id, 17),
            (account_id_10, account_id, 18),
        ])
        .build()
        .execute_with(|| {
            fn is_candidate_pool_bond(account: AccountId, bond: u128) {
                let pool = ParachainStaking::candidate_pool();
                for candidate in pool.0 {
                    if candidate.owner == account {
                        assert_eq!(
                            candidate.amount, bond,
                            "Candidate Bond {:?} is Not Equal to Expected: {:?}",
                            candidate.amount, bond
                        );
                    }
                }
            }
            // 15 + 16 + 17 + 18 + 20 = 86 (top 4 + self bond)
            is_candidate_pool_bond(account_id, 86);
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_3), account_id, 8));
            // 3: 11 -> 19 => 3 is in top, bumps out 7
            // 16 + 17 + 18 + 19 + 20 = 90 (top 4 + self bond)
            is_candidate_pool_bond(account_id, 90);
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(to_acc_id(4)), account_id, 8));
            // 4: 12 -> 20 => 4 is in top, bumps out 8
            // 17 + 18 + 19 + 20 + 20 = 94 (top 4 + self bond)
            is_candidate_pool_bond(account_id, 94);
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_10),
                account_id,
                3
            ));
            roll_to(30);
            // 10: 18 -> 15 => 10 bumped to bottom, 8 bumped to top (- 18 + 16 = -2 for count)
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_10),
                account_id_10,
                account_id
            ));
            // 16 + 17 + 19 + 20 + 20 = 92 (top 4 + self bond)
            is_candidate_pool_bond(account_id, 92);
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_9),
                account_id,
                4
            ));
            roll_to(40);
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_9),
                account_id_9,
                account_id
            ));
            // 15 + 16 + 19 + 20 + 20 = 90 (top 4 + self bond)
            is_candidate_pool_bond(account_id, 90);
        });
}

#[test]
fn only_top_collators_are_counted() {
    let account_id = to_acc_id(1u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    let account_id_5 = to_acc_id(5u64);
    let account_id_6 = to_acc_id(6u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_3, 19),
            (account_id_4, 20),
            (account_id_5, 21),
            (account_id_6, 22),
            (to_acc_id(7), 15),
            (to_acc_id(8), 16),
            (to_acc_id(9), 17),
            (to_acc_id(10), 18),
        ])
        .with_candidates(vec![(account_id, 20)])
        .with_nominations(vec![
            (account_id_3, account_id, 11),
            (account_id_4, account_id, 12),
            (account_id_5, account_id, 13),
            (account_id_6, account_id, 14),
            (to_acc_id(7), account_id, 15),
            (to_acc_id(8), account_id, 16),
            (to_acc_id(9), account_id, 17),
            (to_acc_id(10), account_id, 18),
        ])
        .build()
        .execute_with(|| {
            // sanity check that 3-10 are nominators immediately
            for i in 3..11 {
                assert!(ParachainStaking::is_nominator(&to_acc_id(i)));
            }
            let collator_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 15 + 16 + 17 + 18 + 20 = 86 (top 4 + self bond)
            assert_eq!(collator_state.total_counted, 86);
            // bump bottom to the top
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_3), account_id, 8));
            assert_event_emitted!(Event::NominationIncreased {
                nominator: account_id_3,
                candidate: account_id,
                amount: 8,
                in_top: true,
            });
            let collator_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 16 + 17 + 18 + 19 + 20 = 90 (top 4 + self bond)
            assert_eq!(collator_state.total_counted, 90);
            // bump bottom to the top
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_4), account_id, 8));
            assert_event_emitted!(Event::NominationIncreased {
                nominator: account_id_4,
                candidate: account_id,
                amount: 8,
                in_top: true,
            });
            let collator_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 17 + 18 + 19 + 20 + 20 = 94 (top 4 + self bond)
            assert_eq!(collator_state.total_counted, 94);
            // bump bottom to the top
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_5), account_id, 8));
            assert_event_emitted!(Event::NominationIncreased {
                nominator: account_id_5,
                candidate: account_id,
                amount: 8,
                in_top: true,
            });
            let collator_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 18 + 19 + 20 + 21 + 20 = 98 (top 4 + self bond)
            assert_eq!(collator_state.total_counted, 98);
            // bump bottom to the top
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_6), account_id, 8));
            assert_event_emitted!(Event::NominationIncreased {
                nominator: account_id_6,
                candidate: account_id,
                amount: 8,
                in_top: true,
            });
            let collator_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 19 + 20 + 21 + 22 + 20 = 102 (top 4 + self bond)
            assert_eq!(collator_state.total_counted, 102);
        });
}

#[test]
fn nomination_events_convey_correct_position() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_6 = to_acc_id(6u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 100),
            (account_id_2, 100),
            (to_acc_id(3), 100),
            (to_acc_id(4), 100),
            (to_acc_id(5), 100),
            (to_acc_id(6), 100),
            (to_acc_id(7), 100),
            (to_acc_id(8), 100),
            (to_acc_id(9), 100),
            (to_acc_id(10), 100),
        ])
        .with_candidates(vec![(account_id, 20), (account_id_2, 20)])
        .with_nominations(vec![
            (to_acc_id(3), account_id, 11),
            (to_acc_id(4), account_id, 12),
            (to_acc_id(5), account_id, 13),
            (account_id_6, account_id, 14),
        ])
        .build()
        .execute_with(|| {
            let collator1_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 11 + 12 + 13 + 14 + 20 = 70 (top 4 + self bond)
            assert_eq!(collator1_state.total_counted, 70);
            // Top nominations are full, new highest nomination is made
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(to_acc_id(7)),
                account_id,
                15,
                10,
                10
            ));
            assert_event_emitted!(Event::Nomination {
                nominator: to_acc_id(7),
                locked_amount: 15,
                candidate: account_id,
                nominator_position: NominatorAdded::AddedToTop { new_total: 74 },
            });
            let collator1_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 12 + 13 + 14 + 15 + 20 = 70 (top 4 + self bond)
            assert_eq!(collator1_state.total_counted, 74);
            // New nomination is added to the bottom
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(to_acc_id(8)),
                account_id,
                10,
                10,
                10
            ));
            assert_event_emitted!(Event::Nomination {
                nominator: to_acc_id(8),
                locked_amount: 10,
                candidate: account_id,
                nominator_position: NominatorAdded::AddedToBottom,
            });
            let collator1_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 12 + 13 + 14 + 15 + 20 = 70 (top 4 + self bond)
            assert_eq!(collator1_state.total_counted, 74);
            // 8 increases nomination to the top
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(to_acc_id(8)), account_id, 3));
            assert_event_emitted!(Event::NominationIncreased {
                nominator: to_acc_id(8),
                candidate: account_id,
                amount: 3,
                in_top: true,
            });
            let collator1_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 13 + 13 + 14 + 15 + 20 = 75 (top 4 + self bond)
            assert_eq!(collator1_state.total_counted, 75);
            // 3 increases nomination but stays in bottom
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(to_acc_id(3)), account_id, 1));
            assert_event_emitted!(Event::NominationIncreased {
                nominator: to_acc_id(3),
                candidate: account_id,
                amount: 1,
                in_top: false,
            });
            let collator1_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 13 + 13 + 14 + 15 + 20 = 75 (top 4 + self bond)
            assert_eq!(collator1_state.total_counted, 75);
            // 6 decreases nomination but stays in top
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_6),
                account_id,
                2
            ));
            assert_event_emitted!(Event::NominationDecreaseScheduled {
                nominator: account_id_6,
                candidate: account_id,
                amount_to_decrease: 2,
                execute_era: 3,
            });
            roll_to(30);
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_6),
                account_id_6,
                account_id
            ));
            assert_event_emitted!(Event::NominationDecreased {
                nominator: account_id_6,
                candidate: account_id,
                amount: 2,
                in_top: true,
            });
            let collator1_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 12 + 13 + 13 + 15 + 20 = 73 (top 4 + self bond)ƒ
            assert_eq!(collator1_state.total_counted, 73);
            // 6 decreases nomination and is bumped to bottom
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_6),
                account_id,
                1
            ));
            assert_event_emitted!(Event::NominationDecreaseScheduled {
                nominator: account_id_6,
                candidate: account_id,
                amount_to_decrease: 1,
                execute_era: 9,
            });
            roll_to(40);
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_6),
                account_id_6,
                account_id
            ));
            assert_event_emitted!(Event::NominationDecreased {
                nominator: account_id_6,
                candidate: account_id,
                amount: 1,
                in_top: false,
            });
            let collator1_state = ParachainStaking::candidate_info(account_id).unwrap();
            // 12 + 13 + 13 + 15 + 20 = 73 (top 4 + self bond)
            assert_eq!(collator1_state.total_counted, 73);
        });
}

#[test]
fn no_rewards_paid_until_after_reward_payment_delay() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
        ])
        .with_candidates(vec![
            (account_id, 20),
            (account_id_2, 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
        ])
        .build()
        .execute_with(|| {
            roll_to_era_begin(2);
            // payouts for era 1
            set_author(1, account_id, 1);
            set_author(1, account_id_2, 1);
            set_author(1, account_id_3, 1);
            set_author(1, account_id_4, 1);
            set_author(1, account_id_4, 1);
            let mut expected = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: account_id_2,
                    total_exposed_amount: 20,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 4,
                    total_balance: 80,
                },
            ];
            assert_eq_events!(expected);

            set_reward_pot(5);
            roll_to_era_begin(3);
            expected.append(&mut vec![
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_3,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_4,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era: 3,
                    collator_account: account_id_2,
                    total_exposed_amount: 20,
                },
                Event::NewEra {
                    starting_block: 10,
                    era: 3,
                    selected_collators_number: 4,
                    total_balance: 80,
                },
                // rewards will begin immediately following a NewEra
                Event::Rewarded { account: account_id, rewards: 1 },
            ]);
            assert_eq_events!(expected);

            // roll to the next block where we start era 3; we should have era change and first
            // payout made.
            roll_one_block();
            expected.push(Event::Rewarded { account: account_id_2, rewards: 1 });
            assert_eq_events!(expected);

            roll_one_block();
            expected.push(Event::Rewarded { account: account_id_4, rewards: 2 });
            assert_eq_events!(expected);

            roll_one_block();
            expected.push(Event::Rewarded { account: account_id_3, rewards: 1 });
            assert_eq_events!(expected);

            // there should be no more payments in this era...
            let num_blocks_rolled = roll_to_era_end(3);
            assert_eq_events!(expected);
            assert_eq!(num_blocks_rolled, 1);
        });
}

#[test]
fn deferred_payment_storage_items_are_cleaned_up() {
    use crate::*;

    // this test sets up two collators, gives them points in era one, and focuses on the
    // storage over the next several blocks to show that it is properly cleaned up
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20), (account_id_2, 20)])
        .with_candidates(vec![(account_id, 20), (account_id_2, 20)])
        .build()
        .execute_with(|| {
            let mut era: u32 = 1;
            set_author(era, account_id, 1);
            set_author(era, account_id_2, 1);

            // reflects genesis?
            assert!(<AtStake<Test>>::contains_key(era, account_id));
            assert!(<AtStake<Test>>::contains_key(era, account_id_2));

            era = 2;
            roll_to_era_begin(era.into());
            let mut expected = vec![
                Event::CollatorChosen {
                    era,
                    collator_account: account_id,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era,
                    collator_account: account_id_2,
                    total_exposed_amount: 20,
                },
                Event::NewEra {
                    starting_block: 5,
                    era,
                    selected_collators_number: 2,
                    total_balance: 40,
                },
            ];
            assert_eq_events!(expected);

            // we should have AtStake snapshots as soon as we start a era...
            assert!(<AtStake<Test>>::contains_key(2, account_id));
            assert!(<AtStake<Test>>::contains_key(2, account_id_2));
            // ...and it should persist until the era is fully paid out
            assert!(<AtStake<Test>>::contains_key(1, account_id));
            assert!(<AtStake<Test>>::contains_key(1, account_id_2));

            assert!(
                !<DelayedPayouts<Test>>::contains_key(1),
                "DelayedPayouts shouldn't be populated until after RewardPaymentDelay"
            );
            assert!(
                <Points<Test>>::contains_key(1),
                "Points should be populated during current era"
            );
            assert!(<Staked<Test>>::contains_key(1), "Staked should be populated when era changes");

            assert!(
                !<Points<Test>>::contains_key(2),
                "Points should not be populated until author noted"
            );
            assert!(<Staked<Test>>::contains_key(2), "Staked should be populated when era changes");

            // first payout occurs in era 3
            era = 3;
            set_reward_pot(3);
            roll_to_era_begin(era.into());
            expected.append(&mut vec![
                Event::CollatorChosen {
                    era,
                    collator_account: account_id,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era,
                    collator_account: account_id_2,
                    total_exposed_amount: 20,
                },
                Event::NewEra {
                    starting_block: 10,
                    era,
                    selected_collators_number: 2,
                    total_balance: 40,
                },
                Event::Rewarded { account: account_id, rewards: 1 },
            ]);
            assert_eq_events!(expected);

            // payouts should exist for past eras that haven't been paid out yet..
            assert!(<AtStake<Test>>::contains_key(3, account_id));
            assert!(<AtStake<Test>>::contains_key(3, account_id_2));
            assert!(<AtStake<Test>>::contains_key(2, account_id));
            assert!(<AtStake<Test>>::contains_key(2, account_id_2));

            assert!(
                <DelayedPayouts<Test>>::contains_key(1),
                "DelayedPayouts should be populated after RewardPaymentDelay"
            );
            assert!(<Points<Test>>::contains_key(1));
            assert!(
                !<Staked<Test>>::contains_key(1),
                "Staked should be cleaned up after era change"
            );

            assert!(!<DelayedPayouts<Test>>::contains_key(2));
            assert!(!<Points<Test>>::contains_key(2), "We never rewarded points for era 2");
            assert!(<Staked<Test>>::contains_key(2));

            assert!(!<DelayedPayouts<Test>>::contains_key(3));
            assert!(!<Points<Test>>::contains_key(3), "We never awarded points for era 3");
            assert!(<Staked<Test>>::contains_key(3));

            // collator 1 has been paid in this last block and associated storage cleaned up
            assert!(!<AtStake<Test>>::contains_key(1, account_id));
            assert!(!<AwardedPts<Test>>::contains_key(1, account_id));

            // but collator 2 hasn't been paid
            assert!(<AtStake<Test>>::contains_key(1, account_id_2));
            assert!(<AwardedPts<Test>>::contains_key(1, account_id_2));

            era = 4;
            roll_to_era_begin(era.into());
            expected.append(&mut vec![
                Event::Rewarded { account: account_id_2, rewards: 1 }, // from previous era
                Event::CollatorChosen {
                    era,
                    collator_account: account_id,
                    total_exposed_amount: 20,
                },
                Event::CollatorChosen {
                    era,
                    collator_account: account_id_2,
                    total_exposed_amount: 20,
                },
                Event::NewEra {
                    starting_block: 15,
                    era,
                    selected_collators_number: 2,
                    total_balance: 40,
                },
            ]);
            assert_eq_events!(expected);

            // collators have both been paid and storage fully cleaned up for era 1
            assert!(!<AtStake<Test>>::contains_key(1, account_id_2));
            assert!(!<AwardedPts<Test>>::contains_key(1, account_id_2));
            assert!(!<Staked<Test>>::contains_key(1));
            assert!(!<Points<Test>>::contains_key(1)); // points should be cleaned up
            assert!(!<DelayedPayouts<Test>>::contains_key(1));

            roll_to_era_end(4);

            // no more events expected
            assert_eq_events!(expected);
        });
}

#[test]
fn deferred_payment_steady_state_event_flow() {
    use frame_support::traits::{Currency, ExistenceRequirement, WithdrawReasons};

    // this test "flows" through a number of eras, asserting that certain things do/don't happen
    // once the staking pallet is in a "steady state" (specifically, once we are past the first few
    // eras to clear RewardPaymentDelay)
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    let account_id_4 = to_acc_id(4u64);
    let account_id_11 = to_acc_id(11u64);
    let account_id_22 = to_acc_id(22u64);
    let account_id_33 = to_acc_id(33u64);
    let account_id_44 = to_acc_id(44u64);
    ExtBuilder::default()
        .with_balances(vec![
            // collators
            (account_id, 200),
            (account_id_2, 200),
            (account_id_3, 200),
            (account_id_4, 200),
            // nominators
            (account_id_11, 200),
            (account_id_22, 200),
            (account_id_33, 200),
            (account_id_44, 200),
            // burn account, see `reset_issuance()`
            (to_acc_id(111), 1000),
        ])
        .with_candidates(vec![
            (account_id, 200),
            (account_id_2, 200),
            (account_id_3, 200),
            (account_id_4, 200),
        ])
        .with_nominations(vec![
            // nominator 11 nominates 100 to 1 and 2
            (account_id_11, account_id, 100),
            (account_id_11, account_id_2, 100),
            // nominator 22 nominates 100 to 2 and 3
            (account_id_22, account_id_2, 100),
            (account_id_22, account_id_3, 100),
            // nominator 33 nominates 100 to 3 and 4
            (account_id_33, account_id_3, 100),
            (account_id_33, account_id_4, 100),
            // nominator 44 nominates 100 to 4 and 1
            (account_id_44, account_id_4, 100),
            (account_id_44, account_id, 100),
        ])
        .build()
        .execute_with(|| {
            // convenience to set the era points consistently
            let set_era_points = |era: u64| {
                set_author(era as u32, account_id, 1);
                set_author(era as u32, account_id_2, 1);
                set_author(era as u32, account_id_3, 1);
                set_author(era as u32, account_id_4, 1);
            };

            // grab initial issuance -- we will reset it before era issuance is calculated so that
            // it is consistent every era
            let initial_issuance = Balances::total_issuance();
            let reset_issuance = || {
                let new_issuance = Balances::total_issuance();
                let diff = new_issuance - initial_issuance;
                let account = to_acc_id(111);
                assert_ok!(Balances::burn(Origin::signed(account), diff, false));
            };

            // fn to roll through the first RewardPaymentDelay eras. returns new era index
            let roll_through_initial_eras = |mut era: u64| -> u64 {
                while era < crate::mock::RewardPaymentDelay::get() as u64 + 1 {
                    set_era_points(era);

                    roll_to_era_end(era);
                    era += 1;
                }

                reset_issuance();

                era
            };

            // roll through a "steady state" era and make all of our assertions
            // returns new era index
            let roll_through_steady_state_era = |era: u64| -> u64 {
                set_reward_pot(130);
                let num_eras_rolled = roll_to_era_begin(era);
                assert_eq!(num_eras_rolled, 1, "expected to be at era begin already");

                let expected = vec![
                    Event::CollatorChosen {
                        era: era as u32,
                        collator_account: account_id_3,
                        total_exposed_amount: 400,
                    },
                    Event::CollatorChosen {
                        era: era as u32,
                        collator_account: account_id_4,
                        total_exposed_amount: 400,
                    },
                    Event::CollatorChosen {
                        era: era as u32,
                        collator_account: account_id,
                        total_exposed_amount: 400,
                    },
                    Event::CollatorChosen {
                        era: era as u32,
                        collator_account: account_id_2,
                        total_exposed_amount: 400,
                    },
                    Event::NewEra {
                        starting_block: (era - 1) * 5,
                        era: era as u32,
                        selected_collators_number: 4,
                        total_balance: 1600,
                    },
                    // first payout should occur on era change
                    Event::Rewarded { account: account_id, rewards: 16 },
                    Event::Rewarded { account: account_id_11, rewards: 8 },
                    Event::Rewarded { account: account_id_44, rewards: 8 },
                ];
                assert_eq_last_events!(expected);

                set_era_points(era);

                roll_one_block();
                let expected = vec![
                    Event::Rewarded { account: account_id_2, rewards: 16 },
                    Event::Rewarded { account: account_id_11, rewards: 8 },
                    Event::Rewarded { account: account_id_22, rewards: 8 },
                ];
                assert_eq_last_events!(expected);

                roll_one_block();
                let expected = vec![
                    Event::Rewarded { account: account_id_4, rewards: 16 },
                    Event::Rewarded { account: account_id_33, rewards: 8 },
                    Event::Rewarded { account: account_id_44, rewards: 8 },
                ];
                assert_eq_last_events!(expected);

                roll_one_block();
                let expected = vec![
                    Event::Rewarded { account: account_id_3, rewards: 16 },
                    Event::Rewarded { account: account_id_22, rewards: 8 },
                    Event::Rewarded { account: account_id_33, rewards: 8 },
                ];
                assert_eq_last_events!(expected);

                roll_one_block();
                let expected = vec![
                    // we paid everyone out by now, should repeat last event
                    Event::Rewarded { account: account_id_33, rewards: 8 },
                ];
                assert_eq_last_events!(expected);

                let num_eras_rolled = roll_to_era_end(era);
                assert_eq!(num_eras_rolled, 0, "expected to be at era end already");

                reset_issuance();

                era + 1
            };

            let mut era = 1;
            era = roll_through_initial_eras(era); // we should be at RewardPaymentDelay
            for _ in 1..5 {
                era = roll_through_steady_state_era(era);
            }
        });
}

#[test]
fn nomination_kicked_from_bottom_removes_pending_request() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 30),
            (account_id_2, 29),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
            (to_acc_id(6), 20),
            (to_acc_id(7), 20),
            (to_acc_id(8), 20),
            (to_acc_id(9), 20),
            (to_acc_id(10), 20),
            (to_acc_id(11), 30),
        ])
        .with_candidates(vec![(account_id, 30), (to_acc_id(11), 30)])
        .with_nominations(vec![
            (account_id_2, account_id, 19),
            (account_id_2, to_acc_id(11), 10), /* second nomination so not left after first is
                                                * kicked */
            (to_acc_id(3), account_id, 20),
            (to_acc_id(4), account_id, 20),
            (to_acc_id(5), account_id, 20),
            (to_acc_id(6), account_id, 20),
            (to_acc_id(7), account_id, 20),
            (to_acc_id(8), account_id, 20),
            (to_acc_id(9), account_id, 20),
        ])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id
            ));
            // 10 nominates to full 1 => kicks lowest nomination (2, 19)
            assert_ok!(ParachainStaking::nominate(
                Origin::signed(to_acc_id(10)),
                account_id,
                20,
                8,
                0
            ));
            // check the event
            assert_event_emitted!(Event::NominationKicked {
                nominator: account_id_2,
                candidate: account_id,
                unstaked_amount: 19,
            });
            // ensure request DNE
            assert!(!ParachainStaking::nomination_scheduled_requests(&account_id)
                .iter()
                .any(|x| x.nominator == account_id_2));
        });
}

#[test]
fn no_selected_candidates_defaults_to_last_era_collators() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 30),
            (account_id_2, 30),
            (to_acc_id(3), 30),
            (to_acc_id(4), 30),
            (to_acc_id(5), 30),
        ])
        .with_candidates(vec![
            (account_id, 30),
            (account_id_2, 30),
            (to_acc_id(3), 30),
            (to_acc_id(4), 30),
            (to_acc_id(5), 30),
        ])
        .build()
        .execute_with(|| {
            roll_to_era_begin(1);
            // schedule to leave
            for i in 1..6 {
                assert_ok!(ParachainStaking::schedule_leave_candidates(
                    Origin::signed(to_acc_id(i)),
                    5
                ));
            }
            let old_era = ParachainStaking::era().current;
            let old_selected_candidates = ParachainStaking::selected_candidates();
            let mut old_at_stake_snapshots: BoundedVec<
                CollatorSnapshot<sp_core::sr25519::Public, u128>,
                MaxNominationsPerNominator,
            > = BoundedVec::default();
            for account in old_selected_candidates.clone() {
                old_at_stake_snapshots
                    .try_push(<AtStake<Test>>::get(old_era, account))
                    .unwrap_or_else(|_| ());
            }
            roll_to_era_begin(3);
            // execute leave
            for i in 1..6 {
                assert_ok!(ParachainStaking::execute_leave_candidates(
                    Origin::signed(to_acc_id(i)),
                    to_acc_id(i),
                    0,
                ));
            }
            // next era
            roll_to_era_begin(4);
            let new_era = ParachainStaking::era().current;
            // check AtStake matches previous
            let new_selected_candidates = ParachainStaking::selected_candidates();
            assert_eq!(old_selected_candidates, new_selected_candidates);
            let mut index = 0usize;
            for account in new_selected_candidates {
                assert_eq!(old_at_stake_snapshots[index], <AtStake<Test>>::get(new_era, account));
                index += 1usize;
            }
        });
}

#[test]
fn test_nominator_scheduled_for_revoke_is_rewarded_for_previous_eras_but_not_for_future() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 40),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
        ])
        .with_candidates(vec![(account_id, 20), (to_acc_id(3), 20), (to_acc_id(4), 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            // preset rewards for eras 1, 2 and 3
            (1..=3).for_each(|era| set_author(era, account_id, 1));

            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationRevocationScheduled {
                era: 1,
                nominator: account_id_2,
                candidate: account_id,
                scheduled_exit: 3,
            }));
            let collator =
                ParachainStaking::candidate_info(account_id).expect("candidate must exist");
            assert_eq!(
                1, collator.nomination_count,
                "collator's nominator count was reduced unexpectedly"
            );
            assert_eq!(30, collator.total_counted, "collator's total was reduced unexpectedly");

            set_reward_pot(5);
            roll_to_era_begin(3);
            assert_eq_last_events!(
                vec![
                    Event::<Test>::Rewarded { account: account_id, rewards: 3 },
                    Event::<Test>::Rewarded { account: account_id_2, rewards: 2 },
                ],
                "nominator was not rewarded as intended"
            );

            set_reward_pot(5);
            roll_to_era_begin(4);
            assert_eq_last_events!(
                vec![Event::<Test>::Rewarded { account: account_id, rewards: 5 }],
                "nominator was rewarded unexpectedly"
            );
            let collator_snapshot =
                ParachainStaking::at_stake(ParachainStaking::era().current, account_id);
            assert_eq!(
                1,
                collator_snapshot.nominations.len(),
                "collator snapshot's nominator count was reduced unexpectedly"
            );
            assert_eq!(
                20, collator_snapshot.total,
                "collator snapshot's total was reduced unexpectedly",
            );
        });
}

#[test]
fn test_nominator_scheduled_for_revoke_is_rewarded_when_request_cancelled() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 40),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
        ])
        .with_candidates(vec![(account_id, 20), (to_acc_id(3), 20), (to_acc_id(4), 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            // preset rewards for eras 2, 3 and 4
            (2..=4).for_each(|era| set_author(era, account_id, 1));

            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationRevocationScheduled {
                era: 1,
                nominator: account_id_2,
                candidate: account_id,
                scheduled_exit: 3,
            }));
            let collator =
                ParachainStaking::candidate_info(account_id).expect("candidate must exist");
            assert_eq!(
                1, collator.nomination_count,
                "collator's nominator count was reduced unexpectedly"
            );
            assert_eq!(30, collator.total_counted, "collator's total was reduced unexpectedly");

            roll_to_era_begin(2);
            assert_ok!(ParachainStaking::cancel_nomination_request(
                Origin::signed(account_id_2),
                account_id
            ));

            set_reward_pot(5);
            roll_to_era_begin(4);
            assert_eq_last_events!(
                vec![Event::<Test>::Rewarded { account: account_id, rewards: 5 }],
                "nominator was rewarded unexpectedly",
            );
            let collator_snapshot =
                ParachainStaking::at_stake(ParachainStaking::era().current, account_id);
            assert_eq!(
                1,
                collator_snapshot.nominations.len(),
                "collator snapshot's nominator count was reduced unexpectedly"
            );
            assert_eq!(
                30, collator_snapshot.total,
                "collator snapshot's total was reduced unexpectedly",
            );

            set_reward_pot(5);
            roll_to_era_begin(5);
            assert_eq_last_events!(
                vec![
                    Event::<Test>::Rewarded { account: account_id, rewards: 3 },
                    Event::<Test>::Rewarded { account: account_id_2, rewards: 2 },
                ],
                "nominator was not rewarded as intended",
            );
        });
}

#[test]
fn test_nominator_scheduled_for_bond_decrease_is_rewarded_for_previous_eras_but_less_for_future() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 40),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
        ])
        .with_candidates(vec![(account_id, 20), (to_acc_id(3), 20), (to_acc_id(4), 20)])
        .with_nominations(vec![(account_id_2, account_id, 20), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            // preset rewards for eras 1, 2 and 3
            (1..=3).for_each(|era| set_author(era, account_id, 1));

            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_2),
                account_id,
                10,
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationDecreaseScheduled {
                execute_era: 3,
                nominator: account_id_2,
                candidate: account_id,
                amount_to_decrease: 10,
            }));
            let collator =
                ParachainStaking::candidate_info(account_id).expect("candidate must exist");
            assert_eq!(
                1, collator.nomination_count,
                "collator's nominator count was reduced unexpectedly"
            );
            assert_eq!(40, collator.total_counted, "collator's total was reduced unexpectedly");

            set_reward_pot(5);
            roll_to_era_begin(3);
            assert_eq_last_events!(
                vec![
                    Event::<Test>::Rewarded { account: account_id, rewards: 2 },
                    Event::<Test>::Rewarded { account: account_id_2, rewards: 2 },
                ],
                "nominator was not rewarded as intended"
            );

            set_reward_pot(5);
            roll_to_era_begin(4);
            assert_eq_last_events!(
                vec![
                    Event::<Test>::Rewarded { account: account_id, rewards: 3 },
                    Event::<Test>::Rewarded { account: account_id_2, rewards: 2 },
                ],
                "nominator was rewarded unexpectedly"
            );
            let collator_snapshot =
                ParachainStaking::at_stake(ParachainStaking::era().current, account_id);
            assert_eq!(
                1,
                collator_snapshot.nominations.len(),
                "collator snapshot's nominator count was reduced unexpectedly"
            );
            assert_eq!(
                30, collator_snapshot.total,
                "collator snapshot's total was reduced unexpectedly",
            );
        });
}

#[test]
fn test_nominator_scheduled_for_bond_decrease_is_rewarded_when_request_cancelled() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 40),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
        ])
        .with_candidates(vec![(account_id, 20), (to_acc_id(3), 20), (to_acc_id(4), 20)])
        .with_nominations(vec![(account_id_2, account_id, 20), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            // preset rewards for eras 2, 3 and 4
            (2..=4).for_each(|era| set_author(era, account_id, 1));

            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_2),
                account_id,
                10,
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationDecreaseScheduled {
                execute_era: 3,
                nominator: account_id_2,
                candidate: account_id,
                amount_to_decrease: 10,
            }));
            let collator =
                ParachainStaking::candidate_info(account_id).expect("candidate must exist");
            assert_eq!(
                1, collator.nomination_count,
                "collator's nominator count was reduced unexpectedly"
            );
            assert_eq!(40, collator.total_counted, "collator's total was reduced unexpectedly");

            roll_to_era_begin(2);
            assert_ok!(ParachainStaking::cancel_nomination_request(
                Origin::signed(account_id_2),
                account_id
            ));

            set_reward_pot(5);
            roll_to_era_begin(4);
            assert_eq_last_events!(
                vec![
                    Event::<Test>::Rewarded { account: account_id, rewards: 3 },
                    Event::<Test>::Rewarded { account: account_id_2, rewards: 2 },
                ],
                "nominator was rewarded unexpectedly",
            );
            let collator_snapshot =
                ParachainStaking::at_stake(ParachainStaking::era().current, account_id);
            assert_eq!(
                1,
                collator_snapshot.nominations.len(),
                "collator snapshot's nominator count was reduced unexpectedly"
            );
            assert_eq!(
                40, collator_snapshot.total,
                "collator snapshot's total was reduced unexpectedly",
            );

            set_reward_pot(5);
            roll_to_era_begin(5);
            assert_eq_last_events!(
                vec![
                    Event::<Test>::Rewarded { account: account_id, rewards: 2 },
                    Event::<Test>::Rewarded { account: account_id_2, rewards: 2 },
                ],
                "nominator was not rewarded as intended",
            );
        });
}

#[test]
fn test_nominator_scheduled_for_leave_is_rewarded_for_previous_eras_but_not_for_future() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 40),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
        ])
        .with_candidates(vec![(account_id, 20), (to_acc_id(3), 20), (to_acc_id(4), 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            // preset rewards for eras 1, 2 and 3
            (1..=3).for_each(|era| set_author(era, account_id, 1));

            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2),));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominatorExitScheduled {
                era: 1,
                nominator: account_id_2,
                scheduled_exit: 3,
            }));
            let collator =
                ParachainStaking::candidate_info(account_id).expect("candidate must exist");
            assert_eq!(
                1, collator.nomination_count,
                "collator's nominator count was reduced unexpectedly"
            );
            assert_eq!(30, collator.total_counted, "collator's total was reduced unexpectedly");

            set_reward_pot(5);
            roll_to_era_begin(3);
            assert_eq_last_events!(
                vec![
                    Event::<Test>::Rewarded { account: account_id, rewards: 3 },
                    Event::<Test>::Rewarded { account: account_id_2, rewards: 2 },
                ],
                "nominator was not rewarded as intended"
            );

            set_reward_pot(5);
            roll_to_era_begin(4);
            assert_eq_last_events!(
                vec![Event::<Test>::Rewarded { account: account_id, rewards: 5 },],
                "nominator was rewarded unexpectedly"
            );
            let collator_snapshot =
                ParachainStaking::at_stake(ParachainStaking::era().current, account_id);
            assert_eq!(
                1,
                collator_snapshot.nominations.len(),
                "collator snapshot's nominator count was reduced unexpectedly"
            );
            assert_eq!(
                20, collator_snapshot.total,
                "collator snapshot's total was reduced unexpectedly",
            );
        });
}

#[test]
fn test_nominator_scheduled_for_leave_is_rewarded_when_request_cancelled() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 40),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
        ])
        .with_candidates(vec![(account_id, 20), (to_acc_id(3), 20), (to_acc_id(4), 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            // preset rewards for eras 2, 3 and 4
            (2..=4).for_each(|era| set_author(era, account_id, 1));

            assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(account_id_2)));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominatorExitScheduled {
                era: 1,
                nominator: account_id_2,
                scheduled_exit: 3,
            }));
            let collator =
                ParachainStaking::candidate_info(account_id).expect("candidate must exist");
            assert_eq!(
                1, collator.nomination_count,
                "collator's nominator count was reduced unexpectedly"
            );
            assert_eq!(30, collator.total_counted, "collator's total was reduced unexpectedly");

            roll_to_era_begin(2);
            assert_ok!(ParachainStaking::cancel_leave_nominators(Origin::signed(account_id_2)));

            set_reward_pot(5);
            roll_to_era_begin(4);
            assert_eq_last_events!(
                vec![Event::<Test>::Rewarded { account: account_id, rewards: 5 },],
                "nominator was rewarded unexpectedly",
            );
            let collator_snapshot =
                ParachainStaking::at_stake(ParachainStaking::era().current, account_id);
            assert_eq!(
                1,
                collator_snapshot.nominations.len(),
                "collator snapshot's nominator count was reduced unexpectedly"
            );
            assert_eq!(
                30, collator_snapshot.total,
                "collator snapshot's total was reduced unexpectedly",
            );

            set_reward_pot(5);
            roll_to_era_begin(5);
            assert_eq_last_events!(
                vec![
                    Event::<Test>::Rewarded { account: account_id, rewards: 3 },
                    Event::<Test>::Rewarded { account: account_id_2, rewards: 2 },
                ],
                "nominator was not rewarded as intended",
            );
        });
}

#[test]
fn test_nomination_request_exists_returns_false_when_nothing_exists() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert!(!ParachainStaking::nomination_request_exists(&account_id, &account_id_2));
        });
}

#[test]
fn test_nomination_request_exists_returns_true_when_decrease_exists() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            <NominationScheduledRequests<Test>>::insert(
                account_id,
                BoundedVec::truncate_from(vec![ScheduledRequest {
                    nominator: account_id_2,
                    when_executable: 3,
                    action: NominationAction::Decrease(5),
                }]),
            );
            assert!(ParachainStaking::nomination_request_exists(&account_id, &account_id_2));
        });
}

#[test]
fn test_nomination_request_exists_returns_true_when_revoke_exists() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            <NominationScheduledRequests<Test>>::insert(
                account_id,
                BoundedVec::truncate_from(vec![ScheduledRequest {
                    nominator: account_id_2,
                    when_executable: 3,
                    action: NominationAction::Revoke(5),
                }]),
            );
            assert!(ParachainStaking::nomination_request_exists(&account_id, &account_id_2));
        });
}

#[test]
fn test_nomination_request_revoke_exists_returns_false_when_nothing_exists() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert!(!ParachainStaking::nomination_request_revoke_exists(
                &account_id,
                &account_id_2
            ));
        });
}

#[test]
fn test_nomination_request_revoke_exists_returns_false_when_decrease_exists() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            <NominationScheduledRequests<Test>>::insert(
                account_id,
                BoundedVec::truncate_from(vec![ScheduledRequest {
                    nominator: account_id_2,
                    when_executable: 3,
                    action: NominationAction::Decrease(5),
                }]),
            );
            assert!(!ParachainStaking::nomination_request_revoke_exists(
                &account_id,
                &account_id_2
            ));
        });
}

#[test]
fn test_nomination_request_revoke_exists_returns_true_when_revoke_exists() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            <NominationScheduledRequests<Test>>::insert(
                account_id,
                BoundedVec::truncate_from(vec![ScheduledRequest {
                    nominator: account_id_2,
                    when_executable: 3,
                    action: NominationAction::Revoke(5),
                }]),
            );
            assert!(ParachainStaking::nomination_request_revoke_exists(&account_id, &account_id_2));
        });
}

//*******************************************
#[test]
fn test_hotfix_remove_nomination_requests_exited_candidates_cleans_up() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            // invalid state
            <NominationScheduledRequests<Test>>::insert(
                account_id_2,
                BoundedVec::<ScheduledRequest<AccountId, u128>, MaxNominationsPerNominator>::default(),
            );
            <NominationScheduledRequests<Test>>::insert(
                to_acc_id(3),
                BoundedVec::<ScheduledRequest<AccountId, u128>, MaxNominationsPerNominator>::default(),
            );
            assert_ok!(ParachainStaking::hotfix_remove_nomination_requests_exited_candidates(
                Origin::signed(account_id),
                vec![account_id_2, to_acc_id(3), to_acc_id(4)] /* 4 does not exist, but is OK
                                                                * for idempotency */
            ));

            assert!(!<NominationScheduledRequests<Test>>::contains_key(account_id_2));
            assert!(!<NominationScheduledRequests<Test>>::contains_key(to_acc_id(3)));
        });
}

#[test]
fn test_hotfix_remove_nomination_requests_exited_candidates_cleans_up_only_specified_keys() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            // invalid state
            <NominationScheduledRequests<Test>>::insert(
                account_id_2,
                BoundedVec::<ScheduledRequest<AccountId, u128>, MaxNominationsPerNominator>::default(),
            );
            <NominationScheduledRequests<Test>>::insert(
                to_acc_id(3),
                BoundedVec::<ScheduledRequest<AccountId, u128>, MaxNominationsPerNominator>::default(),
            );
            assert_ok!(ParachainStaking::hotfix_remove_nomination_requests_exited_candidates(
                Origin::signed(account_id),
                vec![account_id_2]
            ));

            assert!(!<NominationScheduledRequests<Test>>::contains_key(account_id_2));
            assert!(<NominationScheduledRequests<Test>>::contains_key(to_acc_id(3)));
        });
}

#[test]
fn test_hotfix_remove_nomination_requests_exited_candidates_errors_when_requests_not_empty() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            // invalid state
            <NominationScheduledRequests<Test>>::insert(
                to_acc_id(2),
                BoundedVec::<ScheduledRequest<AccountId, u128>, MaxNominationsPerNominator>::default(),
            );
            <NominationScheduledRequests<Test>>::insert(
                to_acc_id(3),
                BoundedVec::truncate_from(vec![ScheduledRequest {
                    nominator: to_acc_id(10),
                    when_executable: 1,
                    action: NominationAction::Revoke(10),
                }]),
            );

            assert_noop!(
                ParachainStaking::hotfix_remove_nomination_requests_exited_candidates(
                    Origin::signed(account_id),
                    vec![to_acc_id(2), to_acc_id(3)]
                ),
                <Error<Test>>::CandidateNotLeaving,
            );
        });
}

#[test]
fn test_hotfix_remove_nomination_requests_exited_candidates_errors_when_candidate_not_exited() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            // invalid state
            <NominationScheduledRequests<Test>>::insert(
                account_id,
                BoundedVec::<ScheduledRequest<AccountId, u128>, MaxNominationsPerNominator>::default(),
            );
            assert_noop!(
                ParachainStaking::hotfix_remove_nomination_requests_exited_candidates(
                    Origin::signed(account_id),
                    vec![account_id]
                ),
                <Error<Test>>::CandidateNotLeaving,
            );
        });
}

#[test]
fn locking_zero_amount_is_ignored() {
    let account_id = to_acc_id(1u64);
    use frame_support::traits::{LockableCurrency, WithdrawReasons};

    // this test demonstrates the behavior of pallet Balance's `LockableCurrency` implementation of
    // `set_locks()` when an amount of 0 is provided: it is a no-op

    ExtBuilder::default().with_balances(vec![(account_id, 100)]).build().execute_with(|| {
        assert_eq!(crate::mock::query_lock_amount(account_id, NOMINATOR_LOCK_ID), None);

        Balances::set_lock(NOMINATOR_LOCK_ID, &account_id, 1, WithdrawReasons::all());
        assert_eq!(crate::mock::query_lock_amount(account_id, NOMINATOR_LOCK_ID), Some(1));

        // Balances::set_lock(NOMINATOR_LOCK_ID, &account_id, 0, WithdrawReasons::all());
        // // Note that we tried to call `set_lock(0)` and it ignored it, we still have our lock
        // assert_eq!(crate::mock::query_lock_amount(account_id, NOMINATOR_LOCK_ID), Some(1));
    });
}

#[test]
fn revoke_last_removes_lock() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 100), (account_id_2, 100), (account_id_3, 100)])
        .with_candidates(vec![(account_id, 25), (account_id_2, 25)])
        .with_nominations(vec![(account_id_3, account_id, 30), (account_id_3, account_id_2, 25)])
        .build()
        .execute_with(|| {
            assert_eq!(crate::mock::query_lock_amount(account_id_3, NOMINATOR_LOCK_ID), Some(55));

            // schedule and remove one...
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_3),
                account_id
            ));
            roll_to_era_begin(3);
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_3),
                account_id_3,
                account_id
            ));
            assert_eq!(crate::mock::query_lock_amount(account_id_3, NOMINATOR_LOCK_ID), Some(25));

            // schedule and remove the other...
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_3),
                account_id_2
            ));
            roll_to_era_begin(5);
            assert_ok!(ParachainStaking::execute_nomination_request(
                Origin::signed(account_id_3),
                account_id_3,
                account_id_2
            ));
            assert_eq!(crate::mock::query_lock_amount(account_id_3, NOMINATOR_LOCK_ID), None);
        });
}
