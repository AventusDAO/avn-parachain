//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{
    assert_event_emitted, assert_last_event, encode_signed_nominate_params,
    mock::{
        build_proof, inner_call_failed_event_emitted, sign, AccountId, AvnProxy, ExtBuilder,
        ParachainStaking, RuntimeCall as MockCall, RuntimeEvent as MetaEvent,
        RuntimeOrigin as Origin, Signature, Staker, Test, TestAccount,
    },
    Config, Error, Event, NominatorAdded, Proof, StaticLookup,
};
use frame_support::{assert_noop, assert_ok, error::BadOrigin};
use frame_system::{self as system, RawOrigin};
use pallet_avn_proxy::Error as avn_proxy_error;
use sp_runtime::traits::Zero;

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id();
}

mod proxy_signed_nominate {
    use super::*;

    fn create_call_for_nominate(
        staker: &Staker,
        sender_nonce: u64,
        targets: Vec<<<Test as system::Config>::Lookup as StaticLookup>::Source>,
        amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof = create_proof_for_signed_nominate(sender_nonce, staker, &targets, &amount);

        return Box::new(MockCall::ParachainStaking(super::super::Call::<Test>::signed_nominate {
            proof,
            targets,
            amount,
        }));
    }

    fn create_call_for_nominate_from_proof(
        proof: Proof<Signature, AccountId>,
        targets: Vec<<<Test as system::Config>::Lookup as StaticLookup>::Source>,
        amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        return Box::new(MockCall::ParachainStaking(super::super::Call::<Test>::signed_nominate {
            proof,
            targets,
            amount,
        }));
    }

    fn create_proof_for_signed_nominate(
        sender_nonce: u64,
        staker: &Staker,
        targets: &Vec<<<Test as system::Config>::Lookup as StaticLookup>::Source>,
        amount: &u128,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_nominate_params::<Test>(
            staker.relayer.clone(),
            targets,
            amount,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature);
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let staker: Staker = Default::default();
        let initial_collator_stake = 10;
        let initial_balance = 10000;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1, initial_balance),
                (collator_2, initial_balance),
                (staker.account_id, initial_balance),
                (staker.relayer, initial_balance),
            ])
            .with_candidates(vec![
                (collator_1, initial_collator_stake),
                (collator_2, initial_collator_stake),
            ])
            .build()
            .execute_with(|| {
                let collators = ParachainStaking::selected_candidates();
                let min_user_stake = ParachainStaking::min_total_nominator_stake();

                // Pick an amount that is not perfectly divisible by the number of collators
                let dust = 1u128;
                let amount_to_stake = (min_user_stake * 2u128) + dust;
                let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                let nominate_call = create_call_for_nominate(
                    &staker,
                    nonce,
                    vec![collator_1, collator_2],
                    amount_to_stake,
                );
                assert_ok!(AvnProxy::proxy(Origin::signed(staker.relayer), nominate_call, None));

                // The staker state has also been updated
                let staker_state = ParachainStaking::nominator_state(staker.account_id).unwrap();
                assert_eq!(staker_state.total, amount_to_stake);

                // Each collator has been nominated by the expected amount
                for (index, collator) in collators.into_iter().enumerate() {
                    // We should have one event per collator. One of the collators gets any
                    // remaining dust.
                    let mut staked_amount = min_user_stake;
                    if index == 1 {
                        staked_amount = min_user_stake + dust;
                    }
                    assert_event_emitted!(Event::Nomination {
                        nominator: staker.account_id,
                        locked_amount: staked_amount,
                        candidate: collator,
                        nominator_position: NominatorAdded::AddedToTop {
                            new_total: initial_collator_stake + staked_amount
                        },
                    });

                    // Staker state reflects the new nomination for each collator
                    assert_eq!(staker_state.nominations.0[index].owner, collator);
                    assert_eq!(staker_state.nominations.0[index].amount, staked_amount);

                    // Collator state has been updated
                    let collator_state = ParachainStaking::candidate_info(collator).unwrap();
                    assert_eq!(
                        collator_state.total_counted,
                        initial_collator_stake + staked_amount
                    );

                    // Collator nominations have also been updated
                    let top_nominations = ParachainStaking::top_nominations(collator).unwrap();
                    assert_eq!(top_nominations.nominations[0].owner, staker.account_id);
                    assert_eq!(top_nominations.nominations[0].amount, staked_amount);
                    assert_eq!(top_nominations.total, staked_amount);
                }

                // The staker free balance has been reduced
                assert_eq!(
                    ParachainStaking::get_nominator_stakable_free_balance(&staker.account_id),
                    10000 - amount_to_stake
                );

                // Nonce has increased
                assert_eq!(ParachainStaking::proxy_nonce(staker.account_id), nonce + 1);
            })
    }

    #[test]
    fn succeeds_with_negative_dust() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let collator_3 = to_acc_id(3u64);
        let collator_4 = to_acc_id(4u64);
        let collator_5 = to_acc_id(5u64);
        let staker: Staker = Default::default();
        let initial_collator_stake = 1000;
        let initial_balance = 596583960081717817908u128;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1, initial_balance),
                (collator_2, initial_balance),
                (collator_3, initial_balance),
                (collator_4, initial_balance),
                (collator_5, initial_balance),
                (staker.account_id, initial_balance),
                (staker.relayer, initial_balance),
            ])
            .with_candidates(vec![
                (collator_1, initial_collator_stake),
                (collator_2, initial_collator_stake),
                (collator_3, initial_collator_stake),
                (collator_4, initial_collator_stake),
                (collator_5, initial_collator_stake),
            ])
            .build()
            .execute_with(|| {
                // Pick an amount that is not perfectly divisible by the number of collators
                // 496583960081717817908 / 5 * 5 will give 496583960081717817910 due to rounding
                let amount_to_stake = 496583960081717817908u128;
                let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                let nominate_call = create_call_for_nominate(
                    &staker,
                    nonce,
                    vec![collator_1, collator_2, collator_3, collator_4, collator_5],
                    amount_to_stake,
                );

                assert_ok!(AvnProxy::proxy(Origin::signed(staker.relayer), nominate_call, None));

                let staker_state = ParachainStaking::nominator_state(staker.account_id).unwrap();

                // The staker state has also been updated
                assert_eq!(staker_state.total, amount_to_stake);

                // The staker free balance has been reduced
                assert_eq!(
                    ParachainStaking::get_nominator_stakable_free_balance(&staker.account_id),
                    initial_balance - amount_to_stake
                );

                // Nonce has increased
                assert_eq!(ParachainStaking::proxy_nonce(staker.account_id), nonce + 1);
            })
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let staker: Staker = Default::default();
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, 10000),
                    (collator_2, 10000),
                    (staker.account_id, 10000),
                    (staker.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    let amount_to_stake = ParachainStaking::min_total_nominator_stake() * 2u128;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let targets = vec![collator_1, collator_2];
                    let proof = create_proof_for_signed_nominate(
                        nonce,
                        &staker,
                        &targets,
                        &amount_to_stake,
                    );

                    assert_noop!(
                        ParachainStaking::signed_nominate(
                            RawOrigin::None.into(),
                            proof.clone(),
                            targets.clone(),
                            amount_to_stake
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if it's signed.
                    assert_ok!(ParachainStaking::signed_nominate(
                        Origin::signed(staker.account_id),
                        proof,
                        targets,
                        amount_to_stake
                    ));
                });
        }

        #[test]
        fn proxy_proof_nonce_is_not_valid() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let staker: Staker = Default::default();
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, 10000),
                    (collator_2, 10000),
                    (staker.account_id, 10000),
                    (staker.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    let amount_to_stake = ParachainStaking::min_total_nominator_stake() * 2u128;
                    let bad_nonce = ParachainStaking::proxy_nonce(staker.account_id) + 1;

                    let nominate_call = create_call_for_nominate(
                        &staker,
                        bad_nonce,
                        vec![collator_1, collator_2],
                        amount_to_stake,
                    );

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        nominate_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(
                            avn_proxy_error::<Test>::UnauthorizedProxyTransaction.into()
                        )
                    );
                });
        }

        #[test]
        fn proxy_proof_amount_to_stake_is_not_valid() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let staker: Staker = Default::default();
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, 10000),
                    (collator_2, 10000),
                    (staker.account_id, 10000),
                    (staker.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    let bad_amount_to_stake = 0u128;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);

                    let proof = create_proof_for_signed_nominate(
                        nonce,
                        &staker,
                        &vec![collator_1, collator_2],
                        &bad_amount_to_stake,
                    );
                    let nominate_call = create_call_for_nominate_from_proof(
                        proof,
                        vec![collator_1, collator_2],
                        bad_amount_to_stake,
                    );
                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        nominate_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(
                            Error::<Test>::NominatorBondBelowMin.into()
                        )
                    );
                });
        }

        #[test]
        fn proxy_proof_signature_is_not_valid() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let staker: Staker = Default::default();
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, 10000),
                    (collator_2, 10000),
                    (staker.account_id, 10000),
                    (staker.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    let amount_to_stake = ParachainStaking::min_total_nominator_stake() * 2u128;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let bad_targets = vec![];
                    let proof = create_proof_for_signed_nominate(
                        nonce,
                        &staker,
                        &vec![collator_1, collator_2],
                        &amount_to_stake,
                    );
                    let nominate_call =
                        create_call_for_nominate_from_proof(proof, bad_targets, amount_to_stake);
                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        nominate_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(
                            avn_proxy_error::<Test>::UnauthorizedProxyTransaction.into()
                        )
                    );
                });
        }

        #[test]
        fn staker_does_not_have_enough_funds() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let staker: Staker = Default::default();
            let staker_balance = 10;
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, 10000),
                    (collator_2, 10000),
                    (staker.account_id, staker_balance),
                    (staker.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    let bad_amount_to_stake = staker_balance + 1;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);

                    // Make sure staker has less than they are attempting to stake
                    assert!(staker_balance < bad_amount_to_stake);

                    let nominate_call = create_call_for_nominate(
                        &staker,
                        nonce,
                        vec![collator_1, collator_2],
                        bad_amount_to_stake,
                    );

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        nominate_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(Error::<Test>::InsufficientBalance.into())
                    );
                });
        }

        #[test]
        fn stake_is_less_than_min_allowed() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let staker: Staker = Default::default();
            let staker_balance = 10000;
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, 10000),
                    (collator_2, 10000),
                    (staker.account_id, staker_balance),
                    (staker.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    let min_allowed_amount_to_stake =
                        ParachainStaking::min_total_nominator_stake() * 2u128;
                    let bad_stake_amount = min_allowed_amount_to_stake - 1;

                    // Show that the staker has enough funds to cover the stake
                    assert!(staker_balance > bad_stake_amount);

                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let nominate_call = create_call_for_nominate(
                        &staker,
                        nonce,
                        vec![collator_1, collator_2],
                        bad_stake_amount,
                    );

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        nominate_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(
                            Error::<Test>::NominatorBondBelowMin.into()
                        )
                    );
                });
        }
    }
}

// NOMINATE

mod existing_direct_nominate_tests {
    use super::*;

    #[test]
    fn nominate_event_emits_correctly() {
        let account_id = to_acc_id(1u64);
        ExtBuilder::default()
            .with_balances(vec![(account_id, 30), (to_acc_id(2), 10)])
            .with_candidates(vec![(account_id, 30)])
            .build()
            .execute_with(|| {
                assert_ok!(ParachainStaking::nominate(
                    Origin::signed(to_acc_id(2)),
                    account_id,
                    10,
                    0,
                    0
                ));
                assert_last_event!(MetaEvent::ParachainStaking(Event::Nomination {
                    nominator: to_acc_id(2),
                    locked_amount: 10,
                    candidate: account_id,
                    nominator_position: NominatorAdded::AddedToTop { new_total: 40 },
                }));
            });
    }

    #[test]
    fn nominate_reserves_balance() {
        let account_id = to_acc_id(1u64);
        let account_id_2 = to_acc_id(2u64);
        ExtBuilder::default()
            .with_balances(vec![(account_id, 30), (account_id_2, 10)])
            .with_candidates(vec![(account_id, 30)])
            .build()
            .execute_with(|| {
                assert_eq!(
                    ParachainStaking::get_nominator_stakable_free_balance(&account_id_2),
                    10
                );
                assert_ok!(ParachainStaking::nominate(
                    Origin::signed(account_id_2),
                    account_id,
                    10,
                    0,
                    0
                ));
                assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 0);
            });
    }

    #[test]
    fn nominate_updates_nominator_state() {
        let account_id = to_acc_id(1u64);
        let account_id_2 = to_acc_id(2u64);
        ExtBuilder::default()
            .with_balances(vec![(account_id, 30), (account_id_2, 10)])
            .with_candidates(vec![(account_id, 30)])
            .build()
            .execute_with(|| {
                assert!(ParachainStaking::nominator_state(account_id_2).is_none());
                assert_ok!(ParachainStaking::nominate(
                    Origin::signed(account_id_2),
                    account_id,
                    10,
                    0,
                    0
                ));
                let nominator_state = ParachainStaking::nominator_state(account_id_2)
                    .expect("just nominated => exists");
                assert_eq!(nominator_state.total, 10);
                assert_eq!(nominator_state.nominations.0[0].owner, account_id);
                assert_eq!(nominator_state.nominations.0[0].amount, 10);
            });
    }

    #[test]
    fn nominate_updates_collator_state() {
        let account_id = to_acc_id(1u64);
        let account_id_2 = to_acc_id(2u64);
        ExtBuilder::default()
            .with_balances(vec![(account_id, 30), (account_id_2, 10)])
            .with_candidates(vec![(account_id, 30)])
            .build()
            .execute_with(|| {
                let candidate_state =
                    ParachainStaking::candidate_info(account_id).expect("registered in genesis");
                assert_eq!(candidate_state.total_counted, 30);
                let top_nominations =
                    ParachainStaking::top_nominations(account_id).expect("registered in genesis");
                assert!(top_nominations.nominations.is_empty());
                assert!(top_nominations.total.is_zero());
                assert_ok!(ParachainStaking::nominate(
                    Origin::signed(account_id_2),
                    account_id,
                    10,
                    0,
                    0
                ));
                let candidate_state =
                    ParachainStaking::candidate_info(account_id).expect("just nominated => exists");
                assert_eq!(candidate_state.total_counted, 40);
                let top_nominations = ParachainStaking::top_nominations(account_id)
                    .expect("just nominated => exists");
                assert_eq!(top_nominations.nominations[0].owner, account_id_2);
                assert_eq!(top_nominations.nominations[0].amount, 10);
                assert_eq!(top_nominations.total, 10);
            });
    }

    #[test]
    fn can_nominate_immediately_after_other_join_candidates() {
        let account_id = to_acc_id(1u64);
        let account_id_2 = to_acc_id(2u64);
        ExtBuilder::default()
            .with_balances(vec![(account_id, 20), (account_id_2, 20)])
            .build()
            .execute_with(|| {
                assert_ok!(ParachainStaking::join_candidates(Origin::signed(account_id), 20, 0));
                assert_ok!(ParachainStaking::nominate(
                    Origin::signed(account_id_2),
                    account_id,
                    20,
                    0,
                    0
                ));
            });
    }

    #[test]
    fn can_nominate_if_revoking() {
        let account_id = to_acc_id(1u64);
        let account_id_2 = to_acc_id(2u64);
        let account_id_3 = to_acc_id(3u64);
        let account_id_4 = to_acc_id(4u64);
        ExtBuilder::default()
            .with_balances(vec![
                (account_id, 20),
                (account_id_2, 30),
                (account_id_3, 20),
                (account_id_4, 20),
            ])
            .with_candidates(vec![(account_id, 20), (account_id_3, 20), (account_id_4, 20)])
            .with_nominations(vec![
                (account_id_2, account_id, 10),
                (account_id_2, account_id_3, 10),
            ])
            .build()
            .execute_with(|| {
                assert_ok!(ParachainStaking::schedule_revoke_nomination(
                    Origin::signed(account_id_2),
                    account_id
                ));
                assert_ok!(ParachainStaking::nominate(
                    Origin::signed(account_id_2),
                    account_id_4,
                    10,
                    0,
                    2
                ));
            });
    }

    #[test]
    fn cannot_nominate_if_full_and_new_nomination_less_than_or_equal_lowest_bottom() {
        let account_id = to_acc_id(1u64);
        ExtBuilder::default()
            .with_balances(vec![
                (account_id, 20),
                (to_acc_id(2), 10),
                (to_acc_id(3), 10),
                (to_acc_id(4), 10),
                (to_acc_id(5), 10),
                (to_acc_id(6), 10),
                (to_acc_id(7), 10),
                (to_acc_id(8), 10),
                (to_acc_id(9), 10),
                (to_acc_id(10), 10),
                (to_acc_id(11), 11),
            ])
            .with_candidates(vec![(account_id, 20)])
            .with_nominations(vec![
                (to_acc_id(2), account_id, 10),
                (to_acc_id(3), account_id, 10),
                (to_acc_id(4), account_id, 10),
                (to_acc_id(5), account_id, 10),
                (to_acc_id(6), account_id, 10),
                (to_acc_id(8), account_id, 10),
                (to_acc_id(9), account_id, 10),
                (to_acc_id(10), account_id, 10),
            ])
            .build()
            .execute_with(|| {
                assert_noop!(
                    ParachainStaking::nominate(Origin::signed(to_acc_id(11)), account_id, 10, 8, 0),
                    Error::<Test>::CannotNominateLessThanOrEqualToLowestBottomWhenFull
                );
            });
    }

    #[test]
    fn can_nominate_if_full_and_new_nomination_greater_than_lowest_bottom() {
        let account_id = to_acc_id(1u64);
        ExtBuilder::default()
            .with_balances(vec![
                (account_id, 20),
                (to_acc_id(2), 10),
                (to_acc_id(3), 10),
                (to_acc_id(4), 10),
                (to_acc_id(5), 10),
                (to_acc_id(6), 10),
                (to_acc_id(7), 10),
                (to_acc_id(8), 10),
                (to_acc_id(9), 10),
                (to_acc_id(10), 10),
                (to_acc_id(11), 11),
            ])
            .with_candidates(vec![(account_id, 20)])
            .with_nominations(vec![
                (to_acc_id(2), account_id, 10),
                (to_acc_id(3), account_id, 10),
                (to_acc_id(4), account_id, 10),
                (to_acc_id(5), account_id, 10),
                (to_acc_id(6), account_id, 10),
                (to_acc_id(8), account_id, 10),
                (to_acc_id(9), account_id, 10),
                (to_acc_id(10), account_id, 10),
            ])
            .build()
            .execute_with(|| {
                assert_ok!(ParachainStaking::nominate(
                    Origin::signed(to_acc_id(11)),
                    account_id,
                    11,
                    8,
                    0
                ));
                assert_event_emitted!(Event::NominationKicked {
                    nominator: to_acc_id(10),
                    candidate: account_id,
                    unstaked_amount: 10
                });
                assert_event_emitted!(Event::NominatorLeft {
                    nominator: to_acc_id(10),
                    unstaked_amount: 10
                });
            });
    }

    #[test]
    fn can_still_nominate_if_leaving() {
        let account_id = to_acc_id(1u64);
        let account_id_2 = to_acc_id(2u64);
        let account_id_3 = to_acc_id(3u64);
        ExtBuilder::default()
            .with_balances(vec![(account_id, 20), (account_id_2, 20), (account_id_3, 20)])
            .with_candidates(vec![(account_id, 20), (account_id_3, 20)])
            .with_nominations(vec![(account_id_2, account_id, 10)])
            .build()
            .execute_with(|| {
                assert_ok!(ParachainStaking::schedule_leave_nominators(Origin::signed(
                    account_id_2
                )));
                assert_ok!(ParachainStaking::nominate(
                    Origin::signed(account_id_2),
                    account_id_3,
                    10,
                    0,
                    1
                ),);
            });
    }

    #[test]
    fn cannot_nominate_if_candidate() {
        let account_id = to_acc_id(1u64);
        let account_id_2 = to_acc_id(2u64);
        ExtBuilder::default()
            .with_balances(vec![(account_id, 20), (account_id_2, 30)])
            .with_candidates(vec![(account_id, 20), (account_id_2, 20)])
            .build()
            .execute_with(|| {
                assert_noop!(
                    ParachainStaking::nominate(Origin::signed(account_id_2), account_id, 10, 0, 0),
                    Error::<Test>::CandidateExists
                );
            });
    }

    #[test]
    fn cannot_nominate_if_already_nominated() {
        let account_id = to_acc_id(1u64);
        let account_id_2 = to_acc_id(2u64);
        ExtBuilder::default()
            .with_balances(vec![(account_id, 20), (account_id_2, 30)])
            .with_candidates(vec![(account_id, 20)])
            .with_nominations(vec![(account_id_2, account_id, 20)])
            .build()
            .execute_with(|| {
                assert_noop!(
                    ParachainStaking::nominate(Origin::signed(account_id_2), account_id, 10, 1, 1),
                    Error::<Test>::AlreadyNominatedCandidate
                );
            });
    }

    #[test]
    fn cannot_nominate_more_than_max_nominations() {
        let account_id = to_acc_id(1u64);
        ExtBuilder::default()
            .with_balances(vec![
                (account_id, 20),
                (to_acc_id(2), 110),
                (to_acc_id(3), 20),
                (to_acc_id(4), 20),
                (to_acc_id(5), 20),
                (to_acc_id(6), 20),
                (to_acc_id(7), 20),
                (to_acc_id(8), 20),
                (to_acc_id(9), 20),
                (to_acc_id(10), 20),
                (to_acc_id(11), 20),
                (to_acc_id(12), 20),
            ])
            .with_candidates(vec![
                (account_id, 20),
                (to_acc_id(3), 20),
                (to_acc_id(4), 20),
                (to_acc_id(5), 20),
                (to_acc_id(6), 20),
                (to_acc_id(7), 20),
                (to_acc_id(8), 20),
                (to_acc_id(9), 20),
                (to_acc_id(10), 20),
                (to_acc_id(11), 20),
                (to_acc_id(12), 20),
            ])
            .with_nominations(vec![
                (to_acc_id(2), account_id, 10),
                (to_acc_id(2), to_acc_id(3), 10),
                (to_acc_id(2), to_acc_id(4), 10),
                (to_acc_id(2), to_acc_id(5), 10),
                (to_acc_id(2), to_acc_id(6), 10),
                (to_acc_id(2), to_acc_id(7), 10),
                (to_acc_id(2), to_acc_id(8), 10),
                (to_acc_id(2), to_acc_id(9), 10),
                (to_acc_id(2), to_acc_id(10), 10),
                (to_acc_id(2), to_acc_id(11), 10),
            ])
            .build()
            .execute_with(|| {
                assert_noop!(
                    ParachainStaking::nominate(
                        Origin::signed(to_acc_id(2)),
                        to_acc_id(12),
                        10,
                        0,
                        10
                    ),
                    Error::<Test>::ExceedMaxNominationsPerNominator,
                );
            });
    }

    #[test]
    fn sufficient_nominate_weight_hint_succeeds() {
        let account_id = to_acc_id(1u64);
        ExtBuilder::default()
            .with_balances(vec![
                (account_id, 20),
                (to_acc_id(2), 20),
                (to_acc_id(3), 20),
                (to_acc_id(4), 20),
                (to_acc_id(5), 20),
                (to_acc_id(6), 20),
                (to_acc_id(7), 20),
                (to_acc_id(8), 20),
                (to_acc_id(9), 20),
                (to_acc_id(10), 20),
            ])
            .with_candidates(vec![(account_id, 20), (to_acc_id(2), 20)])
            .with_nominations(vec![
                (to_acc_id(3), account_id, 10),
                (to_acc_id(4), account_id, 10),
                (to_acc_id(5), account_id, 10),
                (to_acc_id(6), account_id, 10),
            ])
            .build()
            .execute_with(|| {
                let mut count = 4u32;
                for i in 7..11 {
                    assert_ok!(ParachainStaking::nominate(
                        Origin::signed(to_acc_id(i)),
                        account_id,
                        10,
                        count,
                        0u32
                    ));
                    count += 1u32;
                }
                let mut count = 0u32;
                for i in 3..11 {
                    assert_ok!(ParachainStaking::nominate(
                        Origin::signed(to_acc_id(i)),
                        to_acc_id(2),
                        10,
                        count,
                        1u32
                    ));
                    count += 1u32;
                }
            });
    }

    #[test]
    fn insufficient_nominate_weight_hint_fails() {
        let account_id = to_acc_id(1u64);
        ExtBuilder::default()
            .with_balances(vec![
                (account_id, 20),
                (to_acc_id(2), 20),
                (to_acc_id(3), 20),
                (to_acc_id(4), 20),
                (to_acc_id(5), 20),
                (to_acc_id(6), 20),
                (to_acc_id(7), 20),
                (to_acc_id(8), 20),
                (to_acc_id(9), 20),
                (to_acc_id(10), 20),
            ])
            .with_candidates(vec![(account_id, 20), (to_acc_id(2), 20)])
            .with_nominations(vec![
                (to_acc_id(3), account_id, 10),
                (to_acc_id(4), account_id, 10),
                (to_acc_id(5), account_id, 10),
                (to_acc_id(6), account_id, 10),
            ])
            .build()
            .execute_with(|| {
                let mut count = 3u32;
                for i in 7..11 {
                    assert_noop!(
                        ParachainStaking::nominate(
                            Origin::signed(to_acc_id(i)),
                            account_id,
                            10,
                            count,
                            0u32
                        ),
                        Error::<Test>::TooLowCandidateNominationCountToNominate
                    );
                }
                // to set up for next error test
                count = 4u32;
                for i in 7..11 {
                    assert_ok!(ParachainStaking::nominate(
                        Origin::signed(to_acc_id(i)),
                        account_id,
                        10,
                        count,
                        0u32
                    ));
                    count += 1u32;
                }
                count = 0u32;
                for i in 3..11 {
                    assert_noop!(
                        ParachainStaking::nominate(
                            Origin::signed(to_acc_id(i)),
                            to_acc_id(2),
                            10,
                            count,
                            0u32
                        ),
                        Error::<Test>::TooLowNominationCountToNominate
                    );
                    count += 1u32;
                }
            });
    }
}
