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
                context.signing_key,
            ));

            // The node is owned by the owner
            assert!(<OwnedNodes<TestRuntime>>::get(&context.owner, &context.node_id).is_some());
            // The node is registered
            assert!(<NodeRegistry<TestRuntime>>::get(&context.node_id).is_some());
            // Total node counter is increased
            assert_eq!(<TotalRegisteredNodes<TestRuntime>>::get(), 1);
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
    }
}
