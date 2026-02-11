// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{mock::*, system};
use frame_support::{assert_noop, assert_ok};
use pallet_avn::Error as AvNError;
use sp_runtime::{generic::Preamble, testing::UintAuthorityId, traits::BadOrigin};
use system::RawOrigin;

fn setup_voting_for_root_id(context: &Context) {
    setup_blocks(&context);

    Summary::insert_root_hash(
        &context.root_id,
        context.root_hash_h256,
        context.validator.account_id.clone(),
        context.tx_id,
    );
    Summary::insert_pending_approval(&context.root_id);
    Summary::register_root_for_voting(&context.root_id, QUORUM, VOTING_PERIOD_END);

    assert_eq!(Summary::get_vote(context.root_id).ayes.is_empty(), true);
    assert_eq!(Summary::get_vote(context.root_id).nays.is_empty(), true);
}

pub fn setup_approved_root(context: Context) {
    setup_voting_for_root_id(&context);

    let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
    let third_validator = get_validator(THIRD_VALIDATOR_INDEX);
    Summary::record_approve_vote(&context.root_id, context.validator.account_id);
    Summary::record_approve_vote(&context.root_id, second_validator.account_id);
    Summary::record_approve_vote(&context.root_id, third_validator.account_id);
}

pub fn vote_to_approve_root(
    validator: &Validator<UintAuthorityId, u64>,
    context: &Context,
) -> bool {
    set_mock_recovered_account_id(validator.account_id);
    Summary::approve_root(
        RawOrigin::None.into(),
        context.root_id,
        validator.clone(),
        context.record_summary_calculation_signature.clone(),
    )
    .is_ok()
}

pub fn vote_to_reject_root(validator: &Validator<UintAuthorityId, u64>, context: &Context) -> bool {
    Summary::reject_root(
        RawOrigin::None.into(),
        context.root_id,
        validator.clone(),
        context.record_summary_calculation_signature.clone(),
    )
    .is_ok()
}

// TODO [TYPE: test][PRI: medium][JIRA: 321]
// Refactor the approve_root and reject_root tests so common codes can be shared
mod approve_root {
    use super::*;

    mod succeeds {
        use super::*;

        #[test]
        fn when_one_validator_votes() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();
                setup_voting_for_root_id(&context);

                assert!(Summary::approve_root(
                    RawOrigin::None.into(),
                    context.root_id,
                    context.validator.clone(),
                    context.record_summary_calculation_signature.clone()
                )
                .is_ok());

                assert_eq!(
                    Summary::get_vote(context.root_id).ayes,
                    vec![context.validator.account_id]
                );
                assert_eq!(Summary::get_vote(context.root_id).nays.is_empty(), true);

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VoteAdded {
                        voter: context.validator.account_id,
                        root_id: context.root_id,
                        agree_vote: true
                    })));
            });
        }

        #[test]
        fn when_two_validators_vote_differently() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                assert!(vote_to_reject_root(&context.validator, &context));
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                set_mock_recovered_account_id(second_validator.account_id);

                assert_eq!(
                    Result::Ok(()),
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        second_validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    )
                );

                assert_eq!(
                    Summary::get_vote(&(context.root_id)).ayes,
                    vec![second_validator.account_id]
                );
                assert_eq!(
                    Summary::get_vote(&(context.root_id)).nays,
                    vec![context.validator.account_id]
                );

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VoteAdded {
                        voter: second_validator.account_id,
                        root_id: context.root_id,
                        agree_vote: true
                    })));
            });
        }

        #[test]
        fn when_two_validators_vote_the_same() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                assert!(vote_to_approve_root(&context.validator, &context));
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                set_mock_recovered_account_id(second_validator.account_id);

                assert_eq!(
                    Result::Ok(()),
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        second_validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    )
                );

                assert_eq!(
                    Summary::get_vote(context.root_id).ayes,
                    vec![context.validator.account_id, second_validator.account_id]
                );
                assert_eq!(Summary::get_vote(context.root_id).nays.is_empty(), true);

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VoteAdded {
                        voter: second_validator.account_id,
                        root_id: context.root_id,
                        agree_vote: true
                    })));
            });
        }

        #[test]
        fn when_voting_is_not_finished() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                let third_validator = get_validator(THIRD_VALIDATOR_INDEX);
                assert!(vote_to_approve_root(&context.validator, &context));
                assert!(vote_to_reject_root(&second_validator, &context));

                set_mock_recovered_account_id(third_validator.account_id);
                assert_eq!(
                    Result::Ok(()),
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        third_validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    )
                );

                assert_eq!(
                    Summary::get_vote(context.root_id).ayes,
                    vec![context.validator.account_id, third_validator.account_id]
                );
                assert_eq!(
                    Summary::get_vote(context.root_id).nays,
                    vec![second_validator.account_id]
                );

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VoteAdded {
                        voter: third_validator.account_id,
                        root_id: context.root_id,
                        agree_vote: true
                    })));
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

                setup_voting_for_root_id(&context);

                assert_noop!(
                    Summary::approve_root(
                        RuntimeOrigin::signed(Default::default()),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    BadOrigin
                );
            });
        }

        #[test]
        fn when_root_is_not_in_pending_approval() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                Summary::remove_pending_approval(&context.root_id.range);

                assert_noop!(
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    AvNError::<TestRuntime>::InvalidVote
                );
            });
        }

        #[test]
        fn when_root_is_not_setup_for_voting() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                Summary::register_root_for_voting(&context.root_id, QUORUM, VOTING_PERIOD_END);
                Summary::deregister_root_for_voting(&context.root_id);

                assert_noop!(
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    Error::<TestRuntime>::RootDataNotFound
                );
            });
        }

        #[test]
        fn when_voter_has_already_approved() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                Summary::record_approve_vote(&context.root_id, context.validator.account_id);

                assert_noop!(
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    AvNError::<TestRuntime>::DuplicateVote
                );
            });
        }

        #[test]
        fn when_voter_has_already_rejected() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                Summary::record_reject_vote(&context.root_id, context.validator.account_id);

                assert_noop!(
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    AvNError::<TestRuntime>::DuplicateVote
                );
            });
        }

        #[test]
        fn when_voting_is_finished() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                let third_validator = get_validator(THIRD_VALIDATOR_INDEX);
                let fourth_validator = get_validator(FOURTH_VALIDATOR_INDEX);
                assert!(vote_to_reject_root(&context.validator, &context));
                assert!(vote_to_reject_root(&second_validator, &context));
                assert!(vote_to_reject_root(&third_validator, &context));

                set_mock_recovered_account_id(fourth_validator.account_id);
                assert_noop!(
                    Summary::approve_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        fourth_validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    ),
                    AvNError::<TestRuntime>::InvalidVote
                );
            });
        }
    }
}

mod reject_root {
    use super::*;

    mod succeeds {
        use super::*;

        #[test]
        fn when_one_validator_votes() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);

                assert!(Summary::reject_root(
                    RawOrigin::None.into(),
                    context.root_id,
                    context.validator.clone(),
                    context.record_summary_calculation_signature.clone()
                )
                .is_ok());

                assert_eq!(Summary::get_vote(context.root_id).ayes.is_empty(), true);
                assert_eq!(
                    Summary::get_vote(context.root_id).nays,
                    vec![context.validator.account_id]
                );

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VoteAdded {
                        voter: context.validator.account_id,
                        root_id: context.root_id,
                        agree_vote: false
                    })));
            });
        }

        #[test]
        fn when_two_validators_vote_differently() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                assert!(vote_to_approve_root(&context.validator, &context));
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);

                assert!(Summary::reject_root(
                    RawOrigin::None.into(),
                    context.root_id,
                    second_validator.clone(),
                    context.record_summary_calculation_signature.clone()
                )
                .is_ok());

                assert_eq!(
                    Summary::get_vote(context.root_id).ayes,
                    vec![context.validator.account_id]
                );
                assert_eq!(
                    Summary::get_vote(&(context.root_id)).nays,
                    vec![second_validator.account_id]
                );

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VoteAdded {
                        voter: second_validator.account_id,
                        root_id: context.root_id,
                        agree_vote: false
                    })));
            });
        }

        #[test]
        fn when_two_validators_vote_the_same() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                assert!(vote_to_reject_root(&context.validator, &context));
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);

                assert!(Summary::reject_root(
                    RawOrigin::None.into(),
                    context.root_id,
                    second_validator.clone(),
                    context.record_summary_calculation_signature.clone()
                )
                .is_ok());

                assert_eq!(Summary::get_vote(context.root_id).ayes.is_empty(), true);
                assert_eq!(
                    Summary::get_vote(context.root_id).nays,
                    vec![context.validator.account_id, second_validator.account_id]
                );

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VoteAdded {
                        voter: second_validator.account_id,
                        root_id: context.root_id,
                        agree_vote: false
                    })));
            });
        }

        #[test]
        fn when_voting_is_not_finished() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                let third_validator = get_validator(THIRD_VALIDATOR_INDEX);
                assert!(vote_to_approve_root(&context.validator, &context));
                assert!(vote_to_reject_root(&second_validator, &context));

                assert!(Summary::reject_root(
                    RawOrigin::None.into(),
                    context.root_id,
                    third_validator.clone(),
                    context.record_summary_calculation_signature.clone()
                )
                .is_ok());

                assert_eq!(
                    Summary::get_vote(context.root_id).ayes,
                    vec![context.validator.account_id]
                );
                assert_eq!(
                    Summary::get_vote(context.root_id).nays,
                    vec![second_validator.account_id, third_validator.account_id]
                );

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VoteAdded {
                        voter: third_validator.account_id,
                        root_id: context.root_id,
                        agree_vote: false
                    })));
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

                setup_voting_for_root_id(&context);

                assert_noop!(
                    Summary::reject_root(
                        RuntimeOrigin::signed(Default::default()),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    BadOrigin
                );
            });
        }

        #[test]
        fn when_voter_is_invalid_validator() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);

                assert_noop!(
                    Summary::reject_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        get_non_validator(),
                        context.record_summary_calculation_signature
                    ),
                    AvNError::<TestRuntime>::NotAValidator
                );
            });
        }

        #[test]
        fn when_root_is_not_in_pending_approval() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                Summary::remove_pending_approval(&context.root_id.range);

                assert_noop!(
                    Summary::reject_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    AvNError::<TestRuntime>::InvalidVote
                );
            });
        }

        #[test]
        fn when_root_is_not_setup_for_voting() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                Summary::register_root_for_voting(&context.root_id, QUORUM, VOTING_PERIOD_END);
                Summary::deregister_root_for_voting(&context.root_id);

                assert_noop!(
                    Summary::reject_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    AvNError::<TestRuntime>::InvalidVote
                );
            });
        }

        #[test]
        fn when_voter_has_already_rejected() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                Summary::record_reject_vote(&context.root_id, context.validator.account_id);

                assert_noop!(
                    Summary::reject_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    AvNError::<TestRuntime>::DuplicateVote
                );
            });
        }

        #[test]
        fn when_voter_has_already_approved() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                Summary::record_approve_vote(&context.root_id, context.validator.account_id);

                assert_noop!(
                    Summary::reject_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator,
                        context.record_summary_calculation_signature
                    ),
                    AvNError::<TestRuntime>::DuplicateVote
                );
            });
        }

        #[test]
        fn when_voting_is_finished() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                let third_validator = get_validator(THIRD_VALIDATOR_INDEX);
                let fourth_validator = get_validator(FOURTH_VALIDATOR_INDEX);
                assert!(vote_to_approve_root(&context.validator, &context));
                assert!(vote_to_approve_root(&second_validator, &context));
                assert!(vote_to_approve_root(&third_validator, &context));

                assert_noop!(
                    Summary::reject_root(
                        RawOrigin::None.into(),
                        context.root_id,
                        fourth_validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    ),
                    AvNError::<TestRuntime>::InvalidVote
                );
            });
        }
    }
}

mod cast_votes_if_required {
    use super::*;

    mod does_not_send_transactions {
        use super::*;

        #[test]
        fn when_setting_lock_with_expiry_has_error() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_voting_for_root_id(&context);

                // TODO [TYPE: test][PRI: medium][JIRA: 321]: mock of set_lock_with_expiry returns
                // error
                let lock_name = vote::create_vote_lock_name::<TestRuntime, ()>(&context.root_id);
                let mut lock = Avn::get_ocw_locker(&lock_name);

                // Protect against sending more than once. When guard is out of scope the lock will
                // be released.
                if let Ok(_guard) = lock.try_lock() {
                    let second_validator = get_validator(SECOND_VALIDATOR_INDEX);
                    cast_votes_if_required::<TestRuntime, ()>(&second_validator);

                    assert!(pool_state.read().transactions.is_empty());
                };
            });
        }

        #[test]
        fn when_getting_root_hash_has_error() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
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

                setup_voting_for_root_id(&context);
                let second_validator = get_validator(SECOND_VALIDATOR_INDEX);

                cast_votes_if_required::<TestRuntime, ()>(&second_validator);

                assert!(pool_state.read().transactions.is_empty());
            });
        }
    }

    #[test]
    fn sends_approve_vote_transaction_when_root_hash_is_valid() {
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

            setup_voting_for_root_id(&context);
            let second_validator = get_validator(SECOND_VALIDATOR_INDEX);

            cast_votes_if_required::<TestRuntime, ()>(&second_validator);

            let tx = pool_state.write().transactions.pop().unwrap();
            assert!(pool_state.read().transactions.is_empty());
            let tx = Extrinsic::decode(&mut &*tx).unwrap();
            assert!(matches!(tx.preamble, Preamble::Bare(_)));

            assert_eq!(
                tx.function,
                mock::RuntimeCall::Summary(crate::Call::approve_root {
                    root_id: context.root_id,
                    validator: second_validator.clone(),
                    signature: get_signature_for_approve_cast_vote(
                        &second_validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id,
                    )
                })
            );
        });
    }

    #[test]
    fn sends_reject_vote_transaction_when_root_hash_is_invalid() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();

            mock_response_of_get_roothash(
                &mut offchain_state.write(),
                context.url_param.clone(),
                Some(b"c9e671fe581fe4ef46311128724c281ece8ad94c8c30382978736dcdecae163e".to_vec()),
            );

            setup_voting_for_root_id(&context);
            let second_validator = get_validator(SECOND_VALIDATOR_INDEX);

            cast_votes_if_required::<TestRuntime, ()>(&second_validator);

            let tx = pool_state.write().transactions.pop().unwrap();
            assert!(pool_state.read().transactions.is_empty());
            let tx = Extrinsic::decode(&mut &*tx).unwrap();
            assert!(matches!(tx.preamble, Preamble::Bare(_)));

            assert_eq!(
                tx.function,
                mock::RuntimeCall::Summary(crate::Call::reject_root {
                    root_id: context.root_id,
                    validator: second_validator.clone(),
                    signature: get_signature_for_reject_cast_vote(
                        &second_validator,
                        CAST_VOTE_CONTEXT,
                        &context.root_id
                    )
                })
            );
        });
    }
}

mod end_voting_period {
    use super::*;

    mod succeeds {
        use super::*;

        #[test]
        fn when_a_vote_reached_quorum() {
            let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_approved_root(context.clone());
                Summary::set_current_slot(10);
                Summary::set_previous_summary_slot(5);

                let primary_validator_id =
                    Avn::calculate_primary_validator_for_block(context.current_block_number)
                        .expect("Should be able to calculate primary validator.");
                let primary_validator = get_validator(primary_validator_id);

                assert_ok!(Summary::end_voting_period(
                    RawOrigin::None.into(),
                    context.root_id,
                    primary_validator.clone(),
                    context.record_summary_calculation_signature.clone(),
                ));
                assert!(Summary::get_root_data(&context.root_id).is_validated);
                assert!(!PendingApproval::<TestRuntime>::contains_key(&context.root_id.range));
                assert_eq!(
                    Summary::get_next_block_to_process(),
                    context.next_block_to_process + Summary::schedule_period()
                );
                assert_eq!(Summary::last_summary_slot(), Summary::current_slot());

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(
                        crate::Event::<TestRuntime>::SummaryRootValidated {
                            block_range: context.root_id.range,
                            root_hash: context.root_hash_h256,
                            ingress_counter: context.root_id.ingress_counter
                        }
                    )));

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VotingEnded {
                        root_id: context.root_id,
                        vote_approved: true
                    })));
            });
        }

        #[test]
        fn when_end_of_voting_period_passed() {
            let (mut ext, _pool_state, _offchain_state) =
                ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                Summary::set_current_slot(10);
                let previous_slot = 5;
                Summary::set_previous_summary_slot(previous_slot);

                setup_voting_for_root_id(&context);
                System::set_block_number(50);

                assert!(Summary::end_voting_period(
                    RawOrigin::None.into(),
                    context.root_id,
                    context.validator.clone(),
                    context.record_summary_calculation_signature.clone(),
                )
                .is_ok());
                assert!(!Summary::get_root_data(&context.root_id).is_validated);
                assert!(!PendingApproval::<TestRuntime>::contains_key(&context.root_id.range));
                assert_eq!(Summary::get_next_block_to_process(), context.next_block_to_process);
                assert_eq!(Summary::last_summary_slot(), previous_slot);

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::Summary(crate::Event::<TestRuntime>::VotingEnded {
                        root_id: context.root_id,
                        vote_approved: false
                    })));
            });
        }
    }

    mod fails {
        use super::*;

        #[test]
        fn when_origin_is_signed() {
            let (mut ext, _pool_state, _offchain_state) =
                ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();

            ext.execute_with(|| {
                let context = setup_context();

                setup_approved_root(context.clone());

                assert_noop!(
                    Summary::end_voting_period(
                        RuntimeOrigin::signed(Default::default()),
                        context.root_id,
                        context.validator.clone(),
                        context.record_summary_calculation_signature.clone(),
                    ),
                    BadOrigin
                );
            });
        }

        mod when_end_voting {
            use super::*;

            #[test]
            fn root_is_not_setup_for_votes() {
                let (mut ext, _pool_state, _offchain_state) =
                    ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_context();

                    setup_approved_root(context.clone());
                    Summary::deregister_root_for_voting(&context.root_id);

                    assert_noop!(
                        Summary::end_voting_period(
                            RawOrigin::None.into(),
                            context.root_id,
                            context.validator.clone(),
                            context.record_summary_calculation_signature.clone(),
                        ),
                        Error::<TestRuntime>::VotingSessionIsNotValid
                    );
                });
            }

            #[test]
            fn cannot_end_vote() {
                let (mut ext, _pool_state, _offchain_state) =
                    ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();

                ext.execute_with(|| {
                    let context = setup_context();

                    setup_voting_for_root_id(&context);

                    assert_noop!(
                        Summary::end_voting_period(
                            RawOrigin::None.into(),
                            context.root_id,
                            context.validator.clone(),
                            context.record_summary_calculation_signature.clone(),
                        ),
                        Error::<TestRuntime>::ErrorEndingVotingPeriod
                    );
                });
            }

            // More tests after the TODO when we didn't get enough votes to approve this root has
            // been implemented
        }
    }

    mod offence_logic {
        use super::*;

        const TEST_VALIDATOR_COUNT: u64 = 7;

        fn validator_indices() -> Vec<ValidatorId> {
            return (1..=TEST_VALIDATOR_COUNT).collect::<Vec<ValidatorId>>()
        }

        mod when_root_is_approved {
            use super::*;

            fn setup_approved_root(
                context: &Context,
            ) -> (
                Vec<ValidatorId>, // ayes
                Vec<ValidatorId>, // nays
            ) {
                let indices = validator_indices();

                let aye_validator_1 = get_validator(indices[0]).account_id;
                let aye_validator_2 = get_validator(indices[1]).account_id;
                let aye_validator_3 = get_validator(indices[2]).account_id;
                let nay_validator_1 = get_validator(indices[3]).account_id;
                let nay_validator_2 = get_validator(indices[4]).account_id;
                assert_eq!(context.validator.account_id, aye_validator_1);

                Summary::record_approve_vote(&context.root_id, aye_validator_1);
                Summary::record_reject_vote(&context.root_id, nay_validator_1);
                Summary::record_approve_vote(&context.root_id, aye_validator_2);
                Summary::record_reject_vote(&context.root_id, nay_validator_2);
                Summary::record_approve_vote(&context.root_id, aye_validator_3);

                return (
                    vec![aye_validator_1, aye_validator_2, aye_validator_3],
                    vec![nay_validator_1, nay_validator_2],
                )
            }

            #[test]
            fn reports_offence_for_nay_voters_only() {
                let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validator_count(TEST_VALIDATOR_COUNT)
                    .for_offchain_worker()
                    .as_externality_with_state();
                ext.execute_with(|| {
                    let active_validators = Avn::validators();
                    assert_eq!(active_validators.len(), TEST_VALIDATOR_COUNT as usize);

                    let context = setup_context();
                    setup_voting_for_root_id(&context);
                    let (_ayes, nays) = setup_approved_root(&context);

                    Summary::set_current_slot(10);
                    Summary::set_previous_summary_slot(5);

                    assert_ok!(Summary::end_voting_period(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    ));
                    assert_eq!(true, Summary::get_vote(context.root_id).has_outcome());
                    assert_eq!(true, Summary::get_root_data(&context.root_id).is_validated);
                    assert_eq!(true, Summary::get_vote(context.root_id).is_approved());

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            context.validator.account_id,
                            TEST_VALIDATOR_COUNT.try_into().unwrap(),
                            vec![nays[0], nays[1]],
                            SummaryOffenceType::RejectedValidRoot
                        )
                    );
                    assert_eq!(
                        false,
                        Summary::reported_offence_of_type(SummaryOffenceType::CreatedInvalidRoot)
                    );
                    assert_eq!(
                        false,
                        Summary::reported_offence_of_type(SummaryOffenceType::ApprovedInvalidRoot)
                    );
                });
            }
        }

        mod when_root_is_rejected {
            use super::*;

            fn setup_rejected_root(
                context: &Context,
            ) -> (
                Vec<ValidatorId>, // ayes
                Vec<ValidatorId>, // nays
            ) {
                let indices = validator_indices();

                let aye_validator_1 = get_validator(indices[0]).account_id;
                let aye_validator_2 = get_validator(indices[1]).account_id;
                let nay_validator_1 = get_validator(indices[2]).account_id;
                let nay_validator_2 = get_validator(indices[3]).account_id;
                let nay_validator_3 = get_validator(indices[4]).account_id;
                assert_eq!(context.validator.account_id, aye_validator_1);

                Summary::record_approve_vote(&context.root_id, aye_validator_1);
                Summary::record_approve_vote(&context.root_id, aye_validator_2);
                Summary::record_reject_vote(&context.root_id, nay_validator_1);
                Summary::record_reject_vote(&context.root_id, nay_validator_2);
                Summary::record_reject_vote(&context.root_id, nay_validator_3);

                return (
                    vec![aye_validator_1, aye_validator_2],
                    vec![nay_validator_1, nay_validator_2, nay_validator_3],
                )
            }

            #[test]
            fn reports_offence_for_root_creator() {
                let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validator_count(TEST_VALIDATOR_COUNT)
                    .for_offchain_worker()
                    .as_externality_with_state();
                ext.execute_with(|| {
                    let active_validators = Avn::validators();
                    assert_eq!(active_validators.len(), TEST_VALIDATOR_COUNT as usize);

                    let context = setup_context();
                    setup_voting_for_root_id(&context);
                    let (_ayes, _nays) = setup_rejected_root(&context);

                    Summary::set_current_slot(10);
                    Summary::set_previous_summary_slot(5);

                    assert_ok!(Summary::end_voting_period(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    ));
                    assert_eq!(true, Summary::get_vote(context.root_id).has_outcome());
                    assert_eq!(false, Summary::get_root_data(&context.root_id).is_validated);
                    assert_eq!(false, Summary::get_vote(context.root_id).is_approved());

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            context.validator.account_id,
                            TEST_VALIDATOR_COUNT.try_into().unwrap(),
                            vec![context.validator.account_id],
                            SummaryOffenceType::CreatedInvalidRoot
                        )
                    );
                });
            }

            #[test]
            fn reports_offence_for_aye_voters_only() {
                let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validator_count(TEST_VALIDATOR_COUNT)
                    .for_offchain_worker()
                    .as_externality_with_state();
                ext.execute_with(|| {
                    let active_validators = Avn::validators();
                    assert_eq!(active_validators.len(), TEST_VALIDATOR_COUNT as usize);

                    let context = setup_context();
                    setup_voting_for_root_id(&context);
                    let (ayes, _nays) = setup_rejected_root(&context);

                    Summary::set_current_slot(10);
                    Summary::set_previous_summary_slot(5);

                    assert_ok!(Summary::end_voting_period(
                        RawOrigin::None.into(),
                        context.root_id,
                        context.validator.clone(),
                        context.record_summary_calculation_signature.clone()
                    ));
                    assert_eq!(true, Summary::get_vote(context.root_id).has_outcome());
                    assert_eq!(false, Summary::get_root_data(&context.root_id).is_validated);
                    assert_eq!(false, Summary::get_vote(context.root_id).is_approved());

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            context.validator.account_id,
                            TEST_VALIDATOR_COUNT.try_into().unwrap(),
                            vec![ayes[0], ayes[1]],
                            SummaryOffenceType::ApprovedInvalidRoot
                        )
                    );
                    assert_eq!(
                        false,
                        Summary::reported_offence_of_type(SummaryOffenceType::RejectedValidRoot)
                    );
                });
            }
        }

        mod when_vote_has_no_outcome {
            use super::*;

            fn setup_root_without_outcome(
                context: &Context,
            ) -> (
                Vec<ValidatorId>, // ayes
                Vec<ValidatorId>, // nays
            ) {
                let indices = validator_indices();

                let aye_validator_1 = get_validator(indices[0]).account_id;
                let aye_validator_2 = get_validator(indices[1]).account_id;
                let nay_validator_1 = get_validator(indices[2]).account_id;
                let nay_validator_2 = get_validator(indices[3]).account_id;
                assert_eq!(context.validator.account_id, aye_validator_1);

                Summary::record_approve_vote(&context.root_id, aye_validator_1);
                Summary::record_approve_vote(&context.root_id, aye_validator_2);
                Summary::record_reject_vote(&context.root_id, nay_validator_1);
                Summary::record_reject_vote(&context.root_id, nay_validator_2);

                return (
                    vec![aye_validator_1, aye_validator_2],
                    vec![nay_validator_1, nay_validator_2],
                )
            }

            fn end_voting_without_outcome(context: &Context) {
                System::set_block_number(50);
                assert_ok!(Summary::end_voting_period(
                    RawOrigin::None.into(),
                    context.root_id,
                    context.validator.clone(),
                    context.record_summary_calculation_signature.clone()
                ));
            }

            #[test]
            fn root_is_not_approved() {
                let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validator_count(TEST_VALIDATOR_COUNT)
                    .for_offchain_worker()
                    .as_externality_with_state();
                ext.execute_with(|| {
                    let active_validators = Avn::validators();
                    assert_eq!(active_validators.len(), TEST_VALIDATOR_COUNT as usize);

                    let context = setup_context();
                    setup_voting_for_root_id(&context);
                    let (_ayes, _nays) = setup_root_without_outcome(&context);

                    end_voting_without_outcome(&context);

                    assert_eq!(false, Summary::get_vote(context.root_id.clone()).has_outcome());
                    assert_eq!(false, Summary::get_root_data(&context.root_id).is_validated);
                    assert_eq!(false, Summary::get_vote(context.root_id.clone()).is_approved());
                });
            }

            #[test]
            fn does_not_report_rejected_valid_root_offences() {
                let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validator_count(TEST_VALIDATOR_COUNT)
                    .for_offchain_worker()
                    .as_externality_with_state();
                ext.execute_with(|| {
                    let active_validators = Avn::validators();
                    assert_eq!(active_validators.len(), TEST_VALIDATOR_COUNT as usize);

                    let context = setup_context();
                    setup_voting_for_root_id(&context);
                    let (_ayes, _nays) = setup_root_without_outcome(&context);

                    end_voting_without_outcome(&context);

                    assert_eq!(
                        false,
                        Summary::reported_offence_of_type(SummaryOffenceType::RejectedValidRoot)
                    );
                });
            }

            #[test]
            fn reports_created_invalid_root_offence() {
                let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validator_count(TEST_VALIDATOR_COUNT)
                    .for_offchain_worker()
                    .as_externality_with_state();
                ext.execute_with(|| {
                    let active_validators = Avn::validators();
                    assert_eq!(active_validators.len(), TEST_VALIDATOR_COUNT as usize);

                    let context = setup_context();
                    setup_voting_for_root_id(&context);
                    let (_ayes, _nays) = setup_root_without_outcome(&context);

                    end_voting_without_outcome(&context);

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            context.validator.account_id,
                            TEST_VALIDATOR_COUNT.try_into().unwrap(),
                            vec![context.validator.account_id],
                            SummaryOffenceType::CreatedInvalidRoot
                        )
                    );
                });
            }

            #[test]
            fn reports_approved_invalid_root_offence() {
                let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
                    .with_validator_count(TEST_VALIDATOR_COUNT)
                    .for_offchain_worker()
                    .as_externality_with_state();
                ext.execute_with(|| {
                    let active_validators = Avn::validators();
                    assert_eq!(active_validators.len(), TEST_VALIDATOR_COUNT as usize);

                    let context = setup_context();
                    setup_voting_for_root_id(&context);
                    let (ayes, _nays) = setup_root_without_outcome(&context);

                    end_voting_without_outcome(&context);

                    assert_eq!(
                        true,
                        Summary::reported_offence(
                            context.validator.account_id,
                            TEST_VALIDATOR_COUNT.try_into().unwrap(),
                            vec![ayes[0], ayes[1]],
                            SummaryOffenceType::ApprovedInvalidRoot
                        )
                    );
                });
            }
        }
    }
}
