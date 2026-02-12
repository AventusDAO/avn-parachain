// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{
    mock::{Summary, *},
    system,
};
use codec::alloc::sync::Arc;
use frame_support::assert_noop;
use pallet_avn::vote::VotingSessionData;
use parking_lot::RwLock;

use sp_core::{offchain::testing::PoolState, H256};
use sp_runtime::{
    generic::Preamble, offchain::storage::StorageValueRef, testing::UintAuthorityId,
    traits::BadOrigin,
};
use system::RawOrigin;

fn record_summary_calculation_is_called(
    current_block_number: BlockNumber,
    this_validator: &Validator<UintAuthorityId, AccountId>,
    pool_state: &Arc<RwLock<PoolState>>,
) -> bool {
    Summary::process_summary_if_required(current_block_number, this_validator);

    return !pool_state.read().transactions.is_empty();
}

fn get_unsigned_record_summary_calculation_call_from_chain(
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

fn expected_unsigned_record_summary_calculation_call(
    context: &Context,
) -> crate::Call<TestRuntime> {
    let signature = context
        .validator
        .key
        .sign(
            &(
                &Summary::update_block_number_context(),
                context.root_hash_h256,
                context.root_id.ingress_counter,
                context.last_block_in_range,
            )
                .encode(),
        )
        .expect("Signature is signed");

    return crate::Call::record_summary_calculation {
        new_block_number: context.last_block_in_range,
        root_hash: context.root_hash_h256,
        ingress_counter: context.root_id.ingress_counter,
        validator: context.validator.clone(),
        signature,
    };
}

fn record_summary_calculation_is_ok(context: &Context) -> bool {
    return Summary::record_summary_calculation(
        RawOrigin::None.into(),
        context.last_block_in_range,
        context.root_hash_h256,
        context.root_id.ingress_counter,
        context.validator.clone(),
        context.record_summary_calculation_signature.clone(),
    )
    .is_ok();
}

mod process_summary_if_required {
    use super::*;

    struct LocalContext {
        pub current_block: u64,
        pub target_block: u64,
        pub min_block_age: u64,
        pub block_number_for_next_slot: u64,
        pub slot_validator: Validator<UintAuthorityId, u64>,
        pub url_param: String,
        pub root_hash: Vec<u8>,
    }

    fn setup_success_preconditions() -> LocalContext {
        let schedule_period = 2;
        let voting_period = 2;
        let min_block_age = <TestRuntime as Config>::MinBlockAge::get();
        let arbitrary_margin = 3;
        let next_block_to_process = 2;
        let target_block = next_block_to_process + schedule_period - 1;

        let current_block = target_block + min_block_age + arbitrary_margin;
        let slot_number = 3;
        let block_number_for_next_slot = current_block + schedule_period;

        // index - Validators:
        // 0 - FIRST_VALIDATOR_INDEX
        // 1 - SECOND_VALIDATOR_INDEX
        // 2 - THIRD_VALIDATOR_INDEX
        // 3 - FOURTH_VALIDATOR_INDEX
        let slot_validator = get_validator(FOURTH_VALIDATOR_INDEX);

        System::set_block_number(current_block);
        Summary::set_schedule_and_voting_periods(schedule_period, voting_period);
        Summary::set_next_block_to_process(next_block_to_process);
        Summary::set_next_slot_block_number(block_number_for_next_slot);
        Summary::set_current_slot(slot_number);
        Summary::set_current_slot_validator(slot_validator.account_id.clone());

        let url_param = get_url_param(next_block_to_process, schedule_period);
        let root_hash = ROOT_HASH_HEX_STRING.to_vec();

        return LocalContext {
            current_block,
            target_block,
            min_block_age,
            slot_validator,
            block_number_for_next_slot,
            url_param,
            root_hash,
        };
    }

    mod calls_record_summary_calculation_successfully {
        use super::*;

        #[test]
        fn when_primary_validator_processes_current_block() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(context.root_hash_vec.clone()),
                );

                setup_blocks(&context);
                setup_total_ingresses(&context);
                assert!(pool_state.read().transactions.is_empty());

                assert!(record_summary_calculation_is_called(
                    context.current_block_number,
                    &context.validator,
                    &pool_state
                ));

                let submitted_unsigned_transaction_call =
                    get_unsigned_record_summary_calculation_call_from_chain(&pool_state);
                let expected_call = expected_unsigned_record_summary_calculation_call(&context);
                assert_eq!(submitted_unsigned_transaction_call, expected_call);
            });
        }

        #[test]
        fn and_retries_if_process_summary_failed_before() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert!(pool_state.read().transactions.is_empty());

                // Fails at the default current block
                let fake_failure_response = b"0".to_vec();
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(fake_failure_response),
                );

                assert!(!record_summary_calculation_is_called(
                    context.current_block_number,
                    &context.validator,
                    &pool_state
                ));

                // Advances to the next block
                let _ = advance_block_numbers(1);

                // Retries and succeeds at the next block
                let fake_successful_response = context.root_hash_vec.clone();
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(fake_successful_response),
                );

                assert!(record_summary_calculation_is_called(
                    context.current_block_number,
                    &context.validator,
                    &pool_state
                ));
                let submitted_unsigned_transaction_call =
                    get_unsigned_record_summary_calculation_call_from_chain(&pool_state);
                let expected_call = expected_unsigned_record_summary_calculation_call(&context);
                assert_eq!(submitted_unsigned_transaction_call, expected_call);
            });
        }

        #[test]
        fn when_preconditions_are_met() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let setup = setup_success_preconditions();

                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    setup.url_param.clone(),
                    Some(setup.root_hash.clone()),
                );

                assert!(pool_state.read().transactions.is_empty());

                assert!(record_summary_calculation_is_called(
                    setup.current_block,
                    &setup.slot_validator,
                    &pool_state
                ));
            });
        }
    }

    mod fails_to_call_record_summary_calculation_when {
        use super::*;

        #[test]
        fn target_block_is_not_old_enough() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let setup = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let early_block_number = setup.target_block + setup.min_block_age;

                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    setup.url_param.clone(),
                    Some(setup.root_hash.clone()),
                );

                assert!(!record_summary_calculation_is_called(
                    early_block_number,
                    &setup.slot_validator,
                    &pool_state
                ));

                assert!(record_summary_calculation_is_called(
                    early_block_number + 1,
                    &setup.slot_validator,
                    &pool_state
                ));
            });
        }

        #[test]
        fn caller_is_not_slot_validator() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let setup = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let non_designated_validator = get_validator(FIRST_VALIDATOR_INDEX);
                assert!(non_designated_validator != setup.slot_validator);
                assert!(!record_summary_calculation_is_called(
                    setup.current_block,
                    &non_designated_validator,
                    &pool_state
                ));
            });
        }

        #[test]
        fn slot_has_ended_but_not_advanced_yet() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let setup = setup_success_preconditions();

                assert!(pool_state.read().transactions.is_empty());

                let last_valid_block_number = setup.block_number_for_next_slot - 1;

                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    setup.url_param.clone(),
                    Some(setup.root_hash.clone()),
                );

                assert!(record_summary_calculation_is_called(
                    last_valid_block_number,
                    &setup.slot_validator,
                    &pool_state
                ));

                let late_block_number = setup.block_number_for_next_slot;

                pool_state.write().transactions.clear();
                assert!(pool_state.read().transactions.is_empty());

                assert!(!record_summary_calculation_is_called(
                    late_block_number,
                    &setup.slot_validator,
                    &pool_state
                ));
            });
        }

        #[test]
        fn get_target_block_returns_overflow_error() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                System::set_block_number(context.current_block_number);
                Summary::set_next_block_to_process(u64::MAX);
                setup_total_ingresses(&context);
                assert!(pool_state.read().transactions.is_empty());

                assert!(!record_summary_calculation_is_called(
                    context.current_block_number,
                    &context.validator,
                    &pool_state
                ));
            });
        }

        #[test]
        fn last_block_in_range_is_already_locked() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);
                let root_lock_name = Summary::create_root_lock_name(context.last_block_in_range);
                let mut lock = Avn::get_ocw_locker(&root_lock_name);
                if let Ok(_guard) = lock.try_lock() {
                    assert!(pool_state.read().transactions.is_empty());

                    assert!(!record_summary_calculation_is_called(
                        context.current_block_number,
                        &context.validator,
                        &pool_state
                    ));
                };
            });
        }

        #[test]
        fn target_block_with_buffer_overflows() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                Summary::set_next_block_to_process(u64::MAX - 2);
                setup_total_ingresses(&context);
                assert!(pool_state.read().transactions.is_empty());

                assert!(!record_summary_calculation_is_called(
                    context.current_block_number,
                    &context.validator,
                    &pool_state
                ));
            });
        }
    }

    mod stops_to_call_record_summary_calculation_when {
        use super::*;

        #[test]
        fn slot_expires_and_process_summary_is_not_successful() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert!(pool_state.read().transactions.is_empty());

                let fake_failure_response = b"0".to_vec();
                let fake_successful_response = context.root_hash_vec.clone();

                // Fails at the default current block #10
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(fake_failure_response.clone()),
                );
                assert!(!record_summary_calculation_is_called(
                    context.current_block_number,
                    &context.validator,
                    &pool_state
                ));

                // Advance to block #11
                let current_block_number = advance_block_numbers(1);
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(fake_failure_response),
                );
                assert!(!record_summary_calculation_is_called(
                    current_block_number,
                    &context.validator,
                    &pool_state
                ));

                // Advance to block #12
                // By providing a successful compute root hash response, the validator should not be
                // able to reach it and call the transaction as the slot has expired
                let current_block_number = advance_block_numbers(1);
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(fake_successful_response),
                );

                assert!(!record_summary_calculation_is_called(
                    current_block_number,
                    &context.validator,
                    &pool_state
                ));

                // Retreat to the previous block #11 and call it again to show a successful unsigned
                // transaction is created This proves the failure in block #12 is
                // not caused by any other reasons except the block number is
                // reaching out of the slot.
                let current_block_number = retreat_block_numbers(1);
                assert!(record_summary_calculation_is_called(
                    current_block_number,
                    &context.validator,
                    &pool_state
                ));

                assert_eq!(System::block_number(), 11);
                assert_eq!(Summary::block_number_for_next_slot(), 12);

                let submitted_unsigned_transaction_call =
                    get_unsigned_record_summary_calculation_call_from_chain(&pool_state);
                let expected_call = expected_unsigned_record_summary_calculation_call(&context);
                assert_eq!(submitted_unsigned_transaction_call, expected_call);
            });
        }
    }
}

mod process_summary {
    use super::*;

    #[test]
    fn succeeds_then_a_record_summary_calculation_transaction_is_created() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();

            mock_response_of_get_roothash(
                &mut offchain_state.write(),
                context.url_param.clone(),
                Some(context.root_hash_vec.clone()),
            );

            setup_blocks(&context);
            setup_total_ingresses(&context);
            assert!(pool_state.read().transactions.is_empty());

            assert!(
                Summary::process_summary(context.last_block_in_range, &context.validator).is_ok()
            );

            let tx = pool_state.write().transactions.pop().unwrap();
            assert!(pool_state.read().transactions.is_empty());
            let tx = Extrinsic::decode(&mut &*tx).unwrap();
            assert!(matches!(tx.preamble, Preamble::Bare(_)));

            // Setup total ingresses will store ingress_counter - 1,
            // and calling process_summary increases the ingress counter by 1.
            let expected_ingress_counter = context.root_id.ingress_counter;
            let signature = context
                .validator
                .key
                .sign(
                    &(
                        &Summary::update_block_number_context(),
                        context.root_hash_h256,
                        expected_ingress_counter,
                        context.last_block_in_range,
                    )
                        .encode(),
                )
                .expect("Signature is signed");

            assert_eq!(
                tx.function,
                mock::RuntimeCall::Summary(crate::Call::record_summary_calculation {
                    new_block_number: context.last_block_in_range,
                    root_hash: context.root_hash_h256,
                    ingress_counter: expected_ingress_counter,
                    validator: context.validator.clone(),
                    signature
                })
            );
        });
    }

    mod fails {
        use super::*;

        #[test]
        fn when_get_root_hash_from_block_number_has_conversion_error() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                let current_block_number = u64::MAX - 50;
                let next_block_number_to_process = u64::MAX - 994;
                System::set_block_number(current_block_number);
                Summary::set_next_block_to_process(next_block_number_to_process);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::process_summary(next_block_number_to_process + 1, &context.validator),
                    Error::<TestRuntime>::ErrorConvertingBlockNumber
                );
            });
        }

        #[test]
        fn when_get_root_hash_to_block_number_has_conversion_error() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                let current_block_number: u64 = (u32::MAX - 50) as u64;
                let next_block_number_to_process: u64 = u32::MAX as u64;
                System::set_block_number(current_block_number);
                Summary::set_next_block_to_process(next_block_number_to_process);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::process_summary(next_block_number_to_process + 1, &context.validator),
                    Error::<TestRuntime>::ErrorConvertingBlockNumber
                );
            });
        }

        #[test]
        #[ignore]
        fn when_get_root_hash_from_service_fails() {
            let (mut ext, _pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                // TODO [TYPE: test][PRI: medium][JIRA: 321]: mock of failure response
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(context.root_hash_vec.clone()),
                );

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::process_summary(context.last_block_in_range, &context.validator),
                    Error::<TestRuntime>::ErrorGettingSummaryDataFromService
                );
            });
        }

        mod when_root_hash_has {
            use super::*;

            #[test]
            fn invalid_root_hash_length() {
                let (mut ext, _pool_state, offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_context();

                    mock_response_of_get_roothash(
                        &mut offchain_state.write(),
                        context.url_param.clone(),
                        Some(b"0".to_vec()),
                    );
                    mock_response_of_get_roothash(
                        &mut offchain_state.write(),
                        context.url_param.clone(),
                        Some(b"0123456789012345678901234567890".to_vec()),
                    );
                    mock_response_of_get_roothash(
                        &mut offchain_state.write(),
                        context.url_param.clone(),
                        Some(b"012345678901234567890123456789012".to_vec()),
                    );

                    setup_blocks(&context);
                    setup_total_ingresses(&context);

                    assert_noop!(
                        Summary::process_summary(context.last_block_in_range, &context.validator),
                        Error::<TestRuntime>::InvalidRootHashLength
                    );

                    assert_noop!(
                        Summary::process_summary(context.last_block_in_range, &context.validator),
                        Error::<TestRuntime>::InvalidRootHashLength
                    );

                    assert_noop!(
                        Summary::process_summary(context.last_block_in_range, &context.validator),
                        Error::<TestRuntime>::InvalidRootHashLength
                    );
                });
            }

            #[test]
            fn invalid_utf_8_bytes() {
                let (mut ext, _pool_state, offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_context();

                    let invalid_utf8_bytes = vec![
                        0, 159, 146, 150, 0, 159, 146, 150, 0, 159, 146, 150, 0, 159, 146, 150, 0,
                        159, 146, 150, 0, 159, 146, 150, 0, 159, 146, 150, 0, 159, 146, 150, 0,
                        159, 146, 150, 0, 159, 146, 150, 0, 159, 146, 150, 0, 159, 146, 150, 0,
                        159, 146, 150, 0, 159, 146, 150, 0, 159, 146, 150, 0, 159, 146, 150,
                    ];

                    mock_response_of_get_roothash(
                        &mut offchain_state.write(),
                        context.url_param.clone(),
                        Some(invalid_utf8_bytes),
                    );

                    setup_blocks(&context);
                    setup_total_ingresses(&context);

                    assert_noop!(
                        Summary::process_summary(context.last_block_in_range, &context.validator),
                        Error::<TestRuntime>::InvalidUTF8Bytes
                    );
                });
            }

            #[test]
            fn invalid_hex_string() {
                let (mut ext, _pool_state, offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_context();

                    mock_response_of_get_roothash(
                        &mut offchain_state.write(),
                        context.url_param.clone(),
                        Some(
                            b"zzzzc9e671fe581fe4ef4631112038297dcdecae163e8724c281ece8ad94c8c3"
                                .to_vec(),
                        ),
                    );

                    setup_blocks(&context);
                    setup_total_ingresses(&context);

                    assert_noop!(
                        Summary::process_summary(context.last_block_in_range, &context.validator),
                        Error::<TestRuntime>::InvalidHexString
                    );
                });
            }
        }

        #[test]
        #[ignore]
        fn when_record_summary_has_error_in_signing() {
            let (mut ext, _pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(context.root_hash_vec.clone()),
                );

                setup_blocks(&context);
                setup_total_ingresses(&context);

                // TODO [TYPE: test][PRI: medium][JIRA: 321]: Mock a validator to cause signing
                // error
                let non_validator = get_non_validator();
                assert_noop!(
                    Summary::process_summary(context.last_block_in_range, &non_validator),
                    Error::<TestRuntime>::ErrorSigning
                );
            });
        }

        #[test]
        #[ignore]
        fn when_record_summary_has_error_in_submitting_transaction() {
            let (mut ext, _pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    context.url_param.clone(),
                    Some(context.root_hash_vec.clone()),
                );
                setup_blocks(&context);
                setup_total_ingresses(&context);

                // TODO [TYPE: test][PRI: medium][JIRA: 321]: Mock a submit_transaction
                // with error
                assert_noop!(
                    Summary::process_summary(context.last_block_in_range, &context.validator),
                    Error::<TestRuntime>::ErrorSubmittingTransaction
                );
            });
        }
    }
}

pub mod record_summary_calculation {
    use super::*;
    use tests_validate_unsigned::assert_validate_unsigned_record_summary_calculation_is_successful;
    use tests_vote::setup_approved_root;

    mod succeeds_implies_that {
        use sp_runtime::BoundedVec;

        use super::*;

        #[test]
        fn next_block_to_process_is_not_updated() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert!(record_summary_calculation_is_ok(&context));

                assert_eq!(Summary::get_next_block_to_process(), context.next_block_to_process);
            });
        }

        mod block_number_for_next_slot_is_updated {
            use super::*;

            #[test]
            fn with_default_schedule_period() {
                let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validators()
                    .for_offchain_worker()
                    .as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_context();

                    setup_blocks(&context);
                    setup_total_ingresses(&context);

                    assert!(record_summary_calculation_is_ok(&context));

                    assert_eq!(
                        Summary::block_number_for_next_slot(),
                        context.current_block_number + Summary::schedule_period()
                    );
                });
            }
        }

        #[test]
        fn root_data_is_correctly_added() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert!(record_summary_calculation_is_ok(&context));

                let root = Summary::get_root_data(&context.root_id);
                assert_eq!(
                    root,
                    RootData::new(
                        context.root_hash_h256,
                        context.validator.account_id.clone(),
                        root.tx_id
                    )
                );
            });
        }

        #[test]
        fn root_range_is_added_to_pending_approval() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert!(record_summary_calculation_is_ok(&context));

                assert!(PendingApproval::<TestRuntime>::contains_key(context.root_id.range));
            });
        }

        #[test]
        fn root_range_is_added_to_votes_repository() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert!(record_summary_calculation_is_ok(&context));

                assert_eq!(
                    Summary::get_vote(context.root_id),
                    VotingSessionData {
                        voting_session_id: context.root_id.session_id(),
                        threshold: QUORUM,
                        ayes: BoundedVec::default(),
                        nays: BoundedVec::default(),
                        end_of_voting_period: VOTING_PERIOD_END,
                        created_at_block: 10 // Setup creates block number 10
                    }
                );
            });
        }

        #[test]
        fn event_is_emitted() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert!(record_summary_calculation_is_ok(&context));

                assert!(System::events().iter().any(|a| a.event
                    == mock::RuntimeEvent::Summary(
                        crate::Event::<TestRuntime>::SummaryCalculated {
                            from: context.next_block_to_process,
                            to: context.last_block_in_range,
                            root_hash: context.root_hash_h256,
                            submitter: context.validator.account_id
                        }
                    )));
            });
        }
    }

    mod fails {
        use super::*;

        #[test]
        fn when_origin_is_signed() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::record_summary_calculation(
                        RuntimeOrigin::signed(Default::default()),
                        context.last_block_in_range,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        context.validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    ),
                    BadOrigin
                );
            });
        }

        #[test]
        fn when_validator_has_invalid_key() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        context.last_block_in_range,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        get_non_validator(),
                        context.record_summary_calculation_signature.clone()
                    ),
                    Error::<TestRuntime>::InvalidKey
                );
            });
        }

        #[test]
        fn when_get_target_block_fails() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                System::set_block_number(context.current_block_number);
                Summary::set_next_block_to_process(u64::MAX);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        context.last_block_in_range,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        context.validator,
                        context.record_summary_calculation_signature.clone()
                    ),
                    Error::<TestRuntime>::Overflow
                );
            });
        }

        #[test]
        fn when_summary_is_already_calculated() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                let tx_id = INITIAL_TRANSACTION_ID;
                Summary::insert_root_hash(
                    &context.root_id,
                    context.root_hash_h256,
                    context.validator.account_id.clone(),
                    tx_id,
                );
                Summary::set_root_as_validated(&context.root_id);

                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        context.last_block_in_range,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        context.validator,
                        context.record_summary_calculation_signature.clone()
                    ),
                    Error::<TestRuntime>::SummaryPendingOrApproved
                );
            });
        }

        #[test]
        fn when_root_has_already_been_registered_for_voting() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);
                Summary::register_root_for_voting(&context.root_id, QUORUM, VOTING_PERIOD_END);
                Summary::record_approve_vote(&context.root_id, context.validator.account_id);

                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        context.last_block_in_range,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        context.validator,
                        context.record_summary_calculation_signature.clone()
                    ),
                    Error::<TestRuntime>::RootHasAlreadyBeenRegisteredForVoting
                );
            });
        }

        #[test]
        fn when_summary_range_is_invalid() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        context.last_block_in_range + 1,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        context.validator,
                        context.record_summary_calculation_signature.clone()
                    ),
                    Error::<TestRuntime>::InvalidSummaryRange
                );
            });
        }

        #[test]
        fn when_voting_period_end_overflows() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                System::set_block_number(u64::MAX);
                Summary::set_next_block_to_process(context.next_block_to_process);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        context.last_block_in_range,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        context.validator,
                        context.record_summary_calculation_signature.clone()
                    ),
                    Error::<TestRuntime>::Overflow
                );
            });
        }

        #[test]
        fn when_next_block_to_process_overflows() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                let next_block_to_process = u64::MAX;
                let last_block_in_range = next_block_to_process - Summary::schedule_period() + 1;
                System::set_block_number(context.current_block_number);
                Summary::set_next_block_to_process(last_block_in_range);
                setup_total_ingresses(&context);

                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        next_block_to_process,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        context.validator,
                        context.record_summary_calculation_signature.clone()
                    ),
                    Error::<TestRuntime>::Overflow
                );
            });
        }
    }

    mod succeeds_when {
        use super::*;

        #[test]
        fn a_rejected_summary_is_recreated() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                // Execute unsigned record_summary_calculation extrinsic
                assert_record_summary_calculation_is_ok(&context);

                // Vote to reject the recorded summary
                setup_a_voting_and_reject_root_to_reach_quorum(&context);

                // End voting
                assert_end_voting_period_is_ok(&context);

                let ingress_counter = context.root_id.ingress_counter + 1;
                let context = get_context_with_new_ingress_counter(&context, ingress_counter);
                setup_total_ingresses(&context);

                // Validate the same unsigned record_summary_calculation extrinsic with a new
                // signature and increased ingress counter
                assert_validate_unsigned_record_summary_calculation_is_successful(&context);

                // Execute unsigned record_summary_calculation extrinsic
                assert_record_summary_calculation_is_ok(&context);

                // Vote to reject the recorded summary
                setup_approved_root(context.clone());

                // End voting
                assert_end_voting_period_is_ok(&context);
            });
        }
    }

    fn get_context_with_new_ingress_counter(
        context: &Context,
        ingress_counter: IngressCounter,
    ) -> Context {
        let mut new_context = context.clone();
        new_context.root_id.ingress_counter = ingress_counter;
        new_context.record_summary_calculation_signature =
            get_signature_for_record_summary_calculation(
                context.validator.clone(),
                &Summary::update_block_number_context(),
                context.root_hash_h256,
                ingress_counter,
                context.last_block_in_range,
            );
        return new_context;
    }
}

pub fn setup_a_voting_and_reject_root_to_reach_quorum(context: &Context) {
    setup_voting_for_root_id(&context);

    let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
    let third_validator = get_validator(THIRD_VALIDATOR_INDEX);
    Summary::record_reject_vote(&context.root_id, context.validator.account_id);
    Summary::record_reject_vote(&context.root_id, second_validator.account_id);
    Summary::record_reject_vote(&context.root_id, third_validator.account_id);
}

pub fn assert_record_summary_calculation_is_ok(context: &Context) {
    assert!(Summary::record_summary_calculation(
        RawOrigin::None.into(),
        context.last_block_in_range,
        context.root_hash_h256,
        context.root_id.ingress_counter,
        context.validator.clone(),
        context.record_summary_calculation_signature.clone()
    )
    .is_ok());
}

fn assert_end_voting_period_is_ok(context: &Context) {
    assert!(Summary::end_voting_period(
        RawOrigin::None.into(),
        context.root_id,
        context.validator.clone(),
        context.record_summary_calculation_signature.clone(),
    )
    .is_ok());
}

// The reason for testing only 2 summaries within the same slot
// is because the SchedulePeriod is currently set to 2.
// TODO [TYPE: tests][PRI: low]: Increase the scheduled period and test at least 3 summaries have
// been processed successfully here.
mod if_process_summary_is_called_a_second_time {
    use super::*;

    const SECOND_ROOT_HASH_HEX_STRING: &'static [u8; 64] =
        b"09ec14e7d5fe581fe4ef4631112038297dcdecae163e8724c281ece8ad94c8c3";
    const SECOND_ROOT_HASH_BYTES: [u8; 32] = [
        9, 236, 20, 231, 213, 254, 88, 31, 228, 239, 70, 49, 17, 32, 56, 41, 125, 205, 236, 174,
        22, 62, 135, 36, 194, 129, 236, 232, 173, 148, 200, 195,
    ];

    fn setup_block_numbers_and_slots(context: &Context) {
        let current_block_number: BlockNumber = 12;
        let next_block_to_process: BlockNumber = 3;
        let previous_slot_number: BlockNumber = 1;
        let current_slot_number: BlockNumber = 2;
        let block_number_for_next_slot: BlockNumber =
            current_block_number + Summary::schedule_period();

        System::set_block_number(current_block_number);
        Summary::set_next_block_to_process(next_block_to_process);
        Summary::set_previous_summary_slot(previous_slot_number);
        Summary::set_current_slot(current_slot_number);
        Summary::set_current_slot_validator(context.validator.account_id);
        Summary::set_next_slot_block_number(block_number_for_next_slot);
    }

    fn setup_second_process_summary_context(current_block_number: BlockNumber) -> Context {
        let next_block_to_process = Summary::get_next_block_to_process();
        let last_block_in_range = Summary::get_target_block().expect("Valid block number");
        let validator = get_validator(FIRST_VALIDATOR_INDEX);
        let root_hash_h256 = H256::from(SECOND_ROOT_HASH_BYTES);
        let root_hash_vec = SECOND_ROOT_HASH_HEX_STRING.to_vec();
        let root_range = RootRange::new(next_block_to_process, last_block_in_range);
        let ingress_counter = Summary::get_ingress_counter() + 1;
        let tx_id = 0;
        let finalised_block_vec = Some(hex::encode(0u32.encode()).into());

        Context {
            current_block_number,
            next_block_to_process,
            last_block_in_range: Summary::get_target_block().expect("Valid block number"),
            validator: validator.clone(),
            root_id: RootId::new(root_range, ingress_counter),
            root_hash_h256,
            root_hash_vec,
            url_param: get_url_param(next_block_to_process, Summary::schedule_period()),
            record_summary_calculation_signature: get_signature_for_record_summary_calculation(
                validator,
                &Summary::update_block_number_context(),
                root_hash_h256,
                ingress_counter,
                last_block_in_range,
            ),
            tx_id,
            current_slot: CURRENT_SLOT + 1,
            finalised_block_vec,
        }
    }

    fn approve_summary(context: &Context) {
        vote_and_end_summary(context, true);

        assert!(Summary::get_root_data(&context.root_id).is_validated);
        assert!(!PendingApproval::<TestRuntime>::contains_key(&context.root_id.range));
        assert_eq!(
            Summary::get_next_block_to_process(),
            context.next_block_to_process + Summary::schedule_period()
        );
        assert_eq!(Summary::last_summary_slot(), Summary::current_slot());

        assert!(System::events().iter().any(|a| a.event
            == mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VotingEnded {
                root_id: context.root_id,
                vote_approved: true
            })));
    }

    fn reject_summary(context: &Context) {
        let previous_summary_slot_before_voting = Summary::last_summary_slot();
        vote_and_end_summary(context, false);

        assert!(!Summary::get_root_data(&context.root_id).is_validated);
        assert!(!PendingApproval::<TestRuntime>::contains_key(&context.root_id.range));
        assert_eq!(Summary::get_next_block_to_process(), context.next_block_to_process);
        assert_eq!(Summary::last_summary_slot(), previous_summary_slot_before_voting);

        assert!(System::events().iter().any(|a| a.event
            == mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VotingEnded {
                root_id: context.root_id,
                vote_approved: false
            })));
    }

    fn vote_and_end_summary(context: &Context, is_approve: bool) {
        Summary::insert_root_hash(
            &context.root_id,
            context.root_hash_h256,
            context.validator.account_id.clone(),
            context.tx_id,
        );
        Summary::insert_pending_approval(&context.root_id);
        Summary::register_root_for_voting(&context.root_id, QUORUM, VOTING_PERIOD_END);

        let validators = vec![
            get_validator(FIRST_VALIDATOR_INDEX),
            get_validator(SECOND_VALIDATOR_INDEX),
            get_validator(THIRD_VALIDATOR_INDEX),
        ];

        validators.iter().for_each(|validator| {
            if is_approve {
                Summary::record_approve_vote(&context.root_id, validator.account_id);
            } else {
                Summary::record_reject_vote(&context.root_id, validator.account_id);
            }
        });

        assert!(Summary::end_voting_period(
            RawOrigin::None.into(),
            context.root_id,
            context.validator.clone(),
            context.record_summary_calculation_signature.clone(),
        )
        .is_ok());
    }

    mod given_the_first_summary_was_approved {
        use super::*;

        #[test]
        fn then_second_call_processes_second_range() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let first_process_summary_context = setup_context();
                setup_total_ingresses(&first_process_summary_context);
                setup_block_numbers_and_slots(&first_process_summary_context);

                assert!(pool_state.read().transactions.is_empty());

                let last_block_in_range_before_first_time_process_summary =
                    Summary::get_target_block().expect("Valid block number");
                let url_param_for_first_summary_process =
                    first_process_summary_context.url_param.clone();

                // Mock a successful compute root hash response for the first time process summary
                let fake_successful_response_1 =
                    first_process_summary_context.root_hash_vec.clone();
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    first_process_summary_context.url_param.clone(),
                    Some(fake_successful_response_1),
                );

                // The first time process summary for root [from:3;to:4] is created successfully at
                // block#12
                assert!(record_summary_calculation_is_called(
                    first_process_summary_context.current_block_number,
                    &first_process_summary_context.validator,
                    &pool_state
                ));
                let submitted_unsigned_transaction_call =
                    get_unsigned_record_summary_calculation_call_from_chain(&pool_state);
                let expected_call = expected_unsigned_record_summary_calculation_call(
                    &first_process_summary_context,
                );
                assert_eq!(submitted_unsigned_transaction_call, expected_call);

                // Simulate the record summary calculation for the first process summary context
                assert!(record_summary_calculation_is_ok(&first_process_summary_context));

                // Approve the first time processed summary
                approve_summary(&first_process_summary_context);

                // Advances to block#13 to process summary for the second time
                let current_block_number = advance_block_numbers(1);

                // Mock the context to process summary for the second time
                let second_process_summary_context =
                    setup_second_process_summary_context(current_block_number);

                // NextBlockToProcess is increased by 1 as the first processed summary is approved
                assert_eq!(
                    second_process_summary_context.last_block_in_range,
                    last_block_in_range_before_first_time_process_summary
                        + Summary::schedule_period()
                );
                // Url Param for the target root is updated from [from:3;to:4] to [from:5;to:6] as
                // the first processed summary is approved
                assert!(
                    second_process_summary_context.url_param != url_param_for_first_summary_process
                );

                // Mock successful compute root hash response for the second summary process
                let fake_successful_response_2 =
                    second_process_summary_context.root_hash_vec.clone();
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    second_process_summary_context.url_param.clone(),
                    Some(fake_successful_response_2),
                );

                // Process summary for the new root [from:5;to:6], that is different from the first
                // time is successfully created
                assert!(record_summary_calculation_is_called(
                    current_block_number,
                    &second_process_summary_context.validator,
                    &pool_state
                ));
                let submitted_unsigned_transaction_call =
                    get_unsigned_record_summary_calculation_call_from_chain(&pool_state);
                let expected_call = expected_unsigned_record_summary_calculation_call(
                    &second_process_summary_context,
                );
                assert_eq!(submitted_unsigned_transaction_call, expected_call);

                // Approve the processed summary that is successfully created at the second time
                approve_summary(&second_process_summary_context);
            });
        }
    }

    mod given_the_first_summary_was_rejected {
        use super::*;

        #[test]
        fn then_second_call_processes_first_range() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let first_process_summary_context = setup_context();
                setup_total_ingresses(&first_process_summary_context);
                setup_block_numbers_and_slots(&first_process_summary_context);

                assert!(pool_state.read().transactions.is_empty());

                let last_block_in_range_before_first_time_process_summary =
                    Summary::get_target_block().expect("Valid block number");
                let url_param_for_first_summary_process =
                    first_process_summary_context.url_param.clone();

                // Mock successful compute root hash response for the first time process summary
                let fake_successful_response_1 =
                    first_process_summary_context.root_hash_vec.clone();
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    first_process_summary_context.url_param.clone(),
                    Some(fake_successful_response_1),
                );

                // The first time process summary for root [from:3;to:4] is created successfully at
                // block#12
                assert!(record_summary_calculation_is_called(
                    first_process_summary_context.current_block_number,
                    &first_process_summary_context.validator,
                    &pool_state
                ));
                let submitted_unsigned_transaction_call =
                    get_unsigned_record_summary_calculation_call_from_chain(&pool_state);
                let expected_call = expected_unsigned_record_summary_calculation_call(
                    &first_process_summary_context,
                );
                assert_eq!(submitted_unsigned_transaction_call, expected_call);

                // Reject the first time processed summary
                reject_summary(&first_process_summary_context);

                // Advances to block#13 to process summary for the second time
                let current_block_number = advance_block_numbers(1);

                // Mock the context to process summary for the second time
                let second_process_summary_context =
                    setup_second_process_summary_context(current_block_number);

                // NextBlockToProcess is not increased as the processed summary for the first time
                // is rejected
                assert_eq!(
                    second_process_summary_context.last_block_in_range,
                    last_block_in_range_before_first_time_process_summary
                );
                // Url Param for target root is not updated from [from:3;to:4] to [from:5;to:6] as
                // the processed summary for the first time is rejected
                assert_eq!(
                    second_process_summary_context.url_param,
                    url_param_for_first_summary_process
                );

                // Mock successful compute root hash response to process summary for the second time
                // [from:3;to:4] This is a new root hash value that is different
                // from the first time
                let fake_successful_response_2 =
                    second_process_summary_context.root_hash_vec.clone();
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    second_process_summary_context.url_param.clone(),
                    Some(fake_successful_response_2),
                );

                // A new processed summary for the same root [from:3;to:4] is successfully created
                // First we need to clear the lock
                let key = Summary::create_root_lock_name(
                    second_process_summary_context.last_block_in_range,
                );
                let mut guard = StorageValueRef::persistent(&key);
                guard.clear();

                assert!(record_summary_calculation_is_called(
                    current_block_number,
                    &second_process_summary_context.validator,
                    &pool_state
                ));
                let submitted_unsigned_transaction_call =
                    get_unsigned_record_summary_calculation_call_from_chain(&pool_state);
                let expected_call = expected_unsigned_record_summary_calculation_call(
                    &second_process_summary_context,
                );
                assert_eq!(submitted_unsigned_transaction_call, expected_call);

                // Approve the processed summary that is successfully created at the second time
                approve_summary(&second_process_summary_context);
            });
        }
    }

    mod given_the_first_summary_is_pending {
        use super::*;

        #[test]
        fn then_second_call_has_no_effect() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let first_process_summary_context = setup_context();
                setup_total_ingresses(&first_process_summary_context);
                setup_block_numbers_and_slots(&first_process_summary_context);

                assert!(pool_state.read().transactions.is_empty());

                let last_block_in_range_before_first_time_process_summary =
                    Summary::get_target_block().expect("Valid block number");
                let url_param_for_first_summary_process =
                    first_process_summary_context.url_param.clone();

                // Mock successful compute root hash response for the first time process summary
                let fake_successful_response_1 =
                    first_process_summary_context.root_hash_vec.clone();
                mock_response_of_get_roothash(
                    &mut offchain_state.write(),
                    first_process_summary_context.url_param.clone(),
                    Some(fake_successful_response_1),
                );

                // The first time process summary for root [from:3;to:4] is created successfully at
                // block#12
                assert!(record_summary_calculation_is_called(
                    first_process_summary_context.current_block_number,
                    &first_process_summary_context.validator,
                    &pool_state
                ));
                let submitted_unsigned_transaction_call =
                    get_unsigned_record_summary_calculation_call_from_chain(&pool_state);
                let expected_call = expected_unsigned_record_summary_calculation_call(
                    &first_process_summary_context,
                );
                assert_eq!(submitted_unsigned_transaction_call, expected_call);

                // Simulate the record summary calculation for the first process summary context
                assert!(record_summary_calculation_is_ok(&first_process_summary_context));

                // The first time processed summary is still waiting for approval

                // Advances to block#13 to process summary
                let current_block_number = advance_block_numbers(1);

                // Mock the context to process summary based on the new current block number
                let second_process_summary_context =
                    setup_second_process_summary_context(current_block_number);

                // NextBlockToProcess is not increased as the processed summary for the first time
                // is still pending for approval
                assert_eq!(
                    second_process_summary_context.last_block_in_range,
                    last_block_in_range_before_first_time_process_summary
                );
                // Url Param for target root is not updated from [from:3;to:4] to [from:5;to:6]
                assert_eq!(
                    second_process_summary_context.url_param,
                    url_param_for_first_summary_process
                );

                // No new summary for the same root [from:3;to:4] be created
                assert!(!record_summary_calculation_is_called(
                    current_block_number,
                    &second_process_summary_context.validator,
                    &pool_state
                ));
            });
        }
    }
}

// TODO: add a test to ensure we pick validators in sequential order of their index

mod constrains {
    use crate::{RootId, RootRange};
    pub type BlockNumber = u32;
    use sp_avn_common::bounds::VotingSessionIdBound;
    use sp_core::Get;

    #[test]
    fn ensure_action_id_encodes_within_boundaries() {
        let action_id = RootId::<BlockNumber>::new(RootRange::new(0u32.into(), 60u32.into()), 1);
        assert!(
            action_id.session_id().len() as u32 <= VotingSessionIdBound::get(),
            "The encoded size of RootId must not exceed the VotingSessionIdBound"
        );
    }
}
