// Copyright 2026 Aventus DAO Ltd
#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::traits::BadOrigin;
use system::RawOrigin;

mod test_set_periods {
    use super::*;

    struct Context {
        origin: RuntimeOrigin,
        schedule_period: BlockNumber,
        new_schedule_period: BlockNumber,
        voting_period: BlockNumber,
        new_voting_period: BlockNumber,
    }

    impl Default for Context {
        fn default() -> Self {
            Context {
                origin: RawOrigin::Root.into(),
                schedule_period: 160,
                new_schedule_period: 200,
                voting_period: 100,
                new_voting_period: 150,
            }
        }
    }

    impl Context {
        fn dispatch_set_schedule_period(&self) -> DispatchResult {
            #[allow(deprecated)]
            return Summary::set_periods(
                self.origin.clone(),
                self.new_schedule_period.clone(),
                self.voting_period.clone(),
            );
        }

        fn dispatch_set_voting_period(&self) -> DispatchResult {
            #[allow(deprecated)]
            return Summary::set_periods(
                self.origin.clone(),
                self.schedule_period.clone(),
                self.new_voting_period.clone(),
            );
        }
    }

    mod successful_cases {
        use super::*;

        #[test]
        fn update_schedule_period() {
            let mut ext = ExtBuilder::build_default()
                .with_validators()
                .with_genesis_config()
                .as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                let initial_next_block_to_process = Summary::get_next_block_to_process();
                assert_ne!(context.new_schedule_period, Summary::schedule_period());

                assert_ok!(context.dispatch_set_schedule_period());
                assert_eq!(
                    initial_next_block_to_process + context.new_schedule_period,
                    Summary::block_number_for_next_slot()
                );
                assert_eq!(context.new_schedule_period, Summary::schedule_period());
            });
        }

        #[test]
        fn schedule_period_can_be_set_via_admin_config() {
            let mut ext = ExtBuilder::build_default()
                .with_validators()
                .with_genesis_config()
                .as_externality();
            ext.execute_with(|| {
                let current_period: BlockNumber = <SchedulePeriod<TestRuntime, Instance1>>::get();
                let new_period = current_period + 100;

                let config = AdminConfig::SchedulePeriod(new_period);
                assert_ok!(Summary::set_admin_config(RawOrigin::Root.into(), config,));

                System::assert_last_event(
                    crate::Event::<TestRuntime>::SchedulePeriodSet { new_period }.into(),
                );
            });
        }

        #[test]
        fn update_voting_period() {
            let mut ext = ExtBuilder::build_default()
                .with_validators()
                .with_genesis_config()
                .as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                let initial_next_block_to_process = Summary::get_next_block_to_process();
                assert_ne!(context.new_voting_period, Summary::voting_period());

                assert_ok!(context.dispatch_set_voting_period());
                assert_eq!(
                    initial_next_block_to_process + context.schedule_period,
                    Summary::block_number_for_next_slot()
                );
                assert_eq!(context.new_voting_period, Summary::voting_period());
            });
        }

        #[test]
        fn voting_period_can_be_set_via_admin_config() {
            let mut ext = ExtBuilder::build_default()
                .with_validators()
                .with_genesis_config()
                .as_externality();
            ext.execute_with(|| {
                let current_period: BlockNumber = <VotingPeriod<TestRuntime, Instance1>>::get();
                let new_period = current_period + 1;

                let config = AdminConfig::VotingPeriod(new_period);
                assert_ok!(Summary::set_admin_config(RawOrigin::Root.into(), config,));

                System::assert_last_event(
                    crate::Event::<TestRuntime>::VotingPeriodSet { new_period }.into(),
                );
            });
        }

        #[test]
        fn threshold_can_be_set_via_admin_config() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let new_threshold = 76;
                let config = AdminConfig::ExternalValidationThreshold(new_threshold);
                assert_ok!(Summary::set_admin_config(RawOrigin::Root.into(), config));

                System::assert_last_event(
                    crate::Event::<TestRuntime>::ExternalValidationThresholdSet { new_threshold }
                        .into(),
                );
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn threshold_is_wrong() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let new_threshold = 0;
                let config = AdminConfig::ExternalValidationThreshold(new_threshold);
                assert_noop!(
                    Summary::set_admin_config(RawOrigin::Root.into(), config),
                    Error::<TestRuntime>::InvalidExternalValidationThreshold
                );

                let new_threshold = 101;
                let config = AdminConfig::ExternalValidationThreshold(new_threshold);
                assert_noop!(
                    Summary::set_admin_config(RawOrigin::Root.into(), config),
                    Error::<TestRuntime>::InvalidExternalValidationThreshold
                );
            });
        }

        mod set_schedule_period {
            use super::*;

            #[test]
            fn origin_is_not_root() {
                let mut ext = ExtBuilder::build_default()
                    .with_validators()
                    .with_genesis_config()
                    .as_externality();
                ext.execute_with(|| {
                    let context: Context = Context {
                        origin: RuntimeOrigin::signed(Default::default()),
                        ..Default::default()
                    };

                    assert_noop!(context.dispatch_set_schedule_period(), BadOrigin);
                    assert_ne!(context.new_schedule_period, Summary::schedule_period());

                    let config = AdminConfig::SchedulePeriod(100);
                    assert_noop!(
                        Summary::set_admin_config(
                            RuntimeOrigin::signed(Default::default()),
                            config
                        ),
                        BadOrigin
                    );
                });
            }

            #[test]
            fn origin_is_unsigned() {
                let mut ext = ExtBuilder::build_default()
                    .with_validators()
                    .with_genesis_config()
                    .as_externality();
                ext.execute_with(|| {
                    let context: Context =
                        Context { origin: RawOrigin::None.into(), ..Default::default() };

                    assert_noop!(context.dispatch_set_schedule_period(), BadOrigin);
                    assert_ne!(context.new_schedule_period, Summary::schedule_period());

                    let config = AdminConfig::SchedulePeriod(100);
                    assert_noop!(
                        Summary::set_admin_config(RawOrigin::None.into(), config),
                        BadOrigin
                    );
                });
            }

            #[test]
            fn less_than_minimum_value_should_fail() {
                let mut ext = ExtBuilder::build_default()
                    .with_validators()
                    .with_genesis_config()
                    .as_externality();
                ext.execute_with(|| {
                    let context: Context = Context {
                        new_schedule_period: (MIN_SCHEDULE_PERIOD - 1).into(),
                        ..Default::default()
                    };

                    assert_noop!(
                        context.dispatch_set_schedule_period(),
                        Error::<TestRuntime>::SchedulePeriodIsTooShort
                    );
                    assert_ne!(context.new_schedule_period, Summary::schedule_period());

                    let config = AdminConfig::SchedulePeriod(1);
                    assert_noop!(
                        Summary::set_admin_config(RawOrigin::Root.into(), config),
                        Error::<TestRuntime>::SchedulePeriodIsTooShort
                    );
                });
            }
        }

        mod set_voting_period {
            use super::*;

            #[test]
            fn origin_is_not_root() {
                let mut ext = ExtBuilder::build_default()
                    .with_validators()
                    .with_genesis_config()
                    .as_externality();
                ext.execute_with(|| {
                    let context: Context = Context {
                        origin: RuntimeOrigin::signed(Default::default()),
                        ..Default::default()
                    };

                    assert_noop!(context.dispatch_set_voting_period(), BadOrigin);
                    assert_ne!(context.new_voting_period, Summary::voting_period());

                    let config = AdminConfig::VotingPeriod(99);
                    assert_noop!(
                        Summary::set_admin_config(
                            RuntimeOrigin::signed(Default::default()),
                            config
                        ),
                        BadOrigin
                    );
                });
            }

            #[test]
            fn origin_is_unsigned() {
                let mut ext = ExtBuilder::build_default()
                    .with_validators()
                    .with_genesis_config()
                    .as_externality();
                ext.execute_with(|| {
                    let context: Context =
                        Context { origin: RawOrigin::None.into(), ..Default::default() };

                    assert_noop!(context.dispatch_set_voting_period(), BadOrigin);
                    assert_ne!(context.new_voting_period, Summary::voting_period());

                    let config = AdminConfig::VotingPeriod(99);
                    assert_noop!(
                        Summary::set_admin_config(RawOrigin::None.into(), config),
                        BadOrigin
                    );
                });
            }

            #[test]
            fn less_than_minimum_value_should_fail() {
                let mut ext = ExtBuilder::build_default()
                    .with_validators()
                    .with_genesis_config()
                    .as_externality();
                ext.execute_with(|| {
                    let context: Context = Context {
                        new_voting_period: (MIN_VOTING_PERIOD - 1).into(),
                        ..Default::default()
                    };

                    assert_noop!(
                        context.dispatch_set_voting_period(),
                        Error::<TestRuntime>::VotingPeriodIsTooShort
                    );
                    assert_ne!(context.new_voting_period, Summary::voting_period());

                    let config = AdminConfig::VotingPeriod(1);
                    assert_noop!(
                        Summary::set_admin_config(RawOrigin::Root.into(), config),
                        Error::<TestRuntime>::VotingPeriodIsTooShort
                    );
                });
            }

            #[test]
            fn equal_to_schedule_period_should_fail() {
                let mut ext = ExtBuilder::build_default()
                    .with_validators()
                    .with_genesis_config()
                    .as_externality();
                ext.execute_with(|| {
                    let schedule_period = Summary::schedule_period();
                    let context: Context = Context {
                        new_voting_period: (schedule_period).into(),
                        ..Default::default()
                    };

                    assert_noop!(
                        context.dispatch_set_voting_period(),
                        Error::<TestRuntime>::VotingPeriodIsEqualOrLongerThanSchedulePeriod
                    );
                    assert_ne!(context.new_voting_period, Summary::voting_period());

                    let config = AdminConfig::VotingPeriod(schedule_period);
                    assert_noop!(
                        Summary::set_admin_config(RawOrigin::Root.into(), config),
                        Error::<TestRuntime>::VotingPeriodIsEqualOrLongerThanSchedulePeriod
                    );
                });
            }

            #[test]
            fn greater_than_schedule_period_should_fail() {
                let mut ext = ExtBuilder::build_default()
                    .with_validators()
                    .with_genesis_config()
                    .as_externality();
                ext.execute_with(|| {
                    let schedule_period = Summary::schedule_period();
                    let context: Context = Context {
                        new_voting_period: (schedule_period + 1).into(),
                        ..Default::default()
                    };

                    assert_noop!(
                        context.dispatch_set_voting_period(),
                        Error::<TestRuntime>::VotingPeriodIsEqualOrLongerThanSchedulePeriod
                    );
                    assert_ne!(context.new_voting_period, Summary::voting_period());

                    let config = AdminConfig::VotingPeriod(schedule_period + 1);
                    assert_noop!(
                        Summary::set_admin_config(RawOrigin::Root.into(), config),
                        Error::<TestRuntime>::VotingPeriodIsEqualOrLongerThanSchedulePeriod
                    );
                });
            }
        }
    }
}
