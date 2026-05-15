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
            Event::NodeMoved { old_owner: ctx.owner, new_owner: ctx.new_owner, node }.into(),
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
