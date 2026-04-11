// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;

#[derive(Clone)]
struct Context {
    origin: RuntimeOrigin,
    owner: AccountId,
    node_id: AccountId,
    signing_key: <mock::TestRuntime as pallet::Config>::SignerId,
}

impl Default for Context {
    fn default() -> Self {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        setup_registrar(&registrar);

        Context {
            origin: RuntimeOrigin::signed(registrar.clone()),
            owner: TestAccount::new([101u8; 32]).account_id(),
            node_id: TestAccount::new([202u8; 32]).account_id(),
            signing_key: <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(None),
        }
    }
}

fn setup_registrar(registrar: &AccountId) {
    <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));
}

mod node_registration {
    use super::*;

    #[test]
    fn succeeds() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            assert_ok!(NodeManager::register_node(
                context.origin,
                context.node_id,
                context.owner,
                context.signing_key.clone(),
            ));

            // The node is owned by the owner
            assert!(<OwnedNodes<TestRuntime>>::get(&context.owner, &context.node_id).is_some());
            // The node is registered
            let node_info = <NodeRegistry<TestRuntime>>::get(&context.node_id);
            assert!(node_info.is_some());
            // Total node counter is increased
            assert_eq!(<TotalRegisteredNodes<TestRuntime>>::get(), 1);

            let node_info = node_info.unwrap();
            assert_eq!(node_info.owner, context.owner);
            assert_eq!(node_info.signing_key, context.signing_key);
            assert_eq!(node_info.stake.amount, 0);
            assert_eq!(node_info.stake.unlocked_stake, 0);
            assert_eq!(node_info.stake.next_unstake_time_sec, None);
            assert_eq!(node_info.stake.restriction, UnstakeRestriction::Locked);

            // The correct event is emitted
            System::assert_last_event(
                Event::NodeRegistered { owner: context.owner, node: context.node_id }.into(),
            );
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn registrar_is_not_set() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                // Setup accounts BUT do not set the registrar
                let registrar = TestAccount::new([1u8; 32]).account_id();
                let context = Context {
                    origin: RuntimeOrigin::signed(registrar.clone()),
                    owner: TestAccount::new([101u8; 32]).account_id(),
                    node_id: TestAccount::new([202u8; 32]).account_id(),
                    signing_key: <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(
                        None,
                    ),
                };

                assert_noop!(
                    NodeManager::register_node(
                        context.origin,
                        context.node_id,
                        context.owner,
                        context.signing_key,
                    ),
                    Error::<TestRuntime>::RegistrarNotSet
                );
            });
        }

        #[test]
        fn sender_is_not_registrar() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                let bad_origin = RuntimeOrigin::signed(context.owner.clone());
                assert_noop!(
                    NodeManager::register_node(
                        bad_origin,
                        context.node_id,
                        context.owner,
                        context.signing_key,
                    ),
                    Error::<TestRuntime>::OriginNotRegistrar
                );
            });
        }

        #[test]
        fn node_is_already_registered() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                assert_ok!(NodeManager::register_node(
                    context.origin.clone(),
                    context.node_id.clone(),
                    context.owner.clone(),
                    context.signing_key.clone(),
                ));

                assert_noop!(
                    NodeManager::register_node(
                        context.origin,
                        context.node_id,
                        context.owner,
                        context.signing_key,
                    ),
                    Error::<TestRuntime>::DuplicateNode
                );
            });
        }
    }
}

mod bonus_node_registration {
    use super::*;

    #[test]
    fn succeeds() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            assert_ok!(NodeManager::register_bonus_node(
                context.origin,
                context.node_id,
                context.owner.clone(),
                context.signing_key.clone(),
            ));

            assert!(<OwnedNodes<TestRuntime>>::get(&context.owner, &context.node_id).is_some());
            assert_eq!(<TotalRegisteredNodes<TestRuntime>>::get(), 1);
            assert_eq!(<TotalRegisteredBonusNodes<TestRuntime>>::get(), 1);

            let node_info = <NodeRegistry<TestRuntime>>::get(&context.node_id).unwrap();
            assert_eq!(node_info.owner, context.owner);
            assert_eq!(node_info.signing_key, context.signing_key);
            assert_eq!(node_info.stake.amount, 0);

            System::assert_last_event(
                Event::NodeRegistered { owner: context.owner, node: context.node_id }.into(),
            );
        });
    }

    #[test]
    fn serial_starts_at_bonus_node_serial_start() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            assert_ok!(NodeManager::register_bonus_node(
                context.origin,
                context.node_id,
                context.owner,
                context.signing_key,
            ));

            let node_info = <NodeRegistry<TestRuntime>>::get(&context.node_id).unwrap();
            assert_eq!(
                node_info.serial_number,
                <TestRuntime as pallet::Config>::BonusNodeSerialStart::get()
            );
        });
    }

    #[test]
    fn serial_increments_independently_from_regular_nodes() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let context = Context::default();

            let regular_node = TestAccount::new([10u8; 32]).account_id();
            let regular_key = <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(None);
            assert_ok!(NodeManager::register_node(
                context.origin.clone(),
                regular_node.clone(),
                context.owner.clone(),
                regular_key,
            ));

            assert_ok!(NodeManager::register_bonus_node(
                context.origin,
                context.node_id,
                context.owner,
                context.signing_key,
            ));

            let regular_info = <NodeRegistry<TestRuntime>>::get(&regular_node).unwrap();
            let bonus_info = <NodeRegistry<TestRuntime>>::get(&context.node_id).unwrap();

            // Regular node gets the next value from the regular counter (starts at 0)
            assert_eq!(regular_info.serial_number, 0);
            // Bonus node gets the next value from the bonus counter (starts at
            // BonusNodeSerialStart)
            assert_eq!(
                bonus_info.serial_number,
                <TestRuntime as pallet::Config>::BonusNodeSerialStart::get()
            );
        });
    }

    #[test]
    fn serial_is_outside_genesis_bonus_ranges() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            assert_ok!(NodeManager::register_bonus_node(
                context.origin,
                context.node_id,
                context.owner,
                context.signing_key,
            ));

            let node_info = <NodeRegistry<TestRuntime>>::get(&context.node_id).unwrap();
            let bonus_50_range = <GenesisBonus50<TestRuntime>>::get();
            let bonus_25_range = <GenesisBonus25<TestRuntime>>::get();

            assert!(!bonus_50_range.contains(&node_info.serial_number));
            assert!(!bonus_25_range.contains(&node_info.serial_number));
        });
    }

    #[test]
    fn register_node_blocked_at_serial_limit_but_bonus_node_is_not() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let limit = <TestRuntime as pallet::Config>::BonusNodeSerialStart::get();

            // Force the regular serial counter to the limit
            <NextNodeSerialNumber<TestRuntime>>::put(limit);

            // register_node must now fail
            assert_noop!(
                NodeManager::register_node(
                    context.origin.clone(),
                    context.node_id.clone(),
                    context.owner.clone(),
                    context.signing_key.clone(),
                ),
                Error::<TestRuntime>::NodeSerialLimitReached
            );

            // register_bonus_node must still succeed
            assert_ok!(NodeManager::register_bonus_node(
                context.origin,
                context.node_id,
                context.owner,
                context.signing_key,
            ));
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn registrar_is_not_set() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let registrar = TestAccount::new([1u8; 32]).account_id();
                let context = Context {
                    origin: RuntimeOrigin::signed(registrar),
                    owner: TestAccount::new([101u8; 32]).account_id(),
                    node_id: TestAccount::new([202u8; 32]).account_id(),
                    signing_key: <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(
                        None,
                    ),
                };

                assert_noop!(
                    NodeManager::register_bonus_node(
                        context.origin,
                        context.node_id,
                        context.owner,
                        context.signing_key,
                    ),
                    Error::<TestRuntime>::RegistrarNotSet
                );
            });
        }

        #[test]
        fn sender_is_not_registrar() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                let bad_origin = RuntimeOrigin::signed(context.owner.clone());
                assert_noop!(
                    NodeManager::register_bonus_node(
                        bad_origin,
                        context.node_id,
                        context.owner,
                        context.signing_key,
                    ),
                    Error::<TestRuntime>::OriginNotRegistrar
                );
            });
        }

        #[test]
        fn node_is_already_registered() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                assert_ok!(NodeManager::register_bonus_node(
                    context.origin.clone(),
                    context.node_id.clone(),
                    context.owner.clone(),
                    context.signing_key.clone(),
                ));

                assert_noop!(
                    NodeManager::register_bonus_node(
                        context.origin,
                        context.node_id,
                        context.owner,
                        context.signing_key,
                    ),
                    Error::<TestRuntime>::DuplicateNode
                );
            });
        }

        #[test]
        fn signing_key_already_in_use() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                assert_ok!(NodeManager::register_bonus_node(
                    context.origin.clone(),
                    context.node_id,
                    context.owner.clone(),
                    context.signing_key.clone(),
                ));

                let node_2 = TestAccount::new([5u8; 32]).account_id();
                assert_noop!(
                    NodeManager::register_bonus_node(
                        context.origin,
                        node_2,
                        context.owner,
                        context.signing_key,
                    ),
                    Error::<TestRuntime>::SigningKeyAlreadyInUse
                );
            });
        }
    }
}

mod rotating_signing_key {
    use super::*;

    mod works {
        use super::*;

        #[test]
        fn when_registrar_sends_tx() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                assert_ok!(NodeManager::register_node(
                    context.origin,
                    context.node_id,
                    context.owner,
                    context.signing_key,
                ));

                let old_info = NodeRegistry::<TestRuntime>::get(&context.node_id).unwrap();
                let new_signing_key =
                    <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(None);
                assert_ok!(NodeManager::update_signing_key(
                    RuntimeOrigin::signed(NodeRegistrar::<TestRuntime>::get().unwrap()),
                    context.node_id.clone(),
                    new_signing_key.clone(),
                ));

                let info = NodeRegistry::<TestRuntime>::get(&context.node_id).unwrap();
                assert_ne!(info.signing_key, old_info.signing_key);
                assert_eq!(info.signing_key, new_signing_key);

                // The correct event is emitted
                System::assert_last_event(
                    Event::SigningKeyUpdated { owner: context.owner, node: context.node_id }.into(),
                );
            })
        }

        #[test]
        fn when_node_owner_sends_tx() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                assert_ok!(NodeManager::register_node(
                    context.origin,
                    context.node_id,
                    context.owner,
                    context.signing_key,
                ));

                let old_info = NodeRegistry::<TestRuntime>::get(&context.node_id).unwrap();

                let new_signing_key =
                    <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(None);
                assert_ok!(NodeManager::update_signing_key(
                    RuntimeOrigin::signed(context.owner.clone()),
                    context.node_id.clone(),
                    new_signing_key.clone(),
                ));

                let info = NodeRegistry::<TestRuntime>::get(&context.node_id).unwrap();
                assert_ne!(info.signing_key, old_info.signing_key);
                assert_eq!(info.signing_key, new_signing_key);

                // The correct event is emitted
                System::assert_last_event(
                    Event::SigningKeyUpdated { owner: context.owner, node: context.node_id }.into(),
                );
            })
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                assert_ok!(NodeManager::register_node(
                    context.origin,
                    context.node_id,
                    context.owner,
                    context.signing_key,
                ));

                let new_signing_key =
                    <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(None);
                assert_noop!(
                    NodeManager::update_signing_key(
                        RawOrigin::None.into(),
                        context.node_id.clone(),
                        new_signing_key.clone(),
                    ),
                    DispatchError::BadOrigin
                );
            })
        }

        #[test]
        fn origin_is_invalid() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                assert_ok!(NodeManager::register_node(
                    context.origin,
                    context.node_id,
                    context.owner,
                    context.signing_key,
                ));

                let new_signing_key =
                    <mock::TestRuntime as pallet::Config>::SignerId::generate_pair(None);

                let bad_origin = RuntimeOrigin::signed(TestAccount::new([45u8; 32]).account_id());
                assert_noop!(
                    NodeManager::update_signing_key(
                        bad_origin,
                        context.node_id.clone(),
                        new_signing_key.clone(),
                    ),
                    Error::<TestRuntime>::UnauthorizedSigningKeyUpdate
                );
            })
        }

        #[test]
        fn signing_not_changed() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                assert_ok!(NodeManager::register_node(
                    context.origin,
                    context.node_id,
                    context.owner,
                    context.signing_key.clone(),
                ));

                let bad_signing_key = context.signing_key.clone();
                assert_noop!(
                    NodeManager::update_signing_key(
                        RuntimeOrigin::signed(NodeRegistrar::<TestRuntime>::get().unwrap()),
                        context.node_id.clone(),
                        bad_signing_key.clone(),
                    ),
                    Error::<TestRuntime>::SigningKeyMustBeDifferent
                );
            })
        }

        #[test]
        fn signing_key_already_in_use() {
            ExtBuilder::build_default()
                .with_genesis_config()
                .as_externality()
                .execute_with(|| {
                    let context = Context::default();
                    let owner_2 = TestAccount::new([3u8; 32]).account_id();
                    let node_2 = TestAccount::new([5u8; 32]).account_id();

                    assert_ok!(NodeManager::register_node(
                        context.origin.clone(),
                        context.node_id,
                        context.owner,
                        context.signing_key.clone(),
                    ));

                    assert_noop!(
                        NodeManager::register_node(
                            context.origin,
                            node_2,
                            owner_2,
                            context.signing_key,
                        ),
                        Error::<TestRuntime>::SigningKeyAlreadyInUse
                    );
                });
        }

        #[test]
        fn reverse_index_points_to_a_different_node() {
            ExtBuilder::build_default()
                .with_genesis_config()
                .as_externality()
                .execute_with(|| {
                    let context = Context::default();

                    let owner_b = TestAccount::new([3u8; 32]).account_id();
                    let node_b = TestAccount::new([5u8; 32]).account_id();
                    let key_b = UintAuthorityId(11);

                    assert_ok!(NodeManager::register_node(
                        context.origin.clone(),
                        context.node_id,
                        context.owner,
                        context.signing_key.clone(),
                    ));

                    assert_ok!(NodeManager::register_node(
                        context.origin.clone(),
                        node_b.clone(),
                        owner_b.clone(),
                        key_b,
                    ));

                    // Corrupt storage: map key_a to node_b, so removing key_a for node_a should
                    // fail.
                    SigningKeyToNodeId::<TestRuntime>::insert(context.signing_key, node_b.clone());

                    assert_noop!(
                        NodeManager::update_signing_key(
                            RuntimeOrigin::signed(context.owner),
                            context.node_id,
                            UintAuthorityId(12),
                        ),
                        Error::<TestRuntime>::InvalidSigningKey
                    );
                });
        }
    }
}
