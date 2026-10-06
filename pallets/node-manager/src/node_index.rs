// Copyright 2026 Aventus DAO.

//! Dense index of registered nodes.
//!
//! `NodeIndex` maps `0..NodeIndexCount` to node ids with no holes, and `NodeIndexOf` is the
//! reverse lookup. Removal is a swap-remove: the last entry moves into the vacated slot. This lets
//! other pallets pick nodes uniformly at random with O(1) storage reads per pick instead of
//! iterating `NodeRegistry`.
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

    /// Appends `node` to the dense index. No-op if it is already indexed.
    pub(crate) fn index_insert(node: &NodeId<T>) {
        if NodeIndexOf::<T>::contains_key(node) {
            return
        }

        let position = NodeIndexCount::<T>::get();
        NodeIndex::<T>::insert(position, node);
        NodeIndexOf::<T>::insert(node, position);
        NodeIndexCount::<T>::put(position.saturating_add(1));
    }

    /// Removes `node` from the dense index with a swap-remove. No-op if it was never indexed
    /// (a node registered before the index existed and not yet backfilled).
    pub(crate) fn index_remove(node: &NodeId<T>) {
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
    }

    /// Adds pre-existing registered nodes to the dense index. All-or-nothing.
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
            ensure!(NodeRegistry::<T>::contains_key(node), Error::<T>::NodeNotRegistered);
            // Also rejects duplicates within `nodes`: the first occurrence indexes it.
            ensure!(!NodeIndexOf::<T>::contains_key(node), Error::<T>::NodeAlreadyIndexed);
            Self::index_insert(node);
        }

        if indexed == total {
            Self::deposit_event(Event::NodeIndexBackfillCompleted { count: indexed });
        } else {
            Self::deposit_event(Event::NodeIndexBackfillProgress { indexed, total });
        }

        Ok(())
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

        Ok(())
    }
}
