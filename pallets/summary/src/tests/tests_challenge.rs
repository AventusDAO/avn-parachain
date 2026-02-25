// Copyright 2026 Aventus DAO Ltd
#![cfg(test)]

use crate::{challenge::challenge_slot_if_required, mock::*};
use codec::alloc::sync::Arc;
use frame_support::{assert_noop, assert_ok, unsigned::ValidateUnsigned};
use frame_system::RawOrigin;
use parking_lot::RwLock;
use sp_core::offchain::testing::PoolState;
use sp_runtime::{
    generic::Preamble,
    testing::{TestSignature, UintAuthorityId},
};

type MockValidator = Validator<UintAuthorityId, u64>;

struct LocalContext {
    pub current_block: BlockNumber,
    pub block_number_for_next_slot: BlockNumber,
    pub slot_validator: MockValidator,
    pub other_validator: MockValidator,
    pub fourth_validator: MockValidator,
    pub slot_number: BlockNumber,
    pub block_after_grace_period: BlockNumber,
    pub challenge_reason: SummaryChallengeReason,
}

fn setup_success_preconditions() -> LocalContext {
    let schedule_period = 2;
    let voting_period = 2;
    let min_block_age = <TestRuntime as Config>::MinBlockAge::get();
    let grace_period = <TestRuntime as Config>::AdvanceSlotGracePeriod::get();
    let arbitrary_margin = 3;
    let next_start_block_to_process_summary = 2;
    let target_block = next_start_block_to_process_summary + schedule_period - 1;

    let current_block = target_block + min_block_age + arbitrary_margin;
    let slot_number = 3 as u64;
    let block_number_for_next_slot = current_block;

    let challenge_reason = SummaryChallengeReason::SlotNotAdvanced(slot_number.try_into().unwrap());

    let block_after_grace_period = block_number_for_next_slot + grace_period + 1;

    let slot_validator = get_validator(SIXTH_VALIDATOR_INDEX);
    let other_validator = get_validator(FIRST_VALIDATOR_INDEX);
    let fourth_validator = get_validator(FOURTH_VALIDATOR_INDEX);
    assert!(slot_validator != other_validator);

    System::set_block_number(current_block);
    Summary::set_schedule_and_voting_periods(schedule_period, voting_period);
    Summary::set_next_block_to_process(next_start_block_to_process_summary);
    Summary::set_next_slot_block_number(block_number_for_next_slot);
    Summary::set_current_slot(slot_number);
    Summary::set_current_slot_validator(slot_validator.account_id.clone());

    return LocalContext {
        current_block,
        slot_number,
        slot_validator,
        other_validator,
        fourth_validator,
        block_number_for_next_slot,
        block_after_grace_period,
        challenge_reason,
    }
}

fn call_challenge_slot_if_required(block_number: BlockNumber, validator: &MockValidator) {
    challenge_slot_if_required::<TestRuntime, ()>(block_number, &validator);
}

fn call_add_challenge(
    challenge: SummaryChallenge<AccountId>,
    validator: &MockValidator,
    signature: TestSignature,
) -> DispatchResult {
    return Summary::add_challenge(RawOrigin::None.into(), challenge, validator.clone(), signature)
}

fn expected_add_challenge_transaction(
    challenge: &SummaryChallenge<AccountId>,
    validator: &MockValidator,
) -> crate::Call<TestRuntime> {
    let signature = sign_challenge(challenge, validator);

    return crate::Call::add_challenge {
        challenge: challenge.clone(),
        validator: validator.clone(),
        signature,
    }
}

fn get_valid_challenge(context: &LocalContext) -> SummaryChallenge<AccountId> {
    return get_challenge(
        context.challenge_reason.clone(),
        context.other_validator.account_id,
        context.slot_validator.account_id,
    )
}

fn get_challenge(
    challenge_reason: SummaryChallengeReason,
    challenger: AccountId,
    challengee: AccountId,
) -> SummaryChallenge<AccountId> {
    return SummaryChallenge { challenge_reason, challenger, challengee }
}

fn sign_challenge(
    challenge: &SummaryChallenge<AccountId>,
    validator: &MockValidator,
) -> TestSignature {
    return validator
        .key
        .sign(&(CHALLENGE_CONTEXT, challenge).encode())
        .expect("Signature is signed")
}

fn expected_transaction_was_called(
    challenge: &SummaryChallenge<AccountId>,
    validator: &MockValidator,
    pool_state: &Arc<RwLock<PoolState>>,
) -> bool {
    if pool_state.read().transactions.is_empty() {
        return false
    }

    let call = take_transaction_from_pool(pool_state);
    let _new_call = expected_add_challenge_transaction(challenge, validator);

    return call == expected_add_challenge_transaction(challenge, validator)
}

pub fn take_transaction_from_pool(pool_state: &Arc<RwLock<PoolState>>) -> crate::Call<TestRuntime> {
    let tx = pool_state.write().transactions.pop().unwrap();
    let tx = Extrinsic::decode(&mut &*tx).unwrap();
    assert!(matches!(tx.preamble, Preamble::Bare(_)));
    match tx.function {
        mock::RuntimeCall::Summary(inner_tx) => inner_tx,
        _ => unreachable!(),
    }
}

mod challenge_slot_if_required {
    use super::*;

    mod calls_add_challenge {
        use super::*;

        #[test]
        fn when_grace_period_elapsed_before_slot_was_advanced() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();
                assert!(pool_state.read().transactions.is_empty());

                System::set_block_number(context.block_after_grace_period);
                let challenge = get_challenge(
                    context.challenge_reason.clone(),
                    context.fourth_validator.account_id,
                    context.slot_validator.account_id,
                );

                call_challenge_slot_if_required(
                    context.block_after_grace_period,
                    &context.fourth_validator,
                );
                assert!(expected_transaction_was_called(
                    &challenge,
                    &context.fourth_validator,
                    &pool_state
                ));
            });
        }

        #[test]
        fn when_called_by_the_challengee() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();
                assert!(pool_state.read().transactions.is_empty());

                // We add 2 to make sure context.slot_validator is the primary for this block number
                let block_after_grace_period = context.block_after_grace_period + 2;

                assert!(Avn::is_primary_for_block(
                    block_after_grace_period,
                    &context.slot_validator.account_id
                )
                .unwrap());

                System::set_block_number(block_after_grace_period);
                let challenge = get_challenge(
                    context.challenge_reason,
                    context.slot_validator.account_id,
                    context.slot_validator.account_id,
                );

                call_challenge_slot_if_required(block_after_grace_period, &context.slot_validator);
                assert!(expected_transaction_was_called(
                    &challenge,
                    &context.slot_validator,
                    &pool_state
                ));
            });
        }
    }

    mod post_conditions {
        use super::*;

        #[test]
        fn increments_slot_number() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();
                assert!(pool_state.read().transactions.is_empty());

                System::set_block_number(context.block_after_grace_period);
                let challenge = get_valid_challenge(&context);
                let validator = &context.other_validator;

                let signature = sign_challenge(&challenge, &validator);
                let old_slot_number = Summary::current_slot();

                assert_ok!(call_add_challenge(challenge, &validator, signature));
                let new_slot_number = Summary::current_slot();

                assert_eq!(new_slot_number, old_slot_number + 1);
            });
        }

        #[test]
        fn emits_slot_advanced_event() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();
                assert!(pool_state.read().transactions.is_empty());

                System::set_block_number(context.block_after_grace_period);
                let challenge = get_valid_challenge(&context);
                let validator = &context.other_validator;
                let signature = sign_challenge(&challenge, &validator);

                assert_ok!(call_add_challenge(challenge.clone(), &validator, signature));

                let new_slot_number = Summary::current_slot();
                let new_validator = Summary::slot_validator().unwrap();
                let new_slot_start = Summary::block_number_for_next_slot();

                let slot_advanced_event =
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::SlotAdvanced {
                        advanced_by: validator.account_id,
                        new_slot: new_slot_number,
                        slot_validator: new_validator,
                        slot_end: new_slot_start,
                    });
                assert!(Summary::emitted_event(&slot_advanced_event));
            });
        }

        #[test]
        fn emits_challenge_added_event() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();
                assert!(pool_state.read().transactions.is_empty());

                System::set_block_number(context.block_after_grace_period);
                let challenge = get_valid_challenge(&context);
                let validator = &context.other_validator;
                let signature = sign_challenge(&challenge, &validator);

                assert_ok!(call_add_challenge(challenge.clone(), &validator, signature));

                let add_challenge_event =
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::ChallengeAdded {
                        challenge_reason: challenge.challenge_reason.clone(),
                        challenger: challenge.challenger,
                        challengee: challenge.challengee,
                    });
                assert!(Summary::emitted_event(&add_challenge_event));
            });
        }

        #[test]
        fn reports_slot_not_advanced_offence() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();
                assert!(pool_state.read().transactions.is_empty());

                System::set_block_number(context.block_after_grace_period);
                let challenge = get_valid_challenge(&context);
                let validator = &context.other_validator;
                let signature = sign_challenge(&challenge, &validator);

                assert_ok!(call_add_challenge(challenge.clone(), &validator, signature));

                assert_eq!(
                    true,
                    Summary::reported_offence(
                        challenge.challenger,
                        VALIDATOR_COUNT,
                        vec![challenge.challengee],
                        SummaryOffenceType::SlotNotAdvanced
                    )
                );

                assert_eq!(
                    true,
                    Summary::emitted_event_for_offence_of_type(SummaryOffenceType::SlotNotAdvanced)
                );
            });
        }
    }

    mod does_not_call_add_challenge {
        use super::*;

        fn sign_advance_slot(slot_number: BlockNumber, validator: &MockValidator) -> TestSignature {
            let signature = validator
                .key
                .sign(&(Summary::advance_block_context(), slot_number).encode())
                .expect("Signature is signed");
            return signature
        }

        fn advance_slot(context: &LocalContext) -> DispatchResult {
            let signature =
                sign_advance_slot(Summary::current_slot(), &context.slot_validator.clone());
            return Summary::advance_slot(
                RawOrigin::None.into(),
                context.slot_validator.clone(),
                signature,
            )
        }

        fn get_primary_for_block(block_number: BlockNumber) -> MockValidator {
            let primary_validator_account_id =
                Avn::calculate_primary_validator_for_block(block_number).unwrap();
            return get_validator(primary_validator_account_id)
        }

        #[test]
        fn when_slot_is_advanced_correctly() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert_ok!(advance_slot(&context));
                let block_number = context.current_block + 1;
                System::set_block_number(block_number);
                let validator = get_primary_for_block(block_number);

                assert!(pool_state.read().transactions.is_empty());
                call_challenge_slot_if_required(block_number, &validator);
                assert!(pool_state.read().transactions.is_empty());
            });
        }

        #[test]
        fn before_grace_period() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                let before_grace_period = context.block_number_for_next_slot;
                assert!(!Summary::grace_period_elapsed(before_grace_period));

                System::set_block_number(before_grace_period);
                let validator = get_primary_for_block(before_grace_period);

                assert!(pool_state.read().transactions.is_empty());
                call_challenge_slot_if_required(before_grace_period, &validator);
                assert!(pool_state.read().transactions.is_empty());
            });
        }

        #[test]
        fn within_the_grace_period() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                let block_within_the_grace_period = context.block_number_for_next_slot + 1;
                assert!(!Summary::grace_period_elapsed(block_within_the_grace_period));

                System::set_block_number(block_within_the_grace_period);
                let validator = get_primary_for_block(block_within_the_grace_period);

                assert!(pool_state.read().transactions.is_empty());
                call_challenge_slot_if_required(block_within_the_grace_period, &validator);
                assert!(pool_state.read().transactions.is_empty());
            });
        }

        #[test]
        fn when_slot_number_is_bigger_than_u32() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();
                let slot_number_bigger_than_u32: u64 = u32::max_value() as u64 + 1;

                Summary::set_current_slot(slot_number_bigger_than_u32);
                System::set_block_number(context.block_after_grace_period);

                assert!(pool_state.read().transactions.is_empty());
                call_challenge_slot_if_required(
                    context.block_after_grace_period,
                    &context.other_validator,
                );
                assert!(pool_state.read().transactions.is_empty());
            });
        }
    }

    mod fails_the_challenge {
        use super::*;

        #[test]
        fn when_slot_is_not_current() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                System::set_block_number(context.block_after_grace_period);
                let mut challenge = get_valid_challenge(&context);

                let bad_slot_number: u32 = (context.slot_number - 1).try_into().unwrap();
                challenge.challenge_reason =
                    SummaryChallengeReason::SlotNotAdvanced(bad_slot_number);

                let signature = sign_challenge(&challenge, &context.other_validator);

                assert_noop!(
                    call_add_challenge(challenge, &context.other_validator, signature),
                    Error::<TestRuntime>::InvalidChallenge
                );
            });
        }

        #[test]
        fn when_challengee_is_not_chosen_validator_for_slot() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                System::set_block_number(context.block_after_grace_period);
                let mut challenge = get_valid_challenge(&context);

                let bad_challengee = context.other_validator.clone();
                assert_ne!(challenge.challengee, bad_challengee.account_id);
                challenge.challengee = bad_challengee.account_id;

                let signature = sign_challenge(&challenge, &context.other_validator);

                assert_noop!(
                    call_add_challenge(challenge, &context.other_validator, signature),
                    Error::<TestRuntime>::InvalidChallenge
                );
            });
        }
    }
}

mod signature_in {
    use super::*;

    mod add_challenge {
        use super::*;

        #[test]
        fn is_accepted_by_validate_unsigned() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                System::set_block_number(context.block_after_grace_period);
                assert!(pool_state.read().transactions.is_empty());

                call_challenge_slot_if_required(
                    context.block_after_grace_period,
                    &context.fourth_validator,
                );

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    mock::RuntimeCall::Summary(inner_tx) => {
                        assert_ok!(Summary::validate_unsigned(TransactionSource::Local, &inner_tx));
                    },
                    _ => unreachable!(),
                }
            });
        }

        #[test]
        fn includes_all_relevant_fields() {
            let (mut ext, pool_state, _) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                System::set_block_number(context.block_after_grace_period);
                assert!(pool_state.read().transactions.is_empty());

                call_challenge_slot_if_required(
                    context.block_after_grace_period,
                    &context.fourth_validator,
                );

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    mock::RuntimeCall::Summary(crate::Call::add_challenge {
                        challenge,
                        validator,
                        signature,
                    }) => {
                        let data = &(CHALLENGE_CONTEXT, challenge);

                        let signature_is_valid = data.using_encoded(|encoded_data| {
                            validator.key.verify(&encoded_data, &signature)
                        });

                        assert!(signature_is_valid);
                    },
                    _ => assert!(false),
                };
            });
        }

        mod is_rejected_when {
            use super::*;

            #[test]
            fn challenger_is_not_a_validator() {
                let (mut ext, pool_state, _) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_success_preconditions();

                    System::set_block_number(context.block_after_grace_period);
                    assert!(pool_state.read().transactions.is_empty());

                    call_challenge_slot_if_required(
                        context.block_after_grace_period,
                        &context.fourth_validator,
                    );

                    let tx = pool_state.write().transactions.pop().unwrap();
                    let tx = Extrinsic::decode(&mut &*tx).unwrap();

                    let non_validator = get_non_validator();

                    match tx.function {
                        mock::RuntimeCall::Summary(crate::Call::add_challenge {
                            challenge,
                            validator: _,
                            signature,
                        }) => {
                            let data = &(CHALLENGE_CONTEXT, challenge);

                            let signature_is_valid = data.using_encoded(|encoded_data| {
                                non_validator.key.verify(&encoded_data, &signature)
                            });

                            assert!(!signature_is_valid);
                        },
                        _ => assert!(false),
                    };
                });
            }

            #[test]
            fn has_wrong_context() {
                let (mut ext, pool_state, _) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_success_preconditions();

                    System::set_block_number(context.block_after_grace_period);
                    assert!(pool_state.read().transactions.is_empty());

                    call_challenge_slot_if_required(
                        context.block_after_grace_period,
                        &context.fourth_validator,
                    );

                    let tx = pool_state.write().transactions.pop().unwrap();
                    let tx = Extrinsic::decode(&mut &*tx).unwrap();

                    let bad_context = "bad context";

                    match tx.function {
                        mock::RuntimeCall::Summary(crate::Call::add_challenge {
                            challenge,
                            validator,
                            signature,
                        }) => {
                            let data = &(bad_context, challenge);

                            let signature_is_valid = data.using_encoded(|encoded_data| {
                                validator.key.verify(&encoded_data, &signature)
                            });

                            assert!(!signature_is_valid);
                        },
                        _ => assert!(false),
                    };
                });
            }
        }
    }
}

mod validate_unsigned {
    use super::*;

    mod rejects_extrinsic_when {
        use super::*;

        #[test]
        fn challenge_reason_is_unknown() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                let challenge = get_challenge(
                    SummaryChallengeReason::Unknown,
                    context.other_validator.account_id,
                    context.slot_validator.account_id,
                );

                let signature = sign_challenge(&challenge, &context.other_validator);

                let call = crate::Call::add_challenge {
                    challenge,
                    validator: context.other_validator,
                    signature,
                };

                assert_noop!(
                    Summary::validate_unsigned(TransactionSource::Local, &call),
                    InvalidTransaction::Custom(UNKNOWN_CHALLENGE_REASON)
                );
            });
        }
    }
}
