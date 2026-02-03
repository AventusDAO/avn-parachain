// Copyright 2022 Aventus Network Services (UK) Ltd.

#![cfg(test)]

use crate::{mock::*, system};
use codec::alloc::sync::Arc;
use frame_support::{assert_noop, assert_ok};
use parking_lot::RwLock;
use sp_core::offchain::testing::PoolState;
use sp_runtime::{
    generic::Preamble,
    testing::{TestSignature, UintAuthorityId},
    traits::BadOrigin,
};
use system::RawOrigin;

type MockValidator = Validator<UintAuthorityId, u64>;
mod advance_slot {
    use super::*;

    pub struct LocalContext {
        pub current_block: BlockNumber,
        pub block_number_for_next_slot: BlockNumber,
        pub slot_validator: MockValidator,
        pub other_validator: MockValidator,
        pub slot_number: BlockNumber,
        pub grace_period: BlockNumber,
        pub summary_last_block_in_range: BlockNumber,
    }

    pub fn setup_success_preconditions() -> LocalContext {
        let schedule_period = 2;
        let voting_period = 2;
        let min_block_age = <TestRuntime as Config>::MinBlockAge::get();
        let grace_period = <TestRuntime as Config>::AdvanceSlotGracePeriod::get();
        let arbitrary_margin = 3;
        let next_block_to_process = 2;
        let summary_last_block_in_range = next_block_to_process + schedule_period - 1;

        let current_block = summary_last_block_in_range + min_block_age + arbitrary_margin;
        let slot_number = 6;
        let block_number_for_next_slot = current_block;

        // index - Validators:
        // 0 - FIRST_VALIDATOR_INDEX
        // 1 - SECOND_VALIDATOR_INDEX
        // 2 - THIRD_VALIDATOR_INDEX
        // 3 - FOURTH_VALIDATOR_INDEX
        let slot_validator = get_validator(SIXTH_VALIDATOR_INDEX);
        let other_validator = get_validator(FIRST_VALIDATOR_INDEX);

        assert!(slot_validator != other_validator);

        System::set_block_number(current_block);
        Summary::set_schedule_and_voting_periods(schedule_period, voting_period);
        Summary::set_next_block_to_process(next_block_to_process);
        Summary::set_next_slot_block_number(block_number_for_next_slot);
        Summary::set_current_slot(slot_number);
        Summary::set_current_slot_validator(slot_validator.account_id.clone());

        return LocalContext {
            current_block,
            slot_number,
            slot_validator,
            other_validator,
            block_number_for_next_slot,
            grace_period,
            summary_last_block_in_range,
        }
    }

    pub fn create_signature(slot_number: BlockNumber, validator: &MockValidator) -> TestSignature {
        let signature = validator
            .key
            .sign(&(Summary::advance_block_context(), slot_number).encode())
            .expect("Signature is signed");
        return signature
    }

    pub fn call_advance_slot(
        validator: &MockValidator,
        signature: TestSignature,
    ) -> DispatchResult {
        return Summary::advance_slot(RawOrigin::None.into(), validator.clone(), signature)
    }

    pub mod _if_required {
        use super::*;

        pub fn call_advance_slot_if_required(block_number: BlockNumber, validator: &MockValidator) {
            Summary::advance_slot_if_required(block_number, &validator);
        }

        fn expected_transaction_was_called(
            validator: &MockValidator,
            pool_state: &Arc<RwLock<PoolState>>,
        ) -> bool {
            if pool_state.read().transactions.is_empty() {
                return false
            }

            let call = take_transaction_from_pool(pool_state);
            return call == expected_advance_slot_transaction(validator)
        }

        fn take_transaction_from_pool(
            pool_state: &Arc<RwLock<PoolState>>,
        ) -> crate::Call<TestRuntime> {
            let tx = pool_state.write().transactions.pop().unwrap();
            let tx = Extrinsic::decode(&mut &*tx).unwrap();
            assert!(matches!(tx.preamble, Preamble::Bare(_)));
            match tx.function {
                mock::RuntimeCall::Summary(inner_tx) => inner_tx,
                _ => unreachable!(),
            }
        }

        fn expected_advance_slot_transaction(
            validator: &MockValidator,
        ) -> crate::Call<TestRuntime> {
            let signature = validator
                .key
                .sign(&(Summary::advance_block_context(), Summary::current_slot()).encode())
                .expect("Signature is signed");

            return crate::Call::advance_slot { validator: validator.clone(), signature }
        }

        mod does_not_call_advance_slot_if {
            use super::*;

            #[test]
            fn called_by_non_slot_validator() {
                let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_success_preconditions();

                    assert!(pool_state.read().transactions.is_empty());

                    let other_validator = context.other_validator;

                    call_advance_slot_if_required(
                        context.block_number_for_next_slot,
                        &other_validator,
                    );
                    assert!(!expected_transaction_was_called(&other_validator, &pool_state));
                });
            }

            #[test]
            fn called_too_early() {
                let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_success_preconditions();

                    assert!(pool_state.read().transactions.is_empty());

                    let early_block = context.block_number_for_next_slot - 1;
                    let validator = context.slot_validator;

                    call_advance_slot_if_required(early_block, &validator);
                    assert!(!expected_transaction_was_called(&validator, &pool_state));
                });
            }
        }

        mod calls_advance_slot_if_called {
            use super::*;

            #[test]
            fn by_slot_validator_in_grace_period() {
                let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_success_preconditions();

                    assert!(pool_state.read().transactions.is_empty());

                    let validator = context.slot_validator;

                    call_advance_slot_if_required(context.block_number_for_next_slot, &validator);
                    assert!(expected_transaction_was_called(&validator, &pool_state));
                });
            }
        }
    }

    mod can_successfully_be_called {
        use super::*;

        #[test]
        fn by_slot_validator_in_grace_period() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                assert_ok!(call_advance_slot(&validator, signature));
            });
        }

        #[test]
        fn by_other_validator_after_grace_period() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let after_grace_period =
                    context.block_number_for_next_slot + context.grace_period + 1;
                let validator = context.other_validator;

                System::set_block_number(after_grace_period);
                let signature = create_signature(Summary::current_slot(), &validator);

                assert_ok!(call_advance_slot(&validator, signature));
            });
        }
    }

    mod post_conditions {
        use super::*;

        #[test]
        fn increments_slot_number() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                let old_slot_number = Summary::current_slot();
                assert_ok!(call_advance_slot(&validator, signature));
                let new_slot_number = Summary::current_slot();

                assert_eq!(new_slot_number, old_slot_number + 1);
            });
        }

        #[test]
        fn computes_new_validator() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                let old_validator = Summary::slot_validator().unwrap();
                assert_ok!(call_advance_slot(&validator, signature));
                let new_validator = Summary::slot_validator().unwrap();

                assert!(old_validator != new_validator);
                assert_eq!(new_validator, get_validator(FIRST_VALIDATOR_INDEX).account_id);
            });
        }

        #[test]
        fn computes_start_of_next_slot() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);
                let schedule_period = Summary::schedule_period();

                let old_slot_start = Summary::block_number_for_next_slot();
                assert_ok!(call_advance_slot(&validator, signature));
                let new_slot_start = Summary::block_number_for_next_slot();

                assert_eq!(new_slot_start, old_slot_start + schedule_period);
            });
        }

        #[test]
        fn emits_events() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                assert_ok!(call_advance_slot(&validator, signature));

                let new_slot_number = Summary::current_slot();
                let new_validator = Summary::slot_validator().unwrap();
                let new_slot_start = Summary::block_number_for_next_slot();

                let event =
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::SlotAdvanced {
                        advanced_by: validator.account_id,
                        new_slot: new_slot_number,
                        slot_validator: new_validator,
                        slot_end: new_slot_start,
                    });

                assert!(Summary::emitted_event(&event));
            });
        }

        #[test]
        fn does_not_report_a_slot_not_advanced_offence() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                assert_ok!(call_advance_slot(&validator, signature));

                assert_eq!(
                    false,
                    Summary::reported_offence_of_type(SummaryOffenceType::SlotNotAdvanced)
                );
                assert_eq!(
                    false,
                    Summary::emitted_event_for_offence_of_type(SummaryOffenceType::SlotNotAdvanced)
                );
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_signed() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                assert_noop!(
                    Summary::advance_slot(
                        RuntimeOrigin::signed(Default::default()),
                        validator.clone(),
                        signature
                    ),
                    BadOrigin
                );
            });
        }

        #[test]
        fn called_by_slot_validator_after_grace_period() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                let after_grace_period =
                    context.block_number_for_next_slot + context.grace_period + 1;
                System::set_block_number(after_grace_period);

                assert_noop!(
                    call_advance_slot(&validator, signature),
                    Error::<TestRuntime>::GracePeriodElapsed
                );
            });
        }

        #[test]
        fn called_by_other_validator_inside_grace_period() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.other_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                assert_noop!(
                    call_advance_slot(&validator, signature),
                    Error::<TestRuntime>::WrongValidator
                );
            });
        }

        #[test]
        fn called_before_end_of_slot() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator.clone();
                let signature = create_signature(Summary::current_slot(), &validator);

                let before_end_of_slot = context.block_number_for_next_slot - 1;
                System::set_block_number(before_end_of_slot);

                assert_noop!(
                    call_advance_slot(&validator, signature),
                    Error::<TestRuntime>::TooEarlyToAdvance
                );
            });
        }
    }

    mod auxiliary_logic {
        use super::*;

        #[test]
        fn successive_slots_have_consecutive_validators() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);
                assert_ok!(call_advance_slot(&validator, signature));
                let new_validator = Summary::slot_validator().unwrap();
                assert_eq!(new_validator, get_validator(FIRST_VALIDATOR_INDEX).account_id);

                let validator = get_validator(Summary::slot_validator().unwrap());
                let signature = create_signature(Summary::current_slot(), &validator);
                System::set_block_number(Summary::block_number_for_next_slot());
                assert_ok!(call_advance_slot(&validator, signature));
                let new_validator = Summary::slot_validator().unwrap();
                assert_eq!(new_validator, get_validator(SECOND_VALIDATOR_INDEX).account_id);

                let validator = get_validator(Summary::slot_validator().unwrap());
                let signature = create_signature(Summary::current_slot(), &validator);
                System::set_block_number(Summary::block_number_for_next_slot());
                assert_ok!(call_advance_slot(&validator, signature));
                let new_validator = Summary::slot_validator().unwrap();
                assert_eq!(new_validator, get_validator(THIRD_VALIDATOR_INDEX).account_id);
            });
        }
    }
}

mod signature_in {
    use super::*;
    use frame_support::unsigned::ValidateUnsigned;

    mod advance_slot {
        use super::*;
        use crate::tests_slots::advance_slot;

        #[test]
        fn is_accepted_by_validate_unsigned() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = advance_slot::setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;

                advance_slot::_if_required::call_advance_slot_if_required(
                    context.block_number_for_next_slot,
                    &validator,
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
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = advance_slot::setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let validator = context.slot_validator;

                advance_slot::_if_required::call_advance_slot_if_required(
                    context.block_number_for_next_slot,
                    &validator,
                );

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    mock::RuntimeCall::Summary(crate::Call::advance_slot {
                        validator,
                        signature,
                    }) => {
                        let data = &(Summary::advance_block_context(), context.slot_number);

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
            fn submitted_by_non_validator() {
                let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();

                    assert!(pool_state.read().transactions.is_empty());

                    let validator = context.slot_validator.clone();

                    advance_slot::_if_required::call_advance_slot_if_required(
                        context.block_number_for_next_slot,
                        &validator,
                    );

                    let tx = pool_state.write().transactions.pop().unwrap();
                    let tx = Extrinsic::decode(&mut &*tx).unwrap();

                    match tx.function {
                        mock::RuntimeCall::Summary(crate::Call::advance_slot {
                            validator: _,
                            signature,
                        }) => {
                            let data = &(Summary::advance_block_context(), context.slot_number);

                            let signature_is_valid = data.using_encoded(|encoded_data| {
                                context.other_validator.key.verify(&encoded_data, &signature)
                            });

                            assert!(!signature_is_valid);
                        },
                        _ => assert!(false),
                    };
                });
            }

            #[test]
            fn has_wrong_context() {
                let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();

                    assert!(pool_state.read().transactions.is_empty());

                    let validator = context.slot_validator;

                    advance_slot::_if_required::call_advance_slot_if_required(
                        context.block_number_for_next_slot,
                        &validator,
                    );

                    let tx = pool_state.write().transactions.pop().unwrap();
                    let tx = Extrinsic::decode(&mut &*tx).unwrap();

                    match tx.function {
                        mock::RuntimeCall::Summary(crate::Call::advance_slot {
                            validator,
                            signature,
                        }) => {
                            let data = &("WRONG CONTEXT", context.slot_number);

                            let signature_is_valid = data.using_encoded(|encoded_data| {
                                validator.key.verify(&encoded_data, &signature)
                            });

                            assert!(!signature_is_valid);
                        },
                        _ => assert!(false),
                    };
                });
            }

            #[test]
            fn has_wrong_slot_number() {
                let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();

                    assert!(pool_state.read().transactions.is_empty());

                    let validator = context.slot_validator;

                    advance_slot::_if_required::call_advance_slot_if_required(
                        context.block_number_for_next_slot,
                        &validator,
                    );

                    let tx = pool_state.write().transactions.pop().unwrap();
                    let tx = Extrinsic::decode(&mut &*tx).unwrap();

                    match tx.function {
                        mock::RuntimeCall::Summary(crate::Call::advance_slot {
                            validator,
                            signature,
                        }) => {
                            let data = &(Summary::advance_block_context(), context.slot_number + 1);

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

mod cases_for_no_summary_created_offences {
    use super::*;
    use advance_slot::{call_advance_slot, create_signature, LocalContext};
    use sp_core::H256;

    pub struct RootContext {
        pub root_id: RootId<BlockNumber>,
        pub root_hash: H256,
        pub tx_id: u64,
    }

    mod when_slot_is_advanced_and {
        use super::*;

        fn setup_for_multi_summaries() -> LocalContext {
            let mut context = advance_slot::setup_success_preconditions();
            context.current_block = 12;
            context.block_number_for_next_slot = context.current_block + Summary::schedule_period();

            System::set_block_number(context.current_block);
            Summary::set_next_slot_block_number(context.block_number_for_next_slot);

            return context
        }

        pub fn setup_approved_root(context: &LocalContext, root_context: RootContext) {
            let root_id = root_context.root_id;
            let root_hash = root_context.root_hash;
            let validator = &context.slot_validator;

            // Setup voting data
            Summary::insert_root_hash(&root_id, root_hash, validator.account_id, 0);
            Summary::insert_pending_approval(&root_id);
            Summary::register_root_for_voting(&root_id, QUORUM, VOTING_PERIOD_END);
            assert_eq!(Summary::get_vote(&root_id).ayes.is_empty(), true);
            assert_eq!(Summary::get_vote(&root_id).nays.is_empty(), true);

            let third_validator = get_validator(THIRD_VALIDATOR_INDEX);
            assert_eq!(
                false,
                vec![validator.clone(), context.other_validator.clone()].contains(&third_validator)
            );

            // Approve root
            Summary::record_approve_vote(&root_id, validator.account_id);
            Summary::record_approve_vote(&root_id, context.other_validator.account_id);
            Summary::record_approve_vote(&root_id, third_validator.account_id);

            // End vote and update `SlotOfLastPublishedSummary`
            let record_summary_signature = get_signature_for_record_summary_calculation(
                validator.clone(),
                &Summary::update_block_number_context(),
                root_hash,
                root_context.root_id.ingress_counter,
                context.summary_last_block_in_range,
            );
            assert_ok!(Summary::end_voting_period(
                RawOrigin::None.into(),
                root_id,
                validator.clone(),
                record_summary_signature
            ));
        }

        mod summaries_were_not_created_for_several_slots {
            use super::*;

            #[test]
            fn several_events_are_emitted() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();

                    let last_summary_slot = 0;
                    Summary::set_previous_summary_slot(last_summary_slot);
                    assert_eq!(true, (context.slot_number - last_summary_slot) > 2);

                    let validator = context.slot_validator;
                    let signature = create_signature(Summary::current_slot(), &validator);

                    assert_ok!(call_advance_slot(&validator, signature));

                    let offence_event = mock::RuntimeEvent::Summary(
                        crate::Event::<TestRuntime>::SummaryNotPublishedOffence {
                            challengee: validator.account_id,
                            void_slot: context.slot_number,
                            last_published: last_summary_slot,
                            end_vote: context.block_number_for_next_slot,
                        },
                    );
                    assert_eq!(true, Summary::emitted_event(&offence_event));

                    let offenders =
                        vec![Summary::create_mock_identification_tuple(validator.account_id)];

                    let offence_reported_event = mock::RuntimeEvent::Summary(
                        crate::Event::<TestRuntime>::SummaryOffenceReported {
                            offence_type: SummaryOffenceType::NoSummaryCreated,
                            offenders,
                        },
                    );
                    assert_eq!(true, Summary::emitted_event(&offence_reported_event));

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
                    assert_eq!(true, Summary::emitted_event(&slot_advanced_event));

                    // Show that we have accounted for all events, and thus not created an offence
                    // for each missing summary
                    assert_eq!(System::events().len(), 3);
                });
            }

            #[test]
            fn an_offence_is_reported() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();

                    let last_summary_slot = 0;
                    Summary::set_previous_summary_slot(last_summary_slot);
                    assert_eq!(true, (context.slot_number - last_summary_slot) > 2);

                    let validator = context.slot_validator;
                    let signature = create_signature(Summary::current_slot(), &validator);

                    assert_ok!(call_advance_slot(&validator, signature));

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            validator.account_id,
                            VALIDATOR_COUNT,
                            vec![validator.account_id],
                            SummaryOffenceType::NoSummaryCreated
                        )
                    );
                });
            }

            #[test]
            fn an_offence_is_reported_by_someone_else_after_grace_period() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let arbitrary_margin = 3;
                    let context = advance_slot::setup_success_preconditions();

                    let last_summary_slot = 0;
                    Summary::set_previous_summary_slot(last_summary_slot);
                    // ensure that the last time we registered a root was several slots ago
                    assert_eq!(true, (context.slot_number - last_summary_slot) > 2);

                    let validator = context.slot_validator;
                    let submitter = context.other_validator;
                    let signature = create_signature(Summary::current_slot(), &submitter);

                    System::set_block_number(
                        context.current_block + context.grace_period + arbitrary_margin,
                    );
                    assert_ok!(call_advance_slot(&submitter, signature));

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            submitter.account_id,
                            VALIDATOR_COUNT,
                            vec![validator.account_id],
                            SummaryOffenceType::NoSummaryCreated
                        )
                    );
                });
            }
        }

        #[test]
        fn an_offence_is_reported_by_someone_else_after_grace_period() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();

            ext.execute_with(|| {
                let context = advance_slot::setup_success_preconditions();

                let last_summary_slot = 0;
                Summary::set_previous_summary_slot(last_summary_slot);
                assert_eq!(true, (context.slot_number - last_summary_slot) > 2);

                let validator = context.slot_validator;
                let signature = create_signature(Summary::current_slot(), &validator);

                System::set_block_number(context.current_block + context.grace_period + 2);
                assert_ok!(call_advance_slot(&context.other_validator, signature));

                assert_eq!(
                    true,
                    Summary::reported_offence(
                        context.other_validator.account_id,
                        VALIDATOR_COUNT,
                        vec![validator.account_id],
                        SummaryOffenceType::NoSummaryCreated
                    )
                );
            });
        }

        mod no_summary_was_created_in_slot {
            use super::*;

            #[test]
            fn an_event_is_emitted() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();
                    let previous_summary_slot = context.slot_number - 1;
                    Summary::set_previous_summary_slot(previous_summary_slot);

                    let validator = context.slot_validator;
                    let signature = create_signature(Summary::current_slot(), &validator);

                    assert_ok!(call_advance_slot(&validator, signature));

                    let event = mock::RuntimeEvent::Summary(
                        crate::Event::<TestRuntime>::SummaryNotPublishedOffence {
                            challengee: validator.account_id,
                            void_slot: context.slot_number,
                            last_published: previous_summary_slot,
                            end_vote: context.block_number_for_next_slot,
                        },
                    );

                    assert_eq!(true, Summary::emitted_event(&event));
                });
            }

            #[test]
            fn an_offence_is_reported() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();
                    let previous_summary_slot = context.slot_number - 1;
                    Summary::set_previous_summary_slot(previous_summary_slot);

                    let validator = context.slot_validator;
                    let signature = create_signature(Summary::current_slot(), &validator);

                    assert_ok!(call_advance_slot(&validator, signature));

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            validator.account_id,
                            VALIDATOR_COUNT,
                            vec![validator.account_id],
                            SummaryOffenceType::NoSummaryCreated
                        )
                    );
                });
            }

            #[test]
            fn an_offence_is_reported_by_someone_else_after_grace_period() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let arbitrary_margin = 3;
                    let context = advance_slot::setup_success_preconditions();
                    let previous_summary_slot = context.slot_number - 1;
                    Summary::set_previous_summary_slot(previous_summary_slot);

                    let validator = context.slot_validator;
                    let submitter = context.other_validator;
                    let signature = create_signature(Summary::current_slot(), &submitter);

                    System::set_block_number(
                        context.current_block + context.grace_period + arbitrary_margin,
                    );
                    assert_ok!(call_advance_slot(&submitter, signature));

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            submitter.account_id,
                            VALIDATOR_COUNT,
                            vec![validator.account_id],
                            SummaryOffenceType::NoSummaryCreated
                        )
                    );
                });
            }
        }

        mod one_summary_is_created {
            use super::*;

            #[test]
            fn no_event_is_emitted() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();

                    let root_context = RootContext {
                        root_id: RootId::new(
                            RootRange::new(
                                Summary::get_next_block_to_process(),
                                context.summary_last_block_in_range,
                            ),
                            DEFAULT_INGRESS_COUNTER,
                        ),
                        root_hash: H256::from(ROOT_HASH_BYTES),
                        tx_id: 1,
                    };

                    setup_approved_root(&context, root_context);

                    let old_slot_number = context.slot_number;
                    let signature =
                        create_signature(Summary::current_slot(), &context.slot_validator);
                    assert_ok!(call_advance_slot(&context.slot_validator, signature));

                    let new_slot_number = Summary::current_slot();
                    assert_eq!(new_slot_number, old_slot_number + 1);

                    // show that no events (with any parameters) related to an offence has been
                    // emitted
                    assert_eq!(false, add_offence_event_emitted());
                });
            }

            #[test]
            fn no_offence_is_reported() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = advance_slot::setup_success_preconditions();

                    let root_context = RootContext {
                        root_id: RootId::new(
                            RootRange::new(
                                Summary::get_next_block_to_process(),
                                context.summary_last_block_in_range,
                            ),
                            DEFAULT_INGRESS_COUNTER,
                        ),
                        root_hash: H256::from(ROOT_HASH_BYTES),
                        tx_id: 1,
                    };

                    setup_approved_root(&context, root_context);

                    let old_slot_number = context.slot_number;
                    let signature =
                        create_signature(Summary::current_slot(), &context.slot_validator);
                    assert_ok!(call_advance_slot(&context.slot_validator, signature));

                    let new_slot_number = Summary::current_slot();
                    assert_eq!(new_slot_number, old_slot_number + 1);

                    // show that no events (with any parameters) related to an offence has been
                    // emitted
                    assert_eq!(false, add_offence_event_emitted());

                    assert_eq!(
                        false,
                        Summary::reported_offence_of_type(SummaryOffenceType::NoSummaryCreated)
                    );
                });
            }
        }
        mod multiple_summaries_are_created {
            use super::*;

            #[test]
            fn no_event_is_emitted() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_for_multi_summaries();

                    let root_context_1 = RootContext {
                        root_id: RootId::new(
                            RootRange::new(
                                Summary::get_next_block_to_process(),
                                context.summary_last_block_in_range,
                            ),
                            DEFAULT_INGRESS_COUNTER,
                        ),
                        root_hash: H256::from(ROOT_HASH_BYTES),
                        tx_id: 1,
                    };

                    setup_approved_root(&context, root_context_1);

                    // make sure we have stored the calculated summary
                    assert_eq!(Summary::last_summary_slot(), context.slot_number);
                    assert_eq!(
                        Summary::get_next_block_to_process(),
                        context.summary_last_block_in_range + 1
                    );

                    // Advance 1 (to block #13) block and create a summary again
                    advance_block_numbers(1);

                    let new_from_block = Summary::get_next_block_to_process();
                    let summary_last_block_in_range =
                        new_from_block + Summary::schedule_period() - 1;
                    let root_context_2 = RootContext {
                        root_id: RootId::new(
                            RootRange::new(
                                Summary::get_next_block_to_process(),
                                summary_last_block_in_range,
                            ),
                            DEFAULT_INGRESS_COUNTER,
                        ),
                        root_hash: H256::from(ROOT_HASH_BYTES),
                        tx_id: 2,
                    };

                    setup_approved_root(&context, root_context_2);
                    // make sure we have stored the second calculated summary
                    assert_eq!(Summary::last_summary_slot(), context.slot_number);
                    assert_eq!(
                        Summary::get_next_block_to_process(),
                        summary_last_block_in_range + 1
                    );

                    // Advance block to the end of the slot (set to block #14) so we can advance the
                    // slot
                    advance_block_numbers(1);

                    let old_slot_number = context.slot_number;
                    let signature =
                        create_signature(Summary::current_slot(), &context.slot_validator);
                    assert_ok!(call_advance_slot(&context.slot_validator, signature));

                    let new_slot_number = Summary::current_slot();
                    assert_eq!(new_slot_number, old_slot_number + 1);

                    // show that no events (with any parameters) related to an offence has been
                    // emitted
                    assert_eq!(false, add_offence_event_emitted());
                });
            }

            #[test]
            fn no_offence_is_reported() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_for_multi_summaries();

                    let root_context_1 = RootContext {
                        root_id: RootId::new(
                            RootRange::new(
                                Summary::get_next_block_to_process(),
                                context.summary_last_block_in_range,
                            ),
                            DEFAULT_INGRESS_COUNTER,
                        ),
                        root_hash: H256::from(ROOT_HASH_BYTES),
                        tx_id: 1,
                    };

                    setup_approved_root(&context, root_context_1);

                    // make sure we have stored the calculated summary
                    assert_eq!(Summary::last_summary_slot(), context.slot_number);
                    assert_eq!(
                        Summary::get_next_block_to_process(),
                        context.summary_last_block_in_range + 1
                    );

                    // Advance 1 (to block #13) block and create a summary again
                    advance_block_numbers(1);

                    let new_from_block = Summary::get_next_block_to_process();
                    let summary_last_block_in_range =
                        new_from_block + Summary::schedule_period() - 1;
                    let root_context_2 = RootContext {
                        root_id: RootId::new(
                            RootRange::new(
                                Summary::get_next_block_to_process(),
                                summary_last_block_in_range,
                            ),
                            DEFAULT_INGRESS_COUNTER,
                        ),
                        root_hash: H256::from(ROOT_HASH_BYTES),
                        tx_id: 2,
                    };

                    setup_approved_root(&context, root_context_2);
                    // make sure we have stored the second calculated summary
                    assert_eq!(Summary::last_summary_slot(), context.slot_number);
                    assert_eq!(
                        Summary::get_next_block_to_process(),
                        summary_last_block_in_range + 1
                    );

                    // Advance block to the end of the slot (set to block #14) so we can advance the
                    // slot
                    advance_block_numbers(1);

                    let signature =
                        create_signature(Summary::current_slot(), &context.slot_validator);
                    assert_ok!(call_advance_slot(&context.slot_validator, signature));

                    assert_eq!(
                        false,
                        Summary::reported_offence_of_type(SummaryOffenceType::NoSummaryCreated)
                    );
                });
            }
        }
    }
}

fn event_is_a_not_published_offence(e: &mock::RuntimeEvent) -> bool {
    if let mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::SummaryNotPublishedOffence {
        ..
    }) = &e
    {
        return true
    } else {
        return false
    }
}

fn add_offence_event_emitted() -> bool {
    return System::events().iter().any(|e| event_is_a_not_published_offence(&e.event))
}
