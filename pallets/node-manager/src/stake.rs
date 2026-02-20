use crate::*;
use sp_runtime::{traits::UniqueSaturatedInto, FixedPointNumber, FixedU128};
use sp_std::ops::RangeInclusive;

// 50% bonus for serial number nodes starting from 2001 to 5000
const FIFTY_PERCENT_GENESIS_BONUS: RangeInclusive<u32> = 2001..=5000;
// 25% bonus for serial number nodes starting from 5001 to 10000
const TWENTY_FIVE_PERCENT_GENESIS_BONUS: RangeInclusive<u32> = 5001..=10000;

impl<T: Config> Pallet<T> {
    fn calculate_genesis_bonus(
        node_info: &NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
        timestamp_sec: Duration,
    ) -> FixedU128 {
        if timestamp_sec >= node_info.auto_stake_expiry {
            return FixedU128::one() // no bonus
        }

        // Node is currently auto-staking, apply bonus if eligible
        if FIFTY_PERCENT_GENESIS_BONUS.contains(&node_info.serial_number) {
            FixedU128::saturating_from_rational(3u128, 2u128) // 1.5x
        } else if TWENTY_FIVE_PERCENT_GENESIS_BONUS.contains(&node_info.serial_number) {
            FixedU128::saturating_from_rational(5u128, 4u128) // 1.25x
        } else {
            FixedU128::one() // no bonus
        }
    }

    // Use linear bonus calculation.
    fn calculate_stake_bonus(
        node_info: &NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
    ) -> FixedU128 {
        let stake_u128: u128 = node_info.stake.amount.unique_saturated_into();
        let step_u128: u128 = T::VirtualNodeStake::get().unique_saturated_into();

        if stake_u128.is_zero() || step_u128.is_zero() {
            return FixedU128::one()
        }

        let ratio = FixedU128::saturating_from_rational(stake_u128, step_u128);
        FixedU128::one().saturating_add(ratio)
    }

    // This function calculated bonus base on VirtualNodeStake interval.
    // Ex: 2000 AVT = 1 virutal node, 3999 AVT = 1 virtual node, 4000 AVT = 2 virtual nodes...
    fn calculate_stake_bonus_step(
        node_info: &NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
    ) -> FixedU128 {
        let stake_amount = node_info.stake.amount;
        let step = T::VirtualNodeStake::get();

        if stake_amount.is_zero() || step.is_zero() {
            return FixedU128::one()
        }

        // virtual = floor(node_stake / step)
        let virtual_nodes: u128 = (stake_amount / step).unique_saturated_into();

        // multiplier = 1 + virtual
        let inner = virtual_nodes.saturating_add(1u128);
        FixedU128::from_inner(inner.saturating_mul(FixedU128::accuracy()))
    }

    pub fn compute_reward_weight(
        node_info: &NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
        reward_period_end_time: Duration,
    ) -> RewardWeight {
        let genesis_bonus = Self::calculate_genesis_bonus(node_info, reward_period_end_time);
        let stake_bonus: FixedU128 = Self::calculate_stake_bonus(node_info);
        RewardWeight { genesis_bonus, stake_multiplier: stake_bonus }
    }

    pub fn effective_heartbeat_weight(
        node_info: &NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
        reward_period_end_time: Duration,
    ) -> u128 {
        let weight_factor = Self::compute_reward_weight(node_info, reward_period_end_time);
        weight_factor.to_heartbeat_weight()
    }

    pub fn do_add_stake(
        owner: &T::AccountId,
        node_id: &NodeId<T>,
        amount: BalanceOf<T>,
    ) -> Result<BalanceOf<T>, DispatchError> {
        ensure!(!amount.is_zero(), Error::<T>::ZeroAmount);
        let free = T::Currency::free_balance(&owner);
        ensure!(free >= amount, Error::<T>::InsufficientFreeBalance);

        // before we read the node, try to snapshot the stake if auto-stake duration has passed.
        Self::set_max_unstake_per_period_if_required(
            &node_id,
            Self::time_now_sec(),
            <MaxUnstakePercentage<T>>::get(),
        );

        let node_info = NodeRegistry::<T>::get(node_id).ok_or(Error::<T>::NodeNotFound)?;
        let mut stake: StakeInfo<BalanceOf<T>> = node_info.stake;
        let new_total = stake.amount.checked_add(&amount).ok_or(Error::<T>::BalanceOverflow)?;

        stake.amount = new_total;

        Self::update_reserves(owner, amount, StakeOperation::Add)?;
        NodeRegistry::<T>::insert(node_id, NodeInfo { stake, ..node_info });

        Ok(new_total)
    }

    pub fn update_reserves(
        owner: &T::AccountId,
        amount: BalanceOf<T>,
        op: StakeOperation,
    ) -> DispatchResult {
        match op {
            StakeOperation::Add => T::Currency::reserve(owner, amount)
                .map_err(|_| Error::<T>::InsufficientFreeBalance.into()),

            StakeOperation::Remove => {
                ensure!(
                    T::Currency::reserved_balance(owner) >= amount,
                    Error::<T>::InsufficientStakedBalance
                );
                let leftover = T::Currency::unreserve(owner, amount);
                ensure!(leftover.is_zero(), Error::<T>::InsufficientStakedBalance);
                Ok(())
            },
        }
    }

    pub fn set_max_unstake_per_period_if_required(
        node_id: &NodeId<T>,
        now_sec: Duration,
        max_unstake_percentage: Perbill,
    ) {
        NodeRegistry::<T>::mutate(node_id, |maybe_node_info| {
            if let Some(node_info) = maybe_node_info {
                if now_sec >= node_info.auto_stake_expiry &&
                    node_info.stake.max_unstake_per_period.is_none() &&
                    !node_info.stake.amount.is_zero()
                {
                    node_info.stake.max_unstake_per_period =
                        Some(max_unstake_percentage * node_info.stake.amount);
                }
            }
        });
    }
}
