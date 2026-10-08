// Copyright 2026 Aventus DAO Ltd

//! Quorum based selection of a single value out of a set of votes.
//!
//! Collators each submit the value they observed (for example the latest block of an external
//! chain). Views differ slightly, so the runtime has to settle on one value once enough votes
//! exist. This module holds the pure selection step shared by pallet-eth-bridge and the
//! watchtower floor oracle.

use sp_std::prelude::*;

/// Outcome of [`select_with_quorum`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuorumSelection<V> {
    /// Fewer than `supermajority` votes exist. Nothing is decided yet.
    BelowThreshold,
    /// Enough votes exist and this value was selected.
    Selected(V),
    /// Enough votes exist but no bucket satisfied the quorum rule. Only possible when `quorum`
    /// is zero or `supermajority` is smaller than `quorum`.
    Unresolved,
}

/// Select one value out of `(value, vote_count)` buckets.
///
/// Algorithm:
/// 1. If the total vote count is below `supermajority`, return `BelowThreshold`.
/// 2. Sort the buckets ascending by value.
/// 3. Start with `remaining = supermajority`. For each bucket, in order, subtract its vote count
///    (saturating at zero). The first bucket where `remaining < quorum` is `Selected`.
/// 4. If no bucket satisfies the rule, return `Unresolved`.
///
/// In other words the selected value is the smallest `v` for which the number of votes at or
/// below `v` exceeds `supermajority - quorum`. With the AVN policy (supermajority = 2n/3,
/// quorum = n - 2n/3) that is the value at least roughly a third of the voters have reached.
pub fn select_with_quorum<V: Ord + Copy>(
    votes: &[(V, usize)],
    supermajority: usize,
    quorum: usize,
) -> QuorumSelection<V> {
    let total_votes: usize = votes.iter().map(|(_, count)| *count).sum();
    if total_votes < supermajority {
        return QuorumSelection::BelowThreshold
    }

    let mut sorted: Vec<(V, usize)> = votes.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut remaining = supermajority;
    for (value, count) in sorted {
        remaining = remaining.saturating_sub(count);
        if remaining < quorum {
            return QuorumSelection::Selected(value)
        }
    }

    QuorumSelection::Unresolved
}

#[cfg(test)]
mod tests {
    use super::*;

    // AVN policy for n validators.
    fn avn(n: usize) -> (usize, usize) {
        let supermajority = n * 2 / 3;
        let quorum = n - supermajority;
        (supermajority, quorum)
    }

    #[test]
    fn eth_bridge_example_picks_third_bucket() {
        // 6 validators, 4 votes with distinct values. Supermajority 4, quorum 2.
        let (sm, q) = avn(6);
        let votes = [(100u32, 1), (200, 1), (300, 1), (400, 1)];
        assert_eq!(select_with_quorum(&votes, sm, q), QuorumSelection::Selected(300));
    }

    #[test]
    fn below_threshold_until_supermajority() {
        let (sm, q) = avn(6);
        let votes = [(100u32, 1), (200, 1), (300, 1)];
        assert_eq!(select_with_quorum(&votes, sm, q), QuorumSelection::BelowThreshold);
    }

    #[test]
    fn input_order_does_not_matter() {
        let (sm, q) = avn(6);
        let votes = [(400u32, 1), (100, 1), (300, 1), (200, 1)];
        assert_eq!(select_with_quorum(&votes, sm, q), QuorumSelection::Selected(300));
    }

    #[test]
    fn single_bucket_with_all_votes() {
        let (sm, q) = avn(6);
        let votes = [(500u32, 4)];
        assert_eq!(select_with_quorum(&votes, sm, q), QuorumSelection::Selected(500));
    }

    #[test]
    fn ties_resolve_to_lower_bucket_when_enough_votes() {
        // Supermajority 4, quorum 2. Two votes at 100 leave remaining 2 (not < 2), two votes at
        // 200 leave remaining 0 -> 200.
        let (sm, q) = avn(6);
        let votes = [(100u32, 2), (200, 2)];
        assert_eq!(select_with_quorum(&votes, sm, q), QuorumSelection::Selected(200));
        // Three votes at 100 leave remaining 1 (< 2) -> 100.
        let votes = [(100u32, 3), (200, 1)];
        assert_eq!(select_with_quorum(&votes, sm, q), QuorumSelection::Selected(100));
    }

    #[test]
    fn more_votes_than_supermajority() {
        let (sm, q) = avn(6);
        let votes = [(100u32, 1), (200, 1), (300, 1), (400, 1), (500, 1), (600, 1)];
        // remaining: 3, 2, 1 -> selected at 300.
        assert_eq!(select_with_quorum(&votes, sm, q), QuorumSelection::Selected(300));
    }

    #[test]
    fn single_validator_resolves_on_first_vote() {
        // n = 1: supermajority 0, quorum 1. remaining 0 < 1 immediately.
        let (sm, q) = avn(1);
        assert_eq!(select_with_quorum(&[(42u32, 1)], sm, q), QuorumSelection::Selected(42));
    }

    #[test]
    fn two_validators_resolve_on_first_vote() {
        // n = 2: supermajority 1, quorum 1.
        let (sm, q) = avn(2);
        assert_eq!(select_with_quorum(&[(42u32, 1)], sm, q), QuorumSelection::Selected(42));
    }

    #[test]
    fn zero_quorum_is_unresolved() {
        // n = 0: supermajority 0, quorum 0. remaining can never be < 0.
        let (sm, q) = avn(0);
        assert_eq!(select_with_quorum(&[(42u32, 1)], sm, q), QuorumSelection::Unresolved);
    }

    #[test]
    fn empty_input() {
        let (sm, q) = avn(6);
        assert_eq!(select_with_quorum::<u32>(&[], sm, q), QuorumSelection::BelowThreshold);
        assert_eq!(select_with_quorum::<u32>(&[], 0, 0), QuorumSelection::Unresolved);
    }
}
