// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::{DispatchError, Perbill};

#[test]
fn origin_is_checked_none() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let new_registrar = TestAccount::new([99u8; 32]).account_id();

        let config = AdminConfig::NodeRegistrar(new_registrar);
        assert_noop!(
            NodeManager::set_admin_config(RawOrigin::None.into(), config,),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn origin_is_checked_signed() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let new_registrar = TestAccount::new([99u8; 32]).account_id();

        let config = AdminConfig::NodeRegistrar(new_registrar);
        assert_noop!(
            NodeManager::set_admin_config(RuntimeOrigin::signed(new_registrar.clone()), config,),
            DispatchError::BadOrigin
        );
    });
}

mod node_registrar {
    use super::*;

    #[test]
    fn node_registrar_can_be_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let current_registrar = NodeRegistrar::<TestRuntime>::get();
            let new_registrar = TestAccount::new([99u8; 32]).account_id();
            assert!(current_registrar.is_none());

            let config = AdminConfig::NodeRegistrar(new_registrar);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::NodeRegistrarSet { new_registrar }.into());
        });
    }
}

mod reward_period {
    use super::*;

    #[test]
    fn can_be_set_for_next_period_only() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let old_configured_period = ConfiguredRewardPeriodLength::<TestRuntime>::get();
            let new_period = old_configured_period + 1;

            let config = AdminConfig::RewardPeriod(new_period);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));

            assert_eq!(ConfiguredRewardPeriodLength::<TestRuntime>::get(), new_period);
            assert_eq!(RewardPeriod::<TestRuntime>::get().length, reward_period.length);

            System::assert_last_event(
                Event::RewardPeriodLengthSet {
                    period_index: reward_period.current,
                    old_reward_period_length: old_configured_period,
                    new_reward_period_length: new_period,
                }
                .into(),
            );
        });
    }

    #[test]
    fn new_reward_period_length_is_applied_on_next_period() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let new_period = reward_period.length + 1;

            assert_ok!(NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::RewardPeriod(new_period),
            ));

            assert_eq!(RewardPeriod::<TestRuntime>::get().length, reward_period.length);

            roll_forward((reward_period.length as u64 - System::block_number()) + 1);

            assert_eq!(RewardPeriod::<TestRuntime>::get().length, new_period);
        });
    }

    mod fails_to_be_set_when {
        use super::*;

        #[test]
        fn period_is_smaller_than_heartbeat() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let heartbeat = <HeartbeatPeriod<TestRuntime>>::get();
                let new_period = heartbeat - 1;

                let config = AdminConfig::RewardPeriod(new_period);
                assert_noop!(
                    NodeManager::set_admin_config(RawOrigin::Root.into(), config,),
                    Error::<TestRuntime>::RewardPeriodInvalid
                );
            });
        }
    }
}

mod batch_size {
    use super::*;

    #[test]
    fn can_be_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let current_size = MaxBatchSize::<TestRuntime>::get();
            let new_size = current_size + 1;

            let config = AdminConfig::BatchSize(new_size);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::BatchSizeSet { new_size }.into());
        });
    }

    mod fails_to_be_set_when {
        use super::*;

        #[test]
        fn period_is_zero() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let new_size = 0u32;

                let config = AdminConfig::BatchSize(new_size);
                assert_noop!(
                    NodeManager::set_admin_config(RawOrigin::Root.into(), config,),
                    Error::<TestRuntime>::BatchSizeInvalid
                );
            });
        }
    }
}

mod heartbeat {
    use super::*;

    #[test]
    fn can_be_set_for_next_period_only() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let current_period = HeartbeatPeriod::<TestRuntime>::get();
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let current_threshold = reward_period.uptime_threshold;
            let new_heartbeat_period = current_period + 1;

            let config = AdminConfig::Heartbeat(new_heartbeat_period);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));

            assert_eq!(HeartbeatPeriod::<TestRuntime>::get(), new_heartbeat_period);
            assert_eq!(RewardPeriod::<TestRuntime>::get().uptime_threshold, current_threshold);

            System::assert_last_event(Event::HeartbeatPeriodSet { new_heartbeat_period }.into());
        });
    }

    #[test]
    fn new_heartbeat_affects_next_period_threshold() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let current_heartbeat = HeartbeatPeriod::<TestRuntime>::get();
            let new_heartbeat_period = current_heartbeat + 1;

            assert_ok!(NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::Heartbeat(new_heartbeat_period),
            ));

            let expected_next_threshold =
                NodeManager::calculate_uptime_threshold(reward_period.length, new_heartbeat_period);

            assert_eq!(
                RewardPeriod::<TestRuntime>::get().heartbeat_period,
                reward_period.heartbeat_period
            );

            roll_forward((reward_period.length as u64 - System::block_number()) + 1);

            assert_eq!(RewardPeriod::<TestRuntime>::get().heartbeat_period, new_heartbeat_period);

            assert_eq!(
                RewardPeriod::<TestRuntime>::get().uptime_threshold,
                expected_next_threshold
            );
        });
    }

    mod fails_to_be_set_when {
        use super::*;

        #[test]
        fn period_is_zero() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let new_heartbeat_period = 0u32;

                let config = AdminConfig::Heartbeat(new_heartbeat_period);
                assert_noop!(
                    NodeManager::set_admin_config(RawOrigin::Root.into(), config,),
                    Error::<TestRuntime>::HeartbeatPeriodZero
                );
            });
        }

        #[test]
        fn period_is_longer_than_configured_reward_period() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let reward_period = ConfiguredRewardPeriodLength::<TestRuntime>::get();
                let new_heartbeat_period = reward_period + 1;

                let config = AdminConfig::Heartbeat(new_heartbeat_period);
                assert_noop!(
                    NodeManager::set_admin_config(RawOrigin::Root.into(), config,),
                    Error::<TestRuntime>::HeartbeatPeriodInvalid
                );
            });
        }
    }
}

mod reward_amount_per_period {
    use super::*;

    #[test]
    fn can_be_set_for_next_period_only() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let current_amount = RewardAmountPerPeriod::<TestRuntime>::get();
            let new_amount = current_amount + 1;

            let config = AdminConfig::RewardAmountPerPeriod(new_amount);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));

            assert_eq!(RewardAmountPerPeriod::<TestRuntime>::get(), new_amount);
            assert_eq!(
                RewardPeriod::<TestRuntime>::get().reward_amount,
                reward_period.reward_amount
            );

            System::assert_last_event(Event::RewardAmountPerPeriodSet { new_amount }.into());
        });
    }

    #[test]
    fn new_reward_amount_is_applied_on_next_period() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let new_amount = reward_period.reward_amount + 1;

            assert_ok!(NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::RewardAmountPerPeriod(new_amount),
            ));

            assert_eq!(
                RewardPeriod::<TestRuntime>::get().reward_amount,
                reward_period.reward_amount
            );

            roll_forward((reward_period.length as u64 - System::block_number()) + 1);

            assert_eq!(RewardPeriod::<TestRuntime>::get().reward_amount, new_amount);
        });
    }

    mod fails_to_be_set_when {
        use super::*;

        #[test]
        fn amount_is_zero() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let new_amount: BalanceOf<TestRuntime> = 0u128;

                let config = AdminConfig::RewardAmountPerPeriod(new_amount);
                assert_noop!(
                    NodeManager::set_admin_config(RawOrigin::Root.into(), config,),
                    Error::<TestRuntime>::RewardAmountPerPeriodZero
                );
            });
        }
    }
}

mod num_periods_to_mint {
    use super::*;

    #[test]
    fn can_be_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let new_periods = 5u32;

            let config = AdminConfig::NumPeriodsToMint(new_periods);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::NumPeriodsToMintSet { periods: new_periods }.into());
        });
    }

    #[test]
    fn can_be_set_to_zero_to_disable_future_minting() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let config = AdminConfig::NumPeriodsToMint(0u32);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));
            assert_eq!(NumPeriodsToMint::<TestRuntime>::get(), 0u32);
            System::assert_last_event(Event::NumPeriodsToMintSet { periods: 0u32 }.into());
        });
    }
}

mod reward_enabled {
    use super::*;

    #[test]
    fn can_be_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let current_flag = RewardEnabled::<TestRuntime>::get();
            let new_flag = !current_flag;

            let config = AdminConfig::RewardToggle(new_flag);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::RewardToggled { enabled: new_flag }.into());
        });
    }
}

mod min_uptime_threshold {
    use super::*;

    #[test]
    fn can_be_set_for_next_period_only() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let current_threshold = reward_period.uptime_threshold;
            let new_threshold = Perbill::from_percent(50);

            assert_ok!(NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::MinUptimeThreshold(new_threshold),
            ));

            assert_eq!(MinUptimeThreshold::<TestRuntime>::get(), Some(new_threshold));
            assert_eq!(RewardPeriod::<TestRuntime>::get().uptime_threshold, current_threshold);

            System::assert_last_event(
                Event::MinUptimeThresholdSet { threshold: new_threshold }.into(),
            );
        });
    }

    #[test]
    fn new_min_threshold_is_applied_on_next_period() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let new_threshold = Perbill::from_percent(50);

            assert_ok!(NodeManager::set_admin_config(
                RawOrigin::Root.into(),
                AdminConfig::MinUptimeThreshold(new_threshold),
            ));

            let expected_next_threshold = NodeManager::calculate_uptime_threshold(
                reward_period.length,
                reward_period.heartbeat_period,
            );

            roll_forward((reward_period.length as u64 - System::block_number()) + 1);

            assert_eq!(
                RewardPeriod::<TestRuntime>::get().uptime_threshold,
                expected_next_threshold
            );
        });
    }
}
