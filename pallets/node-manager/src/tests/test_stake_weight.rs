use super::*;
use crate::mock::*;
use frame_support::{assert_noop, assert_ok};

mod stake_and_reward_weight_tests {
    use super::*;
    use sp_runtime::testing::UintAuthorityId;

    fn get_owner(id: u8) -> AccountId {
        TestAccount::new([id; 32]).account_id()
    }

    fn get_node(id: u8) -> AccountId {
        TestAccount::new([200 + id; 32]).account_id()
    }

    fn get_signing_key(id: u8) -> UintAuthorityId {
        // In mock runtime SignerId is UintAuthorityId (u64 wrapper).
        UintAuthorityId((100 + id) as u64)
    }

    fn setup_registrar(registrar: &AccountId) {
        <NodeRegistrar<TestRuntime>>::set(Some(registrar.clone()));
    }

    fn register_node(
        registrar: &AccountId,
        node_id: &AccountId,
        owner: &AccountId,
        signing_key: UintAuthorityId,
    ) {
        assert_ok!(NodeManager::register_node(
            RuntimeOrigin::signed(registrar.clone()),
            node_id.clone(),
            owner.clone(),
            signing_key,
        ));
    }

    #[test]
    fn add_stake_fails_when_free_balance_is_insufficient() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let registrar = TestAccount::new([1u8; 32]).account_id();
                setup_registrar(&registrar);

                let owner = get_owner(1);
                let node = get_node(3);

                // Give the owner a small balance.
                Balances::make_free_balance_be(&owner, 100 * AVT);
                register_node(&registrar, &node, &owner, UintAuthorityId(10));

                assert_noop!(
                    NodeManager::add_stake(RuntimeOrigin::signed(owner), node, 1_000 * AVT),
                    Error::<TestRuntime>::InsufficientFreeBalance
                );
            });
    }

    #[test]
    fn genesis_bonus_applies_during_auto_stake_window() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let owner = get_owner(1);
            let signing_key = get_signing_key(1);

            // owner has 1 node (needed for stake multiplier denominator)
            OwnedNodesCount::<TestRuntime>::insert(&owner, 1u32);

            // Within auto-stake window => genesis bonus applies
            let now_sec: u64 = 10;
            Timestamp::set_timestamp(now_sec * 1000);
            let expiry = now_sec + 10_000;

            // Serial in 2001..=5000 => 1.5x
            let stake_info = StakeInfo::new(0, 0, now_sec + 10_000);
            let node_info = NodeInfo::new(owner.clone(), signing_key, 3_000u32, expiry, stake_info);

            let w = NodeManager::effective_heartbeat_weight(&node_info, now_sec);

            // base 1_000_000 * 150%
            assert_eq!(w, 150_000_000u128);
        });
    }

    #[test]
    fn genesis_bonus_expires_after_auto_stake_window() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let owner = get_owner(1);
            let signing_key = get_signing_key(1);
            OwnedNodesCount::<TestRuntime>::insert(&owner, 1u32);

            let now_sec: u64 = 1_000;
            Timestamp::set_timestamp(now_sec * 1000);

            // expiry in the past => no bonus
            let stake_info = StakeInfo::new(0, 0, now_sec + 10_000);
            let node_info = NodeInfo::new(owner, signing_key, 3_000u32, now_sec - 1, stake_info);

            let w = NodeManager::effective_heartbeat_weight(&node_info, now_sec);
            assert_eq!(w, 100_000_000u128);
        });
    }

    #[test]
    fn stake_bonus_scales_weight_by_1_plus_stake_over_step_per_owned_node() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let owner = get_owner(1);
            let node = get_node(1);
            let stake_amount: u128 = 4_000_000_000_000_000_000_000; // 4k AVT with 18 decimals
                                                                    // Ensure add_stake is allowed
            NodeRegistry::<TestRuntime>::insert(
                &node,
                NodeInfo {
                    owner: owner.clone(),
                    signing_key: get_signing_key(1),
                    serial_number: 10_500u32,
                    auto_stake_expiry: 0,
                    stake: StakeInfo::new(0, 0, 0),
                },
            );

            OwnedNodesCount::<TestRuntime>::insert(&owner, 1u32);
            OwnedNodes::<TestRuntime>::insert(&owner, &node, ());

            Balances::make_free_balance_be(&owner, stake_amount * 2);

            // Stake 4_000 AVT with step=2_000 => (1 + 2) = 3x
            assert_ok!(NodeManager::add_stake(
                RuntimeOrigin::signed(owner.clone()),
                node,
                stake_amount
            ));

            let now_sec: u64 = 10;
            Timestamp::set_timestamp(now_sec * 1000);

            // No genesis bonus (serial outside range OR expiry in past)
            let stake_info = StakeInfo::new(stake_amount, 0, now_sec + 10_000);
            let node_info = NodeInfo::new(
                owner.clone(),
                get_signing_key(1),
                10_500u32,
                now_sec - 1,
                stake_info,
            );
            let w = NodeManager::effective_heartbeat_weight(&node_info, now_sec);

            assert_eq!(w, 300_000_000u128);

            // Reserve should match staked amount
            let reserved = Balances::reserved_balance(&owner);
            assert_eq!(reserved, stake_amount);
        });
    }

    #[test]
    fn add_stake_increases_existing_lock() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let owner = get_owner(1);
            let node = get_node(1);

            NodeRegistry::<TestRuntime>::insert(
                &node,
                NodeInfo {
                    owner: owner.clone(),
                    signing_key: get_signing_key(1),
                    serial_number: 10_500u32,
                    auto_stake_expiry: 0,
                    stake: StakeInfo::new(0, 0, 0),
                },
            );

            OwnedNodesCount::<TestRuntime>::insert(&owner, 1u32);
            OwnedNodes::<TestRuntime>::insert(&owner, &node, ());

            Balances::make_free_balance_be(&owner, 20_000u128);

            assert_ok!(NodeManager::add_stake(
                RuntimeOrigin::signed(owner.clone()),
                node,
                2_000u128
            ));
            assert_ok!(NodeManager::add_stake(
                RuntimeOrigin::signed(owner.clone()),
                node,
                1_000u128
            ));

            let info = NodeRegistry::<TestRuntime>::get(&node).unwrap();
            assert_eq!(info.stake.amount, 3_000u128);

            let reserved = Balances::reserved_balance(&owner);
            assert_eq!(reserved, 3_000u128);
        });
    }

    #[test]
    fn unstake_is_blocked_until_auto_stake_expiry_and_then_rate_limited() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let owner = get_owner(1);
            let node = get_node(1);
            let start_sec: u64 = 100;

            NodeRegistry::<TestRuntime>::insert(
                &node,
                NodeInfo {
                    owner: owner.clone(),
                    signing_key: get_signing_key(1),
                    serial_number: 10_500u32,
                    auto_stake_expiry: (start_sec + 1) * 1000,
                    stake: StakeInfo::new(0, 0, 0),
                },
            );
            OwnedNodesCount::<TestRuntime>::insert(&owner, 1u32);
            OwnedNodes::<TestRuntime>::insert(&owner, &node, ());
            Balances::make_free_balance_be(&owner, 100_000u128);

            // Set auto-stake duration to 1 week for this test.
            assert_ok!(NodeManager::set_admin_config(
                RuntimeOrigin::root(),
                AdminConfig::AutoStakeDuration(7 * 24 * 60 * 60),
            ));

            Timestamp::set_timestamp(start_sec * 1000);

            assert_ok!(NodeManager::add_stake(
                RuntimeOrigin::signed(owner.clone()),
                node,
                10_000u128
            ));

            // Before expiry => blocked
            assert_noop!(
                NodeManager::remove_stake(
                    RuntimeOrigin::signed(owner.clone()),
                    node,
                    Some(1_000u128)
                ),
                Error::<TestRuntime>::AutoStakeStillActive
            );

            // Move time to: expiry + 1 unstake period + 1s so 10% is available.
            let after_expiry_sec = start_sec
                + 7 * 24 * 60 * 60  // auto-stake window
                + 7 * 24 * 60 * 60  // 1 unstake period
                + 1;
            Timestamp::set_timestamp(after_expiry_sec * 1000);

            // First unstake: max 10% = 1_000
            assert_ok!(NodeManager::remove_stake(
                RuntimeOrigin::signed(owner.clone()),
                node,
                Some(1_000u128)
            ));

            // Second unstake in same “week” should be rate-limited
            assert_noop!(
                NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), node, Some(1u128)),
                Error::<TestRuntime>::UnstakeRateLimited
            );
        });
    }

    #[test]
    fn remove_stake_fails_when_amount_is_zero() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let registrar = TestAccount::new([1u8; 32]).account_id();
                setup_registrar(&registrar);

                let owner = get_owner(1);
                let node = get_node(1);

                Balances::make_free_balance_be(&owner, 100_000 * AVT);
                register_node(&registrar, &node, &owner, UintAuthorityId(10));

                // Stake something first
                assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), 10_000u128));

                // Move time past auto-stake expiry so unstake checks reach ZeroAmount branch.
                let start_sec = 1_000u64;
                Timestamp::set_timestamp(start_sec * 1000);

                let after_expiry_sec = start_sec + 7 * 24 * 60 * 60 + 1;
                Timestamp::set_timestamp(after_expiry_sec * 1000);

                assert_noop!(
                    NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), node, Some(0u128)),
                    Error::<TestRuntime>::ZeroAmount
                );
            });
    }

    #[test]
    fn remove_stake_none_fails_when_no_allowance_available() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let registrar = TestAccount::new([1u8; 32]).account_id();
                setup_registrar(&registrar);

                let owner = get_owner(1);
                let node = get_node(2);

                Balances::make_free_balance_be(&owner, 100_000 * AVT);
                register_node(&registrar, &node, &owner, UintAuthorityId(11));

                // Stake sets next_unstake_time_sec = now + auto_stake_duration
                let start_sec = 10_000u64;
                Timestamp::set_timestamp(start_sec * 1000);

                assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), 10_000u128));

                // Move time to exactly auto-stake expiry (can_unstake passes),
                // but NOT enough for 1 unstake period to elapse from next_unstake_time_sec.
                let expiry_sec = start_sec + 7 * 24 * 60 * 60;
                Timestamp::set_timestamp(expiry_sec * 1000);

                // available_to_unstake() should return 0, so remove_stake(None) should error.
                assert_noop!(
                    NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), node, None),
                    Error::<TestRuntime>::NoAvailableStakeToUnstake
                );
            });
    }

    #[test]
    fn unstake_back_to_back_partial_withdrawals_work_until_allowance_exhausted() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let registrar = TestAccount::new([1u8; 32]).account_id();
                setup_registrar(&registrar);

                let owner = get_owner(1);
                let node = get_node(3);

                Balances::make_free_balance_be(&owner, 100_000 * AVT);
                register_node(&registrar, &node, &owner, UintAuthorityId(12));

                let start_sec = 1_000u64;
                Timestamp::set_timestamp(start_sec * 1000);

                // Stake 10_000 => max unstake per period = 10% = 1_000
                assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), 10_000u128));

                // Move to: expiry + 1 unstake period + 1 second => 1 period unlocked
                let after_expiry_plus_one_period = start_sec
                    + 7 * 24 * 60 * 60  // auto-stake duration
                    + 7 * 24 * 60 * 60  // 1 unstake period
                    + 1;
                Timestamp::set_timestamp(after_expiry_plus_one_period * 1000);

                // Withdraw less than the max unlocked
                assert_ok!(NodeManager::remove_stake(
                    RuntimeOrigin::signed(owner.clone()),
                    node.clone(),
                    Some(400u128)
                ));

                // Withdraw the remainder of the unlocked allowance (1000 - 400 = 600)
                assert_ok!(NodeManager::remove_stake(
                    RuntimeOrigin::signed(owner.clone()),
                    node.clone(),
                    Some(600u128)
                ));

                // Another withdrawal in the same period should fail (no allowance left)
                assert_noop!(
                    NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), node, Some(1u128)),
                    Error::<TestRuntime>::UnstakeRateLimited
                );
            });
    }

    #[test]
    fn unstake_unlock_boundary_just_before_period_is_zero_and_at_exact_period_is_one() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let registrar = TestAccount::new([1u8; 32]).account_id();
                setup_registrar(&registrar);

                let owner = get_owner(1);
                let node = get_node(4);

                Balances::make_free_balance_be(&owner, 100_000 * AVT);
                register_node(&registrar, &node, &owner, UintAuthorityId(13));

                let start_sec = 5_000u64;
                Timestamp::set_timestamp(start_sec * 1000);

                assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), 10_000u128));

                // next_unstake_time_sec is set to (start + auto_stake_duration)
                // At expiry time: can_unstake passes but elapsed=0, so available should be 0.
                let expiry_sec = start_sec + 7 * 24 * 60 * 60;
                Timestamp::set_timestamp(expiry_sec * 1000);

                assert_noop!(
                    NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), None),
                    Error::<TestRuntime>::NoAvailableStakeToUnstake
                );

                // Just before 1 full unstake period completes
                let just_before = expiry_sec + (7 * 24 * 60 * 60) - 1;
                Timestamp::set_timestamp(just_before * 1000);

                assert_noop!(
                    NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), Some(1u128)),
                    Error::<TestRuntime>::UnstakeRateLimited
                );

                // Exactly at 1 period boundary => 10% unlocked
                let exact_boundary = expiry_sec + (7 * 24 * 60 * 60);
                Timestamp::set_timestamp(exact_boundary * 1000);

                assert_ok!(NodeManager::remove_stake(
                    RuntimeOrigin::signed(owner.clone()),
                    node,
                    Some(1_000u128)
                ));
            });
    }

    #[test]
    fn unstake_accumulates_over_multiple_periods_and_advances_period_pointer() {
        ExtBuilder::build_default()
            .with_genesis_config()
            .as_externality()
            .execute_with(|| {
                let registrar = TestAccount::new([1u8; 32]).account_id();
                setup_registrar(&registrar);

                let owner = get_owner(1);
                let node = get_node(5);

                Balances::make_free_balance_be(&owner, 100_000 * AVT);
                register_node(&registrar, &node, &owner, UintAuthorityId(14));

                let start_sec = 1_000u64;
                Timestamp::set_timestamp(start_sec * 1000);

                assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), 10_000u128));

                // Move to: expiry + 2 periods + 1 second => 20% unlocked = 2,000
                let t = start_sec
                    + 7 * 24 * 60 * 60  // auto-stake duration
                    + 2 * 7 * 24 * 60 * 60  // 2 unstake periods
                    + 1;
                Timestamp::set_timestamp(t * 1000);

                // Withdraw part of the allowance
                assert_ok!(NodeManager::remove_stake(
                    RuntimeOrigin::signed(owner.clone()),
                    node.clone(),
                    Some(500u128)
                ));

                // Immediately withdraw remaining allowance in same timestamp
                // Remaining should be 1500.
                assert_ok!(NodeManager::remove_stake(
                    RuntimeOrigin::signed(owner.clone()),
                    node.clone(),
                    Some(1500u128)
                ));

                // No more allowance left until another period passes
                assert_noop!(
                    NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), node.clone(), Some(1u128)),
                    Error::<TestRuntime>::UnstakeRateLimited
                );

                // Advance exactly 1 more period; unlocked should be 10% of the *current* stake (now 8_000)
                // => 800 available.
                let t2 = t + 7 * 24 * 60 * 60;
                Timestamp::set_timestamp(t2 * 1000);

                assert_ok!(NodeManager::remove_stake(
                    RuntimeOrigin::signed(owner.clone()),
                    node,
                    Some(800u128)
                ));
            });
    }

}
