//Copyright 2025 Truth Network.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::DispatchError;

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
    fn can_be_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let reward_period = RewardPeriod::<TestRuntime>::get();
            let new_period = reward_period.length + 1;

            let config = AdminConfig::RewardPeriod(new_period);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(
                Event::RewardPeriodLengthSet {
                    period_index: reward_period.current,
                    old_reward_period_length: new_period - 1,
                    new_reward_period_length: new_period,
                }
                .into(),
            );
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
    fn can_be_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let current_period = HeartbeatPeriod::<TestRuntime>::get();
            let new_heartbeat_period = current_period + 1;

            let config = AdminConfig::Heartbeat(new_heartbeat_period);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::HeartbeatPeriodSet { new_heartbeat_period }.into());
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
        fn period_is_longer_than_reward_period() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let reward_period = RewardPeriod::<TestRuntime>::get().length;
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

mod reward_amount {
    use super::*;

    #[test]
    fn can_be_set() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let current_amount = RewardAmount::<TestRuntime>::get();
            let new_amount = current_amount + 1;

            let config = AdminConfig::RewardAmount(new_amount);
            assert_ok!(NodeManager::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::RewardAmountSet { new_amount }.into());
        });
    }

    mod fails_to_be_set_when {
        use super::*;
        #[test]
        fn amount_is_zero() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let new_amount: BalanceOf<TestRuntime> = 0u128;

                let config = AdminConfig::RewardAmount(new_amount);
                assert_noop!(
                    NodeManager::set_admin_config(RawOrigin::Root.into(), config,),
                    Error::<TestRuntime>::RewardAmountZero
                );
            });
        }
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
