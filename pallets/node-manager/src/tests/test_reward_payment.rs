// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{mock::*, offchain::OCW_ID, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;

#[derive(Clone)]
struct Context {
    registrar: AccountId,
    owner: AccountId,
    ocw_node: AccountId,
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
        let ocw_node = register_nodes(registrar, owner, num_of_nodes);

        Context { registrar, owner, ocw_node }
    }
}

fn register_nodes(registrar: AccountId, owner: AccountId, num_of_nodes: u8) -> AccountId {
    let reward_period = <RewardPeriod<TestRuntime>>::get().current;

    for i in 0..num_of_nodes {
        register_node_and_send_heartbeat(registrar, owner.clone(), reward_period, i, None);
    }

    let this_node = TestAccount::new([0 as u8; 32]).account_id();
    let this_node_signing_key = 0;

    set_ocw_node_id(this_node);
    UintAuthorityId::set_all_keys(vec![UintAuthorityId(this_node_signing_key)]);

    return this_node
}

fn register_node_and_send_heartbeat(
    registrar: AccountId,
    owner: AccountId,
    reward_period: RewardPeriodIndex,
    id: u8,
    stake: Option<BalanceOf<TestRuntime>>,
) -> AccountId {
    let node_id = TestAccount::new([id as u8; 32]).account_id();
    let signing_key_id = id + 1;

    assert_ok!(NodeManager::register_node(
        RuntimeOrigin::signed(registrar),
        node_id,
        owner,
        UintAuthorityId(signing_key_id as u64),
    ));

    if let Some(stake) = stake {
        let owner_balance = Balances::free_balance(&owner);
        Balances::make_free_balance_be(&owner, owner_balance + stake);
        assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), node_id, stake));
    }

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
                info.weight = info.weight.saturating_add(weight);
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

fn remove_ocw_run_lock() {
    let key = [OCW_ID.as_slice(), b"::last_run"].concat();
    let mut storage = StorageValueRef::persistent(&key);
    storage.clear();
}

mod reward {
    use super::*;

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
            assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

            // Check if the transaction from the mempool is what we expected
            assert!(matches!(
                tx.function,
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
            let reward_fee = <AppChainFeePercentage<TestRuntime>>::get() * reward_amount;
            let net_reward = reward_amount - reward_fee;
            assert_eq!(Balances::reserved_balance(&context.owner), net_reward);
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
            assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

            // We should have processed the first batch of payments
            assert_eq!(true, <LastPaidPointer<TestRuntime>>::get().is_some());
            let gross_owner_reward = reward_amount / 2;
            let owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_owner_reward;
            let expected_owner_reward = gross_owner_reward - owner_fee;
            assert_eq!(Balances::reserved_balance(&context.owner), expected_owner_reward);

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
            assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

            // This should complete the payment
            assert_eq!(true, <RewardPot<TestRuntime>>::get(reward_period_to_pay).is_none());
            assert_eq!(
                true,
                <NodeUptime<TestRuntime>>::iter_prefix(reward_period_to_pay).next().is_none()
            );
            assert_eq!(true, <LastPaidPointer<TestRuntime>>::get().is_none());
            let gross_owner_reward = reward_amount;
            let owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_owner_reward;
            let expected_owner_reward = gross_owner_reward - owner_fee;
            assert_eq!(Balances::reserved_balance(&context.owner), expected_owner_reward);
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
                None,
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
            assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));
            // The owner has received the reward
            // total_expected_uptime - 1 because we run the OCW
            let gross_new_owner_reward = Perquintill::from_rational(
                total_expected_uptime as u128 - 1,
                total_uptime._total_heartbeats as u128,
            ) * reward_amount;
            let new_owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_new_owner_reward;
            let expected_new_owner_reward = gross_new_owner_reward - new_owner_fee;

            assert!(
                Balances::reserved_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
                "Value {} and {} differs by more than 10",
                Balances::reserved_balance(&new_owner),
                expected_new_owner_reward
            );

            let gross_old_owner_reward = reward_amount - gross_new_owner_reward;
            let old_owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_old_owner_reward;
            let expected_old_owner_reward = gross_old_owner_reward - old_owner_fee;

            assert!(
                Balances::reserved_balance(&context.owner).abs_diff(expected_old_owner_reward) <= 20,
                "Value {} differs by more than 20",
                Balances::reserved_balance(&context.owner).abs_diff(expected_old_owner_reward)
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
                None,
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
            assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

            // The owner has received the reward
            let gross_new_owner_reward = Perquintill::from_rational(
                total_expected_uptime as u128,
                total_uptime._total_heartbeats as u128,
            ) * reward_amount;
            let new_owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_new_owner_reward;
            let expected_new_owner_reward = gross_new_owner_reward - new_owner_fee;

            assert!(
                Balances::reserved_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
                "Values {} differ by more than 10",
                Balances::reserved_balance(&new_owner).abs_diff(expected_new_owner_reward)
            );
            let gross_old_owner_reward = reward_amount - gross_new_owner_reward;
            let old_owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_old_owner_reward;
            let expected_old_owner_reward = gross_old_owner_reward - old_owner_fee;

            assert!(
                Balances::reserved_balance(&context.owner).abs_diff(expected_old_owner_reward) <= 100,
                "Value {}  differs by more than 100",
                Balances::reserved_balance(&context.owner).abs_diff(expected_old_owner_reward)
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
                None,
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
            assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

            // The owner has received the reward
            // The system limits the reward to the expected uptime
            let gross_new_owner_reward = Perquintill::from_rational(
                total_expected_uptime as u128,
                total_uptime._total_heartbeats as u128,
            ) * reward_amount;
            let new_owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_new_owner_reward;
            let expected_new_owner_reward = gross_new_owner_reward - new_owner_fee;

            assert!(
                Balances::reserved_balance(&new_owner).abs_diff(expected_new_owner_reward) < 1,
                "Values {} and {} differ by more than 1",
                Balances::reserved_balance(&new_owner),
                expected_new_owner_reward,
            );
            //The old owner gets a smaller share of the rewards because the total_uptime has now
            // increased by the extra uptime
            let gross_old_owner_reward =
                Perquintill::from_rational(1u128, total_uptime._total_heartbeats as u128) *
                    reward_amount *
                    (node_count as u128);
            let old_owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_old_owner_reward;
            let expected_old_owner_reward = gross_old_owner_reward - old_owner_fee;

            assert!(
                Balances::reserved_balance(&context.owner).abs_diff(expected_old_owner_reward) < 1,
                "Value {} differs by more than 1",
                Balances::reserved_balance(&context.owner).abs_diff(expected_old_owner_reward)
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
                    .abs_diff(initial_pot - (gross_new_owner_reward + gross_old_owner_reward)) <
                    10,
                "Value {} and {} differs by more than 10",
                Balances::free_balance(&NodeManager::compute_reward_account_id()),
                initial_pot - (gross_new_owner_reward + gross_old_owner_reward)
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
                None,
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
            assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

            // The owner has received the reward
            let gross_new_owner_reward = Perquintill::from_rational(
                total_expected_uptime as u128,
                total_uptime._total_heartbeats as u128,
            ) * reward_amount;
            let new_owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_new_owner_reward;
            let expected_new_owner_reward = gross_new_owner_reward - new_owner_fee;

            assert!(
                Balances::reserved_balance(&new_owner).abs_diff(expected_new_owner_reward) < 10,
                "Values {} and {} differ by more than 10",
                Balances::reserved_balance(&new_owner),
                expected_new_owner_reward
            );
            let gross_old_owner_reward = reward_amount - gross_new_owner_reward;
            let old_owner_fee = <AppChainFeePercentage<TestRuntime>>::get() * gross_old_owner_reward;
            let expected_old_owner_reward = gross_old_owner_reward - old_owner_fee;

            assert!(
                Balances::reserved_balance(&context.owner).abs_diff(expected_old_owner_reward) <= 100,
                "Value {} differs by more than 100",
                Balances::reserved_balance(&context.owner).abs_diff(expected_old_owner_reward)
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
    fn reward_share_increases_with_genesis_and_stake_bonus() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let new_owner_stake = 4_000_000_000_000_000_000_000u128;
            let reward_period_info = <RewardPeriod<TestRuntime>>::get();
            let reward_period_to_pay = reward_period_info.current;
            // No genesis bonus, no stake for default context node
            let context = Context::new(1u8);

            let new_owner = TestAccount::new([111u8; 32]).account_id();

            // Ensure 50% genesis bonus
            NextNodeSerialNumber::<TestRuntime>::put(2001);
            let new_node = register_node_and_send_heartbeat(
                context.registrar.clone(),
                new_owner,
                reward_period_to_pay,
                199,
                Some(new_owner_stake),
            );

            let node_uptime_a =
                NodeUptime::<TestRuntime>::get(reward_period_to_pay, &context.ocw_node).unwrap();
            let node_uptime_b =
                NodeUptime::<TestRuntime>::get(reward_period_to_pay, &new_node).unwrap();
            assert_eq!(node_uptime_a.weight, 100_000_000u128); // Node A: base
                                                            // Node B: 50% genesis bonus + 3x stake multiplier => 4.5x base
            assert_eq!(node_uptime_b.weight, 450_000_000u128);

            let reward_period_length = reward_period_info.length as u64;
            let total_expected_uptime =
                NodeManager::calculate_uptime_threshold(reward_period_length as u32);

            // The node's uptime is exactly the threshold, so they should get the full rewards
            incr_heartbeats(
                reward_period_to_pay,
                vec![context.ocw_node],
                total_expected_uptime as u64 - 1,
            );
            incr_heartbeats(reward_period_to_pay, vec![new_node], total_expected_uptime as u64 - 1);

            let node_uptime_a =
                NodeUptime::<TestRuntime>::get(reward_period_to_pay, &context.ocw_node).unwrap();
            let node_uptime_b =
                NodeUptime::<TestRuntime>::get(reward_period_to_pay, &new_node).unwrap();

            // Node A: base
            assert_eq!(node_uptime_a.weight, 100_000_000u128 * total_expected_uptime as u128);
            // Node B: 50% genesis bonus + 3x stake multiplier => 4.5x base
            assert_eq!(node_uptime_b.weight, 450_000_000u128 * total_expected_uptime as u128);

            // Set a custom reward amount for easier calculations
            <RewardAmount<TestRuntime>>::put(1_000u128);

            // Complete a reward period
            roll_forward((reward_period_length - System::block_number()) + 1);

            // Stake before payout
            let previous_stake_a =
                NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap().stake.amount;
            let previous_stake_b = NodeRegistry::<TestRuntime>::get(&new_node).unwrap().stake.amount;

            // Baance before payout
            let balance_a_before = Balances::free_balance(&context.owner);
            let balance_b_before = Balances::free_balance(&new_owner);

            // Pay out
            mock_get_finalised_block(
                &mut offchain_state.write(),
                &Some(hex::encode(1u32.encode()).into()),
            );
            NodeManager::offchain_worker(System::block_number());
            let tx = pop_tx_from_mempool(pool_state);
            assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));

            // Stake after payout
            let current_stake_a =
                NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap().stake.amount;
            let current_stake_b = NodeRegistry::<TestRuntime>::get(&new_node).unwrap().stake.amount;

            let balance_a_after = Balances::free_balance(&context.owner);
            let balance_b_after = Balances::free_balance(&new_owner);

            // 4.5 / (4.5 + 1.0) = 0.818181... => 818 (0.81%) - 5% fee vs 181 (0.18%) - 5% fee
            // (flooring)
            assert_eq!(current_stake_a, previous_stake_a + 172u128);
            assert_eq!(current_stake_b, previous_stake_b + 778u128);

            // Balances should not increase because funds are reserved
            assert_eq!(balance_a_after, balance_a_before);
            assert_eq!(balance_b_after, balance_b_before);

            // Reserved balance should match staked amount
            let reserved_a = Balances::reserved_balance(&context.owner);
            assert_eq!(reserved_a, current_stake_a);

            let reserved_b = Balances::reserved_balance(&new_owner);
            assert_eq!(reserved_b, current_stake_b);
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

}

mod end_2_end {
    use super::*;

    fn complete_reward_period_and_pay(pool_state: Arc<RwLock<PoolState>>, offchain_state: Arc<RwLock<OffchainState>>) {
        let reward_period = <RewardPeriod<TestRuntime>>::get();
        let reward_period_length = reward_period.length as u64;

        // Complete a reward period
        roll_forward(reward_period_length + 1);

        // Pay out
        mock_get_finalised_block(
            &mut offchain_state.write(),
            &Some(hex::encode(1u32.encode()).into()),
        );
        NodeManager::offchain_worker(System::block_number());
        let tx = pop_tx_from_mempool(pool_state.clone());
        assert_ok!(tx.function.clone().dispatch(frame_system::RawOrigin::None.into()));
    }

    fn increase_timestamp_by(seconds: u64) {
        let now: u64 = Timestamp::now().as_secs();
        Timestamp::set_timestamp((now + seconds) * 1000);
    }

    fn set_timestamp(target_sec: u64) -> Result<(), ()> {
        let now = Timestamp::now().as_secs();
        if target_sec < now {
            return Err(());
        }
        Timestamp::set_timestamp(target_sec * 1000);
        Ok(())
    }

    #[test]
    fn works() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .with_authors()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let total_reward_per_period = 1_000u128;
            let new_owner_stake = 4_000_000_000_000_000_000_000u128;
            let reward_period_info = <RewardPeriod<TestRuntime>>::get();
            let reward_period = reward_period_info.current;
            let new_owner = TestAccount::new([111u8; 32]).account_id();
            // Fund new owner account so it can stake
            Balances::make_free_balance_be(&new_owner, new_owner_stake * 2);
    println!("Reward period a : {:?}", reward_period);
            // Set a custom reward amount for easier calculations
            <RewardAmount<TestRuntime>>::put(total_reward_per_period);

            // No genesis bonus, no stake for default context node
            let context = Context::new(1u8);

            // Reset the reward pot balance
            Balances::make_free_balance_be(&NodeManager::compute_reward_account_id(), total_reward_per_period * 1_000_000_000_000u128);
    println!("Reward pot balance before payout: {}", Balances::free_balance(&NodeManager::compute_reward_account_id()));

            // No stake, no genesis bonus
            let new_node = register_node_and_send_heartbeat(
                context.registrar.clone(),
                new_owner,
                reward_period,
                199, // unique id
                None, // No stake
            );

            let reward_period_length = reward_period_info.length as u64;
            let expected_uptime =
                NodeManager::calculate_uptime_threshold(reward_period_length as u32);

            // The node's uptime is exactly the threshold, so they should get the full rewards
            incr_heartbeats(reward_period, vec![context.ocw_node], expected_uptime as u64 - 1);
            incr_heartbeats(reward_period, vec![new_node], expected_uptime as u64 - 1);

            let context_node_uptime =
                NodeUptime::<TestRuntime>::get(reward_period, &context.ocw_node).unwrap();
            let new_node_uptime =
                NodeUptime::<TestRuntime>::get(reward_period, &new_node).unwrap();

            // The weight is the same for both
            assert_eq!(context_node_uptime.weight, 100_000_000u128 * expected_uptime as u128);
            assert_eq!(new_node_uptime.weight, 100_000_000u128 * expected_uptime as u128);

            // Pay out
            complete_reward_period_and_pay(pool_state.clone(), offchain_state.clone());
        println!("Reward pot balance: {}", Balances::free_balance(&NodeManager::compute_reward_account_id()));
            let context_node_info = NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap();
            let new_node_info = NodeRegistry::<TestRuntime>::get(&new_node).unwrap();

            // Stake after payout - we are still in auto stake period
            let gross_per_node_reward = total_reward_per_period / 2; // equal share 500
            let node_reward = gross_per_node_reward - NodeManager::calculate_appchain_fee(gross_per_node_reward);

            assert_eq!(new_node_info.stake.amount, node_reward);
            assert_eq!(context_node_info.stake.amount,  new_node_info.stake.amount);

            // Get the new reward period
            let reward_period = <RewardPeriod<TestRuntime>>::get().current;
    println!("Reward period b : {:?}", reward_period);

            // Send half of the required hb's before staking
            incr_heartbeats(reward_period, vec![context.ocw_node], (expected_uptime/2) as u64);
            incr_heartbeats(reward_period, vec![new_node], (expected_uptime/2) as u64);

            // Add stake for the new node
            assert_ok!(NodeManager::add_stake(
                RuntimeOrigin::signed(new_owner.clone()),
                    new_node.clone(),
                    new_owner_stake
            ));

            // And the other half after staking. This half will have a bigger weight.
            incr_heartbeats(reward_period, vec![context.ocw_node], (expected_uptime/2) as u64);
            incr_heartbeats(reward_period, vec![new_node], (expected_uptime/2) as u64);

            // Pay out
            complete_reward_period_and_pay(pool_state.clone(), offchain_state.clone());
println!("Reward pot balance : {}", Balances::free_balance(&NodeManager::compute_reward_account_id()));
            let context_node_stake =
                NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap().stake.amount;
            let new_node_stake = NodeRegistry::<TestRuntime>::get(&new_node).unwrap().stake.amount;

            // Get the remaining 1/3rd of the rewards
            let expected_node_a_stake = node_reward + (total_reward_per_period / 3);
            // 4K stake should give 2 extra virtual nodes (3 in total). But half of the heartbeats were before staking,
            // so we have 1 extra virtual node (2 total) => 2/3rd of the rewards
            let expected_node_b_stake = new_owner_stake + node_reward + (total_reward_per_period * 2 / 3);

            assert_eq!(context_node_stake, expected_node_a_stake - NodeManager::calculate_appchain_fee(total_reward_per_period / 3));
            assert_eq!(new_node_stake, expected_node_b_stake - NodeManager::calculate_appchain_fee(total_reward_per_period * 2 / 3));

            // Unstaking is still now allowed
            assert_noop!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    Some(1_000u128)
                ),
                Error::<TestRuntime>::AutoStakeStillActive
            );

            // Set time to unlock the stake. Use context node because its registered first
            //Timestamp::set_timestamp(context_node_info.auto_stake_expiry * 1000);
            set_timestamp(context_node_info.auto_stake_expiry).unwrap();
            let new_owner_balance_before = Balances::free_balance(&new_owner);

            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    Some(1_000u128)
                )
            );

            // Stake was snapshoted and max unstake calculated
            let new_node_info = NodeRegistry::<TestRuntime>::get(&new_node).unwrap();
            let expected_new_node_max_unstake = <MaxUnstakePercentage<TestRuntime>>::get() * (new_node_info.stake.amount + 1_000);
            assert_eq!(new_node_info.stake.max_unstake_per_period.unwrap(), expected_new_node_max_unstake);
            // Remaining allowable unstake can also be claimed
            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    None
                )
            );

            // No more unstake allowed in the same period
            assert_noop!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    Some(1_000u128)
                ),
                Error::<TestRuntime>::NoAvailableStakeToUnstake
            );

            assert_eq!(Balances::free_balance(&new_owner), new_owner_balance_before + expected_new_node_max_unstake);

            // Go forward by 2 periods
            increase_timestamp_by(UnstakePeriodSec::<TestRuntime>::get() * 2);
            let new_owner_balance_before = Balances::free_balance(&new_owner);

            // Unstake 2 period's worth
            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    Some(expected_new_node_max_unstake * 2)
                )
            );

            assert_eq!(Balances::free_balance(&new_owner), new_owner_balance_before + (expected_new_node_max_unstake*2));
            // No more unstake allowed in the same period
            assert_noop!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    Some(1_000u128)
                ),
                Error::<TestRuntime>::NoAvailableStakeToUnstake
            );

            // Go past staking restriction period
            set_timestamp(context_node_info.auto_stake_expiry + RestrictedUnstakeDurationSec::<TestRuntime>::get()).unwrap();


            let new_owner_balance_before = Balances::free_balance(&new_owner);
            let previous_stake = NodeRegistry::<TestRuntime>::get(&new_node).unwrap().stake.amount;
            // Unstake back to back large amounts (> max_unstake_per_period)
            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    Some(new_node_info.stake.max_unstake_per_period.unwrap() + 1)
                )
            );
            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    Some(10u128)
                )
            );
            // Remove all remaining stake
            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    None
                )
            );

            // Remove all context node remaining stake
            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(context.owner.clone()),
                    context.ocw_node,
                    None
                )
            );

            assert_eq!(Balances::free_balance(&new_owner), new_owner_balance_before + previous_stake);
            assert_eq!(NodeRegistry::<TestRuntime>::get(&new_node).unwrap().stake.amount, 0);
            assert_eq!(NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap().stake.amount, 0);

            let reward_period = <RewardPeriod<TestRuntime>>::get().current;

            // Send heartbeats for the new reward period
            incr_heartbeats(reward_period, vec![context.ocw_node], (expected_uptime) as u64);
            incr_heartbeats(reward_period, vec![new_node], (expected_uptime) as u64);

            let context_owner_balance_before = Balances::free_balance(&context.owner);
            let new_owner_balance_before = Balances::free_balance(&new_owner);

            // Pay out
            complete_reward_period_and_pay(pool_state.clone(), offchain_state.clone());

            // We are back to sharing rewards equally because all the stake has been removed
            let gross_per_node_reward = total_reward_per_period / 2; // equal share 500
            let node_reward = gross_per_node_reward - NodeManager::calculate_appchain_fee(gross_per_node_reward);


            let context_node_info = NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap();
            let new_node_info = NodeRegistry::<TestRuntime>::get(&new_node).unwrap();

            // The free balance of the owner increases
            assert_eq!(Balances::free_balance(&context.owner), context_owner_balance_before + node_reward);
            assert_eq!(Balances::free_balance(&new_owner), new_owner_balance_before + node_reward);

            // We are not autostaking anymore so stake doesn't change
            assert_eq!(new_node_info.stake.amount, 0);
            assert_eq!(context_node_info.stake.amount,  new_node_info.stake.amount);

            // Add Stake again
            Balances::make_free_balance_be(&context.owner,  new_owner_stake + 1);
            assert_ok!(NodeManager::add_stake(
                RuntimeOrigin::signed(context.owner.clone()),
                    context.ocw_node.clone(),
                    new_owner_stake + 1
            ));

            // Add stake for the new node
            assert_ok!(NodeManager::add_stake(
                RuntimeOrigin::signed(new_owner.clone()),
                    new_node.clone(),
                    new_owner_stake
            ));

            // Stake is added
            assert_eq!(NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap().stake.amount, new_owner_stake + 1);
            assert_eq!(NodeRegistry::<TestRuntime>::get(&new_node).unwrap().stake.amount, new_owner_stake);

            println!("\nContext node: {:?}", NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap());
            println!("\nNew node: {:?}", NodeRegistry::<TestRuntime>::get(&new_node).unwrap());

            // Unstake everything without changing time
            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(context.owner.clone()),
                    context.ocw_node.clone(),
                    None
                )
            );
            assert_ok!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(new_owner.clone()),
                    new_node,
                    None
                )
            );

            // Stake is removed
            assert_eq!(NodeRegistry::<TestRuntime>::get(&context.ocw_node).unwrap().stake.amount, 0);
            assert_eq!(NodeRegistry::<TestRuntime>::get(&new_node).unwrap().stake.amount, 0);
        });
    }
}


//2800000000000000000775
//2800000000000000000775
