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

        if info.last_period_updated < reward_period {
            // The last stake change happened in the previous period
            info.amount
        } else {
            Self::get_effective_stake_for_period(owner, reward_period)
        }
    }

    pub fn do_add_stake(
        owner: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> Result<BalanceOf<T>, DispatchError> {
        ensure!(!amount.is_zero(), Error::<T>::ZeroAmount);
        let mut stake = OwnerStake::<T>::get(&owner).unwrap_or_default();
        let current_stake = stake.amount;
        let new_total = current_stake.saturating_add(amount);

        let free = T::Currency::free_balance(&owner);
        ensure!(free >= new_total, Error::<T>::InsufficientFreeBalance);

        let current_reward_period = RewardPeriod::<T>::get().current;

        if current_stake.is_zero() {
            let expiry = Self::time_now_sec().saturating_add(AutoStakeDurationSec::<T>::get());
            stake.auto_stake_expiry = expiry;

            if stake.state.next_unstake_time_sec == 0 {
                stake.state = UnstakeState::new(Zero::zero(), expiry);
            }

        }
        stake.amount = new_total;
        stake.last_period_updated = current_reward_period;

        Self::update_stake(owner, stake, current_reward_period)?;

        Ok(new_total)
    }

    // This function will return the last stake before the given reward period
    fn get_effective_stake_for_period(owner: &T::AccountId, payout_period: RewardPeriodIndex) -> BalanceOf<T> {
        let periods = StakeSnapshotPeriods::<T>::get(owner);
        if periods.is_empty() { return Zero::zero(); }

        // Find the last period <= payout_period
        let idx = match periods.binary_search_by(|p| p.cmp(&payout_period)) {
            Ok(i) => i,
            Err(0) => return Zero::zero(), // all snapshots are after payout_period
            Err(i) => i - 1, // insertion point - 1
        };

        StakeSnapshot::<T>::get(periods[idx], owner).unwrap_or_default()
    }

    pub fn update_stake(owner: &T::AccountId, stake: OwnerStakeInfo<BalanceOf<T>>, reward_period: RewardPeriodIndex) -> DispatchResult {
        if stake.amount.is_zero() {
            T::Currency::remove_lock(STAKE_LOCK_ID, &owner);
        } else {
            T::Currency::set_lock(STAKE_LOCK_ID, &owner, stake.amount, WithdrawReasons::all());
        }

        Self::record_stake_snapshot_period(&owner, reward_period)?;
        StakeSnapshot::<T>::insert(reward_period, &owner, stake.amount);
        OwnerStake::<T>::insert(&owner, stake);

        Ok(())
    }

    fn record_stake_snapshot_period(
        owner: &T::AccountId,
        period: RewardPeriodIndex,
    ) -> DispatchResult {
        StakeSnapshotPeriods::<T>::try_mutate(owner, |periods| {
            if let Some(&last) = periods.last() {
                if last == period {
                    // already recorded
                    return Ok(())
                }

                // This should never happen in normal flow
                if period < last {
                    log::warn!("⚠️ snapshot period went backwards. Current period: {:?}, last period: {:?}, owner: {:?}", period, last, owner);
                    return Ok(());
                }
            }

            // Enforce capacity strictly
            periods
                .try_push(period)
                .map_err(|_| Error::<T>::StakeSnapshotFull)?;

            Ok(())
        })
    }
}
