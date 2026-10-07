// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok, BoundedVec};

fn registrar() -> AccountId {
    TestAccount::new([1u8; 32]).account_id()
}

fn owner() -> AccountId {
    TestAccount::new([209u8; 32]).account_id()
}

fn node(id: u8) -> AccountId {
    TestAccount::new([id; 32]).account_id()
}

fn setup_registrar() {
    <NodeRegistrar<TestRuntime>>::set(Some(registrar()));
}

fn register(id: u8) -> AccountId {
    let node_id = node(id);
    assert_ok!(NodeManager::register_node(
        RuntimeOrigin::signed(registrar()),
        node_id,
        owner(),
        UintAuthorityId(id as u64),
    ));
    node_id
}

/// Writes a node straight into the registry, as if it had been registered before the dense
/// index existed.
fn register_unindexed(id: u8) -> AccountId {
    let node_id = node(id);
    let stake = StakeInfo::<BalanceOf<TestRuntime>>::new(0, 0, None, UnstakeRestriction::Locked);
    <NodeRegistry<TestRuntime>>::insert(
        node_id,
        NodeInfo::new(owner(), UintAuthorityId(id as u64), id as u32, 0u64, false, stake),
    );
    <SigningKeyToNodeId<TestRuntime>>::insert(UintAuthorityId(id as u64), node_id);
    <OwnedNodes<TestRuntime>>::insert(owner(), node_id, ());
    <OwnedNodesCount<TestRuntime>>::mutate(owner(), |c| *c += 1);
    <TotalRegisteredNodes<TestRuntime>>::mutate(|t| *t += 1);
    node_id
}

fn deregister(nodes: Vec<AccountId>) {
    assert_ok!(NodeManager::deregister_nodes(
        RuntimeOrigin::signed(registrar()),
        owner(),
        BoundedVec::truncate_from(nodes),
    ));
}

fn backfill(nodes: Vec<AccountId>) -> DispatchResult {
    NodeManager::backfill_node_index(
        RuntimeOrigin::signed(registrar()),
        BoundedVec::truncate_from(nodes),
    )
}

fn assert_index(expected: &[AccountId]) {
    assert_eq!(<NodeIndexCount<TestRuntime>>::get(), expected.len() as u32);
    for (i, n) in expected.iter().enumerate() {
        assert_eq!(<NodeIndex<TestRuntime>>::get(i as u32), Some(*n), "position {}", i);
        assert_eq!(<NodeIndexOf<TestRuntime>>::get(n), Some(i as u32), "reverse of {}", i);
    }
    assert_eq!(<NodeIndex<TestRuntime>>::get(expected.len() as u32), None);
    assert_ok!(NodeManager::check_node_index_invariants());
}

mod registration {
    use super::*;

    #[test]
    fn appends_to_index() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let a = register(10);
            let b = register(11);
            let c = register(12);

            assert_index(&[a, b, c]);
            assert!(NodeManager::node_index_is_complete());
            assert_eq!(NodeManager::node_at_index(1), Some(b));
            assert_eq!(NodeManager::node_at_index(3), None);
            assert_eq!(NodeManager::indexed_node_count(), 3);
        });
    }

    #[test]
    fn re_register_after_deregister_appends_at_end() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let a = register(10);
            let b = register(11);
            deregister(vec![a]);
            assert_index(&[b]);

            // The signing key was freed by deregistration, so the same id can register again.
            let a_again = register(10);
            assert_index(&[b, a_again]);
        });
    }
}

mod deregistration {
    use super::*;

    #[test]
    fn middle_entry_is_swap_removed() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let a = register(10);
            let b = register(11);
            let c = register(12);

            deregister(vec![b]);

            // c moved into b's slot
            assert_index(&[a, c]);
            assert_eq!(<NodeIndexOf<TestRuntime>>::get(b), None);
        });
    }

    #[test]
    fn first_entry_is_swap_removed() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let a = register(10);
            let b = register(11);
            let c = register(12);

            deregister(vec![a]);
            assert_index(&[c, b]);
        });
    }

    #[test]
    fn last_entry_is_removed_without_moves() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let a = register(10);
            let b = register(11);
            let c = register(12);

            deregister(vec![c]);
            assert_index(&[a, b]);
        });
    }

    #[test]
    fn batch_deregistration_empties_the_index() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let nodes: Vec<AccountId> = (10..30u8).map(register).collect();
            assert_index(&nodes);

            deregister(nodes);
            assert_index(&[]);
            assert!(NodeManager::node_index_is_complete());
        });
    }

    #[test]
    fn unindexed_node_leaves_index_untouched() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let legacy = register_unindexed(10);
            let a = register(11);
            assert_eq!(<TotalRegisteredNodes<TestRuntime>>::get(), 2);
            assert_eq!(<NodeIndexCount<TestRuntime>>::get(), 1);
            assert!(!NodeManager::node_index_is_complete());

            deregister(vec![legacy]);

            assert_eq!(<TotalRegisteredNodes<TestRuntime>>::get(), 1);
            assert_index(&[a]);
            assert!(NodeManager::node_index_is_complete());
        });
    }
}

mod backfill {
    use super::*;

    #[test]
    fn indexes_legacy_nodes_in_pages_and_completes() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let legacy: Vec<AccountId> = (10..15u8).map(register_unindexed).collect();
            let fresh = register(20);
            assert_eq!(<TotalRegisteredNodes<TestRuntime>>::get(), 6);
            assert_index(&[fresh]);
            assert!(!NodeManager::node_index_is_complete());

            assert_ok!(backfill(legacy[0..3].to_vec()));
            System::assert_last_event(
                Event::NodeIndexBackfillProgress { indexed: 4, total: 6 }.into(),
            );
            assert_eq!(<NodeIndexCount<TestRuntime>>::get(), 4);
            assert!(!NodeManager::node_index_is_complete());

            assert_ok!(backfill(legacy[3..5].to_vec()));
            System::assert_last_event(Event::NodeIndexBackfillCompleted { count: 6 }.into());
            assert_index(&[fresh, legacy[0], legacy[1], legacy[2], legacy[3], legacy[4]]);
            assert!(NodeManager::node_index_is_complete());

            assert_noop!(
                backfill(vec![legacy[0]]),
                Error::<TestRuntime>::NodeIndexBackfillComplete
            );
        });
    }

    #[test]
    fn rejects_unregistered_node_and_applies_nothing() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let legacy = register_unindexed(10);
            let _other = register_unindexed(11);
            let not_registered = node(99);

            assert_noop!(
                backfill(vec![legacy, not_registered]),
                Error::<TestRuntime>::NodeNotRegistered
            );
            assert_eq!(<NodeIndexCount<TestRuntime>>::get(), 0);
            assert_eq!(<NodeIndexOf<TestRuntime>>::get(legacy), None);
        });
    }

    #[test]
    fn rejects_already_indexed_node_and_applies_nothing() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let legacy = register_unindexed(10);
            let _other = register_unindexed(11);
            let fresh = register(20);

            assert_noop!(backfill(vec![legacy, fresh]), Error::<TestRuntime>::NodeAlreadyIndexed);
            assert_index(&[fresh]);
        });
    }

    #[test]
    fn rejects_list_larger_than_remaining_unindexed_nodes() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let legacy = register_unindexed(10);
            let fresh = register(20);
            // One node remains to be indexed, so any list of two must be rejected up front,
            // even one whose first entry is valid.
            let not_registered = node(99);

            assert_noop!(
                backfill(vec![legacy, not_registered]),
                Error::<TestRuntime>::TooManyNodesToIndex
            );
            assert_index(&[fresh]);
            assert_eq!(<NodeIndexOf<TestRuntime>>::get(legacy), None);
        });
    }

    #[test]
    fn rejects_duplicates_in_the_list() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let legacy = register_unindexed(10);
            let _other = register_unindexed(11);

            assert_noop!(backfill(vec![legacy, legacy]), Error::<TestRuntime>::NodeAlreadyIndexed);
            assert_eq!(<NodeIndexCount<TestRuntime>>::get(), 0);
        });
    }

    #[test]
    fn rejects_empty_list() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let _legacy = register_unindexed(10);
            assert_noop!(backfill(vec![]), Error::<TestRuntime>::EmptyNodeList);
        });
    }

    #[test]
    fn only_registrar_can_call() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let legacy = register_unindexed(10);
            assert_noop!(
                NodeManager::backfill_node_index(
                    RuntimeOrigin::signed(owner()),
                    BoundedVec::truncate_from(vec![legacy]),
                ),
                Error::<TestRuntime>::OriginNotRegistrar
            );
        });
    }

    #[test]
    fn nodes_registered_between_calls_are_not_double_indexed() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            setup_registrar();
            let legacy: Vec<AccountId> = (10..13u8).map(register_unindexed).collect();

            assert_ok!(backfill(vec![legacy[0]]));
            // Registered while the backfill is in progress: indexed immediately.
            let fresh = register(20);
            assert_eq!(<TotalRegisteredNodes<TestRuntime>>::get(), 4);
            assert_eq!(<NodeIndexCount<TestRuntime>>::get(), 2);

            assert_ok!(backfill(vec![legacy[1], legacy[2]]));
            System::assert_last_event(Event::NodeIndexBackfillCompleted { count: 4 }.into());
            assert_index(&[legacy[0], fresh, legacy[1], legacy[2]]);
        });
    }
}
