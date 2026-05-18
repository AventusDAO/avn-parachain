// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};

struct Context {
    registrar: AccountId,
    owner: AccountId,
    new_owner: AccountId,
    nodes: Vec<NodeId<TestRuntime>>,
}

impl Context {
    fn new(num_nodes: u8) -> Self {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        let owner = TestAccount::new([10u8; 32]).account_id();
        let new_owner = TestAccount::new([20u8; 32]).account_id();

        <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));

        let nodes = (0..num_nodes)
            .map(|i| {
                let node = TestAccount::new([100u8 + i; 32]).account_id();
                let signing_key = UintAuthorityId((100 + i) as u64);
                assert_ok!(NodeManager::register_node(
                    RuntimeOrigin::signed(registrar.clone()),
                    node.clone(),
                    owner.clone(),
                    signing_key,
                ));
                node
            })
            .collect();

        Context { registrar, owner, new_owner, nodes }
    }
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
        Balances::make_free_balance_be(&ctx.new_owner, 1);

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
        Balances::make_free_balance_be(&ctx.new_owner, 1);

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

        Balances::make_free_balance_be(&ctx.new_owner, 1);

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

        // Ensure new_owner's account exists before repatriation.
        Balances::make_free_balance_be(&ctx.new_owner, 1);

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
