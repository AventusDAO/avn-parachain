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
        let owner_stake = Self::get_owner_stake_for_period(owner, reward_period);
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

    fn get_owner_stake_for_period(
        owner: &T::AccountId,
        reward_period: RewardPeriodIndex,
    ) -> BalanceOf<T> {
        let info = match OwnerStake::<T>::get(owner) {
            None => return Zero::zero(),
            Some(info) => info,
        };

        // This can happen because payout period is at least current_period - 1
        if info.last_period_updated > reward_period {
            // stake exists but only becomes effective in a later period
            return Zero::zero()
        }

        // Get the latest snapshot as of this reward period
        OwnerStakeSnapshot::<T>::get(info.last_period_updated, owner).unwrap_or_default()
    }

    pub fn available_to_unstake(
        now_sec: u64,
        owner_stake: BalanceOf<T>,
        state: &UnstakeState<BalanceOf<T>>,
    ) -> (BalanceOf<T>, u64) {
        if owner_stake.is_zero() {
            return (Zero::zero(), 0)
        }

        if state.last_updated_sec == 0 {
            return (Zero::zero(), 0)
        }

        let elapsed = now_sec.saturating_sub(state.last_updated_sec);
        let periods = elapsed / <UnstakePeriodSec<T>>::get();
        if periods == 0 {
            // No new stake unlocked yet
            return (state.max_unstake_allowance.min(owner_stake), 0)
        }

        // Increase for whole periods only.
        let per_period: BalanceOf<T> = <MaxUnstakePercentage<T>>::get() * owner_stake;
        let newly_unlocked_stake = per_period.saturating_mul((periods as u32).into());

        let available = state
            .max_unstake_allowance
            .saturating_add(newly_unlocked_stake)
            .min(owner_stake);

        // Return available to unstake and how many periods we advanced (so caller can persist).
        (available, periods)
    }

    pub fn do_add_stake(
        owner: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> Result<BalanceOf<T>, DispatchError> {
        ensure!(!amount.is_zero(), Error::<T>::ZeroAmount);
        let mut current_stake_info = OwnerStake::<T>::get(&owner).unwrap_or_default();
        let current_stake = current_stake_info.amount;
        let new_total = current_stake.saturating_add(amount);

        let free = T::Currency::free_balance(&owner);
        ensure!(free >= new_total, Error::<T>::InsufficientFreeBalance);

        T::Currency::set_lock(STAKE_LOCK_ID, &owner, new_total, WithdrawReasons::all());

        let current_reward_period = RewardPeriod::<T>::get().current;

        if current_stake.is_zero() {
            let expiry = Self::time_now_sec().saturating_add(AutoStakeDurationSec::<T>::get());
            current_stake_info.auto_stake_expiry = expiry;

            OwnerUnstakeState::<T>::mutate(&owner, |s| {
                if s.last_updated_sec == 0 {
                    let start_sec = expiry;
                    s.last_updated_sec = start_sec;
                    s.max_unstake_allowance = Zero::zero();
                }
            });
        }
        current_stake_info.amount = new_total;
        current_stake_info.last_period_updated = current_reward_period;
        OwnerStake::<T>::insert(&owner, current_stake_info);
        OwnerStakeSnapshot::<T>::insert(current_reward_period, &owner, new_total);

        Ok(new_total)
    }
}
