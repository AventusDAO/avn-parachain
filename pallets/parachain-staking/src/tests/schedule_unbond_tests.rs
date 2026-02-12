//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{
    assert_event_emitted, assert_last_event, encode_signed_execute_candidate_unbond_params,
    encode_signed_execute_nomination_request_params,
    encode_signed_schedule_candidate_unbond_params, encode_signed_schedule_nominator_unbond_params,
    mock::{
        build_proof, inner_call_failed_event_emitted, roll_to, roll_to_era_begin, sign, AccountId,
        AvnProxy, ExtBuilder, MinNominationPerCollator, ParachainStaking, RuntimeCall as MockCall,
        RuntimeEvent as MetaEvent, RuntimeOrigin, Signature, Staker, System, Test, TestAccount,
    },
    Bond, Config, Error, Event, NominationAction, Proof, ScheduledRequest,
};
use frame_support::{assert_noop, assert_ok, error::BadOrigin};
use frame_system::RawOrigin;
use pallet_avn_proxy::Error as avn_proxy_error;
use std::cell::RefCell;

thread_local! {
    pub static AMOUNT_TO_UNBOND: RefCell<u128> = RefCell::new(0u128);
}

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id();
}

fn get_accounts(
    num: u32,
    initial_balance: u128,
    additional: Option<Vec<(AccountId, u128)>>,
) -> Vec<(AccountId, u128)> {
    let mut balances: Vec<(AccountId, u128)> = vec![];

    for i in 1..=num {
        balances.push((to_acc_id(i as u64), initial_balance));
    }

    if additional.is_some() {
        balances = [balances, additional.unwrap()].concat()
    }

    return balances;
}

fn get_nominations(
    num: u32,
    nominator: AccountId,
    nominations: &Vec<u128>,
) -> Vec<(AccountId, AccountId, u128)> {
    let mut accounts: Vec<(AccountId, AccountId)> = vec![];

    for i in 1..=num {
        accounts.push((nominator, to_acc_id(i as u64)));
    }

    return accounts
        .into_iter()
        .zip(nominations)
        .map(|v| (v.0 .0, v.0 .1, *v.1))
        .collect::<Vec<_>>();
}

fn get_max_to_unbond(nominations: &Vec<u128>, num_collators: u32) -> u128 {
    let total_nominations = nominations.iter().sum::<u128>();
    return total_nominations - (MinNominationPerCollator::get() * num_collators as u128);
}

mod proxy_signed_schedule_nominator_unbond {
    use super::*;

    pub fn create_call_for_signed_schedule_nominator_unbond(
        staker: &Staker,
        sender_nonce: u64,
        reduction_amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof = create_proof_for_signed_schedule_nominator_unbond(
            sender_nonce,
            staker,
            &reduction_amount,
        );

        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_schedule_nominator_unbond {
                proof,
                less: reduction_amount,
            },
        ));
    }

    pub fn create_call_for_signed_schedule_nominator_unbond_proof(
        proof: Proof<Signature, AccountId>,
        reduction_amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_schedule_nominator_unbond {
                proof,
                less: reduction_amount,
            },
        ));
    }

    fn create_proof_for_signed_schedule_nominator_unbond(
        sender_nonce: u64,
        staker: &Staker,
        reduction_amount: &u128,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_schedule_nominator_unbond_params::<Test>(
            staker.relayer.clone(),
            reduction_amount,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature);
    }

    fn unbond_event_emitted(nominator: AccountId) -> bool {
        System::events()
            .into_iter()
            .map(|r| r.event)
            .filter_map(|e| {
                if let crate::mock::RuntimeEvent::ParachainStaking(inner) = e {
                    Some(inner)
                } else {
                    None
                }
            })
            .filter_map(|inner| {
                if let Event::NominationDecreaseScheduled { nominator, .. } = inner {
                    Some(nominator)
                } else {
                    None
                }
            })
            .any(|n| n == nominator)
    }

    #[test]
    fn succeeds_with_good_values() {
        ExtBuilder::default().build().execute_with(|| {
            let num_collators = 10;

            let mut nominations: Vec<u128> = vec![7, 7, 7, 7, 7, 7, 7, 7, 7, 7];
            for i in 1..=get_max_to_unbond(&nominations, num_collators) {
                AMOUNT_TO_UNBOND.with(|pk| *pk.borrow_mut() = i);
                unbond(num_collators.clone(), &nominations);
            }

            nominations = vec![6, 6, 6, 6, 6, 6, 6, 6, 6, 11];
            for i in 1..=get_max_to_unbond(&nominations, num_collators) {
                AMOUNT_TO_UNBOND.with(|pk| *pk.borrow_mut() = i);
                unbond(num_collators.clone(), &nominations);
            }

            nominations = vec![15, 11, 11, 11, 13, 19, 19, 10, 10, 14];
            for i in 1..=get_max_to_unbond(&nominations, num_collators) {
                AMOUNT_TO_UNBOND.with(|pk| *pk.borrow_mut() = i);
                unbond(num_collators.clone(), &nominations);
            }

            nominations = vec![102, 4, 13, 25, 21, 3, 49, 11, 39, 87];
            for i in 1..=get_max_to_unbond(&nominations, num_collators) {
                AMOUNT_TO_UNBOND.with(|pk| *pk.borrow_mut() = i);
                unbond(num_collators.clone(), &nominations);
            }
        });
    }

    fn unbond(num_collators: u32, nominations: &Vec<u128>) {
        let initial_collator_stake = 10;
        let initial_balance = 1000000000000000000000;
        let staker: Staker = Default::default();
        let staker_balance =
            vec![(staker.account_id, initial_balance), (staker.relayer, initial_balance)];
        ExtBuilder::default()
            .with_balances(get_accounts(num_collators, initial_balance, Some(staker_balance)))
            .with_candidates(get_accounts(num_collators, initial_collator_stake, None))
            .with_nominations(get_nominations(num_collators, staker.account_id, nominations))
            .with_staking_config(
                initial_collator_stake,
                MinNominationPerCollator::get() * num_collators as u128,
            )
            .build()
            .execute_with(|| {
                let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                let unbond_call = create_call_for_signed_schedule_nominator_unbond(
                    &staker,
                    nonce,
                    AMOUNT_TO_UNBOND.with(|v| *v.borrow()),
                );
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(staker.relayer),
                    unbond_call,
                    None
                ));

                assert!(unbond_event_emitted(Staker::default().account_id));

                // Nonce has increased
                assert_eq!(ParachainStaking::proxy_nonce(staker.account_id), nonce + 1);
            });
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
                .with_nominations(vec![
                    (staker.account_id, collator_1, 10),
                    (staker.account_id, collator_2, 10),
                ])
                .build()
                .execute_with(|| {
                    let amount_to_withdraw = 10;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof = create_proof_for_signed_schedule_nominator_unbond(
                        nonce,
                        &staker,
                        &amount_to_withdraw,
                    );

                    assert_noop!(
                        ParachainStaking::signed_schedule_nominator_unbond(
                            RawOrigin::None.into(),
                            proof.clone(),
                            amount_to_withdraw
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if it's signed.
                    assert_ok!(ParachainStaking::signed_schedule_nominator_unbond(
                        RuntimeOrigin::signed(staker.account_id),
                        proof,
                        amount_to_withdraw
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
                .with_nominations(vec![
                    (staker.account_id, collator_1, 10),
                    (staker.account_id, collator_2, 10),
                ])
                .build()
                .execute_with(|| {
                    let amount_to_withdraw = 10;
                    let bad_nonce = ParachainStaking::proxy_nonce(staker.account_id) + 1;
                    let unbond_call = create_call_for_signed_schedule_nominator_unbond(
                        &staker,
                        bad_nonce,
                        amount_to_withdraw,
                    );

                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        unbond_call,
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
                .with_nominations(vec![
                    (staker.account_id, collator_1, 10),
                    (staker.account_id, collator_2, 10),
                ])
                .build()
                .execute_with(|| {
                    let amount_to_withdraw = 10;
                    let bad_amount_to_withdraw = 0u128;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);

                    let proof = create_proof_for_signed_schedule_nominator_unbond(
                        nonce,
                        &staker,
                        &amount_to_withdraw,
                    );

                    let unbond_call = create_call_for_signed_schedule_nominator_unbond_proof(
                        proof,
                        bad_amount_to_withdraw,
                    );
                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        unbond_call,
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
        fn staker_does_not_have_enough_to_withdraw() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let staker: Staker = Default::default();
            let staker_balance = 100;
            let initial_stake = 10;
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, 10000),
                    (collator_2, 10000),
                    (staker.account_id, staker_balance),
                    (staker.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1, initial_stake), (collator_2, initial_stake)])
                .with_nominations(vec![
                    (staker.account_id, collator_1, initial_stake),
                    (staker.account_id, collator_2, initial_stake),
                ])
                .build()
                .execute_with(|| {
                    let total_stake = staker_balance * 2;
                    // amount falls below min total stake
                    let bad_amount_to_unbond =
                        total_stake - ParachainStaking::min_total_nominator_stake() - 1;

                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let unbond_call = create_call_for_signed_schedule_nominator_unbond(
                        &staker,
                        nonce,
                        bad_amount_to_unbond,
                    );

                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        unbond_call,
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
        fn withdrawal_reduces_per_collator_bond_below_min_allowed() {
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
                .with_nominations(vec![
                    (staker.account_id, collator_1, 10),
                    (staker.account_id, collator_2, 10),
                ])
                .with_staking_config(10, 4u128)
                .build()
                .execute_with(|| {
                    let total_stake = 20;
                    let bad_amount_to_unbond =
                        (total_stake - (2 * MinNominationPerCollator::get())) + 1;

                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let unbond_call = create_call_for_signed_schedule_nominator_unbond(
                        &staker,
                        nonce,
                        bad_amount_to_unbond,
                    );
                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        unbond_call,
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

mod proxy_signed_schedule_collator_unbond {
    use super::*;

    pub fn create_call_for_signed_schedule_candidate_unbond(
        staker: &Staker,
        sender_nonce: u64,
        reduction_amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof = create_proof_for_signed_schedule_candidate_unbond(
            sender_nonce,
            staker,
            &reduction_amount,
        );

        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_schedule_candidate_unbond {
                proof,
                less: reduction_amount,
            },
        ));
    }

    fn create_call_for_signed_schedule_candidate_unbond_proof(
        proof: Proof<Signature, AccountId>,
        reduction_amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_schedule_candidate_unbond {
                proof,
                less: reduction_amount,
            },
        ));
    }

    fn create_proof_for_signed_schedule_candidate_unbond(
        sender_nonce: u64,
        staker: &Staker,
        reduction_amount: &u128,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_schedule_candidate_unbond_params::<Test>(
            staker.relayer.clone(),
            reduction_amount,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature);
    }

    #[test]
    fn succeeds_with_good_values() {
        let collator_1: Staker = Default::default();
        let collator_2 = to_acc_id(2u64);
        let initial_stake = 100;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1.account_id, 10000),
                (collator_2, 10000),
                (collator_1.relayer, 10000),
            ])
            .with_candidates(vec![
                (collator_1.account_id, initial_stake),
                (collator_2, initial_stake),
            ])
            .build()
            .execute_with(|| {
                let min_collator_stake = ParachainStaking::min_collator_stake();

                let amount_to_decrease = initial_stake - min_collator_stake;
                let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);
                let candidate_unbond_call = create_call_for_signed_schedule_candidate_unbond(
                    &collator_1,
                    nonce,
                    amount_to_decrease,
                );

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(collator_1.relayer),
                    candidate_unbond_call,
                    None
                ));

                assert_event_emitted!(Event::CandidateBondLessRequested {
                    candidate: collator_1.account_id,
                    amount_to_decrease,
                    execute_era: ParachainStaking::delay() + 1,
                });

                // Nonce has increased
                assert_eq!(ParachainStaking::proxy_nonce(collator_1.account_id), nonce + 1);
            });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_2, 10000),
                    (collator_1.account_id, 10000),
                    (collator_1.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1.account_id, 100), (collator_2, 100)])
                .build()
                .execute_with(|| {
                    let amount_to_withdraw = 10;
                    let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);
                    let proof = create_proof_for_signed_schedule_candidate_unbond(
                        nonce,
                        &collator_1,
                        &amount_to_withdraw,
                    );

                    assert_noop!(
                        ParachainStaking::signed_schedule_candidate_unbond(
                            RawOrigin::None.into(),
                            proof.clone(),
                            amount_to_withdraw
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if its signed.
                    assert_ok!(ParachainStaking::signed_schedule_candidate_unbond(
                        RuntimeOrigin::signed(collator_1.account_id),
                        proof,
                        amount_to_withdraw
                    ));
                });
        }

        #[test]
        fn proxy_proof_nonce_is_not_valid() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_2, 10000),
                    (collator_1.account_id, 10000),
                    (collator_1.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1.account_id, 100), (collator_2, 100)])
                .build()
                .execute_with(|| {
                    let amount_to_withdraw = 10;
                    let bad_nonce = ParachainStaking::proxy_nonce(collator_1.account_id) + 1;
                    let unbond_call = create_call_for_signed_schedule_candidate_unbond(
                        &collator_1,
                        bad_nonce,
                        amount_to_withdraw,
                    );
                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(collator_1.relayer),
                        unbond_call,
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
        fn proxy_proof_signature_is_not_valid() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_2, 10000),
                    (collator_1.account_id, 10000),
                    (collator_1.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1.account_id, 100), (collator_2, 100)])
                .build()
                .execute_with(|| {
                    let amount_to_withdraw = 10;
                    let bad_amount_to_withdraw = 0;
                    let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);

                    let proof = create_proof_for_signed_schedule_candidate_unbond(
                        nonce,
                        &collator_1,
                        &amount_to_withdraw,
                    );

                    let unbond_call = create_call_for_signed_schedule_candidate_unbond_proof(
                        proof,
                        bad_amount_to_withdraw,
                    );
                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(collator_1.relayer),
                        unbond_call,
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
        fn withdrawal_reduces_candidate_bond_below_min_allowed() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            let collator_stake = 100;
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_2, 10000),
                    (collator_1.account_id, 10000),
                    (collator_1.relayer, 10000),
                ])
                .with_candidates(vec![
                    (collator_1.account_id, collator_stake),
                    (collator_2, collator_stake),
                ])
                .build()
                .execute_with(|| {
                    let amount_to_withdraw =
                        (collator_stake - ParachainStaking::min_collator_stake()) + 1;
                    let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);
                    let unbond_call = create_call_for_signed_schedule_candidate_unbond(
                        &collator_1,
                        nonce,
                        amount_to_withdraw,
                    );

                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(collator_1.relayer),
                        unbond_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(
                            Error::<Test>::CandidateBondBelowMin.into()
                        )
                    );
                });
        }
    }
}

mod signed_execute_nomination_request {
    use super::*;
    use crate::schedule_unbond_tests::proxy_signed_schedule_nominator_unbond::create_call_for_signed_schedule_nominator_unbond;

    fn schedule_unbond(staker: &Staker, amount: &u128) -> u64 {
        let nonce = ParachainStaking::proxy_nonce(staker.account_id);
        let unbond_call = create_call_for_signed_schedule_nominator_unbond(staker, nonce, *amount);

        assert_ok!(AvnProxy::proxy(RuntimeOrigin::signed(staker.relayer), unbond_call, None));

        // return updated nonce
        return ParachainStaking::proxy_nonce(staker.account_id);
    }

    fn create_call_for_signed_execute_nomination_request(
        staker: &Staker,
        sender_nonce: u64,
        nominator: AccountId,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof =
            create_proof_for_signed_execute_nomination_request(sender_nonce, staker, &nominator);

        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_execute_nomination_request { proof, nominator },
        ));
    }

    fn create_proof_for_signed_execute_nomination_request(
        sender_nonce: u64,
        staker: &Staker,
        nominator: &AccountId,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_execute_nomination_request_params::<Test>(
            staker.relayer.clone(),
            nominator,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature);
    }

    #[test]
    fn succeeds_with_good_values() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let staker: Staker = Default::default();
        let random_user: Staker = Staker::new(59u64, 88u64);
        let initial_stake = 100;
        let initial_balance = 10000;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1, initial_balance),
                (collator_2, initial_balance),
                (random_user.account_id, initial_balance),
                (staker.account_id, initial_balance),
                (staker.relayer, initial_balance),
            ])
            .with_candidates(vec![(collator_1, initial_stake), (collator_2, initial_stake)])
            .with_nominations(vec![
                (staker.account_id, collator_1, initial_stake),
                (staker.account_id, collator_2, initial_stake),
            ])
            .build()
            .execute_with(|| {
                let amount_to_unbond = 100;
                let initial_free_balance =
                    ParachainStaking::get_nominator_stakable_free_balance(&staker.account_id);
                let initial_total_stake = ParachainStaking::total();
                let initial_state_total =
                    ParachainStaking::nominator_state(staker.account_id).unwrap().total;
                let initial_collator1_state =
                    &ParachainStaking::top_nominations(collator_1).unwrap().nominations[0];
                let initial_collator2_state =
                    &ParachainStaking::top_nominations(collator_2).unwrap().nominations[0];

                let staker_nonce = schedule_unbond(&staker, &amount_to_unbond);
                roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                // Anyone can send this request
                let nonce = ParachainStaking::proxy_nonce(random_user.account_id);
                let execute_unbond_call = create_call_for_signed_execute_nomination_request(
                    &random_user,
                    nonce,
                    staker.account_id,
                );

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(random_user.relayer),
                    execute_unbond_call,
                    None
                ));

                // Nonce has increased
                assert_eq!(ParachainStaking::proxy_nonce(random_user.account_id), nonce + 1);
                // Staker nonce has stayed the same
                assert_eq!(ParachainStaking::proxy_nonce(staker.account_id), staker_nonce);

                assert_eq!(
                    ParachainStaking::get_nominator_stakable_free_balance(&staker.account_id),
                    initial_free_balance + amount_to_unbond
                );
                assert_eq!(ParachainStaking::total(), initial_total_stake - amount_to_unbond);
                assert_eq!(
                    ParachainStaking::nominator_state(staker.account_id).unwrap().total,
                    initial_state_total - amount_to_unbond
                );
                assert_eq!(
                    ParachainStaking::top_nominations(collator_1).unwrap().nominations[0].amount,
                    initial_collator1_state.amount - 50
                );
                assert_eq!(
                    ParachainStaking::top_nominations(collator_2).unwrap().nominations[0].amount,
                    initial_collator2_state.amount - 50
                );
            });
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
                .with_candidates(vec![(collator_1, 100), (collator_2, 100)])
                .with_nominations(vec![
                    (staker.account_id, collator_1, 100),
                    (staker.account_id, collator_2, 100),
                ])
                .build()
                .execute_with(|| {
                    let amount_to_unbond = 100;
                    schedule_unbond(&staker, &amount_to_unbond);
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof = create_proof_for_signed_execute_nomination_request(
                        nonce,
                        &staker,
                        &staker.account_id,
                    );

                    assert_noop!(
                        ParachainStaking::signed_execute_nomination_request(
                            RawOrigin::None.into(),
                            proof.clone(),
                            staker.account_id,
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if its signed.
                    assert_ok!(ParachainStaking::signed_execute_nomination_request(
                        RuntimeOrigin::signed(staker.account_id),
                        proof.clone(),
                        staker.account_id,
                    ));
                });
        }

        #[test]
        fn proxy_proof_is_not_valid_nonce() {
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
                .with_candidates(vec![(collator_1, 100), (collator_2, 100)])
                .with_nominations(vec![
                    (staker.account_id, collator_1, 100),
                    (staker.account_id, collator_2, 100),
                ])
                .build()
                .execute_with(|| {
                    let amount_to_unbond = 100;
                    schedule_unbond(&staker, &amount_to_unbond);
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let bad_nonce = ParachainStaking::proxy_nonce(staker.account_id) + 1;
                    let execute_unbond_call = create_call_for_signed_execute_nomination_request(
                        &staker,
                        bad_nonce,
                        staker.account_id,
                    );

                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        execute_unbond_call,
                        None
                    ),);
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(
                            avn_proxy_error::<Test>::UnauthorizedProxyTransaction.into()
                        )
                    );
                });
        }

        #[test]
        fn proxy_proof_is_not_valid_nominator() {
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
                .with_candidates(vec![(collator_1, 100), (collator_2, 100)])
                .with_nominations(vec![
                    (staker.account_id, collator_1, 100),
                    (staker.account_id, collator_2, 100),
                ])
                .build()
                .execute_with(|| {
                    let amount_to_unbond = 100;
                    schedule_unbond(&staker, &amount_to_unbond);
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let bad_nominator = staker.relayer;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof = create_proof_for_signed_execute_nomination_request(
                        nonce,
                        &staker,
                        &bad_nominator,
                    );

                    assert_noop!(
                        ParachainStaking::signed_execute_nomination_request(
                            RuntimeOrigin::signed(staker.account_id),
                            proof.clone(),
                            staker.account_id,
                        ),
                        Error::<Test>::UnauthorizedSignedExecuteNominationRequestTransaction
                    );
                });
        }
    }
}

mod signed_execute_candidate_unbond {
    use super::*;
    use crate::schedule_unbond_tests::proxy_signed_schedule_collator_unbond::create_call_for_signed_schedule_candidate_unbond;

    fn schedule_candidate_unbond(candidate: &Staker, amount: &u128) -> u64 {
        let nonce = ParachainStaking::proxy_nonce(candidate.account_id);
        let candidate_unbond_call =
            create_call_for_signed_schedule_candidate_unbond(candidate, nonce, *amount);

        assert_ok!(AvnProxy::proxy(
            RuntimeOrigin::signed(candidate.relayer),
            candidate_unbond_call,
            None
        ));

        // return updated nonce
        return ParachainStaking::proxy_nonce(candidate.account_id);
    }

    fn create_call_for_signed_execute_candidate_unbond(
        staker: &Staker,
        sender_nonce: u64,
        candidate: AccountId,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof =
            create_proof_for_signed_execute_candidate_unbond(sender_nonce, staker, &candidate);

        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_execute_candidate_unbond { proof, candidate },
        ));
    }

    fn create_proof_for_signed_execute_candidate_unbond(
        sender_nonce: u64,
        staker: &Staker,
        candidate: &AccountId,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_execute_candidate_unbond_params::<Test>(
            staker.relayer.clone(),
            candidate,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature);
    }

    #[test]
    fn succeeds_with_good_values() {
        let collator_1: Staker = Default::default();
        let collator_2 = to_acc_id(2u64);
        let random_user: Staker = Staker::new(59u64, 88u64);
        let initial_stake = 100;
        let initial_balance = 10000;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_2, initial_balance),
                (random_user.account_id, initial_balance),
                (collator_1.account_id, initial_balance),
                (collator_1.relayer, initial_balance),
            ])
            .with_candidates(vec![
                (collator_1.account_id, initial_stake),
                (collator_2, initial_stake),
            ])
            .build()
            .execute_with(|| {
                let amount_to_unbond = 10;
                let initial_free_balance =
                    ParachainStaking::get_collator_stakable_free_balance(&collator_1.account_id);
                let initial_total_stake = ParachainStaking::total();

                assert_eq!(ParachainStaking::candidate_pool().0[0].owner, collator_1.account_id);
                let initial_candidate_pool_amount = ParachainStaking::candidate_pool().0[0].amount;
                let initial_candidate_state =
                    ParachainStaking::candidate_info(collator_1.account_id).unwrap();

                let collator_1_nonce = schedule_candidate_unbond(&collator_1, &amount_to_unbond);
                roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                // Anyone can send this request
                let nonce = ParachainStaking::proxy_nonce(random_user.account_id);
                let execute_unbond_call = create_call_for_signed_execute_candidate_unbond(
                    &random_user,
                    nonce,
                    collator_1.account_id,
                );

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(random_user.relayer),
                    execute_unbond_call,
                    None
                ));

                assert_event_emitted!(Event::CandidateBondedLess {
                    candidate: collator_1.account_id,
                    amount: amount_to_unbond,
                    new_bond: initial_stake - amount_to_unbond
                });

                assert_eq!(
                    ParachainStaking::get_collator_stakable_free_balance(&collator_1.account_id),
                    initial_free_balance + amount_to_unbond
                );
                assert_eq!(ParachainStaking::total(), initial_total_stake - amount_to_unbond);

                assert_eq!(
                    ParachainStaking::candidate_info(collator_1.account_id).unwrap().bond,
                    initial_candidate_state.bond - amount_to_unbond
                );
                // Candidate pool owner has not changed, but the amount has.
                assert_eq!(ParachainStaking::candidate_pool().0[0].owner, collator_1.account_id);
                assert_eq!(
                    ParachainStaking::candidate_pool().0[0].amount,
                    initial_candidate_pool_amount - amount_to_unbond
                );

                // Nonce has increased
                assert_eq!(ParachainStaking::proxy_nonce(random_user.account_id), nonce + 1);
                // Staker nonce has stayed the same
                assert_eq!(ParachainStaking::proxy_nonce(collator_1.account_id), collator_1_nonce);
            });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_2, 10000),
                    (collator_1.account_id, 10000),
                    (collator_1.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1.account_id, 100), (collator_2, 100)])
                .build()
                .execute_with(|| {
                    let amount_to_unbond = 10;
                    schedule_candidate_unbond(&collator_1, &amount_to_unbond);
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);
                    let proof = create_proof_for_signed_execute_candidate_unbond(
                        nonce,
                        &collator_1,
                        &collator_1.account_id,
                    );

                    assert_noop!(
                        ParachainStaking::signed_execute_candidate_unbond(
                            RawOrigin::None.into(),
                            proof.clone(),
                            collator_1.account_id
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if its signed.
                    assert_ok!(ParachainStaking::signed_execute_candidate_unbond(
                        RuntimeOrigin::signed(collator_1.account_id),
                        proof,
                        collator_1.account_id
                    ));
                });
        }

        #[test]
        fn proxy_proof_is_not_valid() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_2, 10000),
                    (collator_1.account_id, 10000),
                    (collator_1.relayer, 10000),
                ])
                .with_candidates(vec![(collator_1.account_id, 100), (collator_2, 100)])
                .build()
                .execute_with(|| {
                    let amount_to_unbond = 10;
                    schedule_candidate_unbond(&collator_1, &amount_to_unbond);
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let bad_nonce = ParachainStaking::proxy_nonce(collator_1.account_id) + 1;
                    let proof = create_proof_for_signed_execute_candidate_unbond(
                        bad_nonce,
                        &collator_1,
                        &collator_1.account_id,
                    );

                    assert_noop!(
                        ParachainStaking::signed_execute_candidate_unbond(
                            RuntimeOrigin::signed(collator_1.account_id),
                            proof.clone(),
                            collator_1.account_id
                        ),
                        Error::<Test>::UnauthorizedSignedExecuteCandidateUnbondTransaction
                    );
                });
        }
    }
}

// NOMINATOR BOND LESS

#[test]
fn nominator_unbond_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id,
                5
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationDecreaseScheduled {
                nominator: account_id_2,
                candidate: account_id,
                amount_to_decrease: 5,
                execute_era: 3,
            }));
        });
}

#[test]
fn nominator_unbond_updates_nominator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
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
        });
}

#[test]
fn nominator_not_allowed_to_unbond_if_leaving() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(RuntimeOrigin::signed(
                account_id_2
            )));
            assert_noop!(
                ParachainStaking::schedule_nominator_unbond(
                    RuntimeOrigin::signed(account_id_2),
                    account_id,
                    1
                ),
                <Error<Test>>::PendingNominationRequestAlreadyExists,
            );
        });
}

#[test]
fn cannot_nominator_unbond_if_revoking() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25), (account_id_3, 20)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            assert_noop!(
                ParachainStaking::schedule_nominator_unbond(
                    RuntimeOrigin::signed(account_id_2),
                    account_id,
                    1
                ),
                Error::<Test>::PendingNominationRequestAlreadyExists
            );
        });
}

#[test]
fn cannot_nominator_unbond_if_not_nominator() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id,
                5
            ),
            Error::<Test>::NominatorDNE
        );
    });
}

#[test]
fn cannot_nominator_unbond_if_candidate_dne() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::schedule_nominator_unbond(
                    RuntimeOrigin::signed(account_id_2),
                    to_acc_id(3),
                    5
                ),
                Error::<Test>::NominationDNE
            );
        });
}

#[test]
fn cannot_nominator_unbond_if_nomination_dne() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10), (account_id_3, 30)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::schedule_nominator_unbond(
                    RuntimeOrigin::signed(account_id_2),
                    account_id_3,
                    5
                ),
                Error::<Test>::NominationDNE
            );
        });
}

#[test]
fn cannot_nominator_unbond_below_min_collator_stk() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::schedule_nominator_unbond(
                    RuntimeOrigin::signed(account_id_2),
                    account_id,
                    6
                ),
                Error::<Test>::NominatorBondBelowMin
            );
        });
}

#[test]
fn cannot_nominator_unbond_extra_than_total_nomination() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::schedule_nominator_unbond(
                    RuntimeOrigin::signed(account_id_2),
                    account_id,
                    11
                ),
                Error::<Test>::NominatorBondBelowMin
            );
        });
}

#[test]
fn cannot_nominator_unbond_below_min_nomination() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (account_id_3, 30)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::schedule_nominator_unbond(
                    RuntimeOrigin::signed(account_id_2),
                    account_id,
                    10
                ),
                Error::<Test>::NominatorBondBelowMin
            );
        });
}

// SCHEDULE CANDIDATE BOND LESS

#[test]
fn schedule_candidate_unbond_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                10
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateBondLessRequested {
                candidate: account_id,
                amount_to_decrease: 10,
                execute_era: 3,
            }));
        });
}

#[test]
fn cannot_schedule_candidate_unbond_if_request_exists() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                5
            ));
            assert_noop!(
                ParachainStaking::schedule_candidate_unbond(RuntimeOrigin::signed(account_id), 5),
                Error::<Test>::PendingCandidateRequestAlreadyExists
            );
        });
}

#[test]
fn cannot_schedule_candidate_unbond_if_not_candidate() {
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::schedule_candidate_unbond(RuntimeOrigin::signed(to_acc_id(6)), 50),
            Error::<Test>::CandidateDNE
        );
    });
}

#[test]
fn cannot_schedule_candidate_unbond_if_new_total_below_min_candidate_stk() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::schedule_candidate_unbond(RuntimeOrigin::signed(account_id), 21),
                Error::<Test>::CandidateBondBelowMin
            );
        });
}

#[test]
fn can_schedule_candidate_unbond_if_leaving_candidates() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                RuntimeOrigin::signed(account_id),
                1
            ));
            assert_ok!(ParachainStaking::schedule_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                10
            ));
        });
}

#[test]
fn cannot_schedule_candidate_unbond_if_exited_candidates() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                RuntimeOrigin::signed(account_id),
                1
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_leave_candidates(
                RuntimeOrigin::signed(account_id),
                account_id,
                0
            ));
            assert_noop!(
                ParachainStaking::schedule_candidate_unbond(RuntimeOrigin::signed(account_id), 10),
                Error::<Test>::CandidateDNE
            );
        });
}
// EXECUTE CANDIDATE BOND LESS REQUEST

#[test]
fn execute_candidate_unbond_emits_correct_event() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 50)])
        .with_candidates(vec![(account_id, 50)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                30
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                account_id
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateBondedLess {
                candidate: account_id,
                amount: 30,
                new_bond: 20
            }));
        });
}

#[test]
fn execute_candidate_unbond_unreserves_balance() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 0);
            assert_ok!(ParachainStaking::schedule_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                10
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                account_id
            ));
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 10);
        });
}

#[test]
fn execute_candidate_unbond_decreases_total() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            let mut total = ParachainStaking::total();
            assert_ok!(ParachainStaking::schedule_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                10
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                account_id
            ));
            total -= 10;
            assert_eq!(ParachainStaking::total(), total);
        });
}

#[test]
fn execute_candidate_unbond_updates_candidate_state() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("updated => exists");
            assert_eq!(candidate_state.bond, 30);
            assert_ok!(ParachainStaking::schedule_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                10
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                account_id
            ));
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("updated => exists");
            assert_eq!(candidate_state.bond, 20);
        });
}

#[test]
fn execute_candidate_unbond_updates_candidate_pool() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30)])
        .with_candidates(vec![(account_id, 30)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::candidate_pool().0[0].owner, account_id);
            assert_eq!(ParachainStaking::candidate_pool().0[0].amount, 30);
            assert_ok!(ParachainStaking::schedule_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                10
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_candidate_unbond(
                RuntimeOrigin::signed(account_id),
                account_id
            ));
            assert_eq!(ParachainStaking::candidate_pool().0[0].owner, account_id);
            assert_eq!(ParachainStaking::candidate_pool().0[0].amount, 20);
        });
}

// 2. EXECUTE NOMINATOR BOND LESS

#[test]
fn execute_nominator_unbond_unreserves_balance() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 0);
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id,
                5
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 5);
        });
}

#[test]
fn execute_nominator_unbond_decreases_total_staked() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::total(), 40);
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id,
                5
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_eq!(ParachainStaking::total(), 35);
        });
}

#[test]
fn execute_nominator_unbond_updates_nominator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::nominator_state(account_id_2).expect("exists").total, 10);
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id,
                5
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_eq!(ParachainStaking::nominator_state(account_id_2).expect("exists").total, 5);
        });
}

#[test]
fn execute_nominator_unbond_updates_candidate_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(
                ParachainStaking::top_nominations(account_id).unwrap().nominations[0].owner,
                account_id_2
            );
            assert_eq!(
                ParachainStaking::top_nominations(account_id).unwrap().nominations[0].amount,
                10
            );
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id,
                5
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_eq!(
                ParachainStaking::top_nominations(account_id).unwrap().nominations[0].owner,
                account_id_2
            );
            assert_eq!(
                ParachainStaking::top_nominations(account_id).unwrap().nominations[0].amount,
                5
            );
        });
}

#[test]
fn execute_nominator_unbond_updates_just_bottom_nominations() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 10),
            (to_acc_id(3), 11),
            (to_acc_id(4), 12),
            (to_acc_id(5), 14),
            (to_acc_id(6), 15),
        ])
        .with_candidates(vec![(account_id, 20)])
        .with_nominations(vec![
            (account_id_2, account_id, 10),
            (to_acc_id(3), account_id, 11),
            (to_acc_id(4), account_id, 12),
            (to_acc_id(5), account_id, 14),
            (to_acc_id(6), account_id, 15),
        ])
        .build()
        .execute_with(|| {
            let pre_call_candidate_info =
                ParachainStaking::candidate_info(&account_id).expect("nominated by all so exists");
            let pre_call_top_nominations =
                ParachainStaking::top_nominations(&account_id).expect("nominated by all so exists");
            let pre_call_bottom_nominations = ParachainStaking::bottom_nominations(&account_id)
                .expect("nominated by all so exists");
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id,
                2
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            let post_call_candidate_info =
                ParachainStaking::candidate_info(&account_id).expect("nominated by all so exists");
            let post_call_top_nominations =
                ParachainStaking::top_nominations(&account_id).expect("nominated by all so exists");
            let post_call_bottom_nominations = ParachainStaking::bottom_nominations(&account_id)
                .expect("nominated by all so exists");
            let mut not_equal = false;
            for Bond { owner, amount } in pre_call_bottom_nominations.nominations {
                for Bond { owner: post_owner, amount: post_amount } in
                    &post_call_bottom_nominations.nominations
                {
                    if &owner == post_owner {
                        if &amount != post_amount {
                            not_equal = true;
                            break;
                        }
                    }
                }
            }
            assert!(not_equal);
            let mut equal = true;
            for Bond { owner, amount } in pre_call_top_nominations.nominations {
                for Bond { owner: post_owner, amount: post_amount } in
                    &post_call_top_nominations.nominations
                {
                    if &owner == post_owner {
                        if &amount != post_amount {
                            equal = false;
                            break;
                        }
                    }
                }
            }
            assert!(equal);
            assert_eq!(
                pre_call_candidate_info.total_counted,
                post_call_candidate_info.total_counted
            );
        });
}

#[test]
fn execute_nominator_unbond_does_not_delete_bottom_nominations() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_6 = to_acc_id(6u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 20),
            (account_id_2, 10),
            (to_acc_id(3), 11),
            (to_acc_id(4), 12),
            (to_acc_id(5), 14),
            (account_id_6, 15),
        ])
        .with_candidates(vec![(account_id, 20)])
        .with_nominations(vec![
            (account_id_2, account_id, 10),
            (to_acc_id(3), account_id, 11),
            (to_acc_id(4), account_id, 12),
            (to_acc_id(5), account_id, 14),
            (account_id_6, account_id, 15),
        ])
        .build()
        .execute_with(|| {
            let pre_call_candidate_info =
                ParachainStaking::candidate_info(&account_id).expect("nominated by all so exists");
            let pre_call_top_nominations =
                ParachainStaking::top_nominations(&account_id).expect("nominated by all so exists");
            let pre_call_bottom_nominations = ParachainStaking::bottom_nominations(&account_id)
                .expect("nominated by all so exists");
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_6),
                account_id,
                4
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_6),
                account_id_6,
                account_id
            ));
            let post_call_candidate_info =
                ParachainStaking::candidate_info(&account_id).expect("nominated by all so exists");
            let post_call_top_nominations =
                ParachainStaking::top_nominations(&account_id).expect("nominated by all so exists");
            let post_call_bottom_nominations = ParachainStaking::bottom_nominations(&account_id)
                .expect("nominated by all so exists");
            let mut equal = true;
            for Bond { owner, amount } in pre_call_bottom_nominations.nominations {
                for Bond { owner: post_owner, amount: post_amount } in
                    &post_call_bottom_nominations.nominations
                {
                    if &owner == post_owner {
                        if &amount != post_amount {
                            equal = false;
                            break;
                        }
                    }
                }
            }
            assert!(equal);
            let mut not_equal = false;
            for Bond { owner, amount } in pre_call_top_nominations.nominations {
                for Bond { owner: post_owner, amount: post_amount } in
                    &post_call_top_nominations.nominations
                {
                    if &owner == post_owner {
                        if &amount != post_amount {
                            not_equal = true;
                            break;
                        }
                    }
                }
            }
            assert!(not_equal);
            assert_eq!(
                pre_call_candidate_info.total_counted - 4,
                post_call_candidate_info.total_counted
            );
        });
}

#[test]
fn can_execute_nominator_unbond_for_leaving_candidate() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 15)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                RuntimeOrigin::signed(account_id),
                1
            ));
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id,
                5
            ));
            roll_to(10);
            // can execute bond more nomination request for leaving candidate
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
        });
}
