#![cfg(test)]

use super::test_offchain_worker::MockData;
use crate::{
    mock::{RuntimeEvent as Event, *},
    *,
};
use frame_support::{assert_ok, pallet_prelude::DispatchResultWithPostInfo};
use frame_system::RawOrigin;
use sp_core::hash::H256;
use sp_runtime::testing::{TestSignature, UintAuthorityId};

use sp_avn_common::event_types::{CheckResult, EthEventCheckResult, EventData};
use sp_runtime::BoundedVec;

use offence::EthereumLogOffenceType;

const NOT_PROCESSED: bool = false;
const PROCESSED: bool = true;

const EXPECTED_SESSION_VALIDATOR_COUNT: u32 = 3;

mod process_event {
    use super::*;
    struct Context {
        pub block_number: u64,
        pub event_id: EthEventId,
        pub event_data: EventData,
        pub checked_by: AccountId,
        pub min_challenge_votes: u32,
        pub check_result: EthEventCheckResult<BlockNumberFor<mock::TestRuntime>, AccountId>,
        pub validator: Validator<UintAuthorityId, AccountId>,
        pub signature: <AuthorityId as RuntimeAppPublic>::Signature,
        pub first_validator_id: AccountId,
        pub second_validator_id: AccountId,
        pub validator_count: u32,
    }

    impl Default for Context {
        fn default() -> Self {
            System::set_block_number(2);
            let event_data =
                EventData::LogAddedValidator(MockData::get_valid_added_validator_data());
            let event_id = EthEventId {
                signature: ValidEvents::AddedValidator.signature(),
                transaction_hash: H256::from([1; 32]),
            };
            let validator = EthereumEvents::validators()[0].clone();
            let checked_by = validator.account_id.clone();
            let block_number = 4;
            let min_challenge_votes = 1;
            let eth_event_check_result = EthEventCheckResult::new(
                block_number,
                CheckResult::Ok,
                &event_id,
                &event_data,
                checked_by,
                block_number + EVENT_CHALLENGE_PERIOD,
                min_challenge_votes,
            );

            Context {
                block_number,
                event_id,
                event_data,
                checked_by,
                min_challenge_votes,
                validator,
                check_result: eth_event_check_result,
                signature: TestSignature(0, vec![]), /* TODO [TYPE: test][PRI: high][JIRA: 348]:
                                                      * Replace this with a valid signature */
                first_validator_id: EthereumEvents::validators()[1].account_id.clone(),
                second_validator_id: EthereumEvents::validators()[2].account_id.clone(),
                validator_count: EXPECTED_SESSION_VALIDATOR_COUNT,
            }
        }
    }

    impl Context {
        pub fn custom_event_check_result(
            min_challenge_votes: u32,
            check_result: CheckResult,
        ) -> Self {
            let prototype = Self::default();
            let check_result = EthEventCheckResult::new(
                prototype.block_number,
                check_result,
                &prototype.event_id,
                &prototype.event_data,
                prototype.checked_by.clone(),
                prototype.block_number - 1,
                min_challenge_votes,
            );

            Context {
                block_number: prototype.block_number,
                event_id: prototype.event_id,
                event_data: prototype.event_data,
                checked_by: prototype.checked_by,
                min_challenge_votes: prototype.min_challenge_votes,
                validator: prototype.validator,
                check_result,
                signature: prototype.signature,
                first_validator_id: prototype.first_validator_id,
                second_validator_id: prototype.second_validator_id,
                validator_count: prototype.validator_count,
            }
        }
    }

    pub fn mock_on_event_processed_failing_with_invalid_event_to_process() {
        PROCESS_EVENT_SUCCESS.with(|pk| {
            *pk.borrow_mut() = false;
        });
    }

    mod given_a_pending_valid_event {
        use super::*;

        fn tests_check_result() -> CheckResult {
            return CheckResult::Ok;
        }

        mod when_successfully_challenged {
            use super::*;

            fn setup() -> Context {
                return setup_successful_challenge(tests_check_result());
            }

            mod and_event_is_executed_successfully {
                use super::*;

                #[test]
                fn removes_event_from_pending_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(true, there_are_no_pending_events());
                    });
                }

                #[test]
                fn ethereum_event_not_added_to_processed_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(false, event_is_in_processed_list(&context));
                    });
                }

                #[test]
                fn logs_ethereum_event_not_processed() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventProcessed {
                                eth_event_id: context.event_id,
                                processor: context.validator.account_id,
                                outcome: NOT_PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_ethereum_event_rejected() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_challenge_succeeded() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event = Event::EthereumEvents(
                            crate::Event::<TestRuntime>::ChallengeSucceeded {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                            },
                        );
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_offence_reported() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::OffenceReported {
                                offence_type:
                                    EthereumLogOffenceType::IncorrectValidationResultSubmitted,
                                offenders: vec![(context.checked_by, context.checked_by)],
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn creates_offence_for_check_creator() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let offences = OFFENCES.with(|l| l.replace(vec![]));
                        assert_eq!(
                            offences,
                            vec![(
                                vec![context.validator.account_id],
                                InvalidEthereumLogOffence {
                                    session_index: 0,
                                    validator_set_count: context.validator_count,
                                    offenders: vec![(
                                        context.validator.account_id,
                                        context.validator.account_id
                                    )],
                                    offence_type:
                                        EthereumLogOffenceType::IncorrectValidationResultSubmitted,
                                }
                            )]
                        );
                    });
                }
            }

            mod and_event_is_not_executed_successfully {
                use super::*;

                #[test]
                fn removes_event_from_pending_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        assert_eq!(true, there_are_no_pending_events());
                    });
                }

                #[test]
                fn ethereum_event_not_present_to_processed_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        assert_eq!(false, event_is_in_processed_list(&context));
                    });
                }

                #[test]
                fn logs_ethereum_event_not_processed() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventProcessed {
                                eth_event_id: context.event_id,
                                processor: context.validator.account_id,
                                outcome: NOT_PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_ethereum_event_rejected() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_challenge_succeeded() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event = Event::EthereumEvents(
                            crate::Event::<TestRuntime>::ChallengeSucceeded {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                            },
                        );
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_offence_reported() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::OffenceReported {
                                offence_type:
                                    EthereumLogOffenceType::IncorrectValidationResultSubmitted,
                                offenders: vec![(context.checked_by, context.checked_by)],
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn creates_offence_for_check_creator() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let offences = OFFENCES.with(|l| l.replace(vec![]));
                        assert_eq!(
                            offences,
                            vec![(
                                vec![context.validator.account_id],
                                InvalidEthereumLogOffence {
                                    session_index: 0,
                                    validator_set_count: context.validator_count,
                                    offenders: vec![(
                                        context.validator.account_id,
                                        context.validator.account_id
                                    )],
                                    offence_type:
                                        EthereumLogOffenceType::IncorrectValidationResultSubmitted,
                                }
                            )]
                        );
                    });
                }
            }
        }

        mod when_challenged_without_success {
            use super::*;

            fn setup() -> Context {
                return setup_failing_challenge(tests_check_result());
            }

            mod and_event_is_executed_successfully {
                use super::*;

                #[test]
                fn removes_event_from_pending_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(true, there_are_no_pending_events());
                    });
                }

                #[test]
                fn adds_ethereum_event_to_processed_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(true, event_is_in_processed_list(&context));
                    });
                }

                #[test]
                fn logs_ethereum_event_processed() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventProcessed {
                                eth_event_id: context.event_id,
                                processor: context.validator.account_id,
                                outcome: PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_ethereum_event_accepted() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventAccepted {
                                eth_event_id: context.event_id,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_offence_reported() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::OffenceReported {
                                offence_type:
                                    EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                                offenders: vec![
                                    (context.first_validator_id, context.first_validator_id),
                                    (context.second_validator_id, context.second_validator_id),
                                ],
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn creates_offence_for_challengers() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let offences = OFFENCES.with(|l| l.replace(vec![]));
                        assert_eq!(
                            offences,
                            vec![(
                                vec![context.validator.account_id],
                                InvalidEthereumLogOffence {
                                    session_index: 0,
                                    validator_set_count: context.validator_count,
                                    offenders: vec![
                                        (context.first_validator_id, context.first_validator_id),
                                        (context.second_validator_id, context.second_validator_id)
                                    ],
                                    offence_type:
                                        EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                                }
                            )]
                        );
                    });
                }
            }

            mod and_event_is_not_executed_successfully {
                use super::*;

                //The error enum is defined in mock.rs -> on_event_processed

                #[test]
                fn removes_event_from_pending_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: false,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));

                        assert_eq!(true, there_are_no_pending_events());
                    });
                }

                #[test]
                fn adds_ethereum_event_to_processed_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id.clone(),
                                check_result: context.check_result.result.clone(),
                                successful_challenge: false,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));

                        assert_eq!(true, event_is_in_processed_list(&context));
                    });
                }

                #[test]
                fn logs_ethereum_event_processed() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id.clone(),
                                check_result: context.check_result.result.clone(),
                                successful_challenge: false,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventProcessed {
                                eth_event_id: context.event_id.clone(),
                                processor: context.validator.account_id,
                                outcome: PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));

                        let accepted_event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventAccepted {
                                eth_event_id: context.event_id,
                            });
                        assert_eq!(false, an_event_was_emitted(&accepted_event));
                    });
                }

                #[test]
                fn logs_offence_reported() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: false,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::OffenceReported {
                                offence_type:
                                    EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                                offenders: vec![
                                    (context.first_validator_id, context.first_validator_id),
                                    (context.second_validator_id, context.second_validator_id),
                                ],
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn creates_offence_for_challengers() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: false,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));

                        let offences = OFFENCES.with(|l| l.replace(vec![]));
                        assert_eq!(
                            offences,
                            vec![(
                                vec![context.validator.account_id],
                                InvalidEthereumLogOffence {
                                    session_index: 0,
                                    validator_set_count: context.validator_count,
                                    offenders: vec![
                                        (context.first_validator_id, context.first_validator_id),
                                        (context.second_validator_id, context.second_validator_id)
                                    ],
                                    offence_type:
                                        EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                                }
                            )]
                        );
                    });
                }
            }
        }

        mod when_nobody_has_challenged_then {
            use super::*;

            fn setup() -> Context {
                return setup_without_challenge(tests_check_result());
            }

            #[test]
            fn does_not_create_any_offences() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                ext.execute_with(|| {
                    let context = setup();
                    assert_ok!(call_process_event_result(&context));

                    let offences = OFFENCES.with(|l| l.replace(vec![]));
                    assert_eq!(offences, vec![]);
                });
            }
        }
    }

    mod given_a_pending_invalid_event {
        use super::*;

        fn tests_check_result() -> CheckResult {
            return CheckResult::Invalid;
        }

        mod when_successfully_challenged {
            use super::*;

            fn setup() -> Context {
                return setup_successful_challenge(tests_check_result());
            }

            mod and_event_is_executed_successfully {
                use super::*;

                #[test]
                fn removes_event_from_pending_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(true, there_are_no_pending_events());
                    });
                }

                #[test]
                fn does_not_add_ethereum_event_to_processed_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(false, event_is_in_processed_list(&context));
                    });
                }

                #[test]
                fn logs_ethereum_event_not_processed() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventProcessed {
                                eth_event_id: context.event_id,
                                processor: context.validator.account_id,
                                outcome: NOT_PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_ethereum_event_rejected() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_challenge_succeeded() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event = Event::EthereumEvents(
                            crate::Event::<TestRuntime>::ChallengeSucceeded {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                            },
                        );
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_offence_reported() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::OffenceReported {
                                offence_type:
                                    EthereumLogOffenceType::IncorrectValidationResultSubmitted,
                                offenders: vec![(context.checked_by, context.checked_by)],
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn creates_offence_for_check_creator() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let offences = OFFENCES.with(|l| l.replace(vec![]));
                        assert_eq!(
                            offences,
                            vec![(
                                vec![context.validator.account_id],
                                InvalidEthereumLogOffence {
                                    session_index: 0,
                                    validator_set_count: context.validator_count,
                                    offenders: vec![(
                                        context.validator.account_id,
                                        context.validator.account_id
                                    )],
                                    offence_type:
                                        EthereumLogOffenceType::IncorrectValidationResultSubmitted
                                }
                            )]
                        );
                    });
                }
            }

            mod and_event_is_not_executed_successfully {
                use super::*;

                #[test]
                fn removes_event_from_pending_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        assert_eq!(true, there_are_no_pending_events());
                    });
                }

                #[test]
                fn does_not_add_ethereum_event_to_processed_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        assert_eq!(false, event_is_in_processed_list(&context));
                    });
                }

                #[test]
                fn logs_ethereum_event_not_processed() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventProcessed {
                                eth_event_id: context.event_id,
                                processor: context.validator.account_id,
                                outcome: NOT_PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_ethereum_event_rejected() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_challenge_succeeded() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event = Event::EthereumEvents(
                            crate::Event::<TestRuntime>::ChallengeSucceeded {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                            },
                        );
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_offence_reported() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::OffenceReported {
                                offence_type:
                                    EthereumLogOffenceType::IncorrectValidationResultSubmitted,
                                offenders: vec![(context.checked_by, context.checked_by)],
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn creates_offence_for_check_creator() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let offences = OFFENCES.with(|l| l.replace(vec![]));
                        assert_eq!(
                            offences,
                            vec![(
                                vec![context.validator.account_id],
                                InvalidEthereumLogOffence {
                                    session_index: 0,
                                    validator_set_count: context.validator_count,
                                    offenders: vec![(
                                        context.validator.account_id,
                                        context.validator.account_id
                                    )],
                                    offence_type:
                                        EthereumLogOffenceType::IncorrectValidationResultSubmitted
                                }
                            )]
                        );
                    });
                }
            }
        }

        mod when_challenged_without_success {
            use super::*;

            fn setup() -> Context {
                return setup_failing_challenge(tests_check_result());
            }

            mod and_event_is_executed_successfully {
                use super::*;

                #[test]
                fn succeeds() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));
                    });
                }

                #[test]
                fn removes_event_from_pending_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(true, there_are_no_pending_events());
                    });
                }

                #[test]
                fn ethereum_event_not_added_to_processed_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(false, event_is_in_processed_list(&context));
                    });
                }

                #[test]
                fn logs_ethereum_event_processed() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventProcessed {
                                eth_event_id: context.event_id,
                                processor: context.validator.account_id,
                                outcome: PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_ethereum_event_accepted() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(context.check_result.result, CheckResult::Invalid);

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: NOT_PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_offence_reported() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::OffenceReported {
                                offence_type:
                                    EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                                offenders: vec![
                                    (context.first_validator_id, context.first_validator_id),
                                    (context.second_validator_id, context.second_validator_id),
                                ],
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn creates_offence_for_challengers() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        assert_ok!(call_process_event_result(&context));

                        let offences = OFFENCES.with(|l| l.replace(vec![]));
                        assert_eq!(
                            offences,
                            vec![(
                                vec![context.validator.account_id],
                                InvalidEthereumLogOffence {
                                    session_index: 0,
                                    validator_set_count: context.validator_count,
                                    offenders: vec![
                                        (context.first_validator_id, context.first_validator_id),
                                        (context.second_validator_id, context.second_validator_id)
                                    ],
                                    offence_type:
                                        EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                                }
                            )]
                        );
                    });
                }
            }

            mod and_event_is_not_executed_successfully {
                use super::*;

                #[test]
                fn succeeds() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();
                        assert_ok!(call_process_event_result(&context));
                    });
                }

                #[test]
                fn removes_event_from_pending_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        assert_eq!(true, there_are_no_pending_events());
                    });
                }

                #[test]
                fn ethereum_event_not_added_to_processed_list() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));
                        assert_eq!(false, event_is_in_processed_list(&context));
                    });
                }

                #[test]
                fn logs_ethereum_event_processed() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventProcessed {
                                eth_event_id: context.event_id,
                                processor: context.validator.account_id,
                                outcome: PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_ethereum_event_accepted() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        assert_eq!(context.check_result.result, CheckResult::Invalid);

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::EventRejected {
                                eth_event_id: context.event_id,
                                check_result: context.check_result.result,
                                successful_challenge: NOT_PROCESSED,
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn logs_offence_reported() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let event =
                            Event::EthereumEvents(crate::Event::<TestRuntime>::OffenceReported {
                                offence_type:
                                    EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                                offenders: vec![
                                    (context.first_validator_id, context.first_validator_id),
                                    (context.second_validator_id, context.second_validator_id),
                                ],
                            });
                        assert_eq!(true, an_event_was_emitted(&event));
                    });
                }

                #[test]
                fn creates_offence_for_challengers() {
                    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                    ext.execute_with(|| {
                        let context = setup();
                        mock_on_event_processed_failing_with_invalid_event_to_process();

                        assert_ok!(call_process_event_result(&context));

                        let offences = OFFENCES.with(|l| l.replace(vec![]));
                        assert_eq!(
                            offences,
                            vec![(
                                vec![context.validator.account_id],
                                InvalidEthereumLogOffence {
                                    session_index: 0,
                                    validator_set_count: context.validator_count,
                                    offenders: vec![
                                        (context.first_validator_id, context.first_validator_id),
                                        (context.second_validator_id, context.second_validator_id)
                                    ],
                                    offence_type:
                                        EthereumLogOffenceType::ChallengeAttemptedOnValidResult,
                                }
                            )]
                        );
                    });
                }
            }
        }

        mod when_nobody_has_challenged_then {
            use super::*;

            fn setup() -> Context {
                return setup_without_challenge(tests_check_result());
            }

            #[test]
            fn does_not_create_any_offences() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();
                ext.execute_with(|| {
                    let context = setup();
                    assert_ok!(call_process_event_result(&context));

                    let offences = OFFENCES.with(|l| l.replace(vec![]));
                    assert_eq!(offences, vec![]);
                });
            }
        }
    }

    fn setup_preconditions(context: &Context) {
        <EventsPendingChallenge<TestRuntime>>::try_append((
            context.check_result.clone(),
            DEFAULT_INGRESS_COUNTER,
            0,
        ))
        .expect("Cannot append");

        // Set block number to be ready for processing the event
        System::set_block_number(context.check_result.ready_for_processing_after_block + 1);

        assert_eq!(EthereumEvents::events_pending_challenge().len(), 1);
        assert!(!<ProcessedEvents<TestRuntime>>::contains_key(&context.event_id));
    }

    fn call_process_event_result(context: &Context) -> DispatchResultWithPostInfo {
        let process_event_result = EthereumEvents::process_event(
            RawOrigin::None.into(),
            context.event_id.clone(),
            DEFAULT_INGRESS_COUNTER,
            context.validator.clone(),
            context.signature.clone(),
        );

        return process_event_result;
    }

    fn add_challenge(context: &Context) {
        // Adds some challenges to this event
        let _ = <Challenges<TestRuntime>>::insert(
            context.event_id.clone(),
            BoundedVec::truncate_from(vec![
                context.first_validator_id.clone(),
                context.second_validator_id.clone(),
            ]),
        );
    }

    fn setup_successful_challenge(check_result: CheckResult) -> Context {
        let context = Context::custom_event_check_result(1, check_result);

        setup_preconditions(&context);
        add_challenge(&context);
        assert_eq!(true, EthereumEvents::is_challenge_successful(&context.check_result));

        return context;
    }

    fn setup_failing_challenge(check_result: CheckResult) -> Context {
        let context = Context::custom_event_check_result(4, check_result);

        setup_preconditions(&context);
        add_challenge(&context);
        assert_eq!(false, EthereumEvents::is_challenge_successful(&context.check_result));

        return context;
    }

    fn setup_without_challenge(check_result: CheckResult) -> Context {
        let context = Context::custom_event_check_result(1, check_result);

        setup_preconditions(&context);
        assert!(!EthereumEvents::is_challenge_successful(&context.check_result));

        return context;
    }

    fn there_are_no_pending_events() -> bool {
        return EthereumEvents::events_pending_challenge().is_empty();
    }

    fn event_is_in_processed_list(context: &Context) -> bool {
        return <ProcessedEvents<TestRuntime>>::contains_key(&context.event_id);
    }

    fn an_event_was_emitted(event: &Event) -> bool {
        return System::events().iter().any(|a| a.event == *event);
    }
}
