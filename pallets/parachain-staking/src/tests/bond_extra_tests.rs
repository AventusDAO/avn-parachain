//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{
    assert_event_emitted, assert_last_event, encode_signed_bond_extra_params,
    encode_signed_candidate_bond_extra_params,
    mock::{
        build_proof, inner_call_failed_event_emitted, sign, AccountId, AvnProxy, ExtBuilder,
        MinNominationPerCollator, ParachainStaking, RuntimeCall as MockCall,
        RuntimeEvent as MetaEvent, RuntimeOrigin as Origin, Signature, Staker, Test, TestAccount,
    },
    Config, Error, Event, Proof,
};
use frame_support::{assert_noop, assert_ok, error::BadOrigin};
use frame_system::RawOrigin;
use pallet_avn_proxy::Error as avn_proxy_error;

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id()
}

mod proxy_signed_bond_extra {
    use super::*;

    fn create_call_for_bond_extra(
        staker: &Staker,
        sender_nonce: u64,
        extra_amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof = create_proof_for_signed_bond_extra(sender_nonce, staker, &extra_amount);

        return Box::new(MockCall::ParachainStaking(super::super::Call::<Test>::signed_bond_extra {
            proof,
            extra_amount,
        }))
    }

    fn create_call_for_bond_extra_from_proof(
        proof: Proof<Signature, AccountId>,
        extra_amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        return Box::new(MockCall::ParachainStaking(super::super::Call::<Test>::signed_bond_extra {
            proof,
            extra_amount,
        }))
    }

    fn create_proof_for_signed_bond_extra(
        sender_nonce: u64,
        staker: &Staker,
        extra_amount: &u128,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_bond_extra_params::<Test>(
            staker.relayer.clone(),
            extra_amount,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature)
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let collator_3 = to_acc_id(3u64);
        let staker: Staker = Default::default();
        let initial_stake = 10;
        let initial_balance = 10000;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1, initial_balance),
                (collator_2, initial_balance),
                (collator_3, initial_balance),
                (staker.account_id, initial_balance),
                (staker.relayer, initial_balance),
            ])
            .with_candidates(vec![
                (collator_1, initial_stake),
                (collator_2, initial_stake),
                (collator_3, initial_stake),
            ])
            .with_nominations(vec![
                (staker.account_id, collator_1, initial_stake),
                (staker.account_id, collator_2, initial_stake),
            ])
            .build()
            .execute_with(|| {
                let min_user_stake = MinNominationPerCollator::get();
                let initial_total_stake_on_chain = ParachainStaking::total();

                // Pick an amount that is not perfectly divisible by the number of collators
                let dust = 1u128;
                let amount_to_topup = (min_user_stake * 2u128) + dust;
                let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                let bond_extra_call = create_call_for_bond_extra(&staker, nonce, amount_to_topup);
                assert_ok!(AvnProxy::proxy(Origin::signed(staker.relayer), bond_extra_call, None));

                // The staker state has also been updated
                let staker_state = ParachainStaking::nominator_state(staker.account_id).unwrap();
                assert_eq!(staker_state.total, initial_stake * 2 + amount_to_topup);

                // Each collator that has been nominated has been topped up by the expected amount
                let nominations = staker_state.nominations.0.clone();
                for (index, nomination) in nominations.into_iter().enumerate() {
                    // We should have one event per collator. One of the collators gets any
                    // remaining dust. We selected index 1 because the test starts at block #1 and
                    // 1 mod num_of_collators is always 1.
                    let mut topup = min_user_stake;
                    if index == 1 {
                        topup = min_user_stake + dust;
                    }

                    assert_event_emitted!(Event::NominationIncreased {
                        nominator: staker.account_id,
                        candidate: nomination.owner,
                        amount: topup,
                        in_top: true
                    });

                    // Staker state reflects the new nomination for each collator
                    assert_eq!(staker_state.nominations.0[index].owner, nomination.owner);
                    assert_eq!(staker_state.nominations.0[index].amount, initial_stake + topup);

                    // Collator state has been updated
                    let collator_state =
                        ParachainStaking::candidate_info(nomination.owner).unwrap();
                    assert_eq!(collator_state.total_counted, initial_stake + initial_stake + topup);

                    // Collator nominations have also been updated
                    let top_nominations =
                        ParachainStaking::top_nominations(nomination.owner).unwrap();
                    assert_eq!(top_nominations.total, initial_stake + topup);
                }

                // The staker free balance has been reduced
                assert_eq!(
                    ParachainStaking::get_nominator_stakable_free_balance(&staker.account_id),
                    10000 - (initial_stake * 2 + amount_to_topup)
                );

                // The total amount staked on chain should increase
                assert_eq!(
                    initial_total_stake_on_chain + amount_to_topup,
                    ParachainStaking::total()
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
        let collator_6 = to_acc_id(6u64);
        let staker: Staker = Default::default();
        let initial_stake = 10;
        let initial_balance = 596583960081717817908u128;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1, initial_balance),
                (collator_2, initial_balance),
                (collator_3, initial_balance),
                (collator_4, initial_balance),
                (collator_5, initial_balance),
                (collator_6, initial_balance),
                (staker.account_id, initial_balance),
                (staker.relayer, initial_balance),
            ])
            .with_candidates(vec![
                (collator_1, initial_stake),
                (collator_2, initial_stake),
                (collator_3, initial_stake),
                (collator_4, initial_stake),
                (collator_5, initial_stake),
                (collator_6, initial_stake),
            ])
            .with_nominations(vec![
                (staker.account_id, collator_1, initial_stake),
                (staker.account_id, collator_2, initial_stake),
                (staker.account_id, collator_3, initial_stake),
                (staker.account_id, collator_4, initial_stake),
                (staker.account_id, collator_5, initial_stake),
            ])
            .build()
            .execute_with(|| {
                let initial_total_stake_on_chain = ParachainStaking::total();

                // Pick an amount that is not perfectly divisible by the number of collators
                // 496583960081717817908 / 5 * 5 will give 496583960081717817910 due to rounding
                let amount_to_topup = 496583960081717817908u128;
                let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                let bond_extra_call = create_call_for_bond_extra(&staker, nonce, amount_to_topup);
                assert_ok!(AvnProxy::proxy(Origin::signed(staker.relayer), bond_extra_call, None));

                // The staker state has also been updated
                let staker_state = ParachainStaking::nominator_state(staker.account_id).unwrap();
                assert_eq!(staker_state.total, initial_stake * 5 + amount_to_topup);

                // The staker free balance has been reduced
                assert_eq!(
                    ParachainStaking::get_nominator_stakable_free_balance(&staker.account_id),
                    initial_balance - (initial_stake * 5 + amount_to_topup)
                );

                // The total amount staked on chain should increase
                assert_eq!(
                    initial_total_stake_on_chain + amount_to_topup,
                    ParachainStaking::total()
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
                .with_nominations(vec![
                    (staker.account_id, collator_1, 10),
                    (staker.account_id, collator_2, 10),
                ])
                .build()
                .execute_with(|| {
                    let amount_to_topup = MinNominationPerCollator::get() * 2u128;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof =
                        create_proof_for_signed_bond_extra(nonce, &staker, &amount_to_topup);

                    assert_noop!(
                        ParachainStaking::signed_bond_extra(
                            RawOrigin::None.into(),
                            proof.clone(),
                            amount_to_topup
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if it's signed.
                    assert_ok!(ParachainStaking::signed_bond_extra(
                        Origin::signed(staker.account_id),
                        proof,
                        amount_to_topup
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
                    let amount_to_topup = MinNominationPerCollator::get() * 2u128;
                    let bad_nonce = ParachainStaking::proxy_nonce(staker.account_id) + 1;
                    let bond_extra_call =
                        create_call_for_bond_extra(&staker, bad_nonce, amount_to_topup);

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        bond_extra_call,
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
        fn proxy_proof_extra_amount_is_not_valid() {
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
                    let bad_amount_to_topup = 0u128;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);

                    let proof =
                        create_proof_for_signed_bond_extra(nonce, &staker, &bad_amount_to_topup);
                    let bond_extra_call =
                        create_call_for_bond_extra_from_proof(proof, bad_amount_to_topup);

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        bond_extra_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(Error::<Test>::NominationBelowMin.into())
                    );
                });
        }

        #[test]
        fn proxy_proof_extra_signature_is_not_valid() {
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
                    let amount_to_topup = 1u128;
                    let bad_amount_to_topup = 0u128;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);

                    let proof =
                        create_proof_for_signed_bond_extra(nonce, &staker, &amount_to_topup);
                    let bond_extra_call =
                        create_call_for_bond_extra_from_proof(proof, bad_amount_to_topup);

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        bond_extra_call,
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
                    let bad_amount_to_stake = staker_balance + 1;
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);

                    // Make sure 'bad_amount' is over the minimum allowed.
                    assert!(bad_amount_to_stake > MinNominationPerCollator::get() * 2u128);

                    // Make sure staker has less than they are attempting to stake
                    assert!(staker_balance < bad_amount_to_stake);

                    let bond_extra_call =
                        create_call_for_bond_extra(&staker, nonce, bad_amount_to_stake);

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        bond_extra_call,
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
                .with_nominations(vec![
                    (staker.account_id, collator_1, 10),
                    (staker.account_id, collator_2, 10),
                ])
                .build()
                .execute_with(|| {
                    let min_allowed_amount_to_stake = MinNominationPerCollator::get() * 2u128;
                    let bad_stake_amount = min_allowed_amount_to_stake - 1;

                    // Show that the staker has enough funds to cover the stake
                    assert!(staker_balance > bad_stake_amount);

                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let bond_extra_call =
                        create_call_for_bond_extra(&staker, nonce, bad_stake_amount);

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(staker.relayer),
                        bond_extra_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(Error::<Test>::NominationBelowMin.into())
                    );
                });
        }
    }
}

mod proxy_signed_candidate_bond_extra {
    use super::*;

    fn create_call_for_candidate_bond_extra(
        staker: &Staker,
        sender_nonce: u64,
        extra_amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof =
            create_proof_for_signed_candidate_bond_extra(sender_nonce, staker, &extra_amount);

        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_candidate_bond_extra { proof, extra_amount },
        ))
    }

    fn create_call_for_candidate_bond_extra_from_proof(
        proof: Proof<Signature, AccountId>,
        extra_amount: u128,
    ) -> Box<<Test as Config>::RuntimeCall> {
        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_candidate_bond_extra { proof, extra_amount },
        ))
    }

    fn create_proof_for_signed_candidate_bond_extra(
        sender_nonce: u64,
        staker: &Staker,
        extra_amount: &u128,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_candidate_bond_extra_params::<Test>(
            staker.relayer.clone(),
            extra_amount,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature)
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let collator_1: Staker = Default::default();
        let collator_2 = to_acc_id(2u64);
        let initial_stake = 10;
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
                let initial_total_stake_on_chain = ParachainStaking::total();

                let amount_to_topup = min_collator_stake + 1u128;
                let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);
                let bond_extra_call =
                    create_call_for_candidate_bond_extra(&collator_1, nonce, amount_to_topup);
                assert_ok!(AvnProxy::proxy(
                    Origin::signed(collator_1.relayer),
                    bond_extra_call,
                    None
                ));

                assert_event_emitted!(Event::CandidateBondedMore {
                    candidate: collator_1.account_id,
                    amount: amount_to_topup,
                    new_total_bond: initial_stake + amount_to_topup
                });

                // Candidate pool has been updated
                assert_eq!(ParachainStaking::candidate_pool().0[0].owner, collator_1.account_id);
                assert_eq!(
                    ParachainStaking::candidate_pool().0[0].amount,
                    initial_stake + amount_to_topup
                );

                // Collator state has been updated
                let collator_state =
                    ParachainStaking::candidate_info(collator_1.account_id).unwrap();
                assert_eq!(collator_state.bond, initial_stake + amount_to_topup);

                // The staker free balance has been reduced
                assert_eq!(
                    ParachainStaking::get_collator_stakable_free_balance(&collator_1.account_id),
                    10000 - (initial_stake + amount_to_topup)
                );

                // The total amount staked on chain should increase
                assert_eq!(
                    initial_total_stake_on_chain + amount_to_topup,
                    ParachainStaking::total()
                );

                // Nonce has increased
                assert_eq!(ParachainStaking::proxy_nonce(collator_1.account_id), nonce + 1);
            })
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            let initial_stake = 10;
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
                    let amount_to_topup = min_collator_stake + 1u128;
                    let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);
                    let proof = create_proof_for_signed_candidate_bond_extra(
                        nonce,
                        &collator_1,
                        &amount_to_topup,
                    );

                    assert_noop!(
                        ParachainStaking::signed_candidate_bond_extra(
                            RawOrigin::None.into(),
                            proof.clone(),
                            amount_to_topup
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if it's signed.
                    assert_ok!(ParachainStaking::signed_candidate_bond_extra(
                        Origin::signed(collator_1.account_id),
                        proof,
                        amount_to_topup
                    ));
                });
        }

        #[test]
        fn proxy_proof_nonce_is_not_valid() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            let initial_stake = 10;
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
                    let amount_to_topup = min_collator_stake + 1u128;
                    let bad_nonce = ParachainStaking::proxy_nonce(collator_1.account_id) + 1;
                    let bond_extra_call = create_call_for_candidate_bond_extra(
                        &collator_1,
                        bad_nonce,
                        amount_to_topup,
                    );

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(collator_1.relayer),
                        bond_extra_call,
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
            let initial_stake = 10;
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
                    let amount_to_topup = min_collator_stake + 1u128;
                    let bad_amount_to_topup = 0u128;
                    let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);

                    let proof = create_proof_for_signed_candidate_bond_extra(
                        nonce,
                        &collator_1,
                        &amount_to_topup,
                    );
                    let bond_extra_call =
                        create_call_for_candidate_bond_extra_from_proof(proof, bad_amount_to_topup);

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(collator_1.relayer),
                        bond_extra_call,
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
        fn candidate_does_not_have_enough_funds() {
            let collator_1: Staker = Default::default();
            let collator_2 = to_acc_id(2u64);
            let initial_balance = 10000;
            let initial_stake = 10;
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1.account_id, initial_balance),
                    (collator_2, initial_balance),
                    (collator_1.relayer, initial_balance),
                ])
                .with_candidates(vec![
                    (collator_1.account_id, initial_stake),
                    (collator_2, initial_stake),
                ])
                .build()
                .execute_with(|| {
                    let bad_amount_to_stake = initial_balance + 1;
                    let nonce = ParachainStaking::proxy_nonce(collator_1.account_id);

                    // Make sure 'bad_amount' is over the minimum allowed.
                    assert!(bad_amount_to_stake > ParachainStaking::min_collator_stake());

                    // Make sure staker has less than they are attempting to stake
                    assert!(initial_balance < bad_amount_to_stake);

                    let bond_extra_call = create_call_for_candidate_bond_extra(
                        &collator_1,
                        nonce,
                        bad_amount_to_stake,
                    );

                    assert_ok!(AvnProxy::proxy(
                        Origin::signed(collator_1.relayer),
                        bond_extra_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(Error::<Test>::InsufficientBalance.into())
                    );
                });
        }
    }
}

// NOMINATOR BOND EXTRA

#[test]
fn nominator_bond_extra_reserves_balance() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 5);
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_2), account_id, 5));
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 0);
        });
}

#[test]
fn nominator_bond_extra_increases_total_staked() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::total(), 40);
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_2), account_id, 5));
            assert_eq!(ParachainStaking::total(), 45);
        });
}

#[test]
fn nominator_bond_extra_updates_nominator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::nominator_state(account_id_2).expect("exists").total, 10);
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_2), account_id, 5));
            assert_eq!(ParachainStaking::nominator_state(account_id_2).expect("exists").total, 15);
        });
}

#[test]
fn nominator_bond_extra_updates_candidate_state_top_nominations() {
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
            assert_eq!(ParachainStaking::top_nominations(account_id).unwrap().total, 10);
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_2), account_id, 5));
            assert_eq!(
                ParachainStaking::top_nominations(account_id).unwrap().nominations[0].owner,
                account_id_2
            );
            assert_eq!(
                ParachainStaking::top_nominations(account_id).unwrap().nominations[0].amount,
                15
            );
            assert_eq!(ParachainStaking::top_nominations(account_id).unwrap().total, 15);
        });
}

#[test]
fn nominator_bond_extra_updates_candidate_state_bottom_nominations() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![
            (account_id, 30),
            (account_id_2, 20),
            (to_acc_id(3), 20),
            (to_acc_id(4), 20),
            (to_acc_id(5), 20),
            (to_acc_id(6), 20),
        ])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![
            (account_id_2, account_id, 10),
            (to_acc_id(3), account_id, 20),
            (to_acc_id(4), account_id, 20),
            (to_acc_id(5), account_id, 20),
            (to_acc_id(6), account_id, 20),
        ])
        .build()
        .execute_with(|| {
            assert_eq!(
                ParachainStaking::bottom_nominations(account_id).expect("exists").nominations[0]
                    .owner,
                account_id_2
            );
            assert_eq!(
                ParachainStaking::bottom_nominations(account_id).expect("exists").nominations[0]
                    .amount,
                10
            );
            assert_eq!(ParachainStaking::bottom_nominations(account_id).unwrap().total, 10);
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_2), account_id, 5));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationIncreased {
                nominator: account_id_2,
                candidate: account_id,
                amount: 5,
                in_top: false
            }));
            assert_eq!(
                ParachainStaking::bottom_nominations(account_id).expect("exists").nominations[0]
                    .owner,
                account_id_2
            );
            assert_eq!(
                ParachainStaking::bottom_nominations(account_id).expect("exists").nominations[0]
                    .amount,
                15
            );
            assert_eq!(ParachainStaking::bottom_nominations(account_id).unwrap().total, 15);
        });
}

#[test]
fn can_nominator_bond_extra_for_leaving_candidate() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 15)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(Origin::signed(account_id), 1));
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_2), account_id, 5));
        });
}

#[test]
fn nominator_bond_extra_disallowed_when_revoke_scheduled() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                Origin::signed(account_id_2),
                account_id
            ));
            assert_noop!(
                ParachainStaking::bond_extra(Origin::signed(account_id_2), account_id, 5),
                <Error<Test>>::PendingNominationRevoke
            );
        });
}

#[test]
fn nominator_bond_extra_allowed_when_bond_decrease_scheduled() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 25)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 15)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                Origin::signed(account_id_2),
                account_id,
                5,
            ));
            assert_ok!(ParachainStaking::bond_extra(Origin::signed(account_id_2), account_id, 5));
        });
}

// CANDIDATE BOND EXTRA

#[test]
fn candidate_bond_extra_emits_correct_event() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 50)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::candidate_bond_extra(Origin::signed(account_id), 30));
            assert_last_event!(MetaEvent::ParachainStaking(Event::CandidateBondedMore {
                candidate: account_id,
                amount: 30,
                new_total_bond: 50
            }));
        });
}

#[test]
fn candidate_bond_extra_reserves_balance() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 50)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 30);
            assert_ok!(ParachainStaking::candidate_bond_extra(Origin::signed(account_id), 30));
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 0);
        });
}

#[test]
fn candidate_bond_extra_increases_total() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 50)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            let additional_stake = 30;
            let total = ParachainStaking::total();
            assert_ok!(ParachainStaking::candidate_bond_extra(
                Origin::signed(account_id),
                additional_stake
            ));
            assert_eq!(ParachainStaking::total(), total + additional_stake);
        });
}

#[test]
fn candidate_bond_extra_updates_candidate_state() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 50)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("updated => exists");
            assert_eq!(candidate_state.bond, 20);
            assert_ok!(ParachainStaking::candidate_bond_extra(Origin::signed(account_id), 30));
            let candidate_state =
                ParachainStaking::candidate_info(account_id).expect("updated => exists");
            assert_eq!(candidate_state.bond, 50);
        });
}

#[test]
fn candidate_bond_extra_updates_candidate_pool() {
    let account_id = to_acc_id(1u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 50)])
        .with_candidates(vec![(account_id, 20)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::candidate_pool().0[0].owner, account_id);
            assert_eq!(ParachainStaking::candidate_pool().0[0].amount, 20);
            assert_ok!(ParachainStaking::candidate_bond_extra(Origin::signed(account_id), 30));
            assert_eq!(ParachainStaking::candidate_pool().0[0].owner, account_id);
            assert_eq!(ParachainStaking::candidate_pool().0[0].amount, 50);
        });
}
