//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{
    assert_event_emitted, assert_last_event, encode_signed_execute_leave_nominators_params,
    encode_signed_schedule_leave_nominators_params,
    encode_signed_schedule_revoke_nomination_params,
    mock::{
        build_proof, inner_call_failed_event_emitted, roll_to, roll_to_era_begin, sign, AccountId,
        AvnProxy, Balances, ExtBuilder, ParachainStaking, RuntimeCall as MockCall,
        RuntimeEvent as MetaEvent, RuntimeOrigin, Signature, Staker, Test, TestAccount,
    },
    Config, Error, Event, Proof,
};
use frame_support::{assert_noop, assert_ok, error::BadOrigin};
use frame_system::RawOrigin;
use pallet_avn_proxy::Error as avn_proxy_error;
use sp_runtime::traits::Zero;
use std::cell::RefCell;

thread_local! {
    pub static AMOUNT_TO_UNBOND: RefCell<u128> = RefCell::new(0u128);
}

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id()
}

mod proxy_signed_schedule_revoke_nomination {
    use super::*;

    fn create_call_for_signed_schedule_revoke_nomination(
        staker: &Staker,
        sender_nonce: u64,
        collator: &AccountId,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof =
            create_proof_for_signed_schedule_revoke_nomination(sender_nonce, staker, &collator);

        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_schedule_revoke_nomination {
                proof,
                collator: collator.clone(),
            },
        ))
    }

    fn create_proof_for_signed_schedule_revoke_nomination(
        sender_nonce: u64,
        staker: &Staker,
        collator: &AccountId,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_schedule_revoke_nomination_params::<Test>(
            staker.relayer.clone(),
            collator,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature)
    }

    #[test]
    fn succeeds_with_good_values() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let staker: Staker = Default::default();
        let initial_stake = 100;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1, 10000),
                (collator_2, 10000),
                (staker.account_id, 10000),
                (staker.relayer, 10000),
            ])
            .with_candidates(vec![(collator_1, initial_stake), (collator_2, initial_stake)])
            .with_nominations(vec![
                (staker.account_id, collator_1, 10),
                (staker.account_id, collator_2, 10),
            ])
            .build()
            .execute_with(|| {
                let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                let revoke_nomination_call =
                    create_call_for_signed_schedule_revoke_nomination(&staker, nonce, &collator_1);

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(staker.relayer),
                    revoke_nomination_call,
                    None
                ));

                assert_event_emitted!(Event::NominationRevocationScheduled {
                    era: 1,
                    nominator: staker.account_id,
                    candidate: collator_1,
                    scheduled_exit: ParachainStaking::delay() + 1,
                });

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
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof = create_proof_for_signed_schedule_revoke_nomination(
                        nonce,
                        &staker,
                        &collator_1,
                    );

                    assert_noop!(
                        ParachainStaking::signed_schedule_revoke_nomination(
                            RawOrigin::None.into(),
                            proof.clone(),
                            collator_1
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if its signed.
                    assert_ok!(ParachainStaking::signed_schedule_revoke_nomination(
                        RuntimeOrigin::signed(staker.account_id),
                        proof,
                        collator_1
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
                    let bad_nonce = ParachainStaking::proxy_nonce(staker.account_id) + 1;
                    let proof = create_proof_for_signed_schedule_revoke_nomination(
                        bad_nonce,
                        &staker,
                        &collator_1,
                    );

                    assert_noop!(
                        ParachainStaking::signed_schedule_revoke_nomination(
                            RuntimeOrigin::signed(staker.account_id),
                            proof.clone(),
                            collator_1
                        ),
                        Error::<Test>::UnauthorizedSignedRemoveBondTransaction
                    );
                });
        }

        #[test]
        fn proxy_proof_collator_is_not_valid() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let bad_collator = to_acc_id(10000000u64);

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
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof = create_proof_for_signed_schedule_revoke_nomination(
                        nonce,
                        &staker,
                        &bad_collator,
                    );
                    assert_noop!(
                        ParachainStaking::signed_schedule_revoke_nomination(
                            RuntimeOrigin::signed(staker.account_id),
                            proof.clone(),
                            bad_collator
                        ),
                        Error::<Test>::NominationDNE
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
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof = create_proof_for_signed_schedule_revoke_nomination(
                        nonce,
                        &staker,
                        &collator_1,
                    );
                    assert_noop!(
                        ParachainStaking::signed_schedule_revoke_nomination(
                            RuntimeOrigin::signed(staker.account_id),
                            proof.clone(),
                            collator_2
                        ),
                        Error::<Test>::UnauthorizedSignedRemoveBondTransaction
                    );
                });
        }
    }
}

mod proxy_signed_schedule_leave_nominators {
    use super::*;

    pub fn create_call_for_signed_schedule_leave_nominators(
        staker: &Staker,
        sender_nonce: u64,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof = create_proof_for_signed_schedule_leave_nominators(sender_nonce, staker);

        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_schedule_leave_nominators { proof },
        ))
    }

    fn create_proof_for_signed_schedule_leave_nominators(
        sender_nonce: u64,
        staker: &Staker,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_schedule_leave_nominators_params::<Test>(
            staker.relayer.clone(),
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature)
    }

    #[test]
    fn succeeds_with_good_values() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let staker: Staker = Default::default();
        let initial_stake = 100;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1, 10000),
                (collator_2, 10000),
                (staker.account_id, 10000),
                (staker.relayer, 10000),
            ])
            .with_candidates(vec![(collator_1, initial_stake), (collator_2, initial_stake)])
            .with_nominations(vec![
                (staker.account_id, collator_1, 10),
                (staker.account_id, collator_2, 10),
            ])
            .build()
            .execute_with(|| {
                let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                let leave_nominators_call =
                    create_call_for_signed_schedule_leave_nominators(&staker, nonce);

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(staker.relayer),
                    leave_nominators_call,
                    None
                ));

                assert_event_emitted!(Event::NominatorExitScheduled {
                    era: 1,
                    nominator: staker.account_id,
                    scheduled_exit: ParachainStaking::delay() + 1,
                });

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
                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof = create_proof_for_signed_schedule_leave_nominators(nonce, &staker);

                    assert_noop!(
                        ParachainStaking::signed_schedule_leave_nominators(
                            RawOrigin::None.into(),
                            proof.clone(),
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if its signed.
                    assert_ok!(ParachainStaking::signed_schedule_leave_nominators(
                        RuntimeOrigin::signed(staker.account_id),
                        proof,
                    ));
                });
        }

        #[test]
        fn proxy_proof_is_not_valid() {
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
                    let bad_nonce = ParachainStaking::proxy_nonce(staker.account_id) + 1;
                    let leave_nominators_call =
                        create_call_for_signed_schedule_leave_nominators(&staker, bad_nonce);

                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        leave_nominators_call,
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
    }
}

mod proxy_signed_execute_revoke_all_nomination {
    use super::*;

    use crate::schedule_revoke_nomination_tests::proxy_signed_schedule_leave_nominators::create_call_for_signed_schedule_leave_nominators;

    fn schedule_leave(staker: Staker) -> u64 {
        let nonce = ParachainStaking::proxy_nonce(staker.account_id);
        let leave_nominators_call =
            create_call_for_signed_schedule_leave_nominators(&staker, nonce);

        assert_ok!(AvnProxy::proxy(
            RuntimeOrigin::signed(staker.relayer),
            leave_nominators_call,
            None
        ));

        return ParachainStaking::proxy_nonce(staker.account_id)
    }

    fn create_call_for_signed_execute_leave_nominators(
        staker: &Staker,
        sender_nonce: u64,
        nominator: &AccountId,
    ) -> Box<<Test as Config>::RuntimeCall> {
        let proof =
            create_proof_for_signed_execute_leave_nominators(sender_nonce, staker, nominator);

        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_execute_leave_nominators {
                proof,
                nominator: nominator.clone(),
            },
        ))
    }

    fn create_call_for_signed_execute_leave_nominators_from_proof(
        proof: Proof<Signature, AccountId>,
        nominator: &AccountId,
    ) -> Box<<Test as Config>::RuntimeCall> {
        return Box::new(MockCall::ParachainStaking(
            super::super::Call::<Test>::signed_execute_leave_nominators {
                proof,
                nominator: nominator.clone(),
            },
        ))
    }

    fn create_proof_for_signed_execute_leave_nominators(
        sender_nonce: u64,
        staker: &Staker,
        nominator: &AccountId,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = encode_signed_execute_leave_nominators_params::<Test>(
            staker.relayer.clone(),
            nominator,
            sender_nonce,
        );

        let signature = sign(&staker.key_pair, &data_to_sign);
        return build_proof(&staker.account_id, &staker.relayer, signature)
    }

    #[test]
    fn succeeds_with_good_values() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let staker: Staker = Default::default();
        let random_user: Staker = Staker::new(59u64, 88u64);
        let initial_stake = 100;
        let nomination = 10;
        ExtBuilder::default()
            .with_balances(vec![
                (collator_1, 10000),
                (collator_2, 10000),
                (staker.account_id, 10000),
                (staker.relayer, 10000),
            ])
            .with_candidates(vec![(collator_1, initial_stake), (collator_2, initial_stake)])
            .with_nominations(vec![
                (staker.account_id, collator_1, nomination),
                (staker.account_id, collator_2, nomination),
            ])
            .build()
            .execute_with(|| {
                let staker_nonce = schedule_leave(staker.clone());

                // Roll foreward by "Delay" eras to activate leave
                roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                // Anyone can send this request
                let random_user_nonce = ParachainStaking::proxy_nonce(random_user.account_id);
                let execute_leave_nomination_call = create_call_for_signed_execute_leave_nominators(
                    &random_user,
                    random_user_nonce,
                    &staker.account_id,
                );

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(random_user.relayer),
                    execute_leave_nomination_call,
                    None
                ));

                assert_event_emitted!(Event::NominatorLeft {
                    nominator: staker.account_id,
                    unstaked_amount: nomination * 2,
                });

                // Nonce has increased
                assert_eq!(
                    ParachainStaking::proxy_nonce(random_user.account_id),
                    random_user_nonce + 1
                );
                // Staker nonce has not changed
                assert_eq!(ParachainStaking::proxy_nonce(staker.account_id), staker_nonce);
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
                    schedule_leave(staker.clone());

                    // Roll foreward by "Delay" eras to activate leave
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let proof = create_proof_for_signed_execute_leave_nominators(
                        nonce,
                        &staker,
                        &staker.account_id,
                    );

                    assert_noop!(
                        ParachainStaking::signed_execute_leave_nominators(
                            RawOrigin::None.into(),
                            proof.clone(),
                            staker.account_id
                        ),
                        BadOrigin
                    );

                    // Show that we can send a successful transaction if its signed.
                    assert_ok!(ParachainStaking::signed_execute_leave_nominators(
                        RuntimeOrigin::signed(staker.account_id),
                        proof,
                        staker.account_id
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
                    schedule_leave(staker.clone());

                    // Roll foreward by "Delay" eras to activate leave
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let bad_nonce = ParachainStaking::proxy_nonce(staker.account_id) + 1;
                    let execute_leave_nominators_call =
                        create_call_for_signed_execute_leave_nominators(
                            &staker,
                            bad_nonce,
                            &staker.account_id,
                        );

                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        execute_leave_nominators_call,
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
        fn proxy_proof_nominator_is_not_valid() {
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
                    schedule_leave(staker.clone());

                    // Roll foreward by "Delay" eras to activate leave
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);
                    let bad_nominator = to_acc_id(2000u64);

                    let proof = create_proof_for_signed_execute_leave_nominators(
                        nonce,
                        &staker,
                        &bad_nominator,
                    );

                    let execute_leave_nominators_call =
                        create_call_for_signed_execute_leave_nominators_from_proof(
                            proof,
                            &bad_nominator,
                        );

                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        execute_leave_nominators_call,
                        None
                    ));
                    assert_eq!(
                        true,
                        inner_call_failed_event_emitted(Error::<Test>::NominatorDNE.into())
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
                    schedule_leave(staker.clone());

                    // Roll foreward by "Delay" eras to activate leave
                    roll_to_era_begin((ParachainStaking::delay() + 1u32) as u64);

                    let nonce = ParachainStaking::proxy_nonce(staker.account_id);

                    let proof = create_proof_for_signed_execute_leave_nominators(
                        nonce,
                        &staker,
                        &staker.account_id,
                    );

                    let execute_leave_nominators_call =
                        create_call_for_signed_execute_leave_nominators_from_proof(
                            proof,
                            &collator_1,
                        );

                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(staker.relayer),
                        execute_leave_nominators_call,
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
    }
}

// SCHEDULE REVOKE NOMINATION

#[test]
fn revoke_nomination_event_emits_correctly() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (account_id_3, 30)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationRevocationScheduled {
                era: 1,
                nominator: account_id_2,
                candidate: account_id,
                scheduled_exit: 3,
            }));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_event_emitted!(Event::NominatorLeftCandidate {
                nominator: account_id_2,
                candidate: account_id,
                unstaked_amount: 10,
                total_candidate_staked: 30
            });
        });
}

#[test]
fn can_revoke_nomination_if_revoking_another_nomination() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (account_id_3, 20)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            // this is an exit implicitly because last nomination revoked
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id_3
            ));
        });
}

#[test]
fn nominator_not_allowed_revoke_if_already_leaving() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (account_id_3, 20)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_nominators(RuntimeOrigin::signed(
                account_id_2
            )));
            assert_noop!(
                ParachainStaking::schedule_revoke_nomination(
                    RuntimeOrigin::signed(account_id_2),
                    account_id_3
                ),
                <Error<Test>>::PendingNominationRequestAlreadyExists,
            );
        });
}

#[test]
fn cannot_revoke_nomination_if_not_nominator() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default().build().execute_with(|| {
        assert_noop!(
            ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ),
            Error::<Test>::NominatorDNE
        );
    });
}

#[test]
fn cannot_revoke_nomination_that_dne() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_noop!(
                ParachainStaking::schedule_revoke_nomination(
                    RuntimeOrigin::signed(account_id_2),
                    to_acc_id(3)
                ),
                Error::<Test>::NominationDNE
            );
        });
}

#[test]
// See `cannot_execute_revoke_nomination_below_min_nominator_stake` for where the "must be above
// MinTotalNominatorStake" rule is now enforced.
fn can_schedule_revoke_nomination_below_min_nominator_stake() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20), (account_id_2, 8), (to_acc_id(3), 20)])
        .with_candidates(vec![(account_id, 20), (to_acc_id(3), 20)])
        .with_nominations(vec![(account_id_2, account_id, 5), (account_id_2, to_acc_id(3), 3)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
        });
}

// EXECUTE REVOKE NOMINATION REQUEST

#[test]
fn execute_revoke_nomination_emits_exit_event_if_exit_happens() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    // last nomination is revocation
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_event_emitted!(Event::NominatorLeftCandidate {
                nominator: account_id_2,
                candidate: account_id,
                unstaked_amount: 10,
                total_candidate_staked: 30
            });
            assert_event_emitted!(Event::NominatorLeft {
                nominator: account_id_2,
                unstaked_amount: 10
            });
        });
}

#[test]
fn cannot_execute_revoke_nomination_below_min_nominator_stake() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 20), (account_id_2, 8), (to_acc_id(3), 20)])
        .with_candidates(vec![(account_id, 20), (to_acc_id(3), 20)])
        .with_nominations(vec![(account_id_2, account_id, 5), (account_id_2, to_acc_id(3), 3)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_noop!(
                ParachainStaking::execute_nomination_request(
                    RuntimeOrigin::signed(account_id_2),
                    account_id_2,
                    account_id
                ),
                Error::<Test>::NominatorBondBelowMin
            );
            // but nominator can cancel the request and request to leave instead:
            assert_ok!(ParachainStaking::cancel_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            assert_ok!(ParachainStaking::schedule_leave_nominators(RuntimeOrigin::signed(
                account_id_2
            )));
            roll_to(20);
            assert_ok!(ParachainStaking::execute_leave_nominators(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                2
            ));
        });
}

#[test]
fn revoke_nomination_executes_exit_if_last_nomination() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    // last nomination is revocation
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_event_emitted!(Event::NominatorLeftCandidate {
                nominator: account_id_2,
                candidate: account_id,
                unstaked_amount: 10,
                total_candidate_staked: 30
            });
            assert_event_emitted!(Event::NominatorLeft {
                nominator: account_id_2,
                unstaked_amount: 10
            });
        });
}

#[test]
fn execute_revoke_nomination_emits_correct_event() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (to_acc_id(3), 30)])
        .with_candidates(vec![(account_id, 30), (to_acc_id(3), 30)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_event_emitted!(Event::NominatorLeftCandidate {
                nominator: account_id_2,
                candidate: account_id,
                unstaked_amount: 10,
                total_candidate_staked: 30
            });
        });
}

#[test]
fn execute_revoke_nomination_unreserves_balance() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 0);
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 10);
        });
}

#[test]
fn execute_revoke_nomination_adds_revocation_to_nominator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (to_acc_id(3), 20)])
        .with_candidates(vec![(account_id, 30), (to_acc_id(3), 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            assert!(!ParachainStaking::nomination_scheduled_requests(&account_id)
                .iter()
                .any(|x| x.nominator == account_id_2));
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            assert!(ParachainStaking::nomination_scheduled_requests(&account_id)
                .iter()
                .any(|x| x.nominator == account_id_2));
        });
}

#[test]
fn execute_revoke_nomination_removes_revocation_from_nominator_state_upon_execution() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (to_acc_id(3), 20)])
        .with_candidates(vec![(account_id, 30), (to_acc_id(3), 20)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, to_acc_id(3), 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert!(!ParachainStaking::nomination_scheduled_requests(&account_id)
                .iter()
                .any(|x| x.nominator == account_id_2));
        });
}

#[test]
fn execute_revoke_nomination_removes_revocation_from_state_for_single_nomination_leave() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 20), (to_acc_id(3), 20)])
        .with_candidates(vec![(account_id, 30), (to_acc_id(3), 20)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert!(
                !ParachainStaking::nomination_scheduled_requests(&account_id)
                    .iter()
                    .any(|x| x.nominator == account_id_2),
                "nomination was not removed"
            );
        });
}

#[test]
fn execute_revoke_nomination_decreases_total_staked() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(ParachainStaking::total(), 40);
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_eq!(ParachainStaking::total(), 30);
        });
}

#[test]
fn execute_revoke_nomination_for_last_nomination_removes_nominator_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert!(ParachainStaking::nominator_state(account_id_2).is_some());
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            // this will be confusing for people
            // if status is leaving, then execute_nomination_request works if last nomination
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert!(ParachainStaking::nominator_state(account_id_2).is_none());
        });
}

#[test]
fn execute_revoke_nomination_removes_nomination_from_candidate_state() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_eq!(
                ParachainStaking::candidate_info(account_id).expect("exists").nomination_count,
                1u32
            );
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert!(ParachainStaking::candidate_info(account_id)
                .expect("exists")
                .nomination_count
                .is_zero());
        });
}

#[test]
fn can_execute_revoke_nomination_for_leaving_candidate() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                RuntimeOrigin::signed(account_id),
                1
            ));
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            // can execute nomination request for leaving candidate
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
        });
}

#[test]
fn can_execute_leave_candidates_if_revoking_candidate() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 10)])
        .with_candidates(vec![(account_id, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_leave_candidates(
                RuntimeOrigin::signed(account_id),
                1
            ));
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            roll_to(10);
            // revocation executes during execute leave candidates (callable by anyone)
            assert_ok!(ParachainStaking::execute_leave_candidates(
                RuntimeOrigin::signed(account_id),
                account_id,
                1
            ));
            assert!(!ParachainStaking::is_nominator(&account_id_2));
            assert_eq!(Balances::reserved_balance(&account_id_2), 0);
            assert_eq!(Balances::free_balance(&account_id_2), 10);
        });
}

#[test]
fn nominator_bond_extra_after_revoke_nomination_does_not_effect_exit() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 30), (account_id_3, 30)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            assert_ok!(ParachainStaking::bond_extra(
                RuntimeOrigin::signed(account_id_2),
                account_id_3,
                10
            ));
            roll_to(100);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert!(ParachainStaking::is_nominator(&account_id_2));
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 10);
        });
}

#[test]
fn nominator_unbond_after_revoke_nomination_does_not_effect_exit() {
    let account_id = to_acc_id(1u64);
    let account_id_2 = to_acc_id(2u64);
    let account_id_3 = to_acc_id(3u64);
    ExtBuilder::default()
        .with_balances(vec![(account_id, 30), (account_id_2, 30), (account_id_3, 30)])
        .with_candidates(vec![(account_id, 30), (account_id_3, 30)])
        .with_nominations(vec![(account_id_2, account_id, 10), (account_id_2, account_id_3, 10)])
        .build()
        .execute_with(|| {
            assert_ok!(ParachainStaking::schedule_revoke_nomination(
                RuntimeOrigin::signed(account_id_2),
                account_id
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationRevocationScheduled {
                era: 1,
                nominator: account_id_2,
                candidate: account_id,
                scheduled_exit: 3,
            }));
            assert_noop!(
                ParachainStaking::schedule_nominator_unbond(
                    RuntimeOrigin::signed(account_id_2),
                    account_id,
                    2
                ),
                Error::<Test>::PendingNominationRequestAlreadyExists
            );
            assert_ok!(ParachainStaking::schedule_nominator_unbond(
                RuntimeOrigin::signed(account_id_2),
                account_id_3,
                2
            ));
            roll_to(10);
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id
            ));
            assert_ok!(ParachainStaking::execute_nomination_request(
                RuntimeOrigin::signed(account_id_2),
                account_id_2,
                account_id_3
            ));
            assert_last_event!(MetaEvent::ParachainStaking(Event::NominationDecreased {
                nominator: account_id_2,
                candidate: account_id_3,
                amount: 2,
                in_top: true
            }));
            assert!(ParachainStaking::is_nominator(&account_id_2));
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id_2), 22);
        });
}
