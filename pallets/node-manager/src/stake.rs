use crate::*;
use sp_runtime::{traits::UniqueSaturatedInto, FixedPointNumber, FixedU128};
use sp_std::ops::RangeInclusive;

// 50% bonus for serial number nodes starting from 2001 to 5000
const FIFTY_PERCENT_GENESIS_BONUS: RangeInclusive<u32> = 2001..=5000;
// 25% bonus for serial number nodes starting from 5001 to 10000
const TWENTY_FIVE_PERCENT_GENESIS_BONUS: RangeInclusive<u32> = 5001..=10000;

impl<T: Config> Pallet<T> {
    fn calculate_genesis_bonus(
        node_info: &NodeInfo<T::SignerId, T::AccountId>,
        timestamp_sec: u64,
    ) -> Perbill {
        if node_info.auto_stake_expiry < timestamp_sec {
            return Perbill::from_percent(100) // no bonus
        }

        // Node is currently auto-staking, apply bonus if eligible
        if FIFTY_PERCENT_GENESIS_BONUS.contains(&node_info.serial_number) {
            Perbill::from_percent(150) // 1.5x
        } else if TWENTY_FIVE_PERCENT_GENESIS_BONUS.contains(&node_info.serial_number) {
            Perbill::from_percent(125) // 1.25x
        } else {
            Perbill::from_percent(100) // no bonus
        }
    }

    // Node stake is:  owner_stake / owned_nodes
    // Bonus = 1 + (node_stake / stake_step) => 1 + (owner_stake / (owned_nodes * stake_step))
    // (a/b)/c == a/(bxc)
    fn calculate_stake_bonus_from_owner(
        owner: &T::AccountId,
        reward_period: RewardPeriodIndex,
    ) -> FixedU128 {
        let owner_stake = <OwnerStakeSnapshot<T>>::get(reward_period, owner).unwrap_or(0u32.into());
        let owned_nodes = <OwnedNodesCount<T>>::get(owner);

        let stake_step: BalanceOf<T> = T::VirtualNodeStake::get();
        if stake_step.is_zero() || owned_nodes == 0 {
            return FixedU128::one()
        }

        let owner_stake_u128: u128 = owner_stake.unique_saturated_into();
        let stake_step_u128: u128 = stake_step.unique_saturated_into();
        let owned_nodes_u128: u128 = owned_nodes as u128;

        let denom = stake_step_u128.saturating_mul(owned_nodes_u128);
        if denom == 0 {
            return FixedU128::one()
        }

        let ratio = FixedU128::saturating_from_rational(owner_stake_u128, denom);

        FixedU128::one().saturating_add(ratio)
    }

    pub fn compute_reward_weight(
        node_info: &NodeInfo<T::SignerId, T::AccountId>,
        reward_period: RewardPeriodIndex,
        reward_period_end_time: u64,
    ) -> RewardWeight {
        let genesis_bonus = Self::calculate_genesis_bonus(node_info, reward_period_end_time);
        let stake_bonus: FixedU128 =
            Self::calculate_stake_bonus_from_owner(&node_info.owner, reward_period);
        RewardWeight { genesis_bonus, stake_multiplier: stake_bonus }
    }

    pub fn effective_heartbeat_weight(
        node_info: &NodeInfo<T::SignerId, T::AccountId>,
        reward_period: RewardPeriodIndex,
        reward_period_end_time: u64,
    ) -> u128 {
        let weight_factor =
            Self::compute_reward_weight(node_info, reward_period, reward_period_end_time);
        weight_factor.to_heartbeat_weight()
    }

    pub fn snapshot_owner_stake_if_required(
        node_info: &NodeInfo<T::SignerId, T::AccountId>,
        reward_period: RewardPeriodIndex,
    ) -> DispatchResult {
        let owner = &node_info.owner;
        if OwnerStakeSnapshot::<T>::contains_key(reward_period, owner) {
            return Ok(())
        }

        let maybe_stake = <OwnerStake<T>>::get(&node_info.owner);
        let stake = match maybe_stake {
            Some(s) => s,
            None => return Ok(()),
        };

        <OwnerStakeSnapshot<T>>::insert(reward_period, &node_info.owner, stake);
        Ok(())
    }
}
