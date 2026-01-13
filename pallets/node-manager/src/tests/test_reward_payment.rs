//Copyright 2025 Truth Network.

#![cfg(test)]

use crate::{mock::*, offchain::OCW_ID, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;

#[derive(Clone)]
struct Context {
    registrar: AccountId,
    owner: AccountId,
}

impl Context {
    fn new(num_of_nodes: u8) -> Self {
        let registrar = TestAccount::new([1u8; 32]).account_id();
        let owner = TestAccount::new([209u8; 32]).account_id();
        let reward_amount: BalanceOf<TestRuntime> = <RewardAmount<TestRuntime>>::get();

        Balances::make_free_balance_be(
            &NodeManager::compute_reward_account_id(),
            reward_amount * 2u128,
        );
        <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));
        let _ = register_nodes(registrar, owner, num_of_nodes);

        Context { registrar, owner }
    }
}

fn register_nodes(registrar: AccountId, owner: AccountId, num_of_nodes: u8) -> AccountId {
    let reward_period = <RewardPeriod<TestRuntime>>::get().current;

    for i in 0..num_of_nodes {
        register_node_and_send_heartbeat(registrar, owner.clone(), reward_period, i);
    }

    let this_node = TestAccount::new([0 as u8; 32]).account_id();
    let this_node_signing_key = 0;

    set_ocw_node_id(this_node);
    UintAuthorityId::set_all_keys(vec![UintAuthorityId(this_node_signing_key)]);

    return this_node;
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
        <NodeUptime<TestRuntime>>::mutate(&reward_period, &node, |maybe_info| {
            if let Some(info) = maybe_info.as_mut() {
                info.count = info.count.saturating_add(uptime);
                info.last_reported = System::block_number();
            } else {
                *maybe_info = Some(UptimeInfo { count: 1, last_reported: System::block_number() });
            }
        });

        <TotalUptime<TestRuntime>>::mutate(&reward_period, |total| {
            *total = total.saturating_add(uptime);
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

fn remove_ocw_run_lock() {
    let key = [OCW_ID.as_slice(), b"::last_run"].concat();
    let mut storage = StorageValueRef::persistent(&key);
    storage.clear();
}

#[test]
fn payment_transaction_succeed() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .with_authors()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get();
        let context = Context::new(node_count as u8);
        let reward_period = <RewardPeriod<TestRuntime>>::get();
        let reward_amount = <RewardAmount<TestRuntime>>::get();
        let reward_period_length = reward_period.length as u64;
        let reward_period_to_pay = reward_period.current;

        // make sure the pot has the expected amount
        assert_eq!(
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            reward_amount * 2u128
        );

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
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

        // Check if the transaction from the mempool is what we expected
        assert!(matches!(
            tx.call,
            RuntimeCall::NodeManager(crate::Call::offchain_pay_nodes {
                reward_period_index: _,
                author: _,
                signature: _,
            })
        ));

        assert_eq!(true, <RewardPot<TestRuntime>>::get(reward_period_to_pay).is_none());
        assert_eq!(
            true,
            <NodeUptime<TestRuntime>>::iter_prefix(reward_period_to_pay).next().is_none()
        );
        assert_eq!(true, <LastPaidPointer<TestRuntime>>::get().is_none());
        // The owner has received the reward
        assert_eq!(Balances::free_balance(&context.owner), reward_amount);
        // The pot has gone down by half
        assert_eq!(
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            reward_amount
        );

        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
        );
    });
}

#[test]
fn multiple_payments_can_be_triggered_in_the_same_block() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .with_authors()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        // This takes 2 attempts to clear all the payments
        let node_count = <MaxBatchSize<TestRuntime>>::get() * 2;
        let context = Context::new(node_count as u8);
        let reward_period = <RewardPeriod<TestRuntime>>::get();
        let reward_amount = <RewardAmount<TestRuntime>>::get();
        let reward_period_length = reward_period.length as u64;
        let reward_period_to_pay = reward_period.current;

        // Complete a reward period
        roll_forward((reward_period_length - System::block_number()) + 1);

        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state.clone());
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

        // We should have processed the first batch of payments
        assert_eq!(true, <LastPaidPointer<TestRuntime>>::get().is_some());
        assert_eq!(Balances::free_balance(&context.owner), reward_amount / 2);

        // This is a hack: we remove the lock to allow the offchain worker to run again for the same
        // block
        remove_ocw_run_lock();

        // Trigger another payment. In reality this can happy because authors can trigger payments
        // in parallel
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state);
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

        // This should complete the payment
        assert_eq!(true, <RewardPot<TestRuntime>>::get(reward_period_to_pay).is_none());
        assert_eq!(
            true,
            <NodeUptime<TestRuntime>>::iter_prefix(reward_period_to_pay).next().is_none()
        );
        assert_eq!(true, <LastPaidPointer<TestRuntime>>::get().is_none());
        assert_eq!(Balances::free_balance(&context.owner), reward_amount);
        // The pot has gone down by half
        assert_eq!(
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            reward_amount
        );

        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
        );
    });
}

#[test]
fn payment_is_based_on_uptime() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .with_authors()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get() - 1;
        let context = Context::new(node_count as u8);
        let reward_period = <RewardPeriod<TestRuntime>>::get(); // 200
        let reward_amount = <RewardAmount<TestRuntime>>::get(); // 5
        let reward_period_length = reward_period.length as u64;
        let reward_period_to_pay = reward_period.current;

        // make sure the pot has the expected amount
        assert_eq!(
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            reward_amount * 2u128
        );

        let new_owner = TestAccount::new([111u8; 32]).account_id();
        let new_node = register_node_and_send_heartbeat(
            context.registrar.clone(),
            new_owner,
            reward_period_to_pay,
            199,
        );

        let total_expected_uptime =
            NodeManager::calculate_uptime_threshold(reward_period_length as u32);
        // The node falls below the min threshold to get the full rewards. They should still get
        // their share
        incr_heartbeats(reward_period_to_pay, vec![new_node], total_expected_uptime as u64 - 2);

        let total_uptime = <TotalUptime<TestRuntime>>::get(reward_period_to_pay);

        // Complete a reward period
        roll_forward((reward_period_length - System::block_number()) + 1);

        // Pay out
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state);
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

        // The owner has received the reward
        // total_expected_uptime - 1 because we run the OCW
        let expected_new_owner_reward =
            Perquintill::from_rational(total_expected_uptime as u128 - 1, total_uptime as u128) *
                reward_amount;

        assert!(
            Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
            "Value {} and {} differs by more than 10",
            Balances::free_balance(&new_owner),
            expected_new_owner_reward
        );

        let expected_old_owner_reward = reward_amount - expected_new_owner_reward;

        assert!(
            Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward) <= 20,
            "Value {} differs by more than 20",
            Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward)
        );

        // The pot has gone down by half
        assert!(
            Balances::free_balance(&NodeManager::compute_reward_account_id())
                .abs_diff(reward_amount) <=
                20,
            "Value {} differs by more than 20",
            Balances::free_balance(&NodeManager::compute_reward_account_id())
                .abs_diff(reward_amount)
        );

        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
        );
    });
}

#[test]
fn payment_works_when_uptime_is_threshold() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .with_authors()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get() - 1;
        let context = Context::new(node_count as u8);
        let reward_period = <RewardPeriod<TestRuntime>>::get(); // 200
        let reward_amount = <RewardAmount<TestRuntime>>::get(); // 5
        let reward_period_length = reward_period.length as u64;
        let reward_period_to_pay = reward_period.current;

        // make sure the pot has the expected amount
        assert_eq!(
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            reward_amount * 2u128
        );

        let new_owner = TestAccount::new([111u8; 32]).account_id();
        let new_node = register_node_and_send_heartbeat(
            context.registrar.clone(),
            new_owner,
            reward_period_to_pay,
            199,
        );

        let total_expected_uptime =
            NodeManager::calculate_uptime_threshold(reward_period_length as u32);
        // The node's uptime is exactly the threshold, so they should get the full rewards
        incr_heartbeats(reward_period_to_pay, vec![new_node], total_expected_uptime as u64 - 1);

        let total_uptime = <TotalUptime<TestRuntime>>::get(reward_period_to_pay);

        // Complete a reward period
        roll_forward((reward_period_length - System::block_number()) + 1);

        // Pay out
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state);
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

        // The owner has received the reward
        let expected_new_owner_reward =
            Perquintill::from_rational(total_expected_uptime as u128, total_uptime as u128) *
                reward_amount;

        assert!(
            Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
            "Values {} differ by more than 10",
            Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward)
        );
        let expected_old_owner_reward = reward_amount - expected_new_owner_reward;

        assert!(
            Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward) <= 100,
            "Value {}  differs by more than 100",
            Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward)
        );

        // The pot has gone down by half
        assert!(
            Balances::free_balance(&NodeManager::compute_reward_account_id())
                .abs_diff(reward_amount) <=
                100,
            "Value {} differs by more than 100",
            Balances::free_balance(&NodeManager::compute_reward_account_id())
                .abs_diff(reward_amount)
        );

        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
        );
    });
}

#[test]
fn payment_works_even_when_uptime_is_over_threshold() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .with_authors()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get() - 1;
        let context = Context::new(node_count as u8);
        let reward_period = <RewardPeriod<TestRuntime>>::get();
        let reward_amount = <RewardAmount<TestRuntime>>::get();
        let reward_period_length = reward_period.length as u64;
        let reward_period_to_pay = reward_period.current;

        let initial_pot = reward_amount * 2u128;
        // make sure the pot has the expected amount
        assert_eq!(Balances::free_balance(&NodeManager::compute_reward_account_id()), initial_pot);

        let new_owner = TestAccount::new([111u8; 32]).account_id();
        let new_node = register_node_and_send_heartbeat(
            context.registrar.clone(),
            new_owner,
            reward_period_to_pay,
            199,
        );

        let total_expected_uptime =
            NodeManager::calculate_uptime_threshold(reward_period_length as u32);
        // The node's uptime is over the threshold. This is unexpected but handled
        incr_heartbeats(reward_period_to_pay, vec![new_node], total_expected_uptime as u64 + 1u64);

        let total_uptime = <TotalUptime<TestRuntime>>::get(reward_period_to_pay);

        // Complete a reward period
        roll_forward(reward_period_length - System::block_number());

        // Pay out
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state);
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

        // The owner has received the reward
        // The system limits the reward to the expected uptime
        let expected_new_owner_reward =
            Perquintill::from_rational(total_expected_uptime as u128, total_uptime as u128) *
                reward_amount;

        assert!(
            Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward) < 1,
            "Values {} and {} differ by more than 10",
            Balances::free_balance(&new_owner),
            expected_new_owner_reward,
        );
        //The old owner gets a smaller share of the rewards because the total_uptime has now
        // increased by the extra uptime
        let expected_old_owner_reward = Perquintill::from_rational(1u128, total_uptime as u128) *
            reward_amount *
            (node_count as u128);

        assert!(
            Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward) < 1,
            "Value {} differs by more than 10",
            Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward)
        );

        // The pot should have gone down by half (because we started with reward_amount * 2), but it
        // hasn't because it didn't pay out the full reward.
        // This is because one of the nodes went over the expected uptime, which increased the total
        // uptime But we limit how much a node can get paid based on the expected uptime.
        // This is a safeguard against paying out more than the expected amount if nodes
        // somehow manipulate their uptime.
        assert!(Balances::free_balance(&NodeManager::compute_reward_account_id()) > reward_amount);

        // Make sure the pot has gone down by the expected amount
        assert!(
            Balances::free_balance(&NodeManager::compute_reward_account_id())
                .abs_diff(initial_pot - (expected_new_owner_reward + expected_old_owner_reward)) <
                10,
            "Value {} and {} differs by more than 10",
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            expected_new_owner_reward + expected_old_owner_reward
        );

        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
        );
    });
}

#[test]
fn threshold_update_is_respected() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .with_authors()
        .for_offchain_worker()
        .as_externality_with_state();
    ext.execute_with(|| {
        let node_count = <MaxBatchSize<TestRuntime>>::get() - 1;
        let context = Context::new(node_count as u8);
        let reward_period = <RewardPeriod<TestRuntime>>::get();
        let reward_amount = <RewardAmount<TestRuntime>>::get();
        let reward_period_length = reward_period.length as u64;
        let reward_period_to_pay = reward_period.current;

        // make sure the pot has the expected amount
        assert_eq!(
            Balances::free_balance(&NodeManager::compute_reward_account_id()),
            reward_amount * 2u128
        );

        let new_owner = TestAccount::new([111u8; 32]).account_id();
        let new_node = register_node_and_send_heartbeat(
            context.registrar.clone(),
            new_owner,
            reward_period_to_pay,
            199,
        );
        let total_expected_uptime =
            NodeManager::calculate_uptime_threshold(reward_period_length as u32);
        // Increase the uptime of the node by 4 (total 5) to change the rewards
        incr_heartbeats(reward_period_to_pay, vec![new_node], total_expected_uptime as u64 - 1);

        let total_uptime = <TotalUptime<TestRuntime>>::get(reward_period_to_pay);

        // Set a new threshold before rolling forward. This will change the current period's
        // threshold but should not affect the rewards of the previous period
        MinUptimeThreshold::<TestRuntime>::put(Perbill::from_percent(5));

        // Complete a reward period
        roll_forward((reward_period_length - System::block_number()) + 1);

        // Pay out
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state);
        assert_ok!(tx.call.clone().dispatch(frame_system::RawOrigin::None.into()));

        // The owner has received the reward
        let expected_new_owner_reward =
            Perquintill::from_rational(total_expected_uptime as u128, total_uptime as u128) *
                reward_amount;

        assert!(
            Balances::free_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
            "Values {} and {} differ by more than 10",
            Balances::free_balance(&new_owner),
            expected_new_owner_reward
        );
        let expected_old_owner_reward = reward_amount - expected_new_owner_reward;

        assert!(
            Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward) <= 100,
            "Value {} differs by more than 100",
            Balances::free_balance(&context.owner).abs_diff(expected_old_owner_reward)
        );

        // The pot has gone down by half
        assert!(
            Balances::free_balance(&NodeManager::compute_reward_account_id())
                .abs_diff(reward_amount) <=
                100,
            "Value {} differs by more than 100",
            Balances::free_balance(&NodeManager::compute_reward_account_id())
                .abs_diff(reward_amount)
        );

        System::assert_last_event(
            Event::RewardPayoutCompleted { reward_period_index: reward_period_to_pay }.into(),
        );
    });
}

mod fails_when {
    use super::*;

    #[test]
    fn when_period_is_wrong() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get();
            let _ = Context::new(node_count as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_period_length = reward_period.length as u64;
            let bad_reward_period_to_pay = reward_period.current + 10;

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            let signature =
                UintAuthorityId(1).sign(&("DummyProof").encode()).expect("Error signing");
            let author = mock::AVN::active_validators()[0].clone();
            assert_noop!(
                NodeManager::offchain_pay_nodes(
                    RawOrigin::None.into(),
                    bad_reward_period_to_pay,
                    author,
                    signature
                ),
                Error::<TestRuntime>::InvalidRewardPaymentRequest
            );
        });
    }

    #[test]
    fn when_pot_balance_is_not_enough() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get();
            let _ = Context::new(node_count as u8);
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_amount = <RewardAmount<TestRuntime>>::get();
            let reward_period_length = reward_period.length as u64;
            let reward_period_to_pay = reward_period.current;

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            let signature =
                UintAuthorityId(1).sign(&("DummyProof").encode()).expect("Error signing");
            let author = mock::AVN::active_validators()[0].clone();
            // ensure there isn't enough to pay out
            Balances::make_free_balance_be(
                &NodeManager::compute_reward_account_id(),
                reward_amount - 10000u128,
            );

            assert_noop!(
                NodeManager::offchain_pay_nodes(
                    RawOrigin::None.into(),
                    reward_period_to_pay,
                    author,
                    signature
                ),
                Error::<TestRuntime>::InsufficientBalanceForReward
            );
        });
    }

    #[test]
    fn rewards_are_disabled() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let node_count = <MaxBatchSize<TestRuntime>>::get();
            let _ = Context::new(node_count as u8);

            //Disable rewards
            RewardEnabled::<TestRuntime>::put(false);

            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_period_length = reward_period.length as u64;

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            let call = crate::Call::offchain_pay_nodes {
                reward_period_index: 1u64,
                author: mock::AVN::active_validators()[0].clone(),
                signature: UintAuthorityId(1u64)
                    .sign(&("DummyProof").encode())
                    .expect("Error signing"),
            };

            assert_noop!(
                <NodeManager as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::Local,
                    &call
                ),
                InvalidTransaction::Custom(ERROR_CODE_REWARD_DISABLED)
            );
        });
    }

    #[test]
    fn unsigned_calls_are_not_local() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let reward_period = <RewardPeriod<TestRuntime>>::get();
            let reward_period_length = reward_period.length as u64;

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            let call = crate::Call::offchain_pay_nodes {
                reward_period_index: 1u64,
                author: mock::AVN::active_validators()[0].clone(),
                signature: UintAuthorityId(1u64)
                    .sign(&("DummyProof").encode())
                    .expect("Error signing"),
            };

            assert_noop!(
                <NodeManager as ValidateUnsigned>::validate_unsigned(
                    TransactionSource::External,
                    &call
                ),
                InvalidTransaction::Call
            );
        });
    }
}
