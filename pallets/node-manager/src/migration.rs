// Copyright 2026 Aventus DAO.

use crate::{Config, OwnedNodes, OwnedNodesCount, Pallet, STORAGE_VERSION};
use frame_support::{
    pallet_prelude::PhantomData,
    traits::{Get, GetStorageVersion, OnRuntimeUpgrade, StorageVersion},
    weights::Weight,
};
use sp_std::{collections::btree_map::BTreeMap, vec::Vec};

#[cfg(feature = "try-runtime")]
use codec::{Decode, Encode};
#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

/// The number of nodes each account owns, counted from `OwnedNodes`.
fn count_owned_nodes<T: Config>() -> (BTreeMap<T::AccountId, u32>, u64) {
    let mut counts = BTreeMap::<T::AccountId, u32>::new();
    let mut nodes = 0u64;
    for (owner, _node) in OwnedNodes::<T>::iter_keys() {
        nodes = nodes.saturating_add(1);
        let count = counts.entry(owner).or_default();
        *count = count.saturating_add(1);
    }
    (counts, nodes)
}

/// Migration v0 -> v1: `OwnedNodesCount` is recounted from `OwnedNodes`. Before v1 an owner who
/// moved or deregistered all their nodes kept an entry of 0; those entries are removed, and any
/// count that differs from the owner's nodes is corrected.
pub mod v1 {
    use super::*;

    const V1_STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

    pub struct Migration<T>(core::marker::PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            let current = StorageVersion::get::<Pallet<T>>();
            log::warn!("🚧 🚧 Running node manager v1 migration. Current version: {:?}", current);

            if current != 0 {
                log::warn!(
                    "🚧 🚧 v1 migration skipped: expected storage version 0, found {:?}",
                    current,
                );
                return T::DbWeight::get().reads(1)
            }

            let (mut actual, nodes) = count_owned_nodes::<T>();
            let stored: Vec<(T::AccountId, u32)> = OwnedNodesCount::<T>::iter().collect();

            let (mut removed, mut corrected) = (0u64, 0u64);
            for (owner, count) in stored.iter() {
                match actual.remove(owner) {
                    Some(n) if n == *count => {},
                    Some(n) => {
                        OwnedNodesCount::<T>::insert(owner, n);
                        corrected = corrected.saturating_add(1);
                    },
                    None => {
                        OwnedNodesCount::<T>::remove(owner);
                        removed = removed.saturating_add(1);
                    },
                }
            }
            // Owners with nodes but no entry at all.
            for (owner, n) in actual {
                OwnedNodesCount::<T>::insert(owner, n);
                corrected = corrected.saturating_add(1);
            }

            V1_STORAGE_VERSION.put::<Pallet<T>>();
            log::info!(
                "✅ v1 migration: removed {} empty owner counts, corrected {}",
                removed,
                corrected
            );

            let reads = 1 + nodes + stored.len() as u64;
            T::DbWeight::get().reads_writes(reads, 1 + removed + corrected)
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            frame_support::ensure!(
                StorageVersion::get::<Pallet<T>>() == 0,
                TryRuntimeError::Other("expected storage version 0 before migration")
            );
            let (_, nodes) = count_owned_nodes::<T>();
            Ok(nodes.encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let nodes_before = u64::decode(&mut &state[..])
                .map_err(|_| TryRuntimeError::Other("invalid pre-upgrade state"))?;
            frame_support::ensure!(
                StorageVersion::get::<Pallet<T>>() == 1,
                TryRuntimeError::Other("expected storage version 1 after migration")
            );
            let (actual, nodes) = count_owned_nodes::<T>();
            frame_support::ensure!(
                nodes == nodes_before,
                TryRuntimeError::Other("owned nodes changed during migration")
            );
            let stored: BTreeMap<T::AccountId, u32> = OwnedNodesCount::<T>::iter().collect();
            frame_support::ensure!(
                stored == actual,
                TryRuntimeError::Other("owner counts do not match owned nodes")
            );
            Ok(())
        }
    }
}

/// All node manager migrations. Wire this into the runtime `Migrations` tuple.
pub struct NodeManagerMigrations<T>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for NodeManagerMigrations<T> {
    fn on_runtime_upgrade() -> Weight {
        let onchain = Pallet::<T>::on_chain_storage_version();
        let mut weight = T::DbWeight::get().reads(1);

        if onchain < 1 {
            weight = weight.saturating_add(v1::Migration::<T>::on_runtime_upgrade());
        }

        debug_assert!(Pallet::<T>::on_chain_storage_version() == STORAGE_VERSION);
        weight
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        if Pallet::<T>::on_chain_storage_version() < 1 {
            return v1::Migration::<T>::pre_upgrade()
        }
        Ok(sp_std::vec![])
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        frame_support::ensure!(
            Pallet::<T>::on_chain_storage_version() == STORAGE_VERSION,
            TryRuntimeError::Other("storage version not updated")
        );
        if !state.is_empty() {
            v1::Migration::<T>::post_upgrade(state)?;
        }
        Ok(())
    }
}
