use super::*;
use crate::mock::*;
use frame_support::{assert_noop, assert_ok};

mod stake_and_reward_weight_tests {
    use super::*;
    use frame_support::traits::LockableCurrency;
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

    #[test]
    fn genesis_bonus_applies_during_auto_stake_window() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
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
            let node_info = NodeInfo::new(owner.clone(), signing_key, 3_000u32, expiry);

            let w = NodeManager::effective_heartbeat_weight(&node_info, 0u64, now_sec);

            // base 1_000_000 * 150%
            assert_eq!(w, 150_000_000u128);
        });
    }

    #[test]
    fn genesis_bonus_expires_after_auto_stake_window() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
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
            let node_info = NodeInfo::new(owner, signing_key, 3_000u32, now_sec - 1);

            let w = NodeManager::effective_heartbeat_weight(&node_info, 0u64, now_sec);
            assert_eq!(w, 100_000_000u128);
        });
    }

    #[test]
    fn stake_bonus_scales_weight_by_1_plus_stake_over_step_per_owned_node() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let owner = get_owner(1);
            let node = get_node(1);
            let stake_amount: u128 = 4_000_000_000_000_000_000_000; // 4k AVT with 18 decimals
            // Ensure add_stake is allowed
            OwnedNodesCount::<TestRuntime>::insert(&owner, 1u32);
            OwnedNodes::<TestRuntime>::insert(&owner, &node, ());

            Balances::make_free_balance_be(&owner, stake_amount * 2);

            // Stake 4_000 AVT with step=2_000 => (1 + 2) = 3x
            assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), stake_amount));

            let now_sec: u64 = 10;
            Timestamp::set_timestamp(now_sec * 1000);

            // No genesis bonus (serial outside range OR expiry in past)
            let node_info = NodeInfo::new(owner.clone(), get_signing_key(1), 10_500u32, now_sec - 1);
            let w = NodeManager::effective_heartbeat_weight(&node_info, 0u64, now_sec);

            assert_eq!(w, 300_000_000u128);

            // Lock should match staked amount
            let locks = Balances::locks(&owner);
            assert!(locks.iter().any(|l| l.id == STAKE_LOCK_ID && l.amount == stake_amount));
        });
    }

    #[test]
    fn add_stake_increases_existing_lock_and_snapshot() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let owner = get_owner(1);
            let node = get_node(1);

            OwnedNodesCount::<TestRuntime>::insert(&owner, 1u32);
            OwnedNodes::<TestRuntime>::insert(&owner, &node, ());

            Balances::make_free_balance_be(&owner, 20_000u128);

            assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), 2_000u128));
            assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), 1_000u128));

            let info = OwnerStake::<TestRuntime>::get(&owner).expect("stake exists");
            assert_eq!(info.amount, 3_000u128);

            let current_period = RewardPeriod::<TestRuntime>::get().current;
            let snap = StakeSnapshot::<TestRuntime>::get(current_period, &owner).unwrap();
            assert_eq!(snap, 3_000u128);

            let locks = Balances::locks(&owner);
            assert!(locks.iter().any(|l| l.id == STAKE_LOCK_ID && l.amount == 3_000u128));
        });
    }

    #[test]
    fn unstake_is_blocked_until_auto_stake_expiry_and_then_rate_limited() {
        let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();
        ext.execute_with(|| {
            let owner = get_owner(1);
            let node = get_node(1);
            OwnedNodesCount::<TestRuntime>::insert(&owner, 1u32);
            OwnedNodes::<TestRuntime>::insert(&owner, &node, ());
            Balances::make_free_balance_be(&owner, 100_000u128);

            // Set auto-stake duration to 1 week for this test.
            assert_ok!(NodeManager::set_admin_config(
                RuntimeOrigin::root(),
                AdminConfig::AutoStakeDuration(7 * 24 * 60 * 60),
            ));

            let start_sec: u64 = 100;
            Timestamp::set_timestamp(start_sec * 1000);

            assert_ok!(NodeManager::add_stake(RuntimeOrigin::signed(owner.clone()), 10_000u128));

            // Before expiry => blocked
            assert_noop!(
                NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), Some(1_000u128)),
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
                Some(1_000u128)
            ));

            // Second unstake in same “week” should be rate-limited
            assert_noop!(
                NodeManager::remove_stake(RuntimeOrigin::signed(owner.clone()), Some(1u128)),
                Error::<TestRuntime>::UnstakeRateLimited
            );
        });
    }
}
