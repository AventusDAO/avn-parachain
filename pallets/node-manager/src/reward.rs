use crate::*;
use sp_runtime::{ArithmeticError, SaturatedConversion};

impl<T: Config> Pallet<T> {
    // Nodes should not be able to submit over the min uptime required.
    // but we still check it here to be sure.
    pub fn calculate_node_weight(
        node_id: &NodeId<T>,
        uptime_info: UptimeInfo<BlockNumberFor<T>>,
        node_info: &NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
        uptime_threshold: u32,
        reward_period_end_time: u64,
    ) -> u128 {
        let actual_uptime = uptime_info.count;
        let weight = uptime_info.weight;

        if actual_uptime > uptime_threshold.into() {
            log::warn!("⚠️ Node ({:?}) has been up for more than the expected uptime. Actual: {:?}, Expected: {:?}",
                node_id, actual_uptime, uptime_threshold);

            // re-calculate weight using reward_period_end_time. If autostaking expired mid period,
            // the node's reward will reduce because this recalculation will remove the
            // genesis bonus for all heartbeats. This is ok because we are in this
            // situation because the node managed to send more heartbeats than it should.
            let single_node_weight =
                Self::effective_heartbeat_weight(node_info, reward_period_end_time);
            single_node_weight.saturating_mul(u128::from(uptime_threshold))
        } else {
            weight
        }
    }

    pub fn calculate_reward(
        weight: u128,
        total_weight: &u128,
        total_reward: &BalanceOf<T>,
    ) -> Result<BalanceOf<T>, DispatchError> {
        if total_weight.is_zero() {
            return Err(DispatchError::Arithmetic(ArithmeticError::DivisionByZero))
        }

        // Convert everything to u128 to satisfy Perquintill requirements.
        let ratio = Perquintill::from_rational(weight, *total_weight);
        let total_rewards_u128: u128 = (*total_reward).saturated_into();

        Ok(ratio.mul_floor(total_rewards_u128).saturated_into())
    }

    pub fn pay_reward(
        period: &RewardPeriodIndex,
        node_id: NodeId<T>,
        node_info: &NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
        amount: BalanceOf<T>,
    ) -> DispatchResult {
        let node_owner = node_info.owner.clone();
        let reward_pot_account_id = Self::compute_reward_account_id();

        // First pay the owner
        T::Currency::transfer(
            &reward_pot_account_id,
            &node_owner,
            amount,
            ExistenceRequirement::KeepAlive,
        )?;

        if node_info.auto_stake_expiry < Self::time_now_sec() {
            // We are outside the auto stake period, finish paying.
            Self::deposit_event(Event::RewardPaid {
                reward_period: *period,
                owner: node_owner,
                node: node_id,
                amount,
            });
        } else {
            // We are within the auto stake period, auto stake the rewards.
            Self::do_add_stake(&node_owner, &node_id, amount)
                .map_err(|_| Error::<T>::AutoStakeFailed)?;

            Self::deposit_event(Event::RewardAutoStaked {
                reward_period: *period,
                owner: node_owner,
                node: node_id,
                amount,
            });
        }

        Ok(())
    }

    pub fn remove_paid_nodes(
        period_index: RewardPeriodIndex,
        paid_nodes_to_remove: &Vec<T::AccountId>,
    ) {
        // Remove the paid nodes. We do this separatly to avoid changing the map while iterating
        // it
        for node in paid_nodes_to_remove {
            NodeUptime::<T>::remove(period_index, node);
        }
    }

    pub fn complete_reward_payout(period_index: RewardPeriodIndex) {
        // We finished paying all nodes for this period
        OldestUnpaidRewardPeriodIndex::<T>::put(period_index.saturating_add(1));
        LastPaidPointer::<T>::kill();
        <TotalUptime<T>>::remove(period_index);
        <RewardPot<T>>::remove(period_index);

        Self::deposit_event(Event::RewardPayoutCompleted { reward_period_index: period_index });
    }

    pub fn update_last_paid_pointer(
        period_index: RewardPeriodIndex,
        last_node_paid: Option<T::AccountId>,
    ) {
        if let Some(node) = last_node_paid {
            LastPaidPointer::<T>::put(PaymentPointer { period_index, node });
        }
    }

    /// The account ID of the reward pot.
    pub fn compute_reward_account_id() -> T::AccountId {
        T::RewardPotId::get().into_account_truncating()
    }

    /// The total amount of funds stored in this pallet
    pub fn reward_pot_balance() -> BalanceOf<T> {
        // Must never be less than 0 but better be safe.
        <T as pallet::Config>::Currency::free_balance(&Self::compute_reward_account_id())
            .saturating_sub(<T as pallet::Config>::Currency::minimum_balance())
    }

    pub fn get_iterator_from_last_paid(
        oldest_period: RewardPeriodIndex,
        last_paid_pointer: PaymentPointer<T::AccountId>,
    ) -> Result<PrefixIterator<(T::AccountId, UptimeInfo<BlockNumberFor<T>>)>, DispatchError> {
        ensure!(last_paid_pointer.period_index == oldest_period, Error::<T>::InvalidPeriodPointer);
        // Make sure the last paid node has been remove, to be extra sure we won't double pay
        ensure!(
            !NodeUptime::<T>::contains_key(oldest_period, &last_paid_pointer.node),
            Error::<T>::InvalidNodePointer
        );

        // Start iteration just after `(oldest_period, last_paid_pointer.node)`.
        let final_key = last_paid_pointer.get_final_key::<T>();
        Ok(NodeUptime::<T>::iter_prefix_from(oldest_period, final_key))
    }

    /// Get the current time in seconds
    pub fn time_now_sec() -> u64 {
        T::TimeProvider::now().as_secs()
    }
}
