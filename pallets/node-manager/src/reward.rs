use crate::*;
use sp_runtime::{ArithmeticError, SaturatedConversion};
impl<T: Config> Pallet<T> {
    // Nodes should not be able to submit over the min uptime required.
    // but we still check it here to be sure.
    pub fn calculate_node_uptime(
        node_id: &NodeId<T>,
        actual_uptime: u64,
        uptime_threshold: u32,
    ) -> u64 {
        let uptime_threshold_u64 = uptime_threshold as u64;
        if actual_uptime >= uptime_threshold_u64 {
            if actual_uptime > uptime_threshold_u64 {
                log::warn!("✋ Node ({:?}) has been up for more than the expected uptime. Actual: {:?}, Expected: {:?}", node_id, actual_uptime, uptime_threshold);
            }
            uptime_threshold_u64
        } else {
            actual_uptime
        }
    }

    pub fn calculate_reward(
        uptime: u64,
        total_uptime: &u64,
        total_reward: &BalanceOf<T>,
    ) -> Result<BalanceOf<T>, DispatchError> {
        if total_uptime.is_zero() {
            return Err(DispatchError::Arithmetic(ArithmeticError::DivisionByZero))
        }

        // Convert everything to u128 to satisfy Perquintill requirements.
        // We never overflow here because the values are bounded by u64 max.
        let ratio = Perquintill::from_rational(uptime as u128, *total_uptime as u128);
        let total_rewards_u128: u128 = (*total_reward).saturated_into();

        Ok(ratio.mul_floor(total_rewards_u128).saturated_into())
    }

    pub fn pay_reward(
        period: &RewardPeriodIndex,
        node: NodeId<T>,
        amount: BalanceOf<T>,
    ) -> DispatchResult {
        let node_owner = match <NodeRegistry<T>>::get(&node) {
            Some(info) => info.owner,
            None => {
                log::warn!("⚠️ Error paying reward. Node not found in registry. Reward period: {:?}, Node {:?}, Amount: {:?}",
                  period, node, amount
                );

                Self::deposit_event(Event::ErrorPayingReward {
                    reward_period: *period,
                    node: node.clone(),
                    amount,
                    error: Error::<T>::NodeNotRegistered.into(),
                });
                // We skip paying rewards for this node and continue without erroring
                return Ok(())
            },
        };

        let reward_pot_account_id = Self::compute_reward_account_id();

        T::Currency::transfer(
            &reward_pot_account_id,
            &node_owner,
            amount,
            ExistenceRequirement::KeepAlive,
        )?;

        Self::deposit_event(Event::RewardPaid {
            reward_period: *period,
            owner: node_owner,
            node,
            amount,
        });

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
    pub fn time_now() -> u64 {
        T::TimeProvider::now().as_secs()
    }
}
