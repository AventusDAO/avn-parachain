// Copyright 2026 Aventus DAO.

//! Dense indexes of registered nodes and of their owners.
//!
//! `NodeIndex` maps `0..NodeIndexCount` to node ids with no holes, and `NodeIndexOf` is the
//! reverse lookup. Removal is a swap-remove: the last entry moves into the vacated slot. This lets
//! other pallets pick nodes uniformly at random with O(1) storage reads per pick instead of
//! iterating `NodeRegistry`.
//!
//! The same nodes are also indexed by owner, so that other pallets can pick owners at random and
//! then nodes of a given owner: `OwnerIndex` maps `0..OwnerIndexCount` to every owner with at
//! least one indexed node (and how many), and `OwnerNodeIndex` maps `(owner, 0..nodes)` to that
//! owner's nodes. Both use swap-remove; an owner leaves `OwnerIndex` with their last node. A
//! node is in the owner indexes exactly when it is in `NodeIndex`.
//!
//! Nodes registered before the index existed are added by the registrar through
//! `backfill_node_index`; until that finishes `NodeIndexCount < TotalRegisteredNodes`.

use crate::*;

impl<T: Config> Pallet<T> {
    /// Node at position `index` of the dense index, if any.
    pub fn node_at_index(index: u32) -> Option<NodeId<T>> {
        NodeIndex::<T>::get(index)
    }

    /// Number of nodes in the dense index.
    pub fn indexed_node_count() -> u32 {
        NodeIndexCount::<T>::get()
    }

    /// True once every registered node is in the dense index.
    pub fn node_index_is_complete() -> bool {
        NodeIndexCount::<T>::get() == TotalRegisteredNodes::<T>::get()
    }

    /// Owner at position `index` of the dense owner index, and how many indexed nodes they have.
    pub fn owner_at_index(index: u32) -> Option<(T::AccountId, u32)> {
        OwnerIndex::<T>::get(index).map(|entry| (entry.owner, entry.nodes))
    }

    /// Number of owners in the dense owner index.
    pub fn indexed_owner_count() -> u32 {
        OwnerIndexCount::<T>::get()
    }

    /// Node at position `index` of `owner`'s node index, if any.
    pub fn owner_node_at_index(owner: &T::AccountId, index: u32) -> Option<NodeId<T>> {
        OwnerNodeIndex::<T>::get(owner, index)
    }

    /// Appends `node`, owned by `owner`, to the dense indexes. No-op if it is already indexed.
    pub(crate) fn index_insert(node: &NodeId<T>, owner: &T::AccountId) {
        if NodeIndexOf::<T>::contains_key(node) {
            return
        }

        let position = NodeIndexCount::<T>::get();
        NodeIndex::<T>::insert(position, node);
        NodeIndexOf::<T>::insert(node, position);
        NodeIndexCount::<T>::put(position.saturating_add(1));

        Self::owner_index_insert(node, owner);
    }

    /// Removes `node`, owned by `owner`, from the dense indexes with a swap-remove. No-op if it
    /// was never indexed (a node registered before the index existed and not yet backfilled).
    pub(crate) fn index_remove(node: &NodeId<T>, owner: &T::AccountId) {
        let Some(position) = NodeIndexOf::<T>::take(node) else {
            return;
        };

        let count = NodeIndexCount::<T>::get();
        let last = count.saturating_sub(1);

        if position != last {
            match NodeIndex::<T>::get(last) {
                Some(last_node) => {
                    NodeIndex::<T>::insert(position, &last_node);
                    NodeIndexOf::<T>::insert(&last_node, position);
                },
                None => {
                    // Only reachable if storage was tampered with. Leave a hole rather than
                    // panic; consumers detect holes via `node_at_index` returning `None`.
                    log::error!(
                        "💔 Node index corrupt: no node at tail position {} while removing {:?}",
                        last,
                        node
                    );
                    NodeIndex::<T>::remove(position);
                },
            }
        }

        NodeIndex::<T>::remove(last);
        NodeIndexCount::<T>::put(last);

        Self::owner_index_remove(node, owner);
    }

    /// Moves an indexed `node` from `old_owner`'s entries to `new_owner`'s. Its position in
    /// `NodeIndex` is unchanged. No-op for a node that is not indexed yet.
    pub(crate) fn index_change_owner(
        node: &NodeId<T>,
        old_owner: &T::AccountId,
        new_owner: &T::AccountId,
    ) {
        if !NodeIndexOf::<T>::contains_key(node) {
            return
        }
        Self::owner_index_remove(node, old_owner);
        Self::owner_index_insert(node, new_owner);
    }

    /// Appends `node` to `owner`'s node index, adding `owner` to the owner index if it is their
    /// first indexed node.
    fn owner_index_insert(node: &NodeId<T>, owner: &T::AccountId) {
        let (position, mut entry) = match OwnerIndexOf::<T>::get(owner) {
            Some(position) => match OwnerIndex::<T>::get(position) {
                Some(entry) => (position, entry),
                None => {
                    log::error!(
                        "💔 Owner index corrupt: no entry at position {} for {:?}",
                        position,
                        owner
                    );
                    return
                },
            },
            None => {
                let position = OwnerIndexCount::<T>::get();
                OwnerIndexOf::<T>::insert(owner, position);
                OwnerIndexCount::<T>::put(position.saturating_add(1));
                (position, IndexedOwner { owner: owner.clone(), nodes: 0 })
            },
        };

        OwnerNodeIndex::<T>::insert(owner, entry.nodes, node);
        OwnerNodeIndexOf::<T>::insert(node, entry.nodes);
        entry.nodes = entry.nodes.saturating_add(1);
        OwnerIndex::<T>::insert(position, entry);
    }

    /// Removes `node` from `owner`'s node index with a swap-remove, and `owner` from the owner
    /// index with a swap-remove once they have no indexed nodes left.
    fn owner_index_remove(node: &NodeId<T>, owner: &T::AccountId) {
        let Some(node_position) = OwnerNodeIndexOf::<T>::take(node) else {
            log::error!("💔 Owner index corrupt: indexed node {:?} has no owner entry", node);
            return
        };
        let Some(position) = OwnerIndexOf::<T>::get(owner) else {
            log::error!("💔 Owner index corrupt: {:?} is not indexed", owner);
            return
        };
        let Some(mut entry) = OwnerIndex::<T>::get(position) else {
            log::error!("💔 Owner index corrupt: no entry at position {}", position);
            return
        };

        let last_node = entry.nodes.saturating_sub(1);
        if node_position != last_node {
            match OwnerNodeIndex::<T>::get(owner, last_node) {
                Some(moved) => {
                    OwnerNodeIndex::<T>::insert(owner, node_position, &moved);
                    OwnerNodeIndexOf::<T>::insert(&moved, node_position);
                },
                None => {
                    log::error!(
                        "💔 Owner index corrupt: {:?} has no node at tail position {}",
                        owner,
                        last_node
                    );
                    OwnerNodeIndex::<T>::remove(owner, node_position);
                },
            }
        }
        OwnerNodeIndex::<T>::remove(owner, last_node);

        if last_node > 0 {
            entry.nodes = last_node;
            OwnerIndex::<T>::insert(position, entry);
            return
        }

        let last = OwnerIndexCount::<T>::get().saturating_sub(1);
        if position != last {
            match OwnerIndex::<T>::get(last) {
                Some(moved) => {
                    OwnerIndexOf::<T>::insert(&moved.owner, position);
                    OwnerIndex::<T>::insert(position, moved);
                },
                None => {
                    log::error!("💔 Owner index corrupt: no owner at tail position {}", last);
                    OwnerIndex::<T>::remove(position);
                },
            }
        }
        OwnerIndex::<T>::remove(last);
        OwnerIndexOf::<T>::remove(owner);
        OwnerIndexCount::<T>::put(last);
    }

    /// Adds pre-existing registered nodes to the dense indexes. All-or-nothing.
    pub(crate) fn do_backfill_node_index(
        nodes: &BoundedVec<NodeId<T>, MaxBackfillNodes>,
    ) -> DispatchResult {
        ensure!(!nodes.is_empty(), Error::<T>::EmptyNodeList);

        let total = TotalRegisteredNodes::<T>::get();
        let already_indexed = NodeIndexCount::<T>::get();
        ensure!(already_indexed < total, Error::<T>::NodeIndexBackfillComplete);

        // Reject an oversized batch before touching storage. Each node below is registered and
        // not yet indexed, so the count grows by exactly `nodes.len()`.
        let indexed = already_indexed.saturating_add(nodes.len() as u32);
        ensure!(indexed <= total, Error::<T>::TooManyNodesToIndex);

        for node in nodes.iter() {
            let info = NodeRegistry::<T>::get(node).ok_or(Error::<T>::NodeNotRegistered)?;
            // Also rejects duplicates within `nodes`: the first occurrence indexes it.
            ensure!(!NodeIndexOf::<T>::contains_key(node), Error::<T>::NodeAlreadyIndexed);
            Self::index_insert(node, &info.owner);
        }

        if indexed == total {
            Self::deposit_event(Event::NodeIndexBackfillCompleted { count: indexed });
        } else {
            Self::deposit_event(Event::NodeIndexBackfillProgress { indexed, total });
        }

        Ok(())
    }

    /// Registers `n` fresh nodes straight into the registry and the dense indexes, each with an
    /// owner of its own, for benchmarks of pallets that sample the indexes (e.g. watchtower
    /// committee selection).
    #[cfg(feature = "runtime-benchmarks")]
    pub fn benchmark_register_indexed_nodes(n: u32) {
        use frame_benchmarking::account;
        use sp_runtime::traits::Zero;

        let first = TotalRegisteredNodes::<T>::get();
        for i in first..first.saturating_add(n) {
            let owner: T::AccountId = account("index_owner", i, 0);
            let node: NodeId<T> = account("indexed_node", i, 0);
            let signing_key: T::SignerId = account("indexed_key", i, 0);
            let stake = StakeInfo::<BalanceOf<T>>::new(
                Zero::zero(),
                Zero::zero(),
                None,
                UnstakeRestriction::Locked,
            );
            NodeRegistry::<T>::insert(
                &node,
                NodeInfo::new(owner.clone(), signing_key, i, 0u64, false, stake),
            );
            Self::index_insert(&node, &owner);
        }
        TotalRegisteredNodes::<T>::mutate(|total| *total = total.saturating_add(n));
    }

    /// Checks the dense index invariants. Only meaningful once the backfill is complete.
    #[cfg(any(feature = "try-runtime", test))]
    pub fn check_node_index_invariants() -> Result<(), &'static str> {
        let count = NodeIndexCount::<T>::get();
        if !Self::node_index_is_complete() {
            return Ok(())
        }

        for position in 0..count {
            let node = NodeIndex::<T>::get(position).ok_or("hole in NodeIndex")?;
            if NodeIndexOf::<T>::get(&node) != Some(position) {
                return Err("NodeIndexOf does not match NodeIndex")
            }
            if !NodeRegistry::<T>::contains_key(&node) {
                return Err("indexed node is not registered")
            }
        }

        if NodeIndexOf::<T>::iter().count() as u32 != count {
            return Err("NodeIndexOf has a different length than NodeIndexCount")
        }
        if NodeIndex::<T>::iter().count() as u32 != count {
            return Err("NodeIndex has a different length than NodeIndexCount")
        }
        if NodeRegistry::<T>::iter_keys().any(|node| !NodeIndexOf::<T>::contains_key(&node)) {
            return Err("registered node missing from the index")
        }

        Self::check_owner_index_invariants(count)
    }

    /// Every indexed node appears once under its registered owner, with no holes.
    #[cfg(any(feature = "try-runtime", test))]
    fn check_owner_index_invariants(indexed_nodes: u32) -> Result<(), &'static str> {
        let owners = OwnerIndexCount::<T>::get();
        let mut owner_nodes = 0u32;

        for position in 0..owners {
            let entry = OwnerIndex::<T>::get(position).ok_or("hole in OwnerIndex")?;
            if OwnerIndexOf::<T>::get(&entry.owner) != Some(position) {
                return Err("OwnerIndexOf does not match OwnerIndex")
            }
            if entry.nodes == 0 {
                return Err("owner in OwnerIndex without nodes")
            }
            for node_position in 0..entry.nodes {
                let node = OwnerNodeIndex::<T>::get(&entry.owner, node_position)
                    .ok_or("hole in OwnerNodeIndex")?;
                if OwnerNodeIndexOf::<T>::get(&node) != Some(node_position) {
                    return Err("OwnerNodeIndexOf does not match OwnerNodeIndex")
                }
                if NodeRegistry::<T>::get(&node).map(|info| info.owner) != Some(entry.owner.clone())
                {
                    return Err("node indexed under an owner that does not own it")
                }
            }
            owner_nodes = owner_nodes.saturating_add(entry.nodes);
        }

        if owner_nodes != indexed_nodes {
            return Err("OwnerNodeIndex holds a different number of nodes than NodeIndex")
        }
        if OwnerIndex::<T>::iter().count() as u32 != owners ||
            OwnerIndexOf::<T>::iter().count() as u32 != owners
        {
            return Err("OwnerIndex has a different length than OwnerIndexCount")
        }
        if OwnerNodeIndex::<T>::iter().count() as u32 != indexed_nodes ||
            OwnerNodeIndexOf::<T>::iter().count() as u32 != indexed_nodes
        {
            return Err("OwnerNodeIndex has a different length than NodeIndex")
        }

        Ok(())
    }
}
