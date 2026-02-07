use crate::*;
use sp_runtime::{traits::UniqueSaturatedInto, FixedPointNumber, FixedU128};
use sp_std::ops::RangeInclusive;

impl<T: Config> Pallet<T> {

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

    pub fn available_to_unstake(now_sec: u64, owner_stake: BalanceOf<T>, state: &UnstakeState<BalanceOf<T>>) -> (BalanceOf<T>, u64) {
        if owner_stake.is_zero() {
            return (Zero::zero(), 0);
        }

        if state.last_updated_sec == 0 {
            return (Zero::zero(), 0);
        }

        let elapsed = now_sec.saturating_sub(state.last_updated_sec);
        let periods = elapsed / <UnstakePeriodSec<T>>::get();
        if periods == 0 {
            // No new accrual
            return (state.max_unstake_allowance.min(owner_stake), 0);
        }

        // Accrue for whole periods only.
        let per_period: BalanceOf<T> = <MaxUnstakePercentage<T>>::get() * owner_stake;
        let newlyUnlockedStake = per_period.saturating_mul((periods as u32).into());

        let available = state.max_unstake_allowance.saturating_add(newlyUnlockedStake).min(owner_stake);

        // Return available to unstake and how many periods we advanced (so caller can persist).
        (available, periods)
    }
}
