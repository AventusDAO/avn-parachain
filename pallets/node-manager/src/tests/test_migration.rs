// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{migration::NodeManagerMigrations, mock::*, *};
use frame_support::traits::{GetStorageVersion, OnRuntimeUpgrade};

fn account(id: u8) -> AccountId {
    TestAccount::new([id; 32]).account_id()
}

/// Gives `owner` the nodes `ids` in `OwnedNodes` only, leaving `OwnedNodesCount` alone.
fn own_nodes(owner: u8, ids: &[u8]) {
    for id in ids {
        <OwnedNodes<TestRuntime>>::insert(account(owner), account(*id), ());
    }
}

fn ext() -> sp_io::TestExternalities {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| StorageVersion::new(0).put::<NodeManager>());
    ext
}

#[test]
fn v1_recounts_owned_nodes_and_removes_owners_without_nodes() {
    ext().execute_with(|| {
        own_nodes(10, &[100, 101]);
        own_nodes(11, &[102]);
        own_nodes(12, &[103, 104, 105]);
        // Right, wrong, missing and left at 0 after the owner lost all their nodes.
        <OwnedNodesCount<TestRuntime>>::insert(account(10), 2);
        <OwnedNodesCount<TestRuntime>>::insert(account(11), 5);
        <OwnedNodesCount<TestRuntime>>::insert(account(13), 0);

        NodeManagerMigrations::<TestRuntime>::on_runtime_upgrade();

        assert_eq!(<OwnedNodesCount<TestRuntime>>::get(account(10)), 2);
        assert_eq!(<OwnedNodesCount<TestRuntime>>::get(account(11)), 1);
        assert_eq!(<OwnedNodesCount<TestRuntime>>::get(account(12)), 3);
        assert!(!<OwnedNodesCount<TestRuntime>>::contains_key(account(13)));
        assert_eq!(<OwnedNodesCount<TestRuntime>>::iter().count(), 3);
        assert_eq!(NodeManager::on_chain_storage_version(), STORAGE_VERSION);
    });
}

#[test]
fn v1_does_not_run_again() {
    ext().execute_with(|| {
        NodeManagerMigrations::<TestRuntime>::on_runtime_upgrade();
        <OwnedNodesCount<TestRuntime>>::insert(account(13), 0);

        NodeManagerMigrations::<TestRuntime>::on_runtime_upgrade();

        assert!(<OwnedNodesCount<TestRuntime>>::contains_key(account(13)));
    });
}
