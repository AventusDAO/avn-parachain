// Copyright 2026 Aventus DAO.

//! Random committee selection for internal proposals.
//!
//! A proposal that requests a committee of `n` members gets exactly `n`, at most one per node
//! owner: `n` distinct owners are drawn at random from the dense owner index exposed by
//! `NodesInterface`, then one node at random from each chosen owner's nodes. An owner running
//! many nodes holds one seat, like an owner running one. The members are stored in
//! `ProposalCommittee` and the size in `ProposalCommitteeSize` for as long as the proposal
//! exists. A committee is never smaller than requested: a proposal requesting more seats than
//! the owners can fill is cancelled at activation. Submission does not check the seats, so a
//! consumer that submits from its own critical path (summary's `end_voting`) is never failed
//! by a drop in owners; consumers check the configured size when it is set
//! (`ensure_committee_ready`). While the index is incomplete the proposal waits
//! (`committee_ready`), and a corrupt index is an error.
//!
//! The seats per owner are set by `MAX_SEATS_PER_OWNER` and applied by `seats_for`. Picking an
//! owner's nodes already works for any number of seats; counting the seats
//! (`seats_available`) and spreading them over owners (`select_committee`) do not yet, and a
//! compile-time check stops the cap being raised before they do.

use crate::*;
use frame_support::traits::Randomness;
use sp_std::collections::btree_set::BTreeSet;

pub const COMMITTEE_RANDOMNESS_CONTEXT: &'static [u8] = b"wt_committee";

/// Most committee seats one owner can hold, however many nodes they run.
pub const MAX_SEATS_PER_OWNER: u32 = 1;

// `seats_available` counts owners and `select_committee` draws one owner per seat. Both are
// only right for one seat per owner: a larger cap needs the sum of `seats_for` over every
// owner (kept as a counter by the node provider) and seats spread evenly over owners.
const _: () = assert!(MAX_SEATS_PER_OWNER == 1, "committee selection assumes one seat per owner");

impl<T: Config> Pallet<T> {
    /// True if `who` may vote on `proposal_id` as far as committee membership is concerned.
    /// Proposals without a committee accept every node.
    pub fn is_committee_member(proposal_id: ProposalId, who: &T::AccountId) -> bool {
        !ProposalCommitteeSize::<T>::contains_key(proposal_id) ||
            ProposalCommittee::<T>::contains_key(proposal_id, who)
    }

    /// The seat `node` would vote from on `proposal_id` at the current block, or why it cannot
    /// vote. Shared by the transaction pool, the vote itself and the consumers' offchain
    /// workers, so they agree on who may vote.
    ///
    /// A committee proposal takes at most as many votes as its committee has seats, members'
    /// and trusted owners' alike: once trusted votes have filled the empty seats, a member who
    /// had not voted is refused.
    ///
    /// A trusted owner votes once per proposal: through a committee seat or as a trusted owner,
    /// not both.
    pub fn voter_seat(
        proposal_id: ProposalId,
        node: &T::AccountId,
    ) -> Result<VoterSeat<T::AccountId>, Error<T>> {
        let Some(committee_size) = ProposalCommitteeSize::<T>::get(proposal_id) else {
            return Ok(VoterSeat::Open)
        };
        let trusted_owner = Self::trusted_owner_of(node);

        if ProposalCommittee::<T>::contains_key(proposal_id, node) {
            if let Some(owner) = &trusted_owner {
                ensure!(
                    !TrustedVoters::<T>::contains_key(proposal_id, owner),
                    Error::<T>::AlreadyVoted
                );
            }
            ensure!(Self::votes_cast(proposal_id) < committee_size, Error::<T>::NoEmptySeat);
            return Ok(match trusted_owner {
                Some(owner) => VoterSeat::TrustedCommittee { owner },
                None => VoterSeat::Committee,
            })
        }

        let owner = trusted_owner.ok_or(Error::<T>::NotInCommitteeOrTrusted)?;
        ensure!(Self::trusted_voting_open(proposal_id), Error::<T>::TrustedVotingNotOpen);
        ensure!(!TrustedVoters::<T>::contains_key(proposal_id, &owner), Error::<T>::AlreadyVoted);
        ensure!(Self::votes_cast(proposal_id) < committee_size, Error::<T>::NoEmptySeat);

        Ok(VoterSeat::Trusted { owner })
    }

    /// True if `node` may vote on `proposal_id` at the current block. For offchain workers
    /// deciding whether to vote; the pool refuses votes for which this is false.
    pub fn can_vote(proposal_id: ProposalId, node: &T::AccountId) -> bool {
        Self::voter_seat(proposal_id, node).is_ok()
    }

    /// `node`'s owner, if that owner is trusted.
    fn trusted_owner_of(node: &T::AccountId) -> Option<T::AccountId> {
        let trusted = TrustedOwners::<T>::get();
        if trusted.is_empty() {
            return None
        }
        T::Watchtowers::get_node_owner(node).filter(|owner| trusted.binary_search(owner).is_ok())
    }

    /// True once trusted owners may vote on `proposal_id`: `TrustedVotingStart` of its voting
    /// period, counted from activation, has passed.
    pub(crate) fn trusted_voting_open(proposal_id: ProposalId) -> bool {
        let Some(start) = TrustedVotingStart::<T>::get() else { return false };
        let Some(proposal) = Proposals::<T>::get(proposal_id) else { return false };
        let Some(end_at) = proposal.end_at else { return false };

        // Activation sets `end_at` to the activation block plus `vote_duration`.
        let activated_at = end_at.saturating_sub(proposal.vote_duration.into());
        let opens_at = activated_at.saturating_add(start.mul_ceil(proposal.vote_duration).into());
        <frame_system::Pallet<T>>::block_number() >= opens_at
    }

    /// Votes cast on a committee proposal so far. Node votes weigh 1, so it is also the number
    /// of seats taken.
    pub(crate) fn votes_cast(proposal_id: ProposalId) -> u32 {
        match VotingKindOf::<T>::get(proposal_id) {
            VotingKind::Binary => {
                let votes = Votes::<T>::get(proposal_id);
                votes.in_favors.saturating_add(votes.againsts)
            },
            VotingKind::Choice => ChoiceStandings::<T>::get(proposal_id).cast,
        }
    }

    /// Checks that a committee of exactly `size` members can be built right now. The bounds
    /// (`MinCommitteeSize`, `MaxCommitteeSize`) are checked when the proposal is submitted.
    /// Cheap enough for `validate_unsigned` and the OCW.
    pub(crate) fn committee_ready(size: u32) -> Result<(), Error<T>> {
        let indexed = T::Watchtowers::get_indexed_nodes_count();
        let total = T::Watchtowers::get_authorized_watchtowers_count();
        // Sampling from a partially backfilled index would bias the committee.
        ensure!(indexed == total, Error::<T>::NodeIndexNotReady);

        ensure!(size <= Self::seats_available(), Error::<T>::NotEnoughSeatsForCommittee);

        Ok(())
    }

    /// Readiness of the committee a proposal requests. `Ok(None)` for proposals without one,
    /// `Ok(Some(size))` when it can be built now, otherwise the reason it cannot.
    pub(crate) fn committee_ready_for(proposal: &Proposal<T>) -> Result<Option<u32>, Error<T>> {
        proposal
            .committee_size
            .map(|size| Self::committee_ready(size).map(|()| size))
            .transpose()
    }

    /// The seats an owner with `nodes` indexed nodes can hold.
    fn seats_for(nodes: u32) -> u32 {
        nodes.min(MAX_SEATS_PER_OWNER)
    }

    /// The largest committee the indexed owners can fill: `seats_for` summed over every owner.
    /// Every indexed owner has at least one node, so with one seat each it is the owner count.
    fn seats_available() -> u32 {
        T::Watchtowers::get_indexed_owners_count()
    }

    /// Selects and stores a committee of exactly `size` nodes for `proposal_id`, where
    /// `committee_ready(size)` passed in the same block.
    ///
    /// Must run inside a transactional context: on error, rows already written must be rolled
    /// back by the caller's storage layer.
    pub(crate) fn select_committee(proposal_id: ProposalId, size: u32) -> Result<(), Error<T>> {
        let owners = T::Watchtowers::get_indexed_owners_count();
        ensure!(size <= owners, Error::<T>::NodeIndexCorrupt);

        let (seed, _) =
            T::Randomness::random(&(COMMITTEE_RANDOMNESS_CONTEXT, proposal_id).encode());

        // One seat per owner, so `size` distinct owners.
        let owner_positions = Self::sample_indices(&seed, b"owners", owners, size);
        ensure!(owner_positions.len() as u32 == size, Error::<T>::NodeIndexCorrupt);

        let mut seated_owners = BTreeSet::new();
        for position in owner_positions {
            let (owner, nodes) =
                T::Watchtowers::get_owner_at_index(position).ok_or(Error::<T>::NodeIndexCorrupt)?;
            // Distinct positions must give distinct owners, or an owner would hold extra seats.
            ensure!(seated_owners.insert(owner.clone()), Error::<T>::NodeIndexCorrupt);
            // An indexed owner always has a node.
            let seats = Self::seats_for(nodes);
            ensure!(seats > 0, Error::<T>::NodeIndexCorrupt);

            for index in Self::sample_indices(&seed, (b"nodes", position), nodes, seats) {
                let node = T::Watchtowers::get_owner_node_at_index(&owner, index)
                    .ok_or(Error::<T>::NodeIndexCorrupt)?;
                // Distinct positions must give distinct nodes.
                ensure!(
                    !ProposalCommittee::<T>::contains_key(proposal_id, &node),
                    Error::<T>::NodeIndexCorrupt
                );
                ProposalCommittee::<T>::insert(proposal_id, &node, ());
            }
        }

        Ok(())
    }

    /// Exactly `min(k, n)` distinct values in `0..n`. Below `n` it uses Floyd's algorithm:
    /// exactly `k` draws, with no allocation proportional to `n`. `stream` keeps the draws of
    /// separate samples from the same seed independent.
    fn sample_indices(seed: &T::Hash, stream: impl Encode, n: u32, k: u32) -> BTreeSet<u32> {
        if k >= n {
            return (0..n).collect()
        }

        let mut chosen = BTreeSet::new();
        for (counter, j) in (n - k..n).enumerate() {
            let draw = (Self::draw(seed, &stream, counter as u32) % (j as u64 + 1)) as u32;
            if !chosen.insert(draw) {
                chosen.insert(j);
            }
        }
        chosen
    }

    fn draw(seed: &T::Hash, stream: &impl Encode, counter: u32) -> u64 {
        let hash = sp_io::hashing::blake2_256(&(seed, stream, counter).encode());
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&hash[..8]);
        u64::from_le_bytes(bytes)
    }
}
