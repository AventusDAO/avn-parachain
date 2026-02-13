// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use sp_avn_common::{avn_tests_helpers::utilities::SignatureTest, Proof};
use sp_core::Pair;

struct Context {
    registrar_key_pair: TestAccount,
    registrar: AccountId,
    owner: AccountId,
    relayer: AccountId,
    registered_nodes: Vec<NodeId<TestRuntime>>,
}

impl Context {
    fn new(num_of_nodes: u8) -> Self {
        let registrar_key_pair = TestAccount::new([1u8; 32]);
        let registrar = registrar_key_pair.account_id();
        let owner = TestAccount::new([209u8; 32]).account_id();
        let relayer = TestAccount::new([109u8; 32]).account_id();
        let reward_amount: BalanceOf<TestRuntime> = <RewardAmount<TestRuntime>>::get();

        Balances::make_free_balance_be(
            &NodeManager::compute_reward_account_id(),
            reward_amount * 2u128,
        );
        <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));
        let registered_nodes = register_nodes(registrar, owner, num_of_nodes);

        Context { registrar_key_pair, registrar, owner, registered_nodes, relayer }
    }
}

fn register_nodes(
    registrar: AccountId,
    owner: AccountId,
    num_of_nodes: u8,
) -> Vec<NodeId<TestRuntime>> {
    let mut registered_nodes = vec![];
    let reward_period = <RewardPeriod<TestRuntime>>::get().current;

    for i in 0..num_of_nodes {
        registered_nodes.push(register_node_and_send_heartbeat(
            registrar,
            owner.clone(),
            reward_period,
            i,
        ));
    }

    let this_node = TestAccount::new([0 as u8; 32]).account_id();
    let this_node_signing_key = 0;

    set_ocw_node_id(this_node);
    UintAuthorityId::set_all_keys(vec![UintAuthorityId(this_node_signing_key)]);

    return registered_nodes
}

fn register_node_and_send_heartbeat(
    registrar: AccountId,
    owner: AccountId,
    reward_period: RewardPeriodIndex,
    id: u8,
) -> AccountId {
    let node_id = TestAccount::new([id as u8; 32]).account_id();
    let signing_key_id = id + 1;

    assert_ok!(NodeManager::register_node(
        RuntimeOrigin::signed(registrar),
        node_id,
        owner,
        UintAuthorityId(signing_key_id as u64),
    ));

    incr_heartbeats(reward_period, vec![node_id], 1);
    node_id
}

fn incr_heartbeats(reward_period: RewardPeriodIndex, nodes: Vec<NodeId<TestRuntime>>, uptime: u64) {
    for node in nodes {
        let node_info = <NodeRegistry<TestRuntime>>::get(&node).unwrap();
        let single_hb_weight =
            NodeManager::effective_heartbeat_weight(&node_info, NodeManager::time_now_sec());
        let weight = single_hb_weight.saturating_mul(uptime.into());

        <NodeUptime<TestRuntime>>::mutate(&reward_period, &node, |maybe_info| {
            if let Some(info) = maybe_info.as_mut() {
                info.count = info.count.saturating_add(uptime);
                info.last_reported = System::block_number();
            } else {
                *maybe_info =
                    Some(UptimeInfo { count: 1, last_reported: System::block_number(), weight });
            }
        });

        <TotalUptime<TestRuntime>>::mutate(&reward_period, |total| {
            total._total_heartbeats = total._total_heartbeats.saturating_add(uptime);
            total.total_weight = total.total_weight.saturating_add(weight);
        });
    }
}

fn pop_tx_from_mempool(pool_state: Arc<RwLock<PoolState>>) -> Extrinsic {
    let tx = pool_state.write().transactions.pop().unwrap();
    Extrinsic::decode(&mut &*tx).unwrap()
}

fn set_ocw_node_id(node_id: AccountId) {
    let storage = StorageValueRef::persistent(REGISTERED_NODE_KEY);
    storage
        .mutate(|r: Result<Option<AccountId>, StorageRetrievalError>| match r {
            Ok(Some(_)) => Ok(node_id),
            Ok(None) => Ok(node_id),
            _ => Err(()),
        })
        .unwrap();
}

fn create_signed_deregister_proof(
    registrar_key_pair: &TestAccount,
    relayer: &AccountId,
    owner: &AccountId,
    nodes_to_deregister: &BoundedVec<NodeId<TestRuntime>, MaxNodesToDeregister>,
    number_of_nodes_to_deregister: &u32,
    block_number: &BlockNumberFor<TestRuntime>,
) -> Proof<SignatureTest, AccountId> {
    let encoded_payload = encode_signed_deregister_node_params::<TestRuntime>(
        relayer,
        owner,
        nodes_to_deregister,
        number_of_nodes_to_deregister,
        &block_number,
    );

    let signature = SignatureTest::from(registrar_key_pair.key_pair().sign(&encoded_payload));
    let proof = Proof {
        signer: registrar_key_pair.key_pair().public(),
        relayer: relayer.clone(),
        signature,
    };

    proof
}

#[test]
fn deregistration_succeeds() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get();
        let context = Context::new(node_count as u8);
        let num_nodes_to_deregister = context.registered_nodes.len();

        // Show that nodes are registered before deregistration
        for node in &context.registered_nodes {
            assert!(<OwnedNodes<TestRuntime>>::contains_key(context.owner.clone(), node));
            assert!(<NodeRegistry<TestRuntime>>::contains_key(node));
        }

        assert_ok!(NodeManager::deregister_nodes(
            RuntimeOrigin::signed(context.registrar),
            context.owner,
            BoundedVec::truncate_from(context.registered_nodes.clone()),
        ));

        for node in &context.registered_nodes {
            assert!(!<OwnedNodes<TestRuntime>>::contains_key(context.owner.clone(), node));
            assert!(!<NodeRegistry<TestRuntime>>::contains_key(node));
        }
        System::assert_last_event(
            Event::NodeDeregistered {
                owner: context.owner,
                node: context.registered_nodes[num_nodes_to_deregister - 1].clone(),
            }
            .into(),
        );
    });
}

#[test]
fn signed_deregistration_succeeds() {
    let (mut ext, _, _) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get();
        let context = Context::new(node_count as u8);
        let num_nodes_to_deregister = context.registered_nodes.len();
        let block_number = System::block_number();

        // Show that nodes are registered before deregistration
        for node in &context.registered_nodes {
            assert!(<OwnedNodes<TestRuntime>>::contains_key(context.owner.clone(), node));
            assert!(<NodeRegistry<TestRuntime>>::contains_key(node));
        }

        let proof = create_signed_deregister_proof(
            &context.registrar_key_pair,
            &context.relayer,
            &context.owner,
            &(BoundedVec::truncate_from(context.registered_nodes.clone())),
            &(num_nodes_to_deregister as u32),
            &block_number,
        );

        assert_ok!(NodeManager::signed_deregister_nodes(
            RuntimeOrigin::signed(context.registrar),
            proof,
            context.owner,
            BoundedVec::truncate_from(context.registered_nodes.clone()),
            block_number,
        ));

        for node in &context.registered_nodes {
            assert!(!<OwnedNodes<TestRuntime>>::contains_key(context.owner.clone(), node));
            assert!(!<NodeRegistry<TestRuntime>>::contains_key(node));
        }
        System::assert_last_event(
            Event::NodeDeregistered {
                owner: context.owner,
                node: context.registered_nodes[num_nodes_to_deregister - 1].clone(),
            }
            .into(),
        );
    });
}

#[test]
fn payment_works_all_nodes_deregistered() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .with_authors()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get();
        let context = Context::new(node_count as u8);
        let num_nodes_to_deregister = context.registered_nodes.len();

        assert_ok!(NodeManager::deregister_nodes(
            RuntimeOrigin::signed(context.registrar),
            context.owner,
            BoundedVec::truncate_from(context.registered_nodes.clone()),
        ));

        for node in &context.registered_nodes {
            assert!(!<OwnedNodes<TestRuntime>>::contains_key(context.owner.clone(), node));
            assert!(!<NodeRegistry<TestRuntime>>::contains_key(node));
        }

        let reward_period = <RewardPeriod<TestRuntime>>::get();
        let reward_amount = <RewardAmount<TestRuntime>>::get();
        let reward_period_length = reward_period.length as u64;
        let reward_period_to_pay = reward_period.current;

        let initial_pot_balance = Balances::free_balance(&NodeManager::compute_reward_account_id());
        let initial_owner_balance = Balances::free_balance(&context.owner);

        // make sure the pot has the expected amount
        assert_eq!(initial_pot_balance, reward_amount * 2u128);

        // Complete a reward period
        roll_forward((reward_period_length - System::block_number()) + 1);

        assert_eq!(
            <RewardPot<TestRuntime>>::get(reward_period_to_pay).unwrap().total_reward,
            reward_amount
        );
        // mock finalised block response
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );

        // Trigger ocw and send the transaction
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state);
        assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

        // The owner should not get any reward because all the nodes were deregistered
        assert_eq!(Balances::free_balance(&context.owner), initial_owner_balance);

        // The pot balance should stay the same because all the nodes were deregistered
        assert_eq!(
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            initial_pot_balance
        );

        // Make sure the failed payment event is emitted
        System::assert_has_event(
            Event::ErrorPayingReward {
                reward_period: reward_period_to_pay,
                node: context.registered_nodes[num_nodes_to_deregister - 1].clone(),
                error: Error::<TestRuntime>::NodeNotRegistered.into(),
            }
            .into(),
        );

        // The payment should succeed
        assert_eq!(true, <RewardPot<TestRuntime>>::get(reward_period_to_pay).is_none());
        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
        );
    });
}

#[test]
fn payment_works_some_nodes_deregistered() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .with_authors()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get();
        let context = Context::new(node_count as u8);
        let nodes_to_deregister = vec![context.registered_nodes[0].clone()];
        let num_nodes_to_deregister = 1u32;

        assert_ok!(NodeManager::deregister_nodes(
            RuntimeOrigin::signed(context.registrar),
            context.owner,
            BoundedVec::truncate_from(nodes_to_deregister),
        ));

        let reward_period = <RewardPeriod<TestRuntime>>::get();
        let reward_amount = <RewardAmount<TestRuntime>>::get();
        let reward_period_length = reward_period.length as u64;
        let reward_period_to_pay = reward_period.current;

        let initial_pot_balance = Balances::free_balance(&NodeManager::compute_reward_account_id());

        // make sure the pot has the expected amount
        assert_eq!(initial_pot_balance, reward_amount * 2u128);

        // Complete a reward period
        roll_forward((reward_period_length - System::block_number()) + 1);

        assert_eq!(
            <RewardPot<TestRuntime>>::get(reward_period_to_pay).unwrap().total_reward,
            reward_amount
        );

        // mock finalised block response
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );

        // Trigger ocw and send the transaction
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state);
        assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

        // Make sure the failed payment event is emitted
        System::assert_has_event(
            Event::ErrorPayingReward {
                reward_period: reward_period_to_pay,
                node: context.registered_nodes[(num_nodes_to_deregister - 1) as usize].clone(),
                error: Error::<TestRuntime>::NodeNotRegistered.into(),
            }
            .into(),
        );

        // The owner should get all rewards minus the nodes that were deregistered
        let expected_owner_reward_amount =
            reward_amount / node_count as u128 * (node_count - num_nodes_to_deregister) as u128;
        assert_eq!(Balances::reserved_balance(&context.owner), expected_owner_reward_amount);

        // The pot balance should stay the same because all the nodes were deregistered
        assert_eq!(
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            initial_pot_balance - expected_owner_reward_amount
        );

        // The payment for the remaing nodes should succeed
        assert_eq!(true, <RewardPot<TestRuntime>>::get(reward_period_to_pay).is_none());
        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
        );
    });
}

mod fails_when {
    use super::*;

    #[test]
    fn sender_is_not_registrar() {
        let (mut ext, _, _) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get();
            let context = Context::new(node_count as u8);

            let bad_origin = RuntimeOrigin::signed(context.owner);
            assert_noop!(
                NodeManager::deregister_nodes(
                    bad_origin,
                    context.owner,
                    BoundedVec::truncate_from(context.registered_nodes.clone()),
                ),
                Error::<TestRuntime>::OriginNotRegistrar
            );
        });
    }

    #[test]
    fn node_is_not_registered() {
        let (mut ext, _, _) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get();
            let context = Context::new(node_count as u8);

            let bad_node = context.owner;
            assert_noop!(
                NodeManager::deregister_nodes(
                    RuntimeOrigin::signed(context.registrar),
                    context.owner,
                    BoundedVec::truncate_from(vec![bad_node, context.registered_nodes[0].clone()]),
                ),
                Error::<TestRuntime>::NodeNotOwnedByOwner
            );
        });
    }

    #[test]
    fn owner_is_not_registered() {
        let (mut ext, _, _) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get();
            let context = Context::new(node_count as u8);

            let bad_owner = context.registrar;
            assert_noop!(
                NodeManager::deregister_nodes(
                    RuntimeOrigin::signed(context.registrar),
                    bad_owner,
                    BoundedVec::truncate_from(context.registered_nodes.clone()),
                ),
                Error::<TestRuntime>::NodeNotOwnedByOwner
            );
        });
    }
}
