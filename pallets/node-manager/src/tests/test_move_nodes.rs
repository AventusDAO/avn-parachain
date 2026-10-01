// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};

/// Nodes are registered a few seconds apart, mirroring real life where every node has its own
/// `auto_stake_expiry` and therefore its own unlock grid.
const REGISTRATION_BASE_SEC: u64 = 100;
const REGISTRATION_STAGGER_SEC: u64 = 5;

struct Context {
    registrar: AccountId,
    owner: AccountId,
    new_owner: AccountId,
    nodes: Vec<NodeId<TestRuntime>>,
    /// Registration time (seconds) of each node, index-aligned with `nodes`.
    registered_at: Vec<u64>,
}

impl Context {
    fn new(num_nodes: u8) -> Self {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        let owner = TestAccount::new([10u8; 32]).account_id();
        let new_owner = TestAccount::new([20u8; 32]).account_id();

        <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));

        let mut ctx = Context { registrar, owner, new_owner, nodes: vec![], registered_at: vec![] };
        for i in 0..num_nodes {
            let at = REGISTRATION_BASE_SEC + (i as u64) * REGISTRATION_STAGGER_SEC;
            ctx.register_node_at(at);
        }
        ctx
    }

    /// Registers one more node for `owner` at `at_sec` and returns its index.
    fn register_node_at(&mut self, at_sec: u64) -> usize {
        let i = self.nodes.len() as u8;
        set_time(at_sec);
        let node = TestAccount::new([100u8 + i; 32]).account_id();
        let signing_key = UintAuthorityId((100 + i) as u64);
        assert_ok!(NodeManager::register_node(
            RuntimeOrigin::signed(self.registrar.clone()),
            node.clone(),
            self.owner.clone(),
            signing_key,
        ));
        self.nodes.push(node);
        self.registered_at.push(at_sec);
        self.nodes.len() - 1
    }

    fn expiry_of(&self, idx: usize) -> u64 {
        self.registered_at[idx] + AutoStakeDurationSec::<TestRuntime>::get()
    }

    fn restriction_end_of(&self, idx: usize) -> u64 {
        self.expiry_of(idx) + RestrictedUnstakeDurationSec::<TestRuntime>::get()
    }

    fn info(&self, idx: usize) -> NodeInfo<UintAuthorityId, AccountId, BalanceOf<TestRuntime>> {
        <NodeRegistry<TestRuntime>>::get(&self.nodes[idx]).unwrap()
    }

    fn move_stake(
        &self,
        sources: Vec<(usize, Option<BalanceOf<TestRuntime>>)>,
        to: usize,
    ) -> DispatchResult {
        NodeManager::move_stake(
            RuntimeOrigin::signed(self.registrar.clone()),
            self.owner.clone(),
            BoundedVec::truncate_from(
                sources.into_iter().map(|(i, a)| (self.nodes[i].clone(), a)).collect::<Vec<_>>(),
            ),
            self.nodes[to].clone(),
        )
    }

    fn move_nodes_with_stake(
        &self,
        idxs: &[usize],
        total: BalanceOf<TestRuntime>,
    ) -> DispatchResult {
        NodeManager::move_nodes_with_stake(
            RuntimeOrigin::signed(self.registrar.clone()),
            self.owner.clone(),
            self.new_owner.clone(),
            BoundedVec::truncate_from(
                idxs.iter().map(|i| self.nodes[*i].clone()).collect::<Vec<_>>(),
            ),
            total,
        )
    }
}

fn set_time(sec: u64) {
    Timestamp::set_timestamp(sec * 1000);
}

fn unstake_period() -> u64 {
    UnstakePeriodSec::<TestRuntime>::get()
}

/// Withdraws everything currently available from `node` and returns how much was withdrawn.
fn unstake_all_available(owner: &AccountId, node: &NodeId<TestRuntime>) -> BalanceOf<TestRuntime> {
    let before = <NodeRegistry<TestRuntime>>::get(node).unwrap().stake.amount;
    assert_ok!(NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), None));
    before - <NodeRegistry<TestRuntime>>::get(node).unwrap().stake.amount
}

fn add_stake_to_node(
    owner: &AccountId,
    node: &NodeId<TestRuntime>,
    amount: BalanceOf<TestRuntime>,
) {
    Balances::make_free_balance_be(owner, amount * 2);
    assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), amount));
}

// --- success cases ---

#[test]
fn move_single_node_without_stake_succeeds() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let ctx = Context::new(1);
        let node = ctx.nodes[0].clone();

        assert_ok!(NodeManager::move_nodes(
            RuntimeOrigin::signed(ctx.registrar),
            ctx.owner.clone(),
            ctx.new_owner.clone(),
            BoundedVec::truncate_from(vec![node.clone()]),
        ));

        assert!(!<OwnedNodes<TestRuntime>>::contains_key(&ctx.owner, &node));
        assert!(<OwnedNodes<TestRuntime>>::contains_key(&ctx.new_owner, &node));
        assert_eq!(<OwnedNodesCount<TestRuntime>>::get(&ctx.owner), 0);
        assert_eq!(<OwnedNodesCount<TestRuntime>>::get(&ctx.new_owner), 1);
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&node).unwrap().owner, ctx.new_owner);

        System::assert_last_event(
            Event::NodeMoved { old_owner: ctx.owner, new_owner: ctx.new_owner, node, stake: 0 }
                .into(),
        );
    });
}

#[test]
fn move_single_node_with_stake_transfers_funds_and_updates_total_stake() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let ctx = Context::new(1);
        let node = ctx.nodes[0].clone();
        let stake: BalanceOf<TestRuntime> = 1_000_000;

        add_stake_to_node(&ctx.owner, &node, stake);

        assert_ok!(NodeManager::move_nodes(
            RuntimeOrigin::signed(ctx.registrar),
            ctx.owner.clone(),
            ctx.new_owner.clone(),
            BoundedVec::truncate_from(vec![node.clone()]),
        ));

        assert_eq!(Balances::reserved_balance(&ctx.owner), 0);
        assert_eq!(Balances::reserved_balance(&ctx.new_owner), stake);
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.owner), Some(0));
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.new_owner), Some(stake));
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&node).unwrap().owner, ctx.new_owner);
    });
}

#[test]
fn move_multiple_nodes_updates_all_storage() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let ctx = Context::new(3);

        assert_ok!(NodeManager::move_nodes(
            RuntimeOrigin::signed(ctx.registrar),
            ctx.owner.clone(),
            ctx.new_owner.clone(),
            BoundedVec::truncate_from(ctx.nodes.clone()),
        ));

        for node in &ctx.nodes {
            assert!(!<OwnedNodes<TestRuntime>>::contains_key(&ctx.owner, node));
            assert!(<OwnedNodes<TestRuntime>>::contains_key(&ctx.new_owner, node));
            assert_eq!(<NodeRegistry<TestRuntime>>::get(node).unwrap().owner, ctx.new_owner);
        }
        assert_eq!(<OwnedNodesCount<TestRuntime>>::get(&ctx.owner), 0);
        assert_eq!(<OwnedNodesCount<TestRuntime>>::get(&ctx.new_owner), 3);
    });
}

// --- failure cases ---

#[test]
fn move_nodes_fails_when_caller_is_not_registrar() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let ctx = Context::new(1);
        let non_registrar = TestAccount::new([99u8; 32]).account_id();

        assert_noop!(
            NodeManager::move_nodes(
                RuntimeOrigin::signed(non_registrar),
                ctx.owner.clone(),
                ctx.new_owner.clone(),
                BoundedVec::truncate_from(ctx.nodes.clone()),
            ),
            Error::<TestRuntime>::OriginNotRegistrar
        );
    });
}

#[test]
fn move_nodes_fails_when_owners_are_the_same() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let ctx = Context::new(1);

        assert_noop!(
            NodeManager::move_nodes(
                RuntimeOrigin::signed(ctx.registrar),
                ctx.owner.clone(),
                ctx.owner.clone(),
                BoundedVec::truncate_from(ctx.nodes.clone()),
            ),
            Error::<TestRuntime>::NodeOwnersMustBeDifferent
        );
    });
}

#[test]
fn move_nodes_fails_when_node_not_owned_by_current_owner() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let ctx = Context::new(1);
        let wrong_owner = TestAccount::new([30u8; 32]).account_id();

        assert_noop!(
            NodeManager::move_nodes(
                RuntimeOrigin::signed(ctx.registrar),
                wrong_owner,
                ctx.new_owner.clone(),
                BoundedVec::truncate_from(ctx.nodes.clone()),
            ),
            Error::<TestRuntime>::NodeNotOwnedByOwner
        );
    });
}

#[test]
fn move_nodes_fails_when_node_does_not_exist() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        let owner = TestAccount::new([10u8; 32]).account_id();
        let new_owner = TestAccount::new([20u8; 32]).account_id();
        let ghost_node = TestAccount::new([77u8; 32]).account_id();
        <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));

        // Manually insert the ownership record without a NodeRegistry entry
        <OwnedNodes<TestRuntime>>::insert(&owner, &ghost_node, ());

        assert_noop!(
            NodeManager::move_nodes(
                RuntimeOrigin::signed(registrar),
                owner,
                new_owner,
                BoundedVec::truncate_from(vec![ghost_node]),
            ),
            Error::<TestRuntime>::NodeNotRegistered
        );
    });
}

#[test]
fn move_single_node_with_stake_to_brand_new_account_succeeds() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let ctx = Context::new(1);
        let node = ctx.nodes[0].clone();
        let stake: BalanceOf<TestRuntime> = 1_000_000;
        let brand_new_owner = TestAccount::new([255u8; 32]).account_id();

        add_stake_to_node(&ctx.owner, &node, stake);
        assert_eq!(Balances::total_balance(&brand_new_owner), 0);

        assert_ok!(NodeManager::move_nodes(
            RuntimeOrigin::signed(ctx.registrar),
            ctx.owner.clone(),
            brand_new_owner.clone(),
            BoundedVec::truncate_from(vec![node.clone()]),
        ));

        assert_eq!(Balances::reserved_balance(&ctx.owner), 0);
        assert_eq!(Balances::reserved_balance(&brand_new_owner), stake);
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.owner), Some(0));
        assert_eq!(<TotalStake<TestRuntime>>::get(&brand_new_owner), Some(stake));
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&node).unwrap().owner, brand_new_owner);
    });
}

#[test]
fn move_nodes_with_stake_to_brand_new_account_succeeds() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let stake: BalanceOf<TestRuntime> = 500_000;
        let brand_new_owner = TestAccount::new([255u8; 32]).account_id();

        for node in &ctx.nodes {
            add_stake_to_node(&ctx.owner, node, stake);
        }
        let total_stake = stake * ctx.nodes.len() as u128;

        assert_eq!(Balances::total_balance(&brand_new_owner), 0);

        assert_ok!(NodeManager::move_nodes_with_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            brand_new_owner.clone(),
            BoundedVec::truncate_from(ctx.nodes.clone()),
            total_stake,
        ));

        assert_eq!(Balances::reserved_balance(&ctx.owner), 0);
        assert_eq!(Balances::reserved_balance(&brand_new_owner), total_stake);
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.owner), Some(0));
        assert_eq!(<TotalStake<TestRuntime>>::get(&brand_new_owner), Some(total_stake));
        for node in &ctx.nodes {
            assert_eq!(<NodeRegistry<TestRuntime>>::get(node).unwrap().owner, brand_new_owner);
        }
    });
}

// ===== move_stake tests =====

fn ext() -> sp_io::TestExternalities {
    let (e, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    e
}

#[test]
fn move_stake_single_source_full_amount_succeeds() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let from_node = ctx.nodes[0].clone();
        let to_node = ctx.nodes[1].clone();
        let stake: BalanceOf<TestRuntime> = 1_000_000;

        add_stake_to_node(&ctx.owner, &from_node, stake);

        assert_ok!(NodeManager::move_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            BoundedVec::truncate_from(vec![(from_node.clone(), None)]),
            to_node.clone(),
        ));

        assert_eq!(<NodeRegistry<TestRuntime>>::get(&from_node).unwrap().stake.amount, 0);
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&to_node).unwrap().stake.amount, stake);
        // TotalStake must be unchanged
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.owner), Some(stake));
        // Reserved balance must be unchanged
        assert_eq!(Balances::reserved_balance(&ctx.owner), stake);

        System::assert_last_event(
            Event::StakeMoved { owner: ctx.owner, to_node, total_amount: stake }.into(),
        );
    });
}

#[test]
fn move_stake_multiple_sources_accumulate_into_to_node() {
    ext().execute_with(|| {
        let ctx = Context::new(3);
        let to_node = ctx.nodes[2].clone();
        let stake: BalanceOf<TestRuntime> = 500_000;

        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], stake);

        assert_ok!(NodeManager::move_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            BoundedVec::truncate_from(vec![
                (ctx.nodes[0].clone(), None),
                (ctx.nodes[1].clone(), None),
            ]),
            to_node.clone(),
        ));

        assert_eq!(<NodeRegistry<TestRuntime>>::get(&ctx.nodes[0]).unwrap().stake.amount, 0);
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&ctx.nodes[1]).unwrap().stake.amount, 0);
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&to_node).unwrap().stake.amount, stake * 2);
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.owner), Some(stake * 2));
    });
}

#[test]
fn move_stake_partial_amount_leaves_remainder_on_source() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let from_node = ctx.nodes[0].clone();
        let to_node = ctx.nodes[1].clone();
        let stake: BalanceOf<TestRuntime> = 1_000_000;
        let partial: BalanceOf<TestRuntime> = 300_000;

        add_stake_to_node(&ctx.owner, &from_node, stake);

        assert_ok!(NodeManager::move_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            BoundedVec::truncate_from(vec![(from_node.clone(), Some(partial))]),
            to_node.clone(),
        ));

        assert_eq!(
            <NodeRegistry<TestRuntime>>::get(&from_node).unwrap().stake.amount,
            stake - partial
        );
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&to_node).unwrap().stake.amount, partial);
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.owner), Some(stake));
    });
}

#[test]
fn move_stake_fails_when_amount_exceeds_source_stake() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let stake: BalanceOf<TestRuntime> = 100;

        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);

        assert_noop!(
            NodeManager::move_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                ctx.owner.clone(),
                BoundedVec::truncate_from(vec![(ctx.nodes[0].clone(), Some(stake + 1))]),
                ctx.nodes[1].clone(),
            ),
            Error::<TestRuntime>::InsufficientStakedBalance
        );
    });
}

#[test]
fn move_stake_fails_when_some_amount_is_zero() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let stake: BalanceOf<TestRuntime> = 1_000;

        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);

        assert_noop!(
            NodeManager::move_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                ctx.owner.clone(),
                BoundedVec::truncate_from(vec![(ctx.nodes[0].clone(), Some(0))]),
                ctx.nodes[1].clone(),
            ),
            Error::<TestRuntime>::ZeroAmount
        );
    });
}

#[test]
fn move_stake_fails_when_source_equals_destination() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let stake: BalanceOf<TestRuntime> = 1_000;

        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);

        assert_noop!(
            NodeManager::move_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                ctx.owner.clone(),
                BoundedVec::truncate_from(vec![(ctx.nodes[0].clone(), None)]),
                ctx.nodes[0].clone(),
            ),
            Error::<TestRuntime>::SourceAndDestinationNodeMustBeDifferent
        );
    });
}

#[test]
fn move_stake_fails_when_owner_does_not_own_source_node() {
    ext().execute_with(|| {
        let ctx = Context::new(1);
        let stake: BalanceOf<TestRuntime> = 1_000;

        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);

        // Create a separate node owned by other_owner so the to_node check passes.
        let other_owner = TestAccount::new([50u8; 32]).account_id();
        let other_node = TestAccount::new([200u8; 32]).account_id();
        assert_ok!(NodeManager::register_node(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            other_node.clone(),
            other_owner.clone(),
            UintAuthorityId(200u64),
        ));

        // Registrar is caller but other_owner doesn't own ctx.nodes[0].
        assert_noop!(
            NodeManager::move_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                other_owner,
                BoundedVec::truncate_from(vec![(ctx.nodes[0].clone(), None)]),
                other_node,
            ),
            Error::<TestRuntime>::NodeNotOwnedByOwner
        );
    });
}

#[test]
fn move_stake_all_zero_sources_is_noop_no_write_no_event() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        // nodes[0] has no stake; passing None should short-circuit entirely.

        let events_before = System::events().len();

        assert_ok!(NodeManager::move_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            BoundedVec::truncate_from(vec![(ctx.nodes[0].clone(), None)]),
            ctx.nodes[1].clone(),
        ));

        // No event emitted, to_node registry entry unchanged.
        assert_eq!(System::events().len(), events_before);
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&ctx.nodes[1]).unwrap().stake.amount, 0);
    });
}

#[test]
fn move_stake_fails_when_source_node_is_duplicated() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let stake: BalanceOf<TestRuntime> = 1_000;

        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);

        assert_noop!(
            NodeManager::move_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                ctx.owner.clone(),
                BoundedVec::truncate_from(vec![
                    (ctx.nodes[0].clone(), Some(1)),
                    (ctx.nodes[0].clone(), None),
                ]),
                ctx.nodes[1].clone(),
            ),
            Error::<TestRuntime>::DuplicateNodeInList
        );
    });
}

#[test]
fn move_stake_fails_when_not_registrar() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let stake: BalanceOf<TestRuntime> = 1_000;
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);

        assert_noop!(
            NodeManager::move_stake(
                RuntimeOrigin::signed(ctx.owner.clone()),
                ctx.owner.clone(),
                BoundedVec::truncate_from(vec![(ctx.nodes[0].clone(), None)]),
                ctx.nodes[1].clone(),
            ),
            Error::<TestRuntime>::OriginNotRegistrar
        );
    });
}

// ===== move_nodes_with_stake tests =====

#[test]
fn move_nodes_with_stake_equal_split_no_dust_succeeds() {
    ext().execute_with(|| {
        let ctx = Context::new(3);
        let stake_per_node: BalanceOf<TestRuntime> = 1_000_000;
        let total_stake = stake_per_node * 3;

        for node in &ctx.nodes {
            add_stake_to_node(&ctx.owner, node, stake_per_node);
        }

        assert_ok!(NodeManager::move_nodes_with_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            ctx.new_owner.clone(),
            BoundedVec::truncate_from(ctx.nodes.clone()),
            total_stake,
        ));

        for node in &ctx.nodes {
            assert_eq!(
                <NodeRegistry<TestRuntime>>::get(node).unwrap().stake.amount,
                stake_per_node
            );
            assert_eq!(<NodeRegistry<TestRuntime>>::get(node).unwrap().owner, ctx.new_owner);
        }
        assert_eq!(Balances::reserved_balance(&ctx.owner), 0);
        assert_eq!(Balances::reserved_balance(&ctx.new_owner), total_stake);
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.owner), Some(0));
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.new_owner), Some(total_stake));
    });
}

#[test]
fn move_nodes_with_stake_dust_goes_to_last_node() {
    ext().execute_with(|| {
        let ctx = Context::new(3);
        // 3 nodes, stake_amount = 10 => expected per_node = 3, dust = 1, last node gets 4
        let per_node: BalanceOf<TestRuntime> = 3;
        let total_stake: BalanceOf<TestRuntime> = 10;

        // Add all the stake to the first node. The code should recalculate the distribution.
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], total_stake);

        assert_ok!(NodeManager::move_nodes_with_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            ctx.new_owner.clone(),
            BoundedVec::truncate_from(ctx.nodes.clone()),
            total_stake,
        ));

        assert_eq!(<NodeRegistry<TestRuntime>>::get(&ctx.nodes[0]).unwrap().stake.amount, per_node);
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&ctx.nodes[1]).unwrap().stake.amount, per_node);
        assert_eq!(
            <NodeRegistry<TestRuntime>>::get(&ctx.nodes[2]).unwrap().stake.amount,
            per_node + 1
        );
    });
}

#[test]
fn move_nodes_with_stake_fails_on_stake_mismatch() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        let stake: BalanceOf<TestRuntime> = 1_000;

        for node in &ctx.nodes {
            add_stake_to_node(&ctx.owner, node, stake);
        }

        // Total is 2_000 but we request 1_500 — mismatch
        assert_noop!(
            NodeManager::move_nodes_with_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                ctx.owner.clone(),
                ctx.new_owner.clone(),
                BoundedVec::truncate_from(ctx.nodes.clone()),
                1_500,
            ),
            Error::<TestRuntime>::StakeMismatch
        );
    });
}

#[test]
fn move_nodes_with_stake_fails_when_not_registrar() {
    ext().execute_with(|| {
        let ctx = Context::new(1);
        let stake: BalanceOf<TestRuntime> = 1_000;
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);

        assert_noop!(
            NodeManager::move_nodes_with_stake(
                RuntimeOrigin::signed(ctx.owner.clone()),
                ctx.owner.clone(),
                ctx.new_owner.clone(),
                BoundedVec::truncate_from(ctx.nodes.clone()),
                stake,
            ),
            Error::<TestRuntime>::OriginNotRegistrar
        );
    });
}

#[test]
fn move_nodes_with_stake_fails_when_same_owner() {
    ext().execute_with(|| {
        let ctx = Context::new(1);
        let stake: BalanceOf<TestRuntime> = 1_000;
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);

        assert_noop!(
            NodeManager::move_nodes_with_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                ctx.owner.clone(),
                ctx.owner.clone(),
                BoundedVec::truncate_from(ctx.nodes.clone()),
                stake,
            ),
            Error::<TestRuntime>::NodeOwnersMustBeDifferent
        );
    });
}

#[test]
fn move_nodes_with_stake_fails_when_nodes_list_is_empty() {
    ext().execute_with(|| {
        let ctx = Context::new(1);

        assert_noop!(
            NodeManager::move_nodes_with_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                ctx.owner.clone(),
                ctx.new_owner.clone(),
                BoundedVec::truncate_from(vec![]),
                0,
            ),
            Error::<TestRuntime>::EmptyNodeList
        );
    });
}

#[test]
fn move_nodes_with_stake_fails_when_nodes_list_has_duplicates() {
    ext().execute_with(|| {
        let ctx = Context::new(1);

        assert_noop!(
            NodeManager::move_nodes_with_stake(
                RuntimeOrigin::signed(ctx.registrar.clone()),
                ctx.owner.clone(),
                ctx.new_owner.clone(),
                BoundedVec::truncate_from(vec![ctx.nodes[0].clone(), ctx.nodes[0].clone()]),
                0,
            ),
            Error::<TestRuntime>::DuplicateNodeInList
        );
    });
}

#[test]
fn move_stake_then_move_nodes_with_stake_integration() {
    ext().execute_with(|| {
        // Owner has 3 nodes. Move all stake into node[2], then move all 3 nodes to new_owner.
        let ctx = Context::new(3);
        let stake: BalanceOf<TestRuntime> = 1_000;

        add_stake_to_node(&ctx.owner, &ctx.nodes[0], stake);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], stake);
        // nodes[2] has no stake yet

        // Consolidate stake from nodes[0] and nodes[1] into nodes[2]
        assert_ok!(NodeManager::move_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            BoundedVec::truncate_from(vec![
                (ctx.nodes[0].clone(), None),
                (ctx.nodes[1].clone(), None),
            ]),
            ctx.nodes[2].clone(),
        ));

        // nodes[2] now has 2_000, others have 0 — total matches stake * 2
        assert_ok!(NodeManager::move_nodes_with_stake(
            RuntimeOrigin::signed(ctx.registrar.clone()),
            ctx.owner.clone(),
            ctx.new_owner.clone(),
            BoundedVec::truncate_from(ctx.nodes.clone()),
            stake * 2,
        ));

        // Each node gets (stake * 2) / 3 = 666, last gets 668 (dust = 2)
        let per_node = (stake * 2) / 3;
        let dust = (stake * 2) % 3;
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&ctx.nodes[0]).unwrap().stake.amount, per_node);
        assert_eq!(<NodeRegistry<TestRuntime>>::get(&ctx.nodes[1]).unwrap().stake.amount, per_node);
        assert_eq!(
            <NodeRegistry<TestRuntime>>::get(&ctx.nodes[2]).unwrap().stake.amount,
            per_node + dust
        );

        assert_eq!(Balances::reserved_balance(&ctx.owner), 0);
        assert_eq!(Balances::reserved_balance(&ctx.new_owner), stake * 2);
    });
}

// ===== move_stake after auto-stake expiry =====

#[test]
fn move_stake_periodic_to_periodic_full_move_carries_allowance() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], 500);

        // Both nodes past their (slightly different) expiry, neither touched yet.
        set_time(ctx.expiry_of(1));
        assert_ok!(ctx.move_stake(vec![(0, None)], 1));

        let from = ctx.info(0);
        let to = ctx.info(1);
        assert_eq!(from.stake.amount, 0);
        assert_eq!(from.stake.restriction.per_period_allowance(), Some(0));
        assert_eq!(from.stake.unlocked_stake, 0);
        assert_eq!(to.stake.amount, 1_500);
        // 10% of 1_000 + 10% of 500, pooled.
        assert_eq!(to.stake.restriction.per_period_allowance(), Some(150));
        // One period accrued on each side before the move, carried across.
        assert_eq!(to.stake.unlocked_stake, 150);

        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[1]), 150);
        set_time(ctx.expiry_of(1) + unstake_period());
        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[1]), 150);
    });
}

#[test]
fn move_stake_periodic_to_periodic_partial_move_splits_pro_rata() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], 500);

        set_time(ctx.expiry_of(1));
        assert_ok!(ctx.move_stake(vec![(0, Some(400))], 1));

        let from = ctx.info(0);
        let to = ctx.info(1);
        assert_eq!(from.stake.amount, 600);
        assert_eq!(from.stake.restriction.per_period_allowance(), Some(60));
        assert_eq!(from.stake.unlocked_stake, 60);
        assert_eq!(to.stake.amount, 900);
        assert_eq!(to.stake.restriction.per_period_allowance(), Some(90));
        assert_eq!(to.stake.unlocked_stake, 90);

        // Aggregate rate is unchanged: 60 + 90 == 150.
        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[0]), 60);
        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[1]), 90);
    });
}

#[test]
fn move_stake_periodic_to_periodic_settles_accrued_periods_without_double_counting() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], 500);

        // Node 1 unstakes one period at its expiry; node 0 is never touched.
        set_time(ctx.expiry_of(1));
        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[1]), 50);

        // Two periods later move everything from node 0 into node 1.
        set_time(ctx.expiry_of(1) + 2 * unstake_period());
        assert_ok!(ctx.move_stake(vec![(0, None)], 1));

        let to = ctx.info(1);
        assert_eq!(to.stake.amount, 1_450);
        assert_eq!(to.stake.restriction.per_period_allowance(), Some(150));
        // node 0 accrued 3 periods (its grid starts 5s earlier) = 300; node 1 accrued 2 = 100.
        assert_eq!(to.stake.unlocked_stake, 400);
        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[1]), 400);

        // The next period yields exactly one pooled allowance, nothing more.
        set_time(ctx.expiry_of(1) + 3 * unstake_period());
        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[1]), 150);
    });
}

#[test]
fn move_stake_periodic_to_periodic_different_expiries_allowed() {
    ext().execute_with(|| {
        let mut ctx = Context::new(1);
        // Second node registered five weeks later: both are Periodic at `t`, with
        // different expiry and restriction-end timestamps.
        let later = ctx.register_node_at(REGISTRATION_BASE_SEC + 5 * 7 * 24 * 60 * 60);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        add_stake_to_node(&ctx.owner, &ctx.nodes[later], 500);

        set_time(ctx.expiry_of(later));
        assert_ne!(ctx.restriction_end_of(0), ctx.restriction_end_of(later));
        assert_ok!(ctx.move_stake(vec![(0, None)], later));

        let to = ctx.info(later);
        assert_eq!(to.stake.amount, 1_500);
        assert_eq!(to.stake.restriction.per_period_allowance(), Some(150));
    });
}

#[test]
fn move_stake_cannot_bypass_restriction_via_untouched_expired_nodes() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], 500);

        set_time(ctx.expiry_of(1));
        // Empty node 0 into node 1 while both are untouched past expiry.
        assert_ok!(ctx.move_stake(vec![(0, None)], 1));

        // Node 0 was snapshotted during the move, so it is Periodic (not Free) and empty.
        assert_noop!(
            NodeManager::remove_stake(
                RuntimeOrigin::signed(ctx.owner.clone()),
                ctx.nodes[0].clone(),
                None
            ),
            Error::<TestRuntime>::NoAvailableStakeToUnstake
        );
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1);
        assert!(matches!(ctx.info(0).stake.restriction, UnstakeRestriction::Periodic { .. }));

        // Moving everything back does not unlock more than the pooled allowance.
        assert_ok!(ctx.move_stake(vec![(1, None)], 0));
        assert_eq!(ctx.info(0).stake.amount, 1_501);
        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[0]), 150);
    });
}

#[test]
fn move_stake_locked_to_locked_different_expiries_still_allowed() {
    ext().execute_with(|| {
        let mut ctx = Context::new(1);
        let later = ctx.register_node_at(REGISTRATION_BASE_SEC + 30 * 24 * 60 * 60);
        add_stake_to_node(&ctx.owner, &ctx.nodes[later], 1_000);

        // Both still Locked; moving newer stake into the older node is allowed.
        assert_ok!(ctx.move_stake(vec![(later, None)], 0));
        assert_eq!(ctx.info(0).stake.amount, 1_000);
        assert!(matches!(ctx.info(0).stake.restriction, UnstakeRestriction::Locked));
    });
}

#[test]
fn move_stake_free_to_free_succeeds() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], 500);

        set_time(ctx.restriction_end_of(1));
        assert_ok!(ctx.move_stake(vec![(0, None)], 1));

        let to = ctx.info(1);
        assert!(matches!(to.stake.restriction, UnstakeRestriction::Free));
        assert_eq!(to.stake.amount, 1_500);
        assert_eq!(unstake_all_available(&ctx.owner, &ctx.nodes[1]), 1_500);
    });
}

#[test]
fn move_stake_free_to_locked_succeeds_and_relocks() {
    ext().execute_with(|| {
        let mut ctx = Context::new(1);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        // Register a fresh (Locked) node once node 0 is Free.
        let fresh = ctx.register_node_at(ctx.restriction_end_of(0));

        assert_ok!(ctx.move_stake(vec![(0, None)], fresh));

        let to = ctx.info(fresh);
        assert!(matches!(to.stake.restriction, UnstakeRestriction::Locked));
        assert_eq!(to.stake.amount, 1_000);
        assert_noop!(
            NodeManager::remove_stake(
                RuntimeOrigin::signed(ctx.owner.clone()),
                ctx.nodes[fresh].clone(),
                None
            ),
            Error::<TestRuntime>::AutoStakeStillActive
        );
    });
}

#[test]
fn move_stake_fails_when_states_incompatible() {
    const FIVE_WEEKS: u64 = 5 * 7 * 24 * 60 * 60;

    // Locked -> Free and Free -> Periodic / Periodic -> Free
    ext().execute_with(|| {
        let mut ctx = Context::new(1);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        // node 1: registered five weeks later, Periodic once node 0 is Free.
        let periodic = ctx.register_node_at(REGISTRATION_BASE_SEC + FIVE_WEEKS);
        add_stake_to_node(&ctx.owner, &ctx.nodes[periodic], 500);
        // node 2: registered when node 0 is Free, so it is Locked.
        let locked = ctx.register_node_at(ctx.restriction_end_of(0));
        add_stake_to_node(&ctx.owner, &ctx.nodes[locked], 300);

        let t = ctx.restriction_end_of(0);
        assert!(t >= ctx.expiry_of(periodic) && t < ctx.restriction_end_of(periodic));
        set_time(t);

        for (from, to) in [(locked, 0), (0, periodic), (periodic, 0)] {
            assert_noop!(
                ctx.move_stake(vec![(from, None)], to),
                Error::<TestRuntime>::IncompatibleStakeRestriction
            );
        }
    });

    // Locked -> Periodic and Periodic -> Locked
    ext().execute_with(|| {
        let mut ctx = Context::new(1);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        let locked = ctx.register_node_at(ctx.expiry_of(0));
        add_stake_to_node(&ctx.owner, &ctx.nodes[locked], 300);

        for (from, to) in [(locked, 0), (0, locked)] {
            assert_noop!(
                ctx.move_stake(vec![(from, None)], to),
                Error::<TestRuntime>::IncompatibleStakeRestriction
            );
        }
    });
}

// ===== move_nodes_with_stake after auto-stake expiry =====

#[test]
fn move_nodes_with_stake_all_periodic_pools_and_splits_allowance() {
    ext().execute_with(|| {
        let ctx = Context::new(3);
        for (i, amount) in [1_000u128, 500, 300].iter().enumerate() {
            add_stake_to_node(&ctx.owner, &ctx.nodes[i], *amount);
        }

        set_time(ctx.expiry_of(2));
        assert_ok!(ctx.move_nodes_with_stake(&[0, 1, 2], 1_800));

        for i in 0..3 {
            let info = ctx.info(i);
            assert_eq!(info.owner, ctx.new_owner);
            assert_eq!(info.stake.amount, 600);
            // (100 + 50 + 30) / 3
            assert_eq!(info.stake.restriction.per_period_allowance(), Some(60));
            // one period accrued on each node before the move, pooled and split
            assert_eq!(info.stake.unlocked_stake, 60);
            assert_eq!(unstake_all_available(&ctx.new_owner, &ctx.nodes[i]), 60);
        }
        assert_eq!(Balances::reserved_balance(&ctx.new_owner), 1_800 - 180);
    });
}

#[test]
fn move_nodes_with_stake_all_free_succeeds() {
    ext().execute_with(|| {
        let ctx = Context::new(2);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], 500);

        set_time(ctx.restriction_end_of(1));
        assert_ok!(ctx.move_nodes_with_stake(&[0, 1], 1_500));

        for i in 0..2 {
            assert!(matches!(ctx.info(i).stake.restriction, UnstakeRestriction::Free));
            assert_eq!(ctx.info(i).stake.amount, 750);
            assert_eq!(unstake_all_available(&ctx.new_owner, &ctx.nodes[i]), 750);
        }
    });
}

#[test]
fn move_nodes_with_stake_fails_when_mixed_states() {
    const FIVE_WEEKS: u64 = 5 * 7 * 24 * 60 * 60;

    // Free + Periodic, and Locked + Free
    ext().execute_with(|| {
        let mut ctx = Context::new(1);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        let periodic = ctx.register_node_at(REGISTRATION_BASE_SEC + FIVE_WEEKS);
        add_stake_to_node(&ctx.owner, &ctx.nodes[periodic], 500);
        let locked = ctx.register_node_at(ctx.restriction_end_of(0));
        add_stake_to_node(&ctx.owner, &ctx.nodes[locked], 300);
        set_time(ctx.restriction_end_of(0));

        assert_noop!(
            ctx.move_nodes_with_stake(&[0, periodic], 1_500),
            Error::<TestRuntime>::IncompatibleStakeRestriction
        );
        assert_noop!(
            ctx.move_nodes_with_stake(&[0, locked], 1_300),
            Error::<TestRuntime>::IncompatibleStakeRestriction
        );
    });

    // Locked + Periodic
    ext().execute_with(|| {
        let mut ctx = Context::new(1);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        let locked = ctx.register_node_at(ctx.expiry_of(0));
        add_stake_to_node(&ctx.owner, &ctx.nodes[locked], 300);

        assert_noop!(
            ctx.move_nodes_with_stake(&[0, locked], 1_300),
            Error::<TestRuntime>::IncompatibleStakeRestriction
        );
    });
}

#[test]
fn move_stake_then_move_nodes_with_stake_after_expiry_integration() {
    ext().execute_with(|| {
        let ctx = Context::new(3);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 1_000);
        add_stake_to_node(&ctx.owner, &ctx.nodes[1], 1_000);
        // The consolidation target must hold some stake at expiry, otherwise it snapshots
        // to Free and can no longer receive Periodic stake.
        add_stake_to_node(&ctx.owner, &ctx.nodes[2], 100);

        set_time(ctx.expiry_of(2));
        assert_ok!(ctx.move_stake(vec![(0, None), (1, None)], 2));
        assert_eq!(ctx.info(2).stake.amount, 2_100);
        assert_eq!(ctx.info(2).stake.restriction.per_period_allowance(), Some(210));

        assert_ok!(ctx.move_nodes_with_stake(&[0, 1, 2], 2_100));
        for i in 0..3 {
            assert_eq!(ctx.info(i).owner, ctx.new_owner);
            assert_eq!(ctx.info(i).stake.amount, 700);
            assert_eq!(ctx.info(i).stake.restriction.per_period_allowance(), Some(70));
            assert_eq!(ctx.info(i).stake.unlocked_stake, 70);
        }
        assert_eq!(Balances::reserved_balance(&ctx.owner), 0);
        assert_eq!(Balances::reserved_balance(&ctx.new_owner), 2_100);
    });
}

#[test]
fn move_nodes_with_stake_all_periodic_allowance_dust_goes_to_last_node() {
    ext().execute_with(|| {
        let ctx = Context::new(3);
        // Allowances 100 / 50 / 25 pool to 175, which does not divide by 3.
        for (i, amount) in [1_000u128, 500, 250].iter().enumerate() {
            add_stake_to_node(&ctx.owner, &ctx.nodes[i], *amount);
        }

        set_time(ctx.expiry_of(2));
        assert_ok!(ctx.move_nodes_with_stake(&[0, 1, 2], 1_750));

        // Stake: 1_750 / 3 = 583, dust 1 to the last node.
        // Allowance and settled unlocked stake: 175 / 3 = 58, dust 1 to the last node.
        for i in 0..2 {
            let info = ctx.info(i);
            assert_eq!(info.stake.amount, 583);
            assert_eq!(info.stake.restriction.per_period_allowance(), Some(58));
            assert_eq!(info.stake.unlocked_stake, 58);
        }
        let last = ctx.info(2);
        assert_eq!(last.stake.amount, 584);
        assert_eq!(last.stake.restriction.per_period_allowance(), Some(59));
        assert_eq!(last.stake.unlocked_stake, 59);

        // Nothing lost: totals are preserved exactly.
        let total_allowance: u128 = (0..3)
            .map(|i| ctx.info(i).stake.restriction.per_period_allowance().unwrap())
            .sum();
        let total_unlocked: u128 = (0..3).map(|i| ctx.info(i).stake.unlocked_stake).sum();
        assert_eq!(total_allowance, 175);
        assert_eq!(total_unlocked, 175);
        assert_eq!(unstake_all_available(&ctx.new_owner, &ctx.nodes[2]), 59);
    });
}

#[test]
fn move_nodes_with_stake_half_avt_across_fifty_nodes_splits_evenly() {
    ext().execute_with(|| {
        let ctx = Context::new(50);
        let half_avt: u128 = 500_000_000_000_000_000; // 0.5 AVT in planck (18 decimals)
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], half_avt);

        // All nodes Free: node 0 past its restriction window, the rest held nothing at expiry.
        set_time(ctx.restriction_end_of(49));
        let all: Vec<usize> = (0..50).collect();
        assert_ok!(ctx.move_nodes_with_stake(&all, half_avt));

        for i in 0..50 {
            let info = ctx.info(i);
            assert_eq!(info.owner, ctx.new_owner);
            assert_eq!(info.stake.amount, half_avt / 50);
            assert!(matches!(info.stake.restriction, UnstakeRestriction::Free));
        }
        assert_eq!(Balances::reserved_balance(&ctx.owner), 0);
        assert_eq!(Balances::reserved_balance(&ctx.new_owner), half_avt);
        assert_eq!(<TotalStake<TestRuntime>>::get(&ctx.new_owner), Some(half_avt));
    });
}

#[test]
fn move_nodes_with_stake_total_smaller_than_node_count_leaves_everything_on_last_node() {
    ext().execute_with(|| {
        let ctx = Context::new(4);
        add_stake_to_node(&ctx.owner, &ctx.nodes[0], 3);

        set_time(ctx.restriction_end_of(3));
        assert_ok!(ctx.move_nodes_with_stake(&[0, 1, 2, 3], 3));

        // 3 / 4 = 0 per node, dust 3 to the last node.
        for i in 0..3 {
            assert_eq!(ctx.info(i).stake.amount, 0);
        }
        assert_eq!(ctx.info(3).stake.amount, 3);
        assert_eq!(Balances::reserved_balance(&ctx.new_owner), 3);
        assert_eq!(unstake_all_available(&ctx.new_owner, &ctx.nodes[3]), 3);
    });
}
