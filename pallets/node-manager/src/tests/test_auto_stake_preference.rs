// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;

#[derive(Clone)]
struct Context {
    registrar: AccountId,
    owner: AccountId,
    node_id: AccountId,
    signing_key: <mock::TestRuntime as pallet::Config>::SignerId,
}

impl Default for Context {
    fn default() -> Self {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));

        Context {
            registrar,
            owner: TestAccount::new([101u8; 32]).account_id(),
            node_id: TestAccount::new([202u8; 32]).account_id(),
            signing_key: <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(None),
        }
    }
}

fn register_node(context: &Context) {
    assert_ok!(NodeManager::register_node(
        RuntimeOrigin::signed(context.registrar.clone()),
        context.node_id.clone(),
        context.owner.clone(),
        context.signing_key.clone(),
    ));
}

mod update_auto_stake_preference {
    use super::*;

    mod succeeds {
        use super::*;

        #[test]
        fn when_owner_enables_auto_stake() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                register_node(&context);

                assert_ok!(NodeManager::update_auto_stake_preference(
                    RuntimeOrigin::signed(context.owner.clone()),
                    context.node_id.clone(),
                    true,
                ));

                let node_info = NodeRegistry::<TestRuntime>::get(&context.node_id).unwrap();
                assert!(node_info.auto_stake_rewards);

                System::assert_last_event(
                    Event::AutoStakePreferenceUpdated {
                        owner: context.owner,
                        node_id: context.node_id,
                        auto_stake_rewards: true,
                    }
                    .into(),
                );
            });
        }

        #[test]
        fn when_owner_disables_auto_stake() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                register_node(&context);

                // Enable first, then disable
                assert_ok!(NodeManager::update_auto_stake_preference(
                    RuntimeOrigin::signed(context.owner.clone()),
                    context.node_id.clone(),
                    true,
                ));

                assert_ok!(NodeManager::update_auto_stake_preference(
                    RuntimeOrigin::signed(context.owner.clone()),
                    context.node_id.clone(),
                    false,
                ));

                let node_info = NodeRegistry::<TestRuntime>::get(&context.node_id).unwrap();
                assert!(!node_info.auto_stake_rewards);

                System::assert_last_event(
                    Event::AutoStakePreferenceUpdated {
                        owner: context.owner,
                        node_id: context.node_id,
                        auto_stake_rewards: false,
                    }
                    .into(),
                );
            });
        }

        #[test]
        fn when_preference_is_set_to_same_value() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                register_node(&context);

                // Set to true (default after registration) again — should still succeed
                assert_ok!(NodeManager::update_auto_stake_preference(
                    RuntimeOrigin::signed(context.owner.clone()),
                    context.node_id.clone(),
                    true,
                ));

                let node_info = NodeRegistry::<TestRuntime>::get(&context.node_id).unwrap();
                assert!(node_info.auto_stake_rewards);
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                register_node(&context);

                assert_noop!(
                    NodeManager::update_auto_stake_preference(
                        RawOrigin::None.into(),
                        context.node_id.clone(),
                        true,
                    ),
                    DispatchError::BadOrigin
                );
            });
        }

        #[test]
        fn origin_is_root() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                register_node(&context);

                assert_noop!(
                    NodeManager::update_auto_stake_preference(
                        RawOrigin::Root.into(),
                        context.node_id.clone(),
                        true,
                    ),
                    DispatchError::BadOrigin
                );
            });
        }

        #[test]
        fn caller_is_not_the_node_owner() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                register_node(&context);

                let non_owner = TestAccount::new([77u8; 32]).account_id();
                assert_noop!(
                    NodeManager::update_auto_stake_preference(
                        RuntimeOrigin::signed(non_owner),
                        context.node_id.clone(),
                        true,
                    ),
                    Error::<TestRuntime>::NodeNotOwnedByOwner
                );
            });
        }

        #[test]
        fn registrar_calls_on_behalf_of_owner() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                register_node(&context);

                assert_noop!(
                    NodeManager::update_auto_stake_preference(
                        RuntimeOrigin::signed(context.registrar.clone()),
                        context.node_id.clone(),
                        true,
                    ),
                    Error::<TestRuntime>::NodeNotOwnedByOwner
                );
            });
        }

        #[test]
        fn node_does_not_exist() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                // Node is never registered

                assert_noop!(
                    NodeManager::update_auto_stake_preference(
                        RuntimeOrigin::signed(context.owner.clone()),
                        context.node_id.clone(),
                        true,
                    ),
                    Error::<TestRuntime>::NodeNotOwnedByOwner
                );
            });
        }
    }
}
