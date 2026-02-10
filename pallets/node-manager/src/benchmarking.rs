//! # Node manager benchmarks
// Copyright 2026 Aventus DAO.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_system::{EventRecord, RawOrigin};
use sp_avn_common::{benchmarking::convert_sr25519_signature, Proof};
use sp_runtime::SaturatedConversion;

// Macro for comparing fixed point u128.
#[allow(unused_macros)]
macro_rules! assert_approx {
    ($left:expr, $right:expr, $precision:expr $(,)?) => {
        match (&$left, &$right, &$precision) {
            (left_val, right_val, precision_val) => {
                let diff = if *left_val > *right_val {
                    *left_val - *right_val
                } else {
                    *right_val - *left_val
                };
                if diff > $precision {
                    panic!("{:?} is not {:?}-close to {:?}", *left_val, *precision_val, *right_val);
                }
            },
        }
    };
}

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1 as usize)];
    assert_eq!(event, &system_event);
}

fn set_registrar<T: Config>(registrar: T::AccountId) {
    <NodeRegistrar<T>>::set(Some(registrar.clone()));
}

fn register_new_node<T: Config>(node: NodeId<T>, owner: T::AccountId) -> T::SignerId {
    let key = T::SignerId::generate_pair(None);
    <NodeRegistry<T>>::insert(node.clone(), NodeInfo::new(owner.clone(), key.clone(), 0u32, 0u64));
    <OwnedNodes<T>>::insert(owner.clone(), node, ());
    <OwnedNodesCount<T>>::mutate(owner, |count| *count += 1);

    key
}

fn create_heartbeat<T: Config>(node: NodeId<T>, reward_period_index: RewardPeriodIndex) {
    let uptime = 1u64;
    let node_info = <NodeRegistry<T>>::get(&node).unwrap();
    let single_hb_weight = Pallet::<T>::effective_heartbeat_weight(
        &node_info,
        reward_period_index,
        Pallet::<T>::time_now_sec(),
    );
    let weight = single_hb_weight.saturating_mul(uptime.into());

    <NodeUptime<T>>::mutate(&reward_period_index, &node, |maybe_info| {
        if let Some(info) = maybe_info.as_mut() {
            info.count = info.count.saturating_add(uptime);
            info.last_reported = frame_system::Pallet::<T>::block_number();
            info.weight = info.weight.saturating_add(weight);
        } else {
            *maybe_info = Some(UptimeInfo {
                count: 1,
                last_reported: frame_system::Pallet::<T>::block_number(),
                weight,
            });
        }
    });

    <TotalUptime<T>>::mutate(&reward_period_index, |total| {
        total._total_heartbeats = total._total_heartbeats.saturating_add(1u64);
        total.total_weight = total.total_weight.saturating_add(weight);
    });
}

fn fund_reward_pot<T: Config>() {
    let reward_amount = RewardAmount::<T>::get() * 2000u32.into();
    let reward_pot_address = Pallet::<T>::compute_reward_account_id();
    T::Currency::make_free_balance_be(&reward_pot_address, reward_amount);
}

fn create_author<T: Config>() -> Author<T> {
    let account = account("dummy_validator", 0, 0);
    let key = <T as avn::Config>::AuthorityId::generate_pair(Some("//bob".as_bytes().to_vec()));
    Author::<T>::new(account, key)
}

fn create_nodes_and_hearbeat<T: Config>(
    owner: T::AccountId,
    reward_period_index: RewardPeriodIndex,
    node_to_create: u32,
) -> Vec<NodeId<T>> {
    let mut registered_nodes = vec![];
    for i in 1..=node_to_create {
        let node: NodeId<T> = account("node", i, i);
        let _ = register_new_node::<T>(node.clone(), owner.clone());
        create_heartbeat::<T>(node.clone(), reward_period_index);
        registered_nodes.push(node);
    }
    registered_nodes
}

fn set_max_batch_size<T: Config>(batch_size: u32) {
    <MaxBatchSize<T>>::set(batch_size);
}

fn get_proof<T: Config>(
    relayer: &T::AccountId,
    signer: &T::AccountId,
    signature: sp_core::sr25519::Signature,
) -> Proof<T::Signature, T::AccountId> {
    return Proof {
        signer: signer.clone(),
        relayer: relayer.clone(),
        signature: convert_sr25519_signature::<T::Signature>(signature),
    }
}

fn enable_rewards<T: Config>()
where
    T: pallet_timestamp::Config<Moment = u64>,
{
    <RewardEnabled<T>>::set(true);
    pallet_timestamp::Pallet::<T>::set_timestamp(10 * 12_000);
}

fn update_min_threshold<T: Config>(threshold: Perbill) {
    <MinUptimeThreshold<T>>::set(Some(threshold));
    let mut reward_period = RewardPeriod::<T>::get();
    reward_period.length = 1000u32;
    reward_period.uptime_threshold = 100u32.into();
    <RewardPeriod<T>>::put(reward_period);
}

benchmarks! {
    where_clause {
        where T: pallet_timestamp::Config<Moment = u64>
    }

    register_node {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());

        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let signing_key: T::SignerId = account("signing_key", 3, 3);
    }: register_node(RawOrigin::Signed(registrar.clone()), node.clone(), owner.clone(), signing_key.clone())
    verify {
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), node.clone()));
        assert!(<NodeRegistry<T>>::contains_key(node.clone()));
        assert_last_event::<T>(Event::NodeRegistered {owner, node}.into());
    }

    set_admin_config_registrar {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());
        let new_registrar: T::AccountId = account("new_registrar", 0, 0);
        let config = AdminConfig::NodeRegistrar(new_registrar.clone());

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<NodeRegistrar<T>>::get() == Some(new_registrar));
    }

    set_admin_config_reward_period {
        let current_reward_period = <RewardPeriod<T>>::get().length;
        let new_reward_period = current_reward_period + 1u32;
        let config = AdminConfig::RewardPeriod(new_reward_period);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<RewardPeriod<T>>::get().length == new_reward_period);
    }

    set_admin_config_reward_batch_size {
        let current_batch_size = <MaxBatchSize<T>>::get();
        let new_batch_size = current_batch_size + 1u32;
        let config = AdminConfig::BatchSize(new_batch_size);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<MaxBatchSize<T>>::get() == new_batch_size);
    }

    set_admin_config_reward_heartbeat {
        let current_heartbeat = <HeartbeatPeriod<T>>::get();
        let new_heartbeat = current_heartbeat + 1u32;
        let config = AdminConfig::Heartbeat(new_heartbeat);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<HeartbeatPeriod<T>>::get() == new_heartbeat);
    }

    set_admin_config_reward_amount {
        let current_amount = <RewardAmount<T>>::get();
        let new_amount = current_amount + 1u32.into();
        let config = AdminConfig::RewardAmount(new_amount);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<RewardAmount<T>>::get() == new_amount);
    }

    set_admin_config_reward_enabled {
        let current_flag = <RewardEnabled<T>>::get();
        let new_flag = !current_flag;
        let config = AdminConfig::RewardToggle(new_flag);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<RewardEnabled<T>>::get() == new_flag);
    }

    set_admin_config_min_threshold {
        let current_threshold = <MinUptimeThreshold<T>>::get();
        let new_threshold = Perbill::from_percent(80);
        let config = AdminConfig::MinUptimeThreshold(new_threshold);

    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<MinUptimeThreshold<T>>::get() == Some(new_threshold));
    }

    set_admin_config_auto_stake_duration {
        let current_duration = <AutoStakeDurationSec<T>>::get();
        let new_duration = current_duration + 60;
        let config = AdminConfig::AutoStakeDuration(new_duration);
    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<AutoStakeDurationSec<T>>::get() == new_duration);
    }

    set_admin_config_max_unstake_percentage {
        let current_percentage = <MaxUnstakePercentage<T>>::get();
        let new_percentage = Perbill::from_percent(17);
        let config = AdminConfig::MaxUnstakePercentage(new_percentage);
    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<MaxUnstakePercentage<T>>::get() == new_percentage);
    }

    set_admin_config_unstake_period {
        let current_duration = <UnstakePeriodSec<T>>::get();
        let new_duration = current_duration + 60;
        let config = AdminConfig::UnstakePeriod(new_duration);
    }: set_admin_config(RawOrigin::Root, config.clone())
    verify {
        assert!(<UnstakePeriodSec<T>>::get() == new_duration);
    }

    on_initialise_with_new_reward_period {
        let reward_period = <RewardPeriod<T>>::get();
        let block_number: BlockNumberFor<T> = reward_period.first + BlockNumberFor::<T>::from(reward_period.length) + 1u32.into();
        enable_rewards::<T>();
    }: { Pallet::<T>::on_initialize(block_number) }
    verify {
        let new_reward_period_index = reward_period.current + 1u64;
        let new_reward_period = <RewardPeriod<T>>::get();
        assert!(new_reward_period_index== new_reward_period.current);
        assert_last_event::<T>(Event::NewRewardPeriodStarted {
            reward_period_index: new_reward_period_index,
            reward_period_length: reward_period.length,
            uptime_threshold: new_reward_period.uptime_threshold,
            previous_period_reward: RewardAmount::<T>::get()}.into());
    }

    on_initialise_no_reward_period {
        let reward_period = <RewardPeriod<T>>::get();
        let block_number: BlockNumberFor<T> = BlockNumberFor::<T>::from(reward_period.length) - 1u32.into();
        enable_rewards::<T>();
    }: { Pallet::<T>::on_initialize(block_number) }
    verify {
        assert!(reward_period.current== <RewardPeriod<T>>::get().current);
    }

    offchain_submit_heartbeat {
        enable_rewards::<T>();

        // update the min threshold first
        update_min_threshold::<T>(Perbill::from_percent(99));

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let node: NodeId<T> = account("node", 0, 0);
        let owner: T::AccountId = account("owner", 0, 0);
        let signing_key: T::SignerId = register_new_node::<T>(node.clone(), owner.clone());
        create_heartbeat::<T>(node.clone(), reward_period_index);

        // Move forward to the next heartbeat period
        <frame_system::Pallet<T>>::set_block_number(
            frame_system::Pallet::<T>::block_number() + <HeartbeatPeriod<T>>::get().into() + 1u32.into()
        );

        let heartbeat_count = 1u64;
        let signature = signing_key.sign(
            &(HEARTBEAT_CONTEXT, heartbeat_count, reward_period_index).encode()
        ).expect("Error signing");
    }: offchain_submit_heartbeat(RawOrigin::None, node.clone(), reward_period_index, heartbeat_count, signature)
    verify {
        let uptime_info = <NodeUptime<T>>::get(reward_period_index, &node).expect("No uptime info");
        assert!(uptime_info.count == heartbeat_count + 1);
        assert_last_event::<T>(Event::HeartbeatReceived {reward_period_index, node}.into());
    }

    offchain_pay_nodes {
        let registered_nodes = 1001;

        // This should affect the performance of the extrinsic.
        let b in 1 .. 1000;

        enable_rewards::<T>();
        fund_reward_pot::<T>();
        set_max_batch_size::<T>(b);

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);
        let author = create_author::<T>();

        let _ = create_nodes_and_hearbeat::<T>(owner.clone(), reward_period_index, registered_nodes);

        // Move forward to the next reward period
        <frame_system::Pallet<T>>::set_block_number((reward_period.length + 1).into());
        let current_block_number = frame_system::Pallet::<T>::block_number();
        <frame_system::Pallet<T>>::set_block_number(current_block_number + reward_period.length.into());
        Pallet::<T>::on_initialize(current_block_number);
        let signature = author.key.sign(
            &(PAYOUT_REWARD_CONTEXT, reward_period_index).encode()
        ).expect("Error signing");
    }: offchain_pay_nodes(RawOrigin::None, reward_period_index, author ,signature)
    verify {
        let max_batch_size = MaxBatchSize::<T>::get();
        let nodes_to_pay = max_batch_size.min(registered_nodes);
        let ratio = Perquintill::from_rational(nodes_to_pay as u128, registered_nodes as u128);
        let total_rewards_u128: u128 = (RewardAmount::<T>::get()).saturated_into();
        let expected_balance = ratio.mul_floor(total_rewards_u128).saturated_into::<BalanceOf<T>>();

        assert_approx!(T::Currency::free_balance(&owner.clone()), expected_balance, 1_000u32.saturated_into::<BalanceOf<T>>());
    }

    #[extra]
    pay_nodes_constant_batch_size {
        /* Prove that the read/write is constant time with respect to the batch size.
           Even if the number of registered nodes (n) increases. You should see something like:

             Median Slopes Analysis
             ========
             -- Extrinsic Time --

             Model:
             Time ~=    514.2
                + n    0.554 µs

             Reads = 30 + (0 * n)
             Writes = 13 + (0 * n)
             Recorded proof Size = 2601 + (12 * n)

        */

        // This should NOT affect the performance of the extrinsic. The execution time should be constant.
        let n in 1 .. 100;

        enable_rewards::<T>();
        fund_reward_pot::<T>();

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);
        let author = create_author::<T>();

        let _ = create_nodes_and_hearbeat::<T>(owner.clone(), reward_period_index, n);

        // Move forward to the next reward period
        <frame_system::Pallet<T>>::set_block_number((reward_period.length + 1).into());
        let current_block_number = frame_system::Pallet::<T>::block_number();
        <frame_system::Pallet<T>>::set_block_number(current_block_number + reward_period.length.into());
        Pallet::<T>::on_initialize(current_block_number);
        let signature = author.key.sign(
            &(PAYOUT_REWARD_CONTEXT, reward_period_index).encode()
        ).expect("Error signing");
    }: offchain_pay_nodes(RawOrigin::None, reward_period_index, author ,signature)
    verify {
        let max_batch_size = MaxBatchSize::<T>::get();
        let nodes_to_pay = max_batch_size.min(n);
        let ratio = Perquintill::from_rational(nodes_to_pay as u128, n as u128);
        let total_rewards_u128: u128 = (RewardAmount::<T>::get()).saturated_into();
        let expected_balance = ratio.mul_floor(total_rewards_u128).saturated_into::<BalanceOf<T>>();

        assert_approx!(T::Currency::free_balance(&owner.clone()), expected_balance, 1_000u32.saturated_into::<BalanceOf<T>>());
    }

    signed_register_node {
        enable_rewards::<T>();
        let registrar_key = crate::sr25519::app_sr25519::Public::generate_pair(None);
        let registrar: T::AccountId =
            T::AccountId::decode(&mut Encode::encode(&registrar_key).as_slice()).expect("valid account id");
        set_registrar::<T>(registrar.clone());

        let relayer: T::AccountId = account("relayer", 11, 11);
        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let signing_key: T::SignerId = account("signing_key", 3, 3);
        let now = frame_system::Pallet::<T>::block_number();

        let signed_payload = encode_signed_register_node_params::<T>(
            &relayer.clone(),
            &node,
            &owner,
            &signing_key,
            &now.clone(),
        );

        let signature = registrar_key.sign(&signed_payload).ok_or("Error signing proof")?;
        let proof = get_proof::<T>(&relayer.clone(), &registrar, signature.into());
    }: signed_register_node(RawOrigin::Signed(registrar.clone()), proof.clone(), node.clone(), owner.clone(), signing_key.clone(), now)
    verify {
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), node.clone()));
        assert!(<NodeRegistry<T>>::contains_key(node.clone()));
        assert_last_event::<T>(Event::NodeRegistered{owner, node}.into());
    }

    deregister_nodes {
        let b in 1 .. MAX_NODES_TO_DEREGISTER;
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());

        enable_rewards::<T>();
        fund_reward_pot::<T>();

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);

        let nodes_to_deregister = create_nodes_and_hearbeat::<T>(owner.clone(), reward_period_index, b);

        // Show that the nodes are registered
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), nodes_to_deregister[0].clone()));
        assert!(<NodeRegistry<T>>::contains_key(nodes_to_deregister[0].clone()));

    }: deregister_nodes(
        RawOrigin::Signed(registrar.clone()),
        owner.clone(),
        BoundedVec::truncate_from(nodes_to_deregister.clone()))
    verify {
        for node in &nodes_to_deregister {
            assert!(!<OwnedNodes<T>>::contains_key(owner.clone(), node));
            assert!(!<NodeRegistry<T>>::contains_key(node));
        }
        assert_last_event::<T>(Event::NodeDeregistered{
            owner,
            node: nodes_to_deregister[nodes_to_deregister.len() - 1].clone()}.into());
    }

    signed_deregister_nodes {
        let b in 1 .. MAX_NODES_TO_DEREGISTER;
        let registrar_key = crate::sr25519::app_sr25519::Public::generate_pair(None);
        let registrar: T::AccountId =
            T::AccountId::decode(&mut Encode::encode(&registrar_key).as_slice()).expect("valid account id");

        set_registrar::<T>(registrar.clone());
        enable_rewards::<T>();
        fund_reward_pot::<T>();

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);

        let nodes_to_deregister = create_nodes_and_hearbeat::<T>(owner.clone(), reward_period_index, b);

        // Show that at least some of the nodes are registered
        assert!(<OwnedNodes<T>>::contains_key(owner.clone(), nodes_to_deregister[0].clone()));
        assert!(<NodeRegistry<T>>::contains_key(nodes_to_deregister[0].clone()));

        let relayer: T::AccountId = account("relayer", 11, 11);
        let now = frame_system::Pallet::<T>::block_number();

        let bounded_nodes_to_deregister = BoundedVec::truncate_from(nodes_to_deregister.clone());
        let signed_payload = encode_signed_deregister_node_params::<T>(
            &relayer.clone(),
            &owner,
            &bounded_nodes_to_deregister,
            &(nodes_to_deregister.len() as u32),
            &now.clone(),
        );

        let signature = registrar_key.sign(&signed_payload).ok_or("Error signing proof")?;
        let proof = get_proof::<T>(&relayer.clone(), &registrar, signature.into());
    }: signed_deregister_nodes(RawOrigin::Signed(registrar.clone()), proof, owner.clone(), bounded_nodes_to_deregister, now)
    verify {
        for node in &nodes_to_deregister {
            assert!(!<OwnedNodes<T>>::contains_key(owner.clone(), node));
            assert!(!<NodeRegistry<T>>::contains_key(node));
        }
        assert_last_event::<T>(Event::NodeDeregistered{
            owner,
            node: nodes_to_deregister[nodes_to_deregister.len() - 1].clone()}.into());
    }

    update_signing_key {
        let registrar: T::AccountId = account("registrar", 0, 0);
        set_registrar::<T>(registrar.clone());

        let owner: T::AccountId = account("owner", 1, 1);
        let node: NodeId<T> = account("node", 2, 2);
        let current_signing_key: T::SignerId = register_new_node::<T>(node.clone(), owner.clone());
        let new_signing_key: T::SignerId = account("new_signing_key", 3, 3);
    }: update_signing_key(RawOrigin::Signed(owner.clone()), node.clone(), new_signing_key.clone())
    verify {
        let node_info = <NodeRegistry<T>>::get(&node).expect("Node must be registered");
        assert!(node_info.signing_key == new_signing_key);
        assert_last_event::<T>(Event::SigningKeyUpdated {owner, node}.into());
    }

    add_stake {
        let registrar_key = crate::sr25519::app_sr25519::Public::generate_pair(None);
        let registrar: T::AccountId =
            T::AccountId::decode(&mut Encode::encode(&registrar_key).as_slice()).expect("valid account id");

        set_registrar::<T>(registrar.clone());
        enable_rewards::<T>();
        fund_reward_pot::<T>();

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);
        T::Currency::make_free_balance_be(&owner.clone(), 1_000_000u32.into());
        let _ = create_nodes_and_hearbeat::<T>(owner.clone(), reward_period_index, 2);
    }: add_stake(RawOrigin::Signed(owner.clone()), 100u32.into())
    verify {
        let stake = OwnerStake::<T>::get(&owner).unwrap();
        assert!(stake.amount == 100u32.into());
        assert_last_event::<T>(Event::StakeAdded { owner, amount: 100u32.into(), new_total: stake.amount }.into());
    }

    remove_stake {
        let registrar_key = crate::sr25519::app_sr25519::Public::generate_pair(None);
        let registrar: T::AccountId =
            T::AccountId::decode(&mut Encode::encode(&registrar_key).as_slice()).expect("valid account id");

        set_registrar::<T>(registrar.clone());
        enable_rewards::<T>();
        fund_reward_pot::<T>();
        // Make sure we can unstake
        AutoStakeDurationSec::<T>::put(0u64);
        UnstakePeriodSec::<T>::put(1_000u64);

        let reward_period = <RewardPeriod<T>>::get();
        let reward_period_index = reward_period.current;
        let owner: T::AccountId = account("owner", 0, 0);
        T::Currency::make_free_balance_be(&owner.clone(), 1_000_000u32.into());
        let _ = create_nodes_and_hearbeat::<T>(owner.clone(), reward_period_index, 2);
        Pallet::<T>::do_add_stake(&owner, 100u32.into()).unwrap();
        // Go forward in time to make the stake available for unstaking
        pallet_timestamp::Pallet::<T>::set_timestamp(10_000 * 12_000);
    }: remove_stake(RawOrigin::Signed(owner.clone()), Some(10u32.into()))
    verify {
        let stake = OwnerStake::<T>::get(&owner).unwrap();
        assert!(stake.amount == (100u32 - 10u32).into());
        assert_last_event::<T>(Event::StakeRemoved { owner, amount: 10u32.into(), new_total: stake.amount }.into());
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().with_genesis_config().as_externality(),
    crate::mock::TestRuntime,
);
