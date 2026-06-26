// Copyright 2026 Aventus DAO Ltd

//! App-chain reward accounting and payout.
//!
//! `pallet-node-manager` notifies this pallet through [`AppChainInterface`] as it pays native
//! rewards. We snapshot each app chain's per-period reward pool, record every node's share, and
//! later pay each owner `share * pool` in the app chain's own token via the `PaymentHandler`.
//! Payouts are driven opportunistically by `on_idle` and deterministically by the permissionless
//! `process_outstanding_rewards` / `claim` extrinsics.

use crate::*;
use frame_support::{
    storage::with_storage_layer,
    traits::Get,
    weights::{Weight, WeightMeter},
};
use orml_traits::asset_registry::{AvnAssetLocation, Inspect as AssetRegistryInspect};
use sp_avn_common::{AppChainInterface, PaymentHandler, RewardPeriodIndex};
use sp_runtime::{traits::Zero, DispatchError, Perquintill, SaturatedConversion};

impl<T: Config> AppChainInterface for Pallet<T> {
    type AccountId = T::AccountId;

    /// Snapshot every app chain's current per-period reward (token + rate) for the new period.
    /// This is called from on_init so it must be light and non-failing.
    fn on_new_reward_period(period_index: &RewardPeriodIndex) -> Weight {
        let mut chains: u32 = 0;
        for (asset_id, (token, amount)) in NextRewardAmountPerPeriod::<T>::iter() {
            if amount.is_zero() {
                // This should not happen but just in case, ignore them.
                continue
            }
            PeriodChainReward::<T>::insert(*period_index, asset_id, (token, amount));
            chains = chains.saturating_add(1);
        }
        <T as Config>::WeightInfo::on_new_reward_period(chains)
    }

    /// Record a node's share for the period so it can be paid later.
    /// Returns the weight actually consumed so node-manager can fold it into its payout weight.
    fn on_reward_paid(
        period_index: &RewardPeriodIndex,
        node_owner: &Self::AccountId,
        node_id: &Self::AccountId,
        auto_stake_expiry: Moment,
        reward_percentage: Perquintill,
    ) -> Weight {
        if reward_percentage.is_zero() {
            return Weight::zero()
        }

        if PeriodChainReward::<T>::iter_prefix(*period_index).next().is_none() {
            // No app chain funded a reward pool for this period, so there is nothing to pay out.
            // One storage read for the emptiness probe.
            return T::DbWeight::get().reads(1)
        }

        UnpaidByPeriod::<T>::insert(
            *period_index,
            node_id,
            RewardRecord { owner: node_owner.clone(), share: reward_percentage, auto_stake_expiry },
        );
        UnpaidByNode::<T>::insert(node_id, *period_index, ());
        // Cost of recording a single node.
        <T as Config>::WeightInfo::on_reward_paid(1)
    }

    /// Weight to record rewards for `num_nodes` nodes settled in a single period — what
    /// node-manager folds into its batched `offchain_pay_nodes` pre-dispatch bound.
    fn reward_paid_weight(num_nodes: u32) -> Weight {
        <T as Config>::WeightInfo::on_reward_paid(num_nodes)
    }

    fn on_reward_period_completed(period_index: &RewardPeriodIndex) -> Weight {
        // node-manager has finished paying — and therefore recording (`on_reward_paid`) — every
        // node for this period. Mark it so the snapshot may be reclaimed, then reclaim immediately
        // if all app-chain rewards are already settled (e.g. nobody accrued, or all drained early).
        PeriodPayoutCompleted::<T>::insert(*period_index, ());
        Self::try_reclaim_period(*period_index);
        // Charge the worst case (a full snapshot reclaim); cheaper when no clear happened.
        <T as Config>::WeightInfo::on_reward_period_completed()
    }

    fn on_reward_period_completed_weight() -> Weight {
        <T as Config>::WeightInfo::on_reward_period_completed()
    }
}

impl<T: Config> Pallet<T> {
    /// Resolve the appchain native token from the asset-registry entry.
    pub(crate) fn resolve_payout_token(
        asset_id: &T::AppChainAssetId,
    ) -> Result<T::Token, DispatchError> {
        let metadata =
            T::AssetRegistry::metadata(asset_id).ok_or(Error::<T>::AppChainTokenNotResolvable)?;

        ensure!(metadata.additional.appchain_native, Error::<T>::AssetNotAppChainNative);

        match metadata.location {
            Some(AvnAssetLocation::Ethereum(address)) => Ok(address.into()),
            _ => Err(Error::<T>::AppChainTokenNotResolvable.into()),
        }
    }

    /// Reclaim a period's `PeriodChainReward` snapshot, but only once it is safe to do so: the
    /// period must be marked completed by node-manager (`PeriodPayoutCompleted`) AND all its
    /// `UnpaidByPeriod` records must be settled. This prevents clearing the snapshot mid-payout,
    /// while node-manager is still recording nodes in later batches.
    pub(crate) fn try_reclaim_period(period: RewardPeriodIndex) {
        if !PeriodPayoutCompleted::<T>::contains_key(period) {
            return
        }
        if UnpaidByPeriod::<T>::iter_prefix(period).next().is_some() {
            return
        }
        let _ =
            PeriodChainReward::<T>::clear_prefix(period, T::MaxRegisteredAppChains::get(), None);
        // Only finalise once the snapshot is actually empty. If its not empty it means
        // MaxRegisteredAppChains was lowered.
        if PeriodChainReward::<T>::iter_prefix(period).next().is_none() {
            PeriodPayoutCompleted::<T>::remove(period);
            Self::deposit_event(Event::AppChainRewardPayoutCompleted { reward_period: period });
        }
    }

    /// `share * total`, computed in u128 to satisfy `Perquintill` (mirrors node-manager).
    pub(crate) fn share_of(share: Perquintill, total: BalanceOf<T>) -> BalanceOf<T> {
        let total_u128: u128 = total.saturated_into();
        share.mul_floor(total_u128).saturated_into()
    }

    /// Pay all app-chain rewards owed to a single `(period, node)` and clear the record.
    ///
    /// Wrapped in a transactional layer so that if any chain's transfer fails (e.g. the reward pot
    /// is underfunded) the whole payout rolls back and the record is left intact for retry — no
    /// chain is paid twice. The pot balance check is implicit: `pay_recipient` fails on a short
    /// balance, which aborts the layer.
    pub fn try_pay_node_period(period: RewardPeriodIndex, node: &T::AccountId) -> DispatchResult {
        with_storage_layer(|| {
            let record =
                UnpaidByPeriod::<T>::get(period, node).ok_or(Error::<T>::NoUnpaidRewards)?;
            let pot = T::RewardPot::get();

            for (asset_id, (token, total)) in PeriodChainReward::<T>::iter_prefix(period) {
                // Runtime-defined eligibility: skip this chain's payout for the node if ineligible.
                if !T::AppChainRewardEligibility::is_eligible(
                    asset_id,
                    node,
                    period,
                    record.auto_stake_expiry,
                ) {
                    continue
                }
                let amount = Self::share_of(record.share, total);
                if amount.is_zero() {
                    continue
                }
                T::PaymentHandler::pay_recipient(&token, &amount, &pot, &record.owner)?;
                Self::deposit_event(Event::AppChainRewardPaid {
                    reward_period: period,
                    node: node.clone(),
                    owner: record.owner.clone(),
                    asset_id,
                    amount,
                });
            }

            UnpaidByPeriod::<T>::remove(period, node);
            UnpaidByNode::<T>::remove(node, period);
            Self::deposit_event(Event::AppChainRewardSettledForNode {
                reward_period: period,
                node: node.clone(),
            });

            // Reclaim the period snapshot once it is fully settled — but only after node-manager
            // has finished recording the period (gated by `PeriodPayoutCompleted`),
            // never mid-payout.
            Self::try_reclaim_period(period);

            Ok(())
        })
    }

    /// Pay every outstanding period for a single node, up to `max` periods. Returns the number of
    /// `(period, node)` payouts attempted (successful or failed-and-retained).
    pub fn claim_node(node: &T::AccountId, max: u32) -> u32 {
        // Collect first: `try_pay_node_period` mutates `UnpaidByNode` while we would be iterating
        // it.
        let periods: Vec<RewardPeriodIndex> = UnpaidByNode::<T>::iter_prefix(node)
            .map(|(p, _)| p)
            .take(max as usize)
            .collect();

        let mut processed = 0u32;
        for period in periods {
            if let Err(e) = Self::try_pay_node_period(period, node) {
                Self::deposit_event(Event::AppChainRewardPayoutFailed {
                    reward_period: period,
                    node: node.clone(),
                    error: e,
                });
            }
            processed = processed.saturating_add(1);
        }
        processed
    }

    /// Pay outstanding `(period, node)` rewards, bounded by `max` payouts and `meter`. Returns the
    /// number attempted. Failed payouts (e.g. underfunded pot) are left in place for a later retry.
    ///
    /// Resumes from [`SweepCursor`] so the pass walks the whole map across successive calls and a
    /// retained failure is skipped by position rather than re-collected from the front every call,
    /// which would otherwise let one underfunded chain starve every record behind it.
    pub fn sweep(meter: &mut WeightMeter, max: u32) -> u32 {
        // Keep this as MaxRegisteredAppChains because we are iterating over a snapshot which can be
        // != to the current list.
        let unit = <T as Config>::WeightInfo::pay_node_period(T::MaxRegisteredAppChains::get());

        // Resume strictly after the last examined key, or start a fresh pass.
        let mut iter = match SweepCursor::<T>::get() {
            Some((period, node)) =>
                UnpaidByPeriod::<T>::iter_from(UnpaidByPeriod::<T>::hashed_key_for(period, node)),
            None => UnpaidByPeriod::<T>::iter(),
        };

        // Collect a bounded batch up front so we don't mutate `UnpaidByPeriod` while iterating it.
        let mut batch: Vec<(RewardPeriodIndex, T::AccountId)> = Vec::new();
        let mut last: Option<(RewardPeriodIndex, T::AccountId)> = None;
        let mut exhausted = false;
        loop {
            if batch.len() as u32 >= max || !meter.can_consume(unit) {
                break
            }
            match iter.next() {
                Some((period, node, _record)) => {
                    meter.consume(unit);
                    last = Some((period, node.clone()));
                    batch.push((period, node));
                },
                None => {
                    exhausted = true;
                    break
                },
            }
        }

        // Advance the cursor by position (independent of payout success). On exhaustion, wrap to
        // the start so the next pass retries any failures and picks up newly recorded
        // rewards.
        if exhausted {
            SweepCursor::<T>::kill();
        } else if let Some(l) = last {
            SweepCursor::<T>::put(l);
        }

        let processed = batch.len() as u32;
        for (period, node) in batch {
            if let Err(e) = Self::try_pay_node_period(period, &node) {
                Self::deposit_event(Event::AppChainRewardPayoutFailed {
                    reward_period: period,
                    node: node.clone(),
                    error: e,
                });
            }
        }
        processed
    }
}
