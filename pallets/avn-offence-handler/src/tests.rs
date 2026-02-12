//Copyright 2021 Aventus Systems (UK) Ltd.

#![cfg(test)]

use crate::{mock::*, *};
use sp_runtime::Perbill;
use sp_staking::offence::OffenceDetails;

mod on_offence {
    use super::*;

    type Reporter = <TestRuntime as frame_system::Config>::AccountId;
    type Offender = IdentificationTuple<TestRuntime>;

    struct Context {
        offenders: Vec<OffenceDetails<Reporter, Offender>>,
        slash_fraction: Vec<Perbill>,
        session_index: SessionIndex,
    }

    impl Context {
        fn default(offender_ids: Vec<u64>) -> Self {
            AvnOffenceHandler::enable_offence();

            let offenders: Vec<OffenceDetails<Reporter, Offender>> = offender_ids
                .into_iter()
                .map(|offender_id| OffenceDetails {
                    offender: (offender_id, offender_id),
                    reporters: vec![],
                })
                .collect::<Vec<_>>();

            Context {
                offenders,
                slash_fraction: vec![Perbill::from_percent(100)],
                session_index: 1,
            }
        }
    }

    mod succeeds {
        use super::*;

        #[test]
        fn when_slash_validator_succeeds() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();

            ext.execute_with(|| {
                let context = Context::default(vec![VALIDATOR_ID_1, VALIDATOR_ID_2]);

                AvnOffenceHandler::on_offence(
                    &context.offenders,
                    &context.slash_fraction,
                    context.session_index,
                );

                assert_eq!(true, AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_1));
                assert_eq!(true, AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_2));
            });
        }

        mod with_slashing_enable {
            use super::*;

            #[test]
            fn implies_slashed_validator_is_recorded() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = Context::default(vec![VALIDATOR_ID_1, VALIDATOR_ID_2]);

                    AvnOffenceHandler::on_offence(
                        &context.offenders,
                        &context.slash_fraction,
                        context.session_index,
                    );

                    assert_eq!(true, AvnOffenceHandler::can_slash());
                    assert_eq!(true, AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_1));
                    assert_eq!(true, AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_2));
                    assert_eq!(
                        false,
                        AvnOffenceHandler::get_reported_offender(
                            &VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR
                        )
                    );
                });
            }

            #[test]
            fn implies_slashed_validator_events_are_emitted() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = Context::default(vec![VALIDATOR_ID_1, VALIDATOR_ID_2]);

                    AvnOffenceHandler::on_offence(
                        &context.offenders,
                        &context.slash_fraction,
                        context.session_index,
                    );

                    assert!(event_emitted(&mock::RuntimeEvent::AvnOffenceHandler(
                        crate::Event::<TestRuntime>::ReportedOffence { offender: VALIDATOR_ID_1 }
                    )));

                    assert!(event_emitted(&mock::RuntimeEvent::AvnOffenceHandler(
                        crate::Event::<TestRuntime>::ReportedOffence { offender: VALIDATOR_ID_2 }
                    )));
                });
            }
        }

        mod with_slashing_disabled {
            use super::*;

            #[test]
            fn implies_slashed_validator_is_recorded() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = Context::default(vec![VALIDATOR_ID_1, VALIDATOR_ID_2]);
                    AvnOffenceHandler::disable_offence();

                    AvnOffenceHandler::on_offence(
                        &context.offenders,
                        &context.slash_fraction,
                        context.session_index,
                    );

                    assert_eq!(false, AvnOffenceHandler::can_slash());

                    assert_eq!(
                        true,
                        <ReportedOffenders<TestRuntime>>::contains_key(&VALIDATOR_ID_1)
                    );
                    assert_eq!(false, AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_1));

                    assert_eq!(
                        true,
                        <ReportedOffenders<TestRuntime>>::contains_key(&VALIDATOR_ID_2)
                    );
                    assert_eq!(false, AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_2));

                    assert_eq!(
                        false,
                        AvnOffenceHandler::get_reported_offender(
                            &VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR
                        )
                    );
                });
            }

            #[test]
            fn implies_slashed_validator_events_are_emitted() {
                let mut ext = ExtBuilder::build_default().with_validators().as_externality();

                ext.execute_with(|| {
                    let context = Context::default(vec![VALIDATOR_ID_1, VALIDATOR_ID_2]);
                    AvnOffenceHandler::disable_offence();

                    AvnOffenceHandler::on_offence(
                        &context.offenders,
                        &context.slash_fraction,
                        context.session_index,
                    );

                    assert!(event_emitted(&mock::RuntimeEvent::AvnOffenceHandler(
                        crate::Event::<TestRuntime>::ReportedOffence { offender: VALIDATOR_ID_1 }
                    )));

                    assert!(event_emitted(&mock::RuntimeEvent::AvnOffenceHandler(
                        crate::Event::<TestRuntime>::ReportedOffence { offender: VALIDATOR_ID_2 }
                    )));
                });
            }
        }
    }

    mod fails {
        use super::*;

        #[test]
        fn when_slash_validator_fails() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();

            ext.execute_with(|| {
                let context = Context::default(vec![VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR]);

                AvnOffenceHandler::on_offence(
                    &context.offenders,
                    &context.slash_fraction,
                    context.session_index,
                );

                assert_eq!(
                    false,
                    AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR)
                );
                assert_eq!(
                    true,
                    ReportedOffenders::<TestRuntime>::contains_key(
                        &VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR
                    )
                );

                // ReportedOffence event should always be emitted, doesn't matter if the slash
                // action is successful or not
                assert_eq!(
                    true,
                    event_emitted(&mock::RuntimeEvent::AvnOffenceHandler(crate::Event::<
                        TestRuntime,
                    >::ReportedOffence {
                        offender: VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR
                    }))
                );
            });
        }

        #[test]
        fn when_not_all_offenders_are_slashed() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();

            ext.execute_with(|| {
                let context =
                    Context::default(vec![VALIDATOR_ID_1, VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR]);

                AvnOffenceHandler::on_offence(
                    &context.offenders.as_slice(),
                    &context.slash_fraction,
                    context.session_index,
                );

                assert_eq!(true, AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_1));
                assert_eq!(
                    false,
                    AvnOffenceHandler::get_reported_offender(&VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR)
                );
                assert_eq!(
                    true,
                    ReportedOffenders::<TestRuntime>::contains_key(
                        &VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR
                    )
                );

                // ReportedOffence event should always be emitted, doesn't matter if the slash
                // action is successful or not
                assert_eq!(
                    true,
                    event_emitted(&mock::RuntimeEvent::AvnOffenceHandler(crate::Event::<
                        TestRuntime,
                    >::ReportedOffence {
                        offender: VALIDATOR_ID_1
                    }))
                );
                assert_eq!(
                    true,
                    event_emitted(&mock::RuntimeEvent::AvnOffenceHandler(crate::Event::<
                        TestRuntime,
                    >::ReportedOffence {
                        offender: VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR
                    }))
                );
            });
        }
    }
}

pub fn event_emitted(event: &mock::RuntimeEvent) -> bool {
    return System::events().iter().any(|a| a.event == *event);
}
