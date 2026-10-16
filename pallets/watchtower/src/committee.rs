// Copyright 2026 Aventus DAO.

//! Random committee selection for internal proposals.
//!
//! A proposal that requests a committee gets `min(requested, n)` distinct nodes chosen at random
//! from the dense node index exposed by `NodesInterface`. The members are stored in
//! `ProposalCommittee` and the effective size in `ProposalCommitteeSize` for as long as the
//! proposal exists. Selection never degrades silently: if the index is incomplete or too small
//! the proposal is not activated (`committee_ready`), and a corrupt index is an error.

use crate::*;
use frame_support::traits::Randomness;
use sp_std::collections::btree_set::BTreeSet;

pub const COMMITTEE_RANDOMNESS_CONTEXT: &'static [u8] = b"wt_committee";

impl<T: Config> Pallet<T> {
    /// True if `who` may vote on `proposal_id` as far as committee membership is concerned.
    /// Proposals without a committee accept every node.
    pub fn is_committee_member(proposal_id: ProposalId, who: &T::AccountId) -> bool {
        !ProposalCommitteeSize::<T>::contains_key(proposal_id) ||
            ProposalCommittee::<T>::contains_key(proposal_id, who)
    }

    /// Checks that a committee of `requested` nodes can be built right now and returns the
    /// effective size `min(requested, n)`. Cheap enough for `validate_unsigned` and the OCW.
    pub(crate) fn committee_ready(requested: u32) -> Result<u32, Error<T>> {
        let indexed = T::Watchtowers::get_indexed_nodes_count();
        let total = T::Watchtowers::get_authorized_watchtowers_count();
        // Sampling from a partially backfilled index would bias the committee.
        ensure!(indexed == total, Error::<T>::NodeIndexNotReady);

        let effective = requested.min(indexed);
        ensure!(effective >= T::MinCommitteeSize::get(), Error::<T>::NotEnoughNodesForCommittee);

        Ok(effective)
    }

    /// Readiness of the committee a proposal requests. `Ok(None)` for proposals without one,
    /// `Ok(Some(effective))` when it can be built now, otherwise the reason it cannot.
    pub(crate) fn committee_ready_for(proposal: &Proposal<T>) -> Result<Option<u32>, Error<T>> {
        proposal.committee_size.map(Self::committee_ready).transpose()
    }

    /// Selects and stores a committee of exactly `effective` nodes for `proposal_id`, where
    /// `effective` comes from `committee_ready` for the same block.
    ///
    /// Must run inside a transactional context: on error, rows already written must be rolled
    /// back by the caller's storage layer.
    pub(crate) fn select_committee(
        proposal_id: ProposalId,
        effective: u32,
    ) -> Result<(), Error<T>> {
        let n = T::Watchtowers::get_indexed_nodes_count();
        ensure!(effective <= n, Error::<T>::NodeIndexCorrupt);

        let indices: BTreeSet<u32> = if effective == n {
            (0..n).collect()
        } else {
            Self::sample_indices(proposal_id, n, effective)
        };
        ensure!(indices.len() as u32 == effective, Error::<T>::NodeIndexCorrupt);

        for index in indices {
            let node =
                T::Watchtowers::get_node_at_index(index).ok_or(Error::<T>::NodeIndexCorrupt)?;
            // Distinct indices must give distinct nodes.
            ensure!(
                !ProposalCommittee::<T>::contains_key(proposal_id, &node),
                Error::<T>::NodeIndexCorrupt
            );
            ProposalCommittee::<T>::insert(proposal_id, &node, ());
        }

        Ok(())
    }

    /// Floyd's algorithm: exactly `k` distinct values in `0..n` using exactly `k` draws, with no
    /// allocation proportional to `n`. Requires `k < n`.
    fn sample_indices(proposal_id: ProposalId, n: u32, k: u32) -> BTreeSet<u32> {
        let (seed, _) =
            T::Randomness::random(&(COMMITTEE_RANDOMNESS_CONTEXT, proposal_id).encode());

        let mut chosen = BTreeSet::new();
        for (counter, j) in (n.saturating_sub(k)..n).enumerate() {
            let draw = (Self::draw(&seed, counter as u32) % (j as u64 + 1)) as u32;
            if !chosen.insert(draw) {
                chosen.insert(j);
            }
        }
        chosen
    }

    fn draw(seed: &T::Hash, counter: u32) -> u64 {
        let hash = sp_io::hashing::blake2_256(&(seed, counter).encode());
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&hash[..8]);
        u64::from_le_bytes(bytes)
    }
}
