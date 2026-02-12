// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{
    avn::vote::{APPROVE_VOTE_IS_NOT_VALID, REJECT_VOTE_IS_NOT_VALID, VOTE_SESSION_IS_NOT_VALID},
    mock::*,
    system,
    tests::assert_record_summary_calculation_is_ok,
    tests_vote::{setup_approved_root, vote_to_approve_root, vote_to_reject_root},
};
use frame_support::{assert_noop, unsigned::ValidateUnsigned};
use pallet_avn::Error as AvNError;
use sp_core::H256;
use sp_runtime::{
    testing::{TestSignature, UintAuthorityId},
    transaction_validity::ValidTransaction,
};
use system::RawOrigin;

pub mod input_is_record_summary_calculation {
    use super::*;

    #[test]
    fn succeeds() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();

            setup_blocks(&context);
            setup_total_ingresses(&context);

            let transaction_call = crate::Call::record_summary_calculation {
                new_block_number: context.last_block_in_range,
                root_hash: context.root_hash_h256.clone(),
                ingress_counter: context.root_id.ingress_counter,
                validator: context.validator,
                signature: context.record_summary_calculation_signature,
            };

            assert_eq!(
                <Summary as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::Local,
                    &transaction_call
                ),
                expected_valid_record_summary_calculation_transaction(
                    context.root_hash_h256,
                    context.root_id.ingress_counter
                )
            );
        });
    }

    mod fails_when {
        use super::*;

        fn test_record_summary_calculation_call_fails(call: &crate::Call<TestRuntime>) {
            assert_noop!(
                <Summary as ValidateUnsigned>::validate_unsigned(TransactionSource::Local, call),
                InvalidTransaction::Custom(ERROR_CODE_VALIDATOR_IS_NOT_PRIMARY)
            );
        }

        #[test]
        fn validator_is_not_primary() {
            let mut ext = ExtBuilder::build_default().as_externality();

            ext.execute_with(|| {
                let context = setup_context();
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                let second_validator_signed_signature =
                    get_signature_for_record_summary_calculation(
                        second_validator.clone(),
                        &Summary::update_block_number_context(),
                        context.root_hash_h256,
                        context.last_block_in_range,
                        context.root_id.ingress_counter,
                    );
                let record_summary_calculation_call = crate::Call::record_summary_calculation {
                    new_block_number: context.last_block_in_range,
                    root_hash: context.root_hash_h256,
                    ingress_counter: context.root_id.ingress_counter,
                    validator: second_validator,
                    signature: second_validator_signed_signature,
                };

                test_record_summary_calculation_call_fails(&record_summary_calculation_call);
            });
        }

        #[test]
        fn from_non_validator() {
            let mut ext = ExtBuilder::build_default().as_externality();

            ext.execute_with(|| {
                let context = setup_context();
                let non_validator = get_non_validator();
                let non_validator_signed_signature = get_signature_for_record_summary_calculation(
                    non_validator.clone(),
                    &Summary::update_block_number_context(),
                    context.root_hash_h256,
                    context.last_block_in_range,
                    context.root_id.ingress_counter,
                );
                let record_summary_calculation_call = crate::Call::record_summary_calculation {
                    new_block_number: context.last_block_in_range,
                    root_hash: context.root_hash_h256,
                    ingress_counter: context.root_id.ingress_counter,
                    validator: non_validator,
                    signature: non_validator_signed_signature,
                };

                test_record_summary_calculation_call_fails(&record_summary_calculation_call);
            });
        }

        mod signature_has_wrong {
            use super::*;

            fn get_call(context: &Context, signature: TestSignature) -> crate::Call<TestRuntime> {
                crate::Call::record_summary_calculation {
                    new_block_number: context.last_block_in_range.clone(),
                    root_hash: context.root_hash_h256.clone(),
                    ingress_counter: context.root_id.ingress_counter,
                    validator: context.validator.clone(),
                    signature,
                }
            }

            #[test]
            fn signer() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                    let signature_signed_by_different_validator =
                        get_signature_for_record_summary_calculation(
                            second_validator,
                            &Summary::update_block_number_context(),
                            context.root_hash_h256,
                            context.last_block_in_range,
                            context.root_id.ingress_counter,
                        );
                    let record_summary_calculation_call =
                        get_call(&context, signature_signed_by_different_validator);

                    test_signature_is_wrong(&context, &record_summary_calculation_call);
                });
            }

            #[test]
            fn signature_context() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let wrong_context_signature = get_signature_for_record_summary_calculation(
                        context.validator.clone(),
                        OTHER_CONTEXT,
                        context.root_hash_h256,
                        context.last_block_in_range,
                        context.root_id.ingress_counter,
                    );
                    let record_summary_calculation_call =
                        get_call(&context, wrong_context_signature);

                    test_signature_is_wrong(&context, &record_summary_calculation_call);
                });
            }

            #[test]
            fn last_processed_block_number() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let wrong_last_processed_block_number =
                        context.last_block_in_range + VALIDATORS_COUNT;
                    let signature_with_wrong_last_processed_block_number =
                        get_signature_for_record_summary_calculation(
                            context.validator.clone(),
                            &Summary::update_block_number_context(),
                            context.root_hash_h256.clone(),
                            wrong_last_processed_block_number,
                            context.root_id.ingress_counter,
                        );
                    let record_summary_calculation_call =
                        get_call(&context, signature_with_wrong_last_processed_block_number);

                    test_signature_is_wrong(&context, &record_summary_calculation_call);
                });
            }

            #[test]
            fn root_hash() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let wrong_root_hash = get_root_hash_return_submit_to_tier1_fails();
                    let signature_with_wrong_root_hash =
                        get_signature_for_record_summary_calculation(
                            context.validator.clone(),
                            &Summary::update_block_number_context(),
                            wrong_root_hash,
                            context.last_block_in_range.clone(),
                            context.root_id.ingress_counter,
                        );
                    let record_summary_calculation_call =
                        get_call(&context, signature_with_wrong_root_hash);

                    test_signature_is_wrong(&context, &record_summary_calculation_call);
                });
            }
        }

        #[test]
        fn signature_is_reused() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_blocks(&context);
                setup_total_ingresses(&context);

                assert_validate_unsigned_record_summary_calculation_is_successful(&context);

                // Execute unsigned record_summary_calculation extrinsic
                assert_record_summary_calculation_is_ok(&context);

                Summary::set_next_block_to_process(context.next_block_to_process);

                // Reuse the signature to validate the same unsigned record_summary_calculation
                // extrinsic
                assert_validate_unsigned_record_summary_calculation_is_successful(&context);

                // Reuse the signature to validate unsigned record_summary_calculation extrinsic
                // with a different ingress counter
                let transaction_call = crate::Call::record_summary_calculation {
                    new_block_number: context.last_block_in_range,
                    root_hash: context.root_hash_h256.clone(),
                    ingress_counter: context.root_id.ingress_counter + 1,
                    validator: context.validator.clone(),
                    signature: context.record_summary_calculation_signature.clone(),
                };
                assert_noop!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &transaction_call
                    ),
                    InvalidTransaction::BadProof
                );

                // Reuse the signature to execute unsigned record_summary_calculation extrinsic with
                // the same ingress counter
                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        context.last_block_in_range,
                        context.root_hash_h256,
                        context.root_id.ingress_counter,
                        context.validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    ),
                    Error::<TestRuntime>::InvalidIngressCounter
                );

                assert_noop!(
                    Summary::record_summary_calculation(
                        RawOrigin::None.into(),
                        context.last_block_in_range,
                        context.root_hash_h256,
                        context.root_id.ingress_counter + 1,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    Error::<TestRuntime>::SummaryPendingOrApproved
                );
            });
        }
    }
}

mod input_is_end_voting_period {
    use super::*;

    fn expected_valid_end_voting_period_transaction(context: Context) -> TransactionValidity {
        ValidTransaction::with_tag_prefix("vote")
            .priority(TransactionPriority::max_value())
            .and_provides(vec![(
                END_VOTING_PERIOD_CONTEXT,
                context.root_id.encode(),
                context.validator,
            )
                .encode()])
            .longevity(64_u64)
            .propagate(true)
            .build()
    }

    fn get_signature_for_end_voting_period(
        validator: Validator<UintAuthorityId, u64>,
        context: &[u8],
        root_id: &RootId<BlockNumber>,
    ) -> TestSignature {
        validator.key.sign(&(context, root_id.encode()).encode()).expect("Signature is signed")
    }

    #[test]
    fn succeeds() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();

            setup_approved_root(context.clone());

            let signature = get_signature_for_end_voting_period(
                context.validator.clone(),
                END_VOTING_PERIOD_CONTEXT,
                &context.root_id,
            );

            assert_eq!(
                <Summary as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::Local,
                    &crate::Call::end_voting_period {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature
                    }
                ),
                expected_valid_end_voting_period_transaction(context)
            );
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn from_non_validator() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_approved_root(context.clone());

                let non_validator = get_non_validator();
                let non_validator_signed_signature = get_signature_for_end_voting_period(
                    non_validator.clone(),
                    END_VOTING_PERIOD_CONTEXT,
                    &context.root_id,
                );

                assert_noop!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &crate::Call::end_voting_period {
                            root_id: context.root_id,
                            validator: non_validator,
                            signature: non_validator_signed_signature
                        }
                    ),
                    InvalidTransaction::BadProof
                );
            });
        }

        mod signature_has_wrong {
            use super::*;

            #[test]
            fn signer() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                    let signature_signed_by_different_validator =
                        get_signature_for_end_voting_period(
                            second_validator,
                            END_VOTING_PERIOD_CONTEXT,
                            &context.root_id,
                        );

                    let end_voting_period_call = crate::Call::end_voting_period {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_signed_by_different_validator,
                    };

                    test_signature_is_wrong(&context, &end_voting_period_call);
                });
            }

            #[test]
            fn signature_context() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let signature_with_wrong_context = get_signature_for_end_voting_period(
                        context.validator.clone(),
                        OTHER_CONTEXT,
                        &context.root_id,
                    );
                    let end_voting_period_call = crate::Call::end_voting_period {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_with_wrong_context,
                    };

                    test_signature_is_wrong(&context, &end_voting_period_call);
                });
            }

            #[test]
            fn root_id() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let wrong_root_id = RootId::new(
                        RootRange::new(
                            context.root_id.range.from_block + 1,
                            context.root_id.range.to_block + 1,
                        ),
                        context.root_id.ingress_counter,
                    );
                    let signature_with_wrong_root_id = get_signature_for_end_voting_period(
                        context.validator.clone(),
                        END_VOTING_PERIOD_CONTEXT,
                        &wrong_root_id,
                    );
                    let end_voting_period_call = crate::Call::end_voting_period {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_with_wrong_root_id,
                    };

                    test_signature_is_wrong(&context, &end_voting_period_call);
                });
            }
        }

        #[test]
        fn signature_is_reused() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_total_ingresses(&context);
                setup_approved_root(context.clone());

                let signature = get_signature_for_end_voting_period(
                    context.validator.clone(),
                    END_VOTING_PERIOD_CONTEXT,
                    &context.root_id,
                );

                // Validate unsigned end_voting_period extrinsic
                assert_eq!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &crate::Call::end_voting_period {
                            root_id: context.root_id,
                            validator: context.validator.clone(),
                            signature: signature.clone(),
                        }
                    ),
                    expected_valid_end_voting_period_transaction(context.clone())
                );

                // Execute the unsigned end_voting_period extrinsic
                assert!(Summary::end_voting_period(
                    RawOrigin::None.into(),
                    context.root_id,
                    context.validator.clone(),
                    context.record_summary_calculation_signature.clone(),
                )
                .is_ok());

                // Reuse the signature to validate the same unsigned end_voting_period extrinsic
                // again
                let end_voting_period_call = crate::Call::end_voting_period {
                    root_id: context.root_id,
                    validator: context.validator.clone(),
                    signature: signature.clone(),
                };
                assert_noop!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &end_voting_period_call
                    ),
                    InvalidTransaction::Custom(VOTE_SESSION_IS_NOT_VALID)
                );

                // Reuse the signature to validate unsigned end_voting_period extrinsic with a
                // different ingress counter
                let root_id =
                    RootId::new(context.root_id.range, context.root_id.ingress_counter + 1);
                let end_voting_period_call = crate::Call::end_voting_period {
                    root_id,
                    validator: context.validator.clone(),
                    signature,
                };
                assert_noop!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &end_voting_period_call
                    ),
                    InvalidTransaction::Custom(VOTE_SESSION_IS_NOT_VALID)
                );

                // Reuse the signature to execute the same unsigned end_voting_period extrinsic
                // again
                assert_noop!(
                    Summary::end_voting_period(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator.clone(),
                        context.record_summary_calculation_signature
                    ),
                    Error::<TestRuntime>::VotingSessionIsNotValid
                );
            });
        }
    }
}

fn expected_valid_cast_vote_transaction(
    context: Context,
    is_approve_root: bool,
) -> TransactionValidity {
    ValidTransaction::with_tag_prefix("vote")
        .priority(TransactionPriority::max_value())
        .and_provides(vec![(
            CAST_VOTE_CONTEXT,
            context.root_id.encode(),
            is_approve_root,
            context.validator,
        )
            .encode()])
        .longevity(64_u64)
        .propagate(true)
        .build()
}

fn get_cast_vote_call(context: &Context, is_approve_root: bool) -> crate::Call<TestRuntime> {
    if is_approve_root {
        let signature = get_signature_for_approve_cast_vote(
            &context.validator,
            CAST_VOTE_CONTEXT,
            &context.root_id,
        );

        crate::Call::approve_root {
            root_id: context.root_id.clone(),
            validator: context.validator.clone(),
            signature,
        }
    } else {
        let signature = get_signature_for_reject_cast_vote(
            &context.validator,
            CAST_VOTE_CONTEXT,
            &context.root_id,
        );

        crate::Call::reject_root {
            root_id: context.root_id,
            validator: context.validator.clone(),
            signature,
        }
    }
}

fn test_vote_is_successful(is_approve_root: bool) {
    let context = setup_context();
    setup_voting_for_root_id(&context);

    assert_eq!(
        <Summary as ValidateUnsigned>::validate_unsigned(
            TransactionSource::Local,
            &get_cast_vote_call(&context, is_approve_root)
        ),
        expected_valid_cast_vote_transaction(context, is_approve_root)
    );
}

fn test_vote_is_invalid(context: &Context, call: &crate::Call<TestRuntime>, error_code: u8) {
    setup_voting_for_root_id(context);

    assert_noop!(
        <Summary as ValidateUnsigned>::validate_unsigned(TransactionSource::Local, call),
        InvalidTransaction::Custom(error_code)
    );
}

fn test_vote_is_invalid_when_root_is_not_pending_approval(error_code: u8, is_approve_root: bool) {
    let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
        .with_validators()
        .for_offchain_worker()
        .as_externality_with_state();

    ext.execute_with(|| {
        let context = setup_context();

        setup_voting_for_root_id(&context);
        Summary::remove_pending_approval(&context.root_id.range);

        assert_noop!(
            <Summary as ValidateUnsigned>::validate_unsigned(
                TransactionSource::Local,
                &get_cast_vote_call(&context, is_approve_root)
            ),
            InvalidTransaction::Custom(error_code)
        );
    });
}

fn test_vote_is_invalid_root_is_not_registered_for_voting(
    call: &crate::Call<TestRuntime>,
    error_code: u8,
) {
    let context = setup_context();

    setup_voting_for_root_id(&context);
    Summary::deregister_root_for_voting(&context.root_id);

    assert_noop!(
        <Summary as ValidateUnsigned>::validate_unsigned(TransactionSource::Local, call),
        InvalidTransaction::Custom(error_code)
    );
}

fn test_vote_is_invalid_when_validator_has_voted_already(
    context: &Context,
    error_code: u8,
    call: &crate::Call<TestRuntime>,
) {
    setup_voting_for_root_id(context);
    Summary::record_approve_vote(&context.root_id, context.validator.account_id);

    assert_noop!(
        <Summary as ValidateUnsigned>::validate_unsigned(TransactionSource::Local, call),
        InvalidTransaction::Custom(error_code)
    );
}

fn test_signature_is_wrong(context: &Context, call: &crate::Call<TestRuntime>) {
    setup_total_ingresses(&context);
    setup_voting_for_root_id(context);

    assert_noop!(
        <Summary as ValidateUnsigned>::validate_unsigned(TransactionSource::Local, call),
        InvalidTransaction::BadProof
    );
}

mod input_is_approve_root {
    use super::*;

    #[test]
    fn succeeds() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();

        ext.execute_with(|| {
            test_vote_is_successful(true);
        });
    }

    mod fails_when {
        use super::*;

        mod vote_is_invalid {
            use super::*;

            #[test]
            fn for_non_validator() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let non_validator = get_non_validator();
                    let signature = get_signature_for_approve_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id,
                    );
                    let approve_root_call = crate::Call::approve_root {
                        root_id: context.root_id,
                        validator: non_validator,
                        signature,
                    };

                    test_vote_is_invalid(&context, &approve_root_call, APPROVE_VOTE_IS_NOT_VALID);
                });
            }

            #[test]
            fn when_root_is_not_pending_approval() {
                test_vote_is_invalid_when_root_is_not_pending_approval(
                    APPROVE_VOTE_IS_NOT_VALID,
                    true,
                );
            }

            #[test]
            fn root_is_not_registered_for_voting() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let signature = get_signature_for_approve_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id,
                    );
                    let approve_root_call = crate::Call::approve_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature,
                    };

                    test_vote_is_invalid_root_is_not_registered_for_voting(
                        &approve_root_call,
                        APPROVE_VOTE_IS_NOT_VALID,
                    );
                });
            }

            #[test]
            fn when_validator_has_voted_already() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let signature = get_signature_for_approve_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id,
                    );
                    let approve_root_call = crate::Call::approve_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature,
                    };

                    test_vote_is_invalid_when_validator_has_voted_already(
                        &context,
                        APPROVE_VOTE_IS_NOT_VALID,
                        &approve_root_call,
                    );
                });
            }
        }

        mod signature_has_wrong {
            use super::*;

            #[test]
            fn signer() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                    let signature_signed_by_different_validator =
                        get_signature_for_approve_cast_vote(
                            &second_validator,
                            CAST_VOTE_CONTEXT,
                            &context.root_id,
                        );
                    let approve_root_call = crate::Call::approve_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_signed_by_different_validator,
                    };

                    test_signature_is_wrong(&context, &approve_root_call);
                });
            }

            #[test]
            fn signature_context() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let signature_with_wrong_context = get_signature_for_approve_cast_vote(
                        &context.validator,
                        OTHER_CONTEXT,
                        &context.root_id,
                    );
                    let approve_root_call = crate::Call::approve_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_with_wrong_context,
                    };

                    test_signature_is_wrong(&context, &approve_root_call);
                });
            }

            #[test]
            fn root_id() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let wrong_root_id = RootId::new(
                        RootRange::new(
                            context.root_id.range.from_block + 1,
                            context.root_id.range.to_block + 1,
                        ),
                        context.root_id.ingress_counter,
                    );
                    let signature_with_wrong_root_id = get_signature_for_approve_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &wrong_root_id,
                    );
                    let approve_root_call = crate::Call::approve_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_with_wrong_root_id,
                    };

                    test_signature_is_wrong(&context, &approve_root_call);
                });
            }

            #[test]
            fn approve_root() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let signature_with_wrong_approve_root = get_signature_for_reject_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id,
                    );
                    let approve_root_call = crate::Call::approve_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_with_wrong_approve_root,
                    };

                    test_signature_is_wrong(&context, &approve_root_call);
                });
            }
        }

        #[test]
        fn signature_is_reused() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_total_ingresses(&context);
                setup_voting_for_root_id(&context);

                let signature = get_signature_for_approve_cast_vote(
                    &context.validator,
                    CAST_VOTE_CONTEXT,
                    &context.root_id,
                );

                // Validate unsigned approve_vote extrinsic
                let approve_root_call = crate::Call::approve_root {
                    root_id: context.root_id,
                    validator: context.validator.clone(),
                    signature: signature.clone(),
                };
                assert_eq!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &approve_root_call
                    ),
                    expected_valid_cast_vote_transaction(context.clone(), true)
                );

                // Execute the unsigned approve_root extrinsic
                assert_eq!(true, vote_to_approve_root(&context.validator, &context));

                // Reuse the signature to validate the same unsigned approve_vote extrinsic again
                assert_noop!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &approve_root_call
                    ),
                    InvalidTransaction::Custom(APPROVE_VOTE_IS_NOT_VALID)
                );

                // Reuse the signature to validate unsigned approve_root extrinsic with a different
                // ingress counter
                let root_id =
                    RootId::new(context.root_id.range, context.root_id.ingress_counter + 1);
                let approve_root_call = crate::Call::approve_root {
                    root_id,
                    validator: context.validator.clone(),
                    signature: signature.clone(),
                };
                assert_noop!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &approve_root_call
                    ),
                    InvalidTransaction::Custom(ERROR_CODE_INVALID_ROOT_RANGE)
                );

                // Reuse the signature to execute the same unsigned approve_root extrinsic again
                assert_noop!(
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature,
                    ),
                    AvNError::<TestRuntime>::DuplicateVote
                );
            });
        }
    }
}

mod input_is_reject_root {
    use super::*;

    #[test]
    fn succeeds() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();

        ext.execute_with(|| {
            test_vote_is_successful(false);
        });
    }

    mod fails_when {
        use super::*;

        mod vote_is_invalid {
            use super::*;

            #[test]
            fn for_non_validator() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let non_validator = get_non_validator();
                    let signature = get_signature_for_reject_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id,
                    );
                    let reject_root_call = crate::Call::reject_root {
                        root_id: context.root_id,
                        validator: non_validator,
                        signature,
                    };

                    test_vote_is_invalid(&context, &reject_root_call, REJECT_VOTE_IS_NOT_VALID);
                });
            }

            #[test]
            fn when_root_is_not_pending_approval() {
                test_vote_is_invalid_when_root_is_not_pending_approval(
                    REJECT_VOTE_IS_NOT_VALID,
                    false,
                );
            }

            #[test]
            fn root_is_not_registered_for_voting() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let signature = get_signature_for_reject_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id,
                    );
                    let reject_root_call = crate::Call::reject_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature,
                    };

                    test_vote_is_invalid_root_is_not_registered_for_voting(
                        &reject_root_call,
                        REJECT_VOTE_IS_NOT_VALID,
                    );
                });
            }

            #[test]
            fn when_validator_has_voted_already() {
                let mut ext = ExtBuilder::build_default().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let reject_root_call = crate::Call::reject_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: get_signature_for_reject_cast_vote(
                            &context.validator,
                            CAST_VOTE_CONTEXT,
                            &context.root_id,
                        ),
                    };

                    test_vote_is_invalid_when_validator_has_voted_already(
                        &context,
                        REJECT_VOTE_IS_NOT_VALID,
                        &reject_root_call,
                    );
                });
            }
        }

        mod signature_has_wrong {
            use super::*;

            #[test]
            fn signer() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                    let signature_signed_by_different_validator =
                        get_signature_for_reject_cast_vote(
                            &second_validator,
                            CAST_VOTE_CONTEXT,
                            &context.root_id,
                        );
                    let reject_root_call = crate::Call::reject_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_signed_by_different_validator,
                    };

                    test_signature_is_wrong(&context, &reject_root_call);
                });
            }

            #[test]
            fn signature_context() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let signature_with_wrong_context = get_signature_for_reject_cast_vote(
                        &context.validator,
                        OTHER_CONTEXT,
                        &context.root_id,
                    );
                    let reject_root_call = crate::Call::reject_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_with_wrong_context,
                    };

                    test_signature_is_wrong(&context, &reject_root_call);
                });
            }

            #[test]
            fn root_id() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let wrong_root_id = RootId::new(
                        RootRange::new(
                            context.root_id.range.from_block + 1,
                            context.root_id.range.to_block + 1,
                        ),
                        context.root_id.ingress_counter,
                    );
                    let signature_with_wrong_root_id = get_signature_for_reject_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &wrong_root_id,
                    );
                    let reject_root_call = crate::Call::reject_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_with_wrong_root_id,
                    };

                    test_signature_is_wrong(&context, &reject_root_call);
                });
            }

            #[test]
            fn reject_root() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = setup_context();
                    let signature_with_wrong_reject_root = get_signature_for_approve_cast_vote(
                        &context.validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id,
                    );
                    let reject_root_call = crate::Call::reject_root {
                        root_id: context.root_id,
                        validator: context.validator.clone(),
                        signature: signature_with_wrong_reject_root,
                    };

                    test_signature_is_wrong(&context, &reject_root_call);
                });
            }
        }

        #[test]
        fn signature_is_reused() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_total_ingresses(&context);
                setup_voting_for_root_id(&context);

                let signature = get_signature_for_reject_cast_vote(
                    &context.validator,
                    CAST_VOTE_CONTEXT,
                    &context.root_id,
                );

                // Validate unsigned reject_vote extrinsic
                let reject_root_call = crate::Call::reject_root {
                    root_id: context.root_id,
                    validator: context.validator.clone(),
                    signature: signature.clone(),
                };
                assert_eq!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &reject_root_call
                    ),
                    expected_valid_cast_vote_transaction(context.clone(), false)
                );

                // Execute the unsigned reject_root extrinsic
                assert_eq!(true, vote_to_reject_root(&context.validator, &context));

                // Reuse the signature to validate the same unsigned reject_vote extrinsic again
                assert_noop!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &reject_root_call
                    ),
                    InvalidTransaction::Custom(REJECT_VOTE_IS_NOT_VALID)
                );

                // Reuse the signature to validate unsigned reject_root extrinsic with a different
                // ingress counter
                let root_id =
                    RootId::new(context.root_id.range, context.root_id.ingress_counter + 1);
                let reject_root_call = crate::Call::reject_root {
                    root_id,
                    validator: context.validator.clone(),
                    signature: signature.clone(),
                };
                assert_noop!(
                    <Summary as ValidateUnsigned>::validate_unsigned(
                        TransactionSource::Local,
                        &reject_root_call
                    ),
                    InvalidTransaction::Custom(REJECT_VOTE_IS_NOT_VALID)
                );

                // Reuse the signature to execute the same unsigned reject_root extrinsic again
                assert_noop!(
                    Summary::reject_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature,
                    ),
                    AvNError::<TestRuntime>::DuplicateVote
                );
            });
        }
    }
}

pub fn expected_valid_record_summary_calculation_transaction(
    root_hash: H256,
    ingress_counter: IngressCounter,
) -> TransactionValidity {
    ValidTransaction::with_tag_prefix("Summary")
        .priority(TransactionPriority::max_value())
        .and_provides(vec![
            (&Summary::update_block_number_context(), root_hash, ingress_counter).encode()
        ])
        .longevity(64_u64)
        .propagate(true)
        .build()
}

pub fn assert_validate_unsigned_record_summary_calculation_is_successful(context: &Context) {
    let transaction_call = crate::Call::record_summary_calculation {
        new_block_number: context.last_block_in_range,
        root_hash: context.root_hash_h256.clone(),
        ingress_counter: context.root_id.ingress_counter,
        validator: context.validator.clone(),
        signature: context.record_summary_calculation_signature.clone(),
    };
    assert_eq!(
        <Summary as ValidateUnsigned>::validate_unsigned(
            TransactionSource::Local,
            &transaction_call
        ),
        expected_valid_record_summary_calculation_transaction(
            context.root_hash_h256,
            context.root_id.ingress_counter
        )
    );
}
