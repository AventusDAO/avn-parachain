// Copyright 2019-2022 PureStake Inc.
// This file is part of Moonbeam.

// Moonbeam is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonbeam is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonbeam.  If not, see <http://www.gnu.org/licenses/>.

//! Types for parachain-staking

use crate::{
    set::BoundedOrderedSet, BalanceOf, BottomNominations, CandidateInfo, Config, Delay, Era,
    EraIndex, Error, Event, GrowthPeriodIndex, MinCollatorStake, NominatorState, Pallet,
    RewardPoint, TopNominations, Total, COLLATOR_LOCK_ID, NOMINATOR_LOCK_ID,
};
use codec::{Decode, Encode};
use frame_support::{
    pallet_prelude::*,
    traits::{tokens::WithdrawReasons, LockableCurrency},
};
use sp_avn_common::eth::EthereumId;
use sp_runtime::{
    traits::{Saturating, Zero},
    RuntimeDebug,
};
use sp_std::{cmp::Ordering, prelude::*};

pub struct CountedNominations<T: Config> {
    pub uncounted_stake: BalanceOf<T>,
    pub rewardable_nominations: BoundedVec<Bond<T::AccountId, BalanceOf<T>>, MaxNominations>,
}

#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct MaxCloneableNominations;

impl Get<u32> for MaxCloneableNominations {
    fn get() -> u32 {
        const MAX_NOMINATIONS: u32 = 300;
        MAX_NOMINATIONS
    }
}

pub type MaxNominations = ConstU32<300>;

#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct Bond<AccountId, Balance> {
    pub owner: AccountId,
    pub amount: Balance,
}

impl<A: Decode, B: Default> Default for Bond<A, B> {
    fn default() -> Bond<A, B> {
        Bond {
            owner: A::decode(&mut sp_runtime::traits::TrailingZeroInput::zeroes())
                .expect("infinite length input; no invalid inputs for type; qed"),
            amount: B::default(),
        }
    }
}

impl<A, B: Default> Bond<A, B> {
    pub fn from_owner(owner: A) -> Self {
        Bond { owner, amount: B::default() }
    }
}

impl<AccountId: Ord, Balance> Eq for Bond<AccountId, Balance> {}

impl<AccountId: Ord, Balance> Ord for Bond<AccountId, Balance> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.owner.cmp(&other.owner)
    }
}

impl<AccountId: Ord, Balance> PartialOrd for Bond<AccountId, Balance> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<AccountId: Ord, Balance> PartialEq for Bond<AccountId, Balance> {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// The activity status of the collator
pub enum CollatorStatus {
    /// Committed to be online and producing valid blocks (not equivocating)
    Active,
    /// Temporarily inactive and excused for inactivity
    Idle,
    /// Bonded until the inner era
    Leaving(EraIndex),
}

impl Default for CollatorStatus {
    fn default() -> CollatorStatus {
        CollatorStatus::Active
    }
}

#[derive(Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// Snapshot of collator state at the start of the era for which they are selected
pub struct CollatorSnapshot<AccountId, Balance> {
    /// The total value locked by the collator.
    pub bond: Balance,

    /// The rewardable nominations. This list is a subset of total nominators, where certain
    /// nominators are adjusted based on their scheduled
    /// [NominationChange::Revoke] or [NominationChange::Decrease] action.
    pub nominations: BoundedVec<Bond<AccountId, Balance>, MaxNominations>,

    /// The total counted value locked for the collator, including the self bond + total staked by
    /// top nominators.
    pub total: Balance,
}

impl<A: PartialEq, B: PartialEq> PartialEq for CollatorSnapshot<A, B> {
    fn eq(&self, other: &Self) -> bool {
        let must_be_true = self.bond == other.bond && self.total == other.total;
        if !must_be_true {
            return false;
        }
        for (Bond { owner: o1, amount: a1 }, Bond { owner: o2, amount: a2 }) in
            self.nominations.iter().zip(other.nominations.iter())
        {
            if o1 != o2 || a1 != a2 {
                return false;
            }
        }
        true
    }
}

impl<A, B: Default> Default for CollatorSnapshot<A, B> {
    fn default() -> CollatorSnapshot<A, B> {
        CollatorSnapshot {
            bond: B::default(),
            nominations: BoundedVec::default(),
            total: B::default(),
        }
    }
}

#[derive(Default, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// Info needed to make delayed payments to stakers after era end
pub struct DelayedPayout<Balance> {
    /// Total era reward (result of compute_total_reward_to_pay() at era end)
    pub total_staking_reward: Balance,
}

#[derive(PartialEq, Clone, Copy, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// Request scheduled to change the collator candidate self-bond
pub struct CandidateBondLessRequest<Balance> {
    pub amount: Balance,
    pub when_executable: EraIndex,
}

#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// Type for top and bottom nomination storage item
pub struct Nominations<AccountId, Balance> {
    pub nominations: BoundedVec<Bond<AccountId, Balance>, MaxNominations>,
    pub total: Balance,
}

impl<A, B: Default> Default for Nominations<A, B> {
    fn default() -> Nominations<A, B> {
        Nominations { nominations: BoundedVec::default(), total: B::default() }
    }
}

impl<AccountId, Balance: Copy + Ord + sp_std::ops::AddAssign + Zero + Saturating>
    Nominations<AccountId, Balance>
{
    pub fn sort_greatest_to_least(&mut self) {
        self.nominations.sort_by(|a, b| b.amount.cmp(&a.amount));
    }
    /// Insert sorted greatest to least and increase .total accordingly
    /// Insertion respects first come first serve so new nominations are pushed after existing
    /// nominations if the amount is the same
    pub fn insert_sorted_greatest_to_least(&mut self, nomination: Bond<AccountId, Balance>) {
        self.total = self.total.saturating_add(nomination.amount);
        // if nominations nonempty && last_element == nomination.amount => push input and return
        if !self.nominations.is_empty() {
            // if last_element == nomination.amount => push the nomination and return early
            if self.nominations[self.nominations.len() - 1].amount == nomination.amount {
                self.nominations.try_push(nomination).unwrap_or_else(|_| ());
                // early return
                return;
            }
        }
        // else binary search insertion
        match self.nominations.binary_search_by(|x| nomination.amount.cmp(&x.amount)) {
            // sorted insertion on sorted vec
            // enforces first come first serve for equal bond amounts
            Ok(i) => {
                let mut new_index = i + 1;
                while new_index <= (self.nominations.len() - 1) {
                    if self.nominations[new_index].amount == nomination.amount {
                        new_index = new_index.saturating_add(1);
                    } else {
                        self.nominations.try_insert(new_index, nomination).unwrap_or_else(|_| ());
                        return;
                    }
                }
                self.nominations.try_push(nomination).unwrap_or_else(|_| ())
            },
            Err(i) => self.nominations.try_insert(i, nomination).unwrap_or_else(|_| ()),
        }
    }
    /// Return the capacity status for top nominations
    pub fn top_capacity<T: Config>(&self) -> CapacityStatus {
        match &self.nominations {
            x if x.len() as u32 >= T::MaxTopNominationsPerCandidate::get() => CapacityStatus::Full,
            x if x.is_empty() => CapacityStatus::Empty,
            _ => CapacityStatus::Partial,
        }
    }
    /// Return the capacity status for bottom nominations
    pub fn bottom_capacity<T: Config>(&self) -> CapacityStatus {
        match &self.nominations {
            x if x.len() as u32 >= T::MaxBottomNominationsPerCandidate::get() => {
                CapacityStatus::Full
            },
            x if x.is_empty() => CapacityStatus::Empty,
            _ => CapacityStatus::Partial,
        }
    }
    /// Return last nomination amount without popping the nomination
    pub fn lowest_nomination_amount(&self) -> Balance {
        self.nominations.last().map(|x| x.amount).unwrap_or(Balance::zero())
    }
    /// Return highest nomination amount
    pub fn highest_nomination_amount(&self) -> Balance {
        self.nominations.first().map(|x| x.amount).unwrap_or(Balance::zero())
    }
}

#[derive(PartialEq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// Capacity status for top or bottom nominations
pub enum CapacityStatus {
    /// Reached capacity
    Full,
    /// Empty aka contains no nominations
    Empty,
    /// Partially full (nonempty and not full)
    Partial,
}

#[derive(Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// All candidate info except the top and bottom nominations
pub struct CandidateMetadata<Balance> {
    /// This candidate's self bond amount
    pub bond: Balance,
    /// Total number of nominations to this candidate
    pub nomination_count: u32,
    /// Self bond + sum of top nominations
    pub total_counted: Balance,
    /// The smallest top nomination amount
    pub lowest_top_nomination_amount: Balance,
    /// The highest bottom nomination amount
    pub highest_bottom_nomination_amount: Balance,
    /// The smallest bottom nomination amount
    pub lowest_bottom_nomination_amount: Balance,
    /// Capacity status for top nominations
    pub top_capacity: CapacityStatus,
    /// Capacity status for bottom nominations
    pub bottom_capacity: CapacityStatus,
    /// Maximum 1 pending request to decrease candidate self bond at any given time
    pub request: Option<CandidateBondLessRequest<Balance>>,
    /// Current status of the collator
    pub status: CollatorStatus,
}

impl<
        Balance: Copy
            + Zero
            + PartialOrd
            + sp_std::ops::AddAssign
            + sp_std::ops::SubAssign
            + sp_std::ops::Sub<Output = Balance>
            + sp_std::fmt::Debug
            + Saturating,
    > CandidateMetadata<Balance>
{
    pub fn new(bond: Balance) -> Self {
        CandidateMetadata {
            bond,
            nomination_count: 0u32,
            total_counted: bond,
            lowest_top_nomination_amount: Zero::zero(),
            highest_bottom_nomination_amount: Zero::zero(),
            lowest_bottom_nomination_amount: Zero::zero(),
            top_capacity: CapacityStatus::Empty,
            bottom_capacity: CapacityStatus::Empty,
            request: None,
            status: CollatorStatus::Active,
        }
    }
    pub fn is_active(&self) -> bool {
        matches!(self.status, CollatorStatus::Active)
    }
    pub fn is_leaving(&self) -> bool {
        matches!(self.status, CollatorStatus::Leaving(_))
    }
    pub fn schedule_leave<T: Config>(&mut self) -> Result<(EraIndex, EraIndex), DispatchError> {
        ensure!(!self.is_leaving(), Error::<T>::CandidateAlreadyLeaving);
        let now = <Era<T>>::get().current;
        let when = now + <Delay<T>>::get();
        self.status = CollatorStatus::Leaving(when);
        Ok((now, when))
    }
    pub fn can_leave<T: Config>(&self) -> DispatchResult {
        if let CollatorStatus::Leaving(when) = self.status {
            ensure!(<Era<T>>::get().current >= when, Error::<T>::CandidateCannotLeaveYet);
            Ok(())
        } else {
            Err(Error::<T>::CandidateNotLeaving.into())
        }
    }
    pub fn go_offline(&mut self) {
        self.status = CollatorStatus::Idle;
    }
    pub fn go_online(&mut self) {
        self.status = CollatorStatus::Active;
    }
    pub fn bond_extra<T: Config>(&mut self, who: T::AccountId, more: Balance) -> DispatchResult
    where
        BalanceOf<T>: From<Balance>,
    {
        ensure!(
            <Pallet<T>>::get_collator_stakable_free_balance(&who) >= more.into(),
            Error::<T>::InsufficientBalance
        );
        let new_total = <Total<T>>::get().saturating_add(more.into());
        <Total<T>>::put(new_total);
        self.bond = self.bond.saturating_add(more);
        T::Currency::set_lock(
            COLLATOR_LOCK_ID,
            &who.clone(),
            self.bond.into(),
            WithdrawReasons::all(),
        );
        self.total_counted = self.total_counted.saturating_add(more);
        <Pallet<T>>::deposit_event(Event::CandidateBondedMore {
            candidate: who.clone(),
            amount: more.into(),
            new_total_bond: self.bond.into(),
        });
        Ok(())
    }
    /// Schedule executable decrease of collator candidate self bond
    /// Returns the era at which the collator can execute the pending request
    pub fn schedule_unbond<T: Config>(&mut self, less: Balance) -> Result<EraIndex, DispatchError>
    where
        BalanceOf<T>: Into<Balance>,
    {
        // ensure no pending request
        ensure!(self.request.is_none(), Error::<T>::PendingCandidateRequestAlreadyExists);
        // ensure bond above min after decrease
        ensure!(self.bond > less, Error::<T>::CandidateBondBelowMin);
        ensure!(
            self.bond - less >= <MinCollatorStake<T>>::get().into(),
            Error::<T>::CandidateBondBelowMin
        );
        let when_executable = <Era<T>>::get().current + <Delay<T>>::get();
        self.request = Some(CandidateBondLessRequest { amount: less, when_executable });
        Ok(when_executable)
    }
    /// Execute pending request to decrease the collator self bond
    /// Returns the event to be emitted
    pub fn execute_unbond<T: Config>(&mut self, who: T::AccountId) -> DispatchResult
    where
        BalanceOf<T>: From<Balance>,
    {
        let request = self.request.ok_or(Error::<T>::PendingCandidateRequestsDNE)?;
        ensure!(
            request.when_executable <= <Era<T>>::get().current,
            Error::<T>::PendingCandidateRequestNotDueYet
        );
        let new_total_staked = <Total<T>>::get().saturating_sub(request.amount.into());
        <Total<T>>::put(new_total_staked);
        // Arithmetic assumptions are self.bond > less && self.bond - less > CollatorMinBond
        // (assumptions enforced by `schedule_unbond`; if storage corrupts, must re-verify)
        self.bond = self.bond.saturating_sub(request.amount);
        T::Currency::set_lock(
            COLLATOR_LOCK_ID,
            &who.clone(),
            self.bond.into(),
            WithdrawReasons::all(),
        );
        self.total_counted = self.total_counted.saturating_sub(request.amount);
        let event = Event::CandidateBondedLess {
            candidate: who.clone().into(),
            amount: request.amount.into(),
            new_bond: self.bond.into(),
        };
        // reset s.t. no pending request
        self.request = None;
        // update candidate pool value because it must change if self bond changes
        if self.is_active() {
            Pallet::<T>::update_active(who.into(), self.total_counted.into());
        }
        Pallet::<T>::deposit_event(event);
        Ok(())
    }
    /// Cancel candidate bond less request
    pub fn cancel_unbond<T: Config>(&mut self, who: T::AccountId) -> DispatchResult
    where
        BalanceOf<T>: From<Balance>,
    {
        let request = self.request.ok_or(Error::<T>::PendingCandidateRequestsDNE)?;
        let event = Event::CancelledCandidateBondLess {
            candidate: who.clone().into(),
            amount: request.amount.into(),
            execute_era: request.when_executable,
        };
        self.request = None;
        Pallet::<T>::deposit_event(event);
        Ok(())
    }
    /// Reset top nominations metadata
    pub fn reset_top_data<T: Config>(
        &mut self,
        candidate: T::AccountId,
        top_nominations: &Nominations<T::AccountId, BalanceOf<T>>,
    ) where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        self.lowest_top_nomination_amount = top_nominations.lowest_nomination_amount().into();
        self.top_capacity = top_nominations.top_capacity::<T>();
        let old_total_counted = self.total_counted;
        self.total_counted = self.bond.saturating_add(top_nominations.total.into());
        // CandidatePool value for candidate always changes if top nominations total changes
        // so we moved the update into this function to deduplicate code and patch a bug that
        // forgot to apply the update when increasing top nomination
        if old_total_counted != self.total_counted && self.is_active() {
            Pallet::<T>::update_active(candidate, self.total_counted.into());
        }
    }
    /// Reset bottom nominations metadata
    pub fn reset_bottom_data<T: Config>(
        &mut self,
        bottom_nominations: &Nominations<T::AccountId, BalanceOf<T>>,
    ) where
        BalanceOf<T>: Into<Balance>,
    {
        self.lowest_bottom_nomination_amount = bottom_nominations.lowest_nomination_amount().into();
        self.highest_bottom_nomination_amount =
            bottom_nominations.highest_nomination_amount().into();
        self.bottom_capacity = bottom_nominations.bottom_capacity::<T>();
    }
    /// Add nomination
    /// Returns whether nominator was added and an optional negative total counted remainder
    /// for if a bottom nomination was kicked
    /// MUST ensure no nomination exists for this candidate in the `NominatorState` before call
    pub fn add_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nomination: Bond<T::AccountId, BalanceOf<T>>,
    ) -> Result<(NominatorAdded<Balance>, Option<Balance>), DispatchError>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let mut less_total_staked = None;
        let nominator_added = match self.top_capacity {
            CapacityStatus::Full => {
                // top is full, insert into top iff the lowest_top < amount
                if self.lowest_top_nomination_amount < nomination.amount.into() {
                    // bumps lowest top to the bottom inside this function call
                    less_total_staked = self.add_top_nomination::<T>(candidate, nomination);
                    NominatorAdded::AddedToTop { new_total: self.total_counted }
                } else {
                    // if bottom is full, only insert if greater than lowest bottom (which will
                    // be bumped out)
                    if matches!(self.bottom_capacity, CapacityStatus::Full) {
                        ensure!(
                            nomination.amount.into() > self.lowest_bottom_nomination_amount,
                            Error::<T>::CannotNominateLessThanOrEqualToLowestBottomWhenFull
                        );
                        // need to subtract from total staked
                        less_total_staked = Some(self.lowest_bottom_nomination_amount);
                    }
                    // insert into bottom
                    self.add_bottom_nomination::<T>(false, candidate, nomination);
                    NominatorAdded::AddedToBottom
                }
            },
            // top is either empty or partially full
            _ => {
                self.add_top_nomination::<T>(candidate, nomination);
                NominatorAdded::AddedToTop { new_total: self.total_counted }
            },
        };
        Ok((nominator_added, less_total_staked))
    }
    /// Add nomination to top nomination
    /// Returns Option<negative_total_staked_remainder>
    /// Only call if lowest top nomination is less than nomination.amount || !top_full
    pub fn add_top_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nomination: Bond<T::AccountId, BalanceOf<T>>,
    ) -> Option<Balance>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let mut less_total_staked = None;
        let mut top_nominations = <TopNominations<T>>::get(candidate)
            .expect("CandidateInfo existence => TopNominations existence");
        let max_top_nominations_per_candidate = T::MaxTopNominationsPerCandidate::get();
        if top_nominations.nominations.len() as u32 == max_top_nominations_per_candidate {
            // pop lowest top nomination
            let new_bottom_nomination = top_nominations.nominations.pop().expect("");
            top_nominations.total =
                top_nominations.total.saturating_sub(new_bottom_nomination.amount);
            if matches!(self.bottom_capacity, CapacityStatus::Full) {
                less_total_staked = Some(self.lowest_bottom_nomination_amount);
            }
            self.add_bottom_nomination::<T>(true, candidate, new_bottom_nomination);
        }
        // insert into top
        top_nominations.insert_sorted_greatest_to_least(nomination);
        // update candidate info
        self.reset_top_data::<T>(candidate.clone(), &top_nominations);
        if less_total_staked.is_none() {
            // only increment nomination count if we are not kicking a bottom nomination
            self.nomination_count = self.nomination_count.saturating_add(1u32);
        }
        <TopNominations<T>>::insert(&candidate, top_nominations);
        less_total_staked
    }
    /// Add nomination to bottom nominations
    /// Check before call that if capacity is full, inserted nomination is higher than lowest
    /// bottom nomination (and if so, need to adjust the total storage item)
    /// CALLER MUST ensure(lowest_bottom_to_be_kicked.amount < nomination.amount)
    pub fn add_bottom_nomination<T: Config>(
        &mut self,
        bumped_from_top: bool,
        candidate: &T::AccountId,
        nomination: Bond<T::AccountId, BalanceOf<T>>,
    ) where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let mut bottom_nominations = <BottomNominations<T>>::get(candidate)
            .expect("CandidateInfo existence => BottomNominations existence");
        // if bottom is full, kick the lowest bottom (which is expected to be lower than input
        // as per check)
        let increase_nomination_count = if bottom_nominations.nominations.len() as u32
            == T::MaxBottomNominationsPerCandidate::get()
        {
            let lowest_bottom_to_be_kicked = bottom_nominations
                .nominations
                .pop()
                .expect("if at full capacity (>0), then >0 bottom nominations exist; qed");
            // EXPECT lowest_bottom_to_be_kicked.amount < nomination.amount enforced by caller
            // if lowest_bottom_to_be_kicked.amount == nomination.amount, we will still kick
            // the lowest bottom to enforce first come first served
            bottom_nominations.total =
                bottom_nominations.total.saturating_sub(lowest_bottom_to_be_kicked.amount);
            // update nominator state
            // total staked is updated via propagation of lowest bottom nomination amount prior
            // to call
            let mut nominator_state = <NominatorState<T>>::get(&lowest_bottom_to_be_kicked.owner)
                .expect("Nomination existence => NominatorState existence");
            let leaving = nominator_state.nominations.0.len() == 1usize;
            nominator_state.rm_nomination::<T>(candidate);
            <Pallet<T>>::nomination_remove_request_with_state(
                &candidate,
                &lowest_bottom_to_be_kicked.owner,
                &mut nominator_state,
            );

            Pallet::<T>::deposit_event(Event::NominationKicked {
                nominator: lowest_bottom_to_be_kicked.owner.clone(),
                candidate: candidate.clone(),
                unstaked_amount: lowest_bottom_to_be_kicked.amount,
            });
            if leaving {
                <NominatorState<T>>::remove(&lowest_bottom_to_be_kicked.owner);
                Pallet::<T>::deposit_event(Event::NominatorLeft {
                    nominator: lowest_bottom_to_be_kicked.owner,
                    unstaked_amount: lowest_bottom_to_be_kicked.amount,
                });
            } else {
                <NominatorState<T>>::insert(&lowest_bottom_to_be_kicked.owner, nominator_state);
            }
            false
        } else {
            !bumped_from_top
        };
        // only increase nomination count if new bottom nomination (1) doesn't come from top &&
        // (2) doesn't pop the lowest nomination from the bottom
        if increase_nomination_count {
            self.nomination_count = self.nomination_count.saturating_add(1u32);
        }
        bottom_nominations.insert_sorted_greatest_to_least(nomination);
        self.reset_bottom_data::<T>(&bottom_nominations);
        <BottomNominations<T>>::insert(candidate, bottom_nominations);
    }
    /// Remove nomination
    /// Removes from top if amount is above lowest top or top is not full
    /// Return Ok(if_total_counted_changed)
    pub fn rm_nomination_if_exists<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
        amount: Balance,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let amount_geq_lowest_top = amount >= self.lowest_top_nomination_amount;
        let top_is_not_full = !matches!(self.top_capacity, CapacityStatus::Full);
        let lowest_top_eq_highest_bottom =
            self.lowest_top_nomination_amount == self.highest_bottom_nomination_amount;
        let nomination_dne_err: DispatchError = Error::<T>::NominationDNE.into();
        if top_is_not_full || (amount_geq_lowest_top && !lowest_top_eq_highest_bottom) {
            self.rm_top_nomination::<T>(candidate, nominator)
        } else if amount_geq_lowest_top && lowest_top_eq_highest_bottom {
            let result = self.rm_top_nomination::<T>(candidate, nominator.clone());
            if result == Err(nomination_dne_err) {
                // worst case removal
                self.rm_bottom_nomination::<T>(candidate, nominator)
            } else {
                result
            }
        } else {
            self.rm_bottom_nomination::<T>(candidate, nominator)
        }
    }
    /// Remove top nomination, bumps top bottom nomination if exists
    pub fn rm_top_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let old_total_counted = self.total_counted;
        // remove top nomination
        let mut top_nominations = <TopNominations<T>>::get(candidate)
            .expect("CandidateInfo exists => TopNominations exists");
        let mut actual_amount_option: Option<BalanceOf<T>> = None;
        let filtered_nominations_vec: Vec<_> = top_nominations
            .nominations
            .clone()
            .into_iter()
            .filter(|d| {
                if d.owner != nominator {
                    true
                } else {
                    actual_amount_option = Some(d.amount);
                    false
                }
            })
            .collect();
        top_nominations.nominations = BoundedVec::truncate_from(filtered_nominations_vec);
        let actual_amount = actual_amount_option.ok_or(Error::<T>::NominationDNE)?;
        top_nominations.total = top_nominations.total.saturating_sub(actual_amount);
        // if bottom nonempty => bump top bottom to top
        if !matches!(self.bottom_capacity, CapacityStatus::Empty) {
            let mut bottom_nominations =
                <BottomNominations<T>>::get(candidate).expect("bottom is nonempty as just checked");
            // expect already stored greatest to least by bond amount
            let highest_bottom_nomination = bottom_nominations.nominations.remove(0);
            bottom_nominations.total =
                bottom_nominations.total.saturating_sub(highest_bottom_nomination.amount);
            self.reset_bottom_data::<T>(&bottom_nominations);
            <BottomNominations<T>>::insert(candidate, bottom_nominations);
            // insert highest bottom into top nominations
            top_nominations.insert_sorted_greatest_to_least(highest_bottom_nomination);
        }
        // update candidate info
        self.reset_top_data::<T>(candidate.clone(), &top_nominations);
        self.nomination_count = self.nomination_count.saturating_sub(1u32);
        <TopNominations<T>>::insert(candidate, top_nominations);
        // return whether total counted changed
        Ok(old_total_counted == self.total_counted)
    }
    /// Remove bottom nomination
    /// Returns if_total_counted_changed: bool
    pub fn rm_bottom_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance>,
    {
        // remove bottom nomination
        let mut bottom_nominations = <BottomNominations<T>>::get(candidate)
            .expect("CandidateInfo exists => BottomNominations exists");
        let mut actual_amount_option: Option<BalanceOf<T>> = None;
        let filtered_bottom_nominations: Vec<_> = bottom_nominations
            .nominations
            .clone()
            .into_iter()
            .filter(|d| {
                if d.owner != nominator {
                    true
                } else {
                    actual_amount_option = Some(d.amount);
                    false
                }
            })
            .collect();

        bottom_nominations.nominations = BoundedVec::truncate_from(filtered_bottom_nominations);
        let actual_amount = actual_amount_option.ok_or(Error::<T>::NominationDNE)?;
        bottom_nominations.total = bottom_nominations.total.saturating_sub(actual_amount);
        // update candidate info
        self.reset_bottom_data::<T>(&bottom_nominations);
        self.nomination_count = self.nomination_count.saturating_sub(1u32);
        <BottomNominations<T>>::insert(candidate, bottom_nominations);
        Ok(false)
    }
    /// Increase nomination amount
    pub fn increase_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
        bond: BalanceOf<T>,
        more: BalanceOf<T>,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let lowest_top_eq_highest_bottom =
            self.lowest_top_nomination_amount == self.highest_bottom_nomination_amount;
        let bond_geq_lowest_top = bond.into() >= self.lowest_top_nomination_amount;
        let nomination_dne_err: DispatchError = Error::<T>::NominationDNE.into();
        if bond_geq_lowest_top && !lowest_top_eq_highest_bottom {
            // definitely in top
            self.increase_top_nomination::<T>(candidate, nominator.clone(), more)
        } else if bond_geq_lowest_top && lowest_top_eq_highest_bottom {
            // update top but if error then update bottom (because could be in bottom because
            // lowest_top_eq_highest_bottom)
            let result = self.increase_top_nomination::<T>(candidate, nominator.clone(), more);
            if result == Err(nomination_dne_err) {
                self.increase_bottom_nomination::<T>(candidate, nominator, bond, more)
            } else {
                result
            }
        } else {
            self.increase_bottom_nomination::<T>(candidate, nominator, bond, more)
        }
    }
    /// Increase top nomination
    pub fn increase_top_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
        more: BalanceOf<T>,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let mut top_nominations = <TopNominations<T>>::get(candidate)
            .expect("CandidateInfo exists => TopNominations exists");
        let mut in_top = false;
        let filtered_top_nominations: Vec<_> = top_nominations
            .nominations
            .clone()
            .into_iter()
            .map(|d| {
                if d.owner != nominator {
                    d
                } else {
                    in_top = true;
                    let new_amount = d.amount.saturating_add(more);
                    Bond { owner: d.owner, amount: new_amount }
                }
            })
            .collect();
        top_nominations.nominations = BoundedVec::truncate_from(filtered_top_nominations);
        ensure!(in_top, Error::<T>::NominationDNE);
        top_nominations.total = top_nominations.total.saturating_add(more);
        top_nominations.sort_greatest_to_least();
        self.reset_top_data::<T>(candidate.clone(), &top_nominations);
        <TopNominations<T>>::insert(candidate, top_nominations);
        Ok(true)
    }
    /// Increase bottom nomination
    pub fn increase_bottom_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
        bond: BalanceOf<T>,
        more: BalanceOf<T>,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let mut bottom_nominations =
            <BottomNominations<T>>::get(candidate).ok_or(Error::<T>::CandidateDNE)?;
        let mut nomination_option: Option<Bond<T::AccountId, BalanceOf<T>>> = None;
        let in_top_after = if (bond.saturating_add(more)).into() > self.lowest_top_nomination_amount
        {
            // bump it from bottom
            let filtered_bottom_nominations: Vec<_> = bottom_nominations
                .nominations
                .clone()
                .into_iter()
                .filter(|d| {
                    if d.owner != nominator {
                        true
                    } else {
                        nomination_option = Some(Bond {
                            owner: d.owner.clone(),
                            amount: d.amount.saturating_add(more),
                        });
                        false
                    }
                })
                .collect();
            bottom_nominations.nominations = BoundedVec::truncate_from(filtered_bottom_nominations);
            let nomination = nomination_option.ok_or(Error::<T>::NominationDNE)?;
            bottom_nominations.total = bottom_nominations.total.saturating_sub(bond);
            // add it to top
            let mut top_nominations = <TopNominations<T>>::get(candidate)
                .expect("CandidateInfo existence => TopNominations existence");
            // if top is full, pop lowest top
            if matches!(top_nominations.top_capacity::<T>(), CapacityStatus::Full) {
                // pop lowest top nomination
                let new_bottom_nomination = top_nominations
                    .nominations
                    .pop()
                    .expect("Top capacity full => Exists at least 1 top nomination");
                top_nominations.total =
                    top_nominations.total.saturating_sub(new_bottom_nomination.amount);
                bottom_nominations.insert_sorted_greatest_to_least(new_bottom_nomination);
            }
            // insert into top
            top_nominations.insert_sorted_greatest_to_least(nomination);
            self.reset_top_data::<T>(candidate.clone(), &top_nominations);
            <TopNominations<T>>::insert(candidate, top_nominations);
            true
        } else {
            let mut in_bottom = false;
            // just increase the nomination
            let filtered_bottom_nominations = bottom_nominations
                .nominations
                .clone()
                .into_iter()
                .map(|d| {
                    if d.owner != nominator {
                        d
                    } else {
                        in_bottom = true;
                        Bond { owner: d.owner, amount: d.amount.saturating_add(more) }
                    }
                })
                .collect();
            bottom_nominations.nominations = BoundedVec::truncate_from(filtered_bottom_nominations);
            ensure!(in_bottom, Error::<T>::NominationDNE);
            bottom_nominations.total = bottom_nominations.total.saturating_add(more);
            bottom_nominations.sort_greatest_to_least();
            false
        };
        self.reset_bottom_data::<T>(&bottom_nominations);
        <BottomNominations<T>>::insert(candidate, bottom_nominations);
        Ok(in_top_after)
    }
    /// Decrease nomination
    pub fn decrease_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
        bond: Balance,
        less: BalanceOf<T>,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        let lowest_top_eq_highest_bottom =
            self.lowest_top_nomination_amount == self.highest_bottom_nomination_amount;
        let bond_geq_lowest_top = bond >= self.lowest_top_nomination_amount;
        let nomination_dne_err: DispatchError = Error::<T>::NominationDNE.into();
        if bond_geq_lowest_top && !lowest_top_eq_highest_bottom {
            // definitely in top
            self.decrease_top_nomination::<T>(candidate, nominator.clone(), bond.into(), less)
        } else if bond_geq_lowest_top && lowest_top_eq_highest_bottom {
            // update top but if error then update bottom (because could be in bottom because
            // lowest_top_eq_highest_bottom)
            let result =
                self.decrease_top_nomination::<T>(candidate, nominator.clone(), bond.into(), less);
            if result == Err(nomination_dne_err) {
                self.decrease_bottom_nomination::<T>(candidate, nominator, less)
            } else {
                result
            }
        } else {
            self.decrease_bottom_nomination::<T>(candidate, nominator, less)
        }
    }
    /// Decrease top nomination
    pub fn decrease_top_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
        bond: BalanceOf<T>,
        less: BalanceOf<T>,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance> + From<Balance>,
    {
        // The nomination after the `decrease-nomination` will be strictly less than the
        // highest bottom nomination
        let bond_after_less_than_highest_bottom =
            bond.saturating_sub(less).into() < self.highest_bottom_nomination_amount;
        // The top nominations is full and the bottom nominations has at least one nomination
        let full_top_and_nonempty_bottom = matches!(self.top_capacity, CapacityStatus::Full)
            && !matches!(self.bottom_capacity, CapacityStatus::Empty);
        let mut top_nominations =
            <TopNominations<T>>::get(candidate).ok_or(Error::<T>::CandidateDNE)?;
        let in_top_after = if bond_after_less_than_highest_bottom && full_top_and_nonempty_bottom {
            let mut nomination_option: Option<Bond<T::AccountId, BalanceOf<T>>> = None;
            // take nomination from top
            let filtered_top_nominations = top_nominations
                .nominations
                .clone()
                .into_iter()
                .filter(|d| {
                    if d.owner != nominator {
                        true
                    } else {
                        top_nominations.total = top_nominations.total.saturating_sub(d.amount);
                        nomination_option = Some(Bond {
                            owner: d.owner.clone(),
                            amount: d.amount.saturating_sub(less),
                        });
                        false
                    }
                })
                .collect();
            top_nominations.nominations = BoundedVec::truncate_from(filtered_top_nominations);
            let nomination = nomination_option.ok_or(Error::<T>::NominationDNE)?;
            // pop highest bottom by reverse and popping
            let mut bottom_nominations = <BottomNominations<T>>::get(candidate)
                .expect("CandidateInfo existence => BottomNominations existence");
            let highest_bottom_nomination = bottom_nominations.nominations.remove(0);
            bottom_nominations.total =
                bottom_nominations.total.saturating_sub(highest_bottom_nomination.amount);
            // insert highest bottom into top
            top_nominations.insert_sorted_greatest_to_least(highest_bottom_nomination);
            // insert previous top into bottom
            bottom_nominations.insert_sorted_greatest_to_least(nomination);
            self.reset_bottom_data::<T>(&bottom_nominations);
            <BottomNominations<T>>::insert(candidate, bottom_nominations);
            false
        } else {
            // keep it in the top
            let mut is_in_top = false;
            let filtered_top_nominations = top_nominations
                .nominations
                .clone()
                .into_iter()
                .map(|d| {
                    if d.owner != nominator {
                        d
                    } else {
                        is_in_top = true;
                        Bond { owner: d.owner, amount: d.amount.saturating_sub(less) }
                    }
                })
                .collect();
            top_nominations.nominations = BoundedVec::truncate_from(filtered_top_nominations);
            ensure!(is_in_top, Error::<T>::NominationDNE);
            top_nominations.total = top_nominations.total.saturating_sub(less);
            top_nominations.sort_greatest_to_least();
            true
        };
        self.reset_top_data::<T>(candidate.clone(), &top_nominations);
        <TopNominations<T>>::insert(candidate, top_nominations);
        Ok(in_top_after)
    }
    /// Decrease bottom nomination
    pub fn decrease_bottom_nomination<T: Config>(
        &mut self,
        candidate: &T::AccountId,
        nominator: T::AccountId,
        less: BalanceOf<T>,
    ) -> Result<bool, DispatchError>
    where
        BalanceOf<T>: Into<Balance>,
    {
        let mut bottom_nominations = <BottomNominations<T>>::get(candidate)
            .expect("CandidateInfo exists => BottomNominations exists");
        let mut in_bottom = false;
        let filtered_bottom_nominations = bottom_nominations
            .nominations
            .clone()
            .into_iter()
            .map(|d| {
                if d.owner != nominator {
                    d
                } else {
                    in_bottom = true;
                    Bond { owner: d.owner, amount: d.amount.saturating_sub(less) }
                }
            })
            .collect();
        bottom_nominations.nominations = BoundedVec::truncate_from(filtered_bottom_nominations);
        ensure!(in_bottom, Error::<T>::NominationDNE);
        bottom_nominations.sort_greatest_to_least();
        self.reset_bottom_data::<T>(&bottom_nominations);
        <BottomNominations<T>>::insert(candidate, bottom_nominations);
        Ok(false)
    }
}

/// Convey relevant information describing if a nominator was added to the top or bottom
/// Nominations added to the top yield a new total
#[derive(Clone, Copy, PartialEq, Encode, Decode, RuntimeDebug, TypeInfo, DecodeWithMemTracking)]
pub enum NominatorAdded<B> {
    AddedToTop { new_total: B },
    AddedToBottom,
}

#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// Nominator state
pub struct Nominator<AccountId, Balance> {
    /// Nominator account
    pub id: AccountId,
    /// All current nominations
    pub nominations: BoundedOrderedSet<Bond<AccountId, Balance>, MaxCloneableNominations>,
    /// Total balance locked for this nominator
    pub total: Balance,
    /// Sum of pending revocation amounts + bond less amounts
    pub less_total: Balance,
}

// Temporary manual implementation for migration testing purposes
impl<A: PartialEq, B: PartialEq + Clone> PartialEq for Nominator<A, B> {
    fn eq(&self, other: &Self) -> bool {
        let must_be_true =
            self.id == other.id && self.total == other.total && self.less_total == other.less_total;
        if !must_be_true {
            return false;
        }
        for (Bond { owner: o1, amount: a1 }, Bond { owner: o2, amount: a2 }) in
            self.nominations.0.iter().zip(other.nominations.0.iter())
        {
            if o1 != o2 || a1 != a2 {
                return false;
            }
        }
        true
    }
}

impl<
        AccountId: Ord + Clone,
        Balance: Copy
            + sp_std::ops::AddAssign
            + sp_std::ops::Add<Output = Balance>
            + sp_std::ops::SubAssign
            + sp_std::ops::Sub<Output = Balance>
            + Ord
            + Zero
            + Default
            + Saturating,
    > Nominator<AccountId, Balance>
{
    pub fn new(id: AccountId, collator: AccountId, amount: Balance) -> Self {
        Nominator {
            id,
            nominations: BoundedOrderedSet::from(BoundedVec::truncate_from(vec![Bond {
                owner: collator,
                amount,
            }])),
            total: amount,
            less_total: Balance::zero(),
        }
    }

    pub fn default_with_total(id: AccountId, amount: Balance) -> Self {
        Nominator {
            id,
            total: amount,
            nominations: BoundedOrderedSet::from(BoundedVec::default()),
            less_total: Balance::zero(),
        }
    }

    pub fn total(&self) -> Balance {
        self.total
    }

    pub fn total_sub_if<T, F>(&mut self, amount: Balance, check: F) -> DispatchResult
    where
        T: Config,
        T::AccountId: From<AccountId>,
        BalanceOf<T>: From<Balance>,
        F: Fn(Balance) -> DispatchResult,
    {
        let total = self.total.saturating_sub(amount);
        check(total)?;
        self.total = total;
        self.adjust_bond_lock::<T>(BondAdjust::Decrease)?;
        Ok(())
    }

    pub fn total_add<T, F>(&mut self, amount: Balance) -> DispatchResult
    where
        T: Config,
        T::AccountId: From<AccountId>,
        BalanceOf<T>: From<Balance>,
    {
        self.total = self.total.saturating_add(amount);
        self.adjust_bond_lock::<T>(BondAdjust::Increase(amount))?;
        Ok(())
    }

    pub fn total_sub<T>(&mut self, amount: Balance) -> DispatchResult
    where
        T: Config,
        T::AccountId: From<AccountId>,
        BalanceOf<T>: From<Balance>,
    {
        self.total = self.total.saturating_sub(amount);
        self.adjust_bond_lock::<T>(BondAdjust::Decrease)?;
        Ok(())
    }

    pub fn add_nomination(&mut self, bond: Bond<AccountId, Balance>) -> bool {
        let amt = bond.amount;
        if self.nominations.try_insert(bond.clone()).unwrap() {
            self.total = self.total.saturating_add(amt);
            true
        } else {
            false
        }
    }
    // Return Some(remaining balance), must be more than MinTotalNominatorStake
    // Return None if nomination not found
    pub fn rm_nomination<T: Config>(&mut self, collator: &AccountId) -> Option<Balance>
    where
        BalanceOf<T>: From<Balance>,
        T::AccountId: From<AccountId>,
    {
        let mut amt: Option<Balance> = None;

        let mut nominations = BoundedOrderedSet::new(); // Use the BoundedOrderedSet instead of   Vec.

        for x in &self.nominations.0 {
            if &x.owner == collator {
                amt = Some(x.amount);
            } else {
                nominations.try_insert(x.clone()).unwrap(); // Use try_insert to add elements to the
                                                            // set.
            }
        }

        if let Some(balance) = amt {
            self.nominations = nominations;
            self.total_sub::<T>(balance).expect("Decreasing lock cannot fail, qed");
            Some(self.total)
        } else {
            None
        }
    }
    pub fn increase_nomination<T: Config>(
        &mut self,
        candidate: AccountId,
        amount: Balance,
    ) -> DispatchResult
    where
        BalanceOf<T>: From<Balance>,
        T::AccountId: From<AccountId>,
        Nominator<T::AccountId, BalanceOf<T>>: From<Nominator<AccountId, Balance>>,
    {
        let nominator_id: T::AccountId = self.id.clone().into();
        let candidate_id: T::AccountId = candidate.clone().into();
        let balance_amt: BalanceOf<T> = amount.into();
        // increase nomination
        for x in &mut self.nominations.0 {
            if x.owner == candidate {
                let before_amount: BalanceOf<T> = x.amount.into();
                x.amount = x.amount.saturating_add(amount);
                self.total = self.total.saturating_add(amount);
                self.adjust_bond_lock::<T>(BondAdjust::Increase(amount))?;

                // update collator state nomination
                let mut collator_state =
                    <CandidateInfo<T>>::get(&candidate_id).ok_or(Error::<T>::CandidateDNE)?;
                let before = collator_state.total_counted;
                let in_top = collator_state.increase_nomination::<T>(
                    &candidate_id,
                    nominator_id.clone(),
                    before_amount,
                    balance_amt,
                )?;
                let after = collator_state.total_counted;
                if collator_state.is_active() && (before != after) {
                    Pallet::<T>::update_active(candidate_id.clone(), after);
                }
                <CandidateInfo<T>>::insert(&candidate_id, collator_state);
                let new_total_staked = <Total<T>>::get().saturating_add(balance_amt);
                <Total<T>>::put(new_total_staked);
                let nom_st: Nominator<T::AccountId, BalanceOf<T>> = self.clone().into();
                <NominatorState<T>>::insert(&nominator_id, nom_st);
                Pallet::<T>::deposit_event(Event::NominationIncreased {
                    nominator: nominator_id,
                    candidate: candidate_id,
                    amount: balance_amt,
                    in_top,
                });
                return Ok(());
            }
        }
        Err(Error::<T>::NominationDNE.into())
    }

    /// Updates the bond locks for this nominator.
    ///
    /// This will take the current self.total and ensure that a lock of the same amount is applied
    /// and when increasing the bond lock will also ensure that the account has enough free balance.
    ///
    /// `additional_required_balance` should reflect the change to the amount that should be locked
    /// if positive, 0 otherwise (e.g. `min(0, change_in_total_bond)`). This is necessary
    /// because it is not possible to query the amount that is locked for a given lock id.
    pub fn adjust_bond_lock<T: Config>(
        &mut self,
        additional_required_balance: BondAdjust<Balance>,
    ) -> DispatchResult
    where
        BalanceOf<T>: From<Balance>,
        T::AccountId: From<AccountId>,
    {
        match additional_required_balance {
            BondAdjust::Increase(amount) => {
                ensure!(
                    <Pallet<T>>::get_nominator_stakable_free_balance(&self.id.clone().into())
                        >= amount.into(),
                    Error::<T>::InsufficientBalance,
                );

                // additional sanity check: shouldn't ever want to lock more than total
                if amount > self.total {
                    log::warn!("LOGIC ERROR: request to reserve more than bond total");
                    return Err(DispatchError::Other("Invalid additional_required_balance"));
                }
            },
            BondAdjust::Decrease => (), // do nothing on decrease
        };

        if self.total.is_zero() {
            T::Currency::remove_lock(NOMINATOR_LOCK_ID, &self.id.clone().into());
        } else {
            T::Currency::set_lock(
                NOMINATOR_LOCK_ID,
                &self.id.clone().into(),
                self.total.into(),
                WithdrawReasons::all(),
            );
        }
        Ok(())
    }

    /// Retrieves the bond amount that a nominator has provided towards a collator.
    /// Returns `None` if missing.
    pub fn get_bond_amount(&self, collator: &AccountId) -> Option<Balance> {
        self.nominations.0.iter().find(|b| &b.owner == collator).map(|b| b.amount)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// The current era index and transition information
pub struct EraInfo<BlockNumber> {
    /// Current era index
    pub current: EraIndex,
    /// The first block of the current era
    pub first: BlockNumber,
    /// The length of the current era in number of blocks
    pub length: u32,
}
impl<
        B: Copy + sp_std::ops::Add<Output = B> + sp_std::ops::Sub<Output = B> + From<u32> + PartialOrd,
    > EraInfo<B>
{
    pub fn new(current: EraIndex, first: B, length: u32) -> EraInfo<B> {
        EraInfo { current, first, length }
    }
    /// Check if the era should be updated
    pub fn should_update(&self, now: B) -> bool {
        now - self.first >= self.length.into()
    }
    /// New era
    pub fn update(&mut self, now: B) {
        self.current = self.current.saturating_add(1u32);
        self.first = now;
    }
}
impl<
        B: Copy + sp_std::ops::Add<Output = B> + sp_std::ops::Sub<Output = B> + From<u32> + PartialOrd,
    > Default for EraInfo<B>
{
    fn default() -> EraInfo<B> {
        EraInfo::new(1u32, 1u32.into(), 20u32)
    }
}

pub enum BondAdjust<Balance> {
    Increase(Balance),
    Decrease,
}

#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct CollatorScore<AccountId> {
    pub collator: AccountId,
    pub points: RewardPoint,
}

// Datastructure for tracking collator performance
impl<A: Decode> Default for CollatorScore<A> {
    fn default() -> CollatorScore<A> {
        CollatorScore {
            collator: A::decode(&mut sp_runtime::traits::TrailingZeroInput::zeroes())
                .expect("infinite length input; no invalid inputs for type; qed"),
            points: 0u32,
        }
    }
}

impl<AccountId> CollatorScore<AccountId> {
    pub fn new(collator: AccountId, points: RewardPoint) -> Self {
        CollatorScore { collator, points }
    }
}

impl<AccountId: Ord> Eq for CollatorScore<AccountId> {}

impl<AccountId: Ord> Ord for CollatorScore<AccountId> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.collator.cmp(&other.collator)
    }
}

impl<AccountId: Ord> PartialOrd for CollatorScore<AccountId> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<AccountId: Ord> PartialEq for CollatorScore<AccountId> {
    fn eq(&self, other: &Self) -> bool {
        self.collator == other.collator
    }
}

// Data structure for tracking collator rewards
#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct GrowthInfo<AccountId, Balance> {
    pub number_of_accumulations: GrowthPeriodIndex,
    pub total_stake_accumulated: Balance,
    pub total_staker_reward: Balance,
    pub total_points: RewardPoint,
    pub collator_scores: BoundedVec<CollatorScore<AccountId>, ConstU32<10000>>,
    pub tx_id: Option<EthereumId>,
    pub triggered: Option<bool>,
}

impl<
        AccountId: Clone,
        Balance: Copy
            + sp_std::ops::AddAssign
            + sp_std::ops::Add<Output = Balance>
            + sp_std::ops::SubAssign
            + sp_std::ops::Sub<Output = Balance>
            + Ord
            + Zero
            + Default
            + Saturating,
    > GrowthInfo<AccountId, Balance>
{
    pub fn new(number_of_accumulations: GrowthPeriodIndex) -> Self {
        GrowthInfo {
            number_of_accumulations,
            total_stake_accumulated: Balance::zero(),
            total_staker_reward: Balance::zero(),
            total_points: 0u32.into(),
            collator_scores: BoundedVec::default(),
            tx_id: None,
            triggered: None,
        }
    }
}

impl<A: Decode, B: Default> Default for GrowthInfo<A, B> {
    fn default() -> GrowthInfo<A, B> {
        GrowthInfo {
            number_of_accumulations: Default::default(),
            total_stake_accumulated: B::default(),
            total_staker_reward: B::default(),
            total_points: Default::default(),
            collator_scores: BoundedVec::default(),
            tx_id: None,
            triggered: None,
        }
    }
}

// Data structure for tracking collator reward periods
#[derive(
    Clone,
    PartialEq,
    Eq,
    Encode,
    Decode,
    RuntimeDebug,
    Default,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
pub struct GrowthPeriodInfo {
    pub start_era_index: EraIndex,
    pub index: GrowthPeriodIndex,
}

#[derive(Encode, Decode, Clone, PartialEq, Debug, Eq, TypeInfo, DecodeWithMemTracking)]
pub enum AdminSettings<Balance> {
    /// The delay, in blocks, for actions to wait before being executed
    Delay(EraIndex),
    /// Minimum collator stake amount
    MinCollatorStake(Balance),
    /// Minimum nominator stake amount
    MinTotalNominatorStake(Balance),
}

impl<
        Balance: Copy
            + sp_std::ops::AddAssign
            + sp_std::ops::Add<Output = Balance>
            + sp_std::ops::SubAssign
            + sp_std::ops::Sub<Output = Balance>
            + Ord
            + Zero
            + Default
            + Saturating,
    > AdminSettings<Balance>
{
    #[allow(unreachable_patterns)]
    pub fn is_valid<T: Config>(&self) -> bool
    where
        Balance: From<BalanceOf<T>>,
    {
        return match self {
            AdminSettings::Delay(d) => d > &0,
            AdminSettings::MinTotalNominatorStake(s) => {
                s >= &<<T as Config>::MinNominationPerCollator as Get<BalanceOf<T>>>::get().into()
            },
            AdminSettings::MinCollatorStake(_) => true,
            _ => false,
        };
    }
}

// Amount based stake data. Note: 2 stakes with the same free amount are considered equal
#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct StakeInfo<AccountId, Balance> {
    pub owner: AccountId,
    pub free_amount: Balance,
    pub reserved_amount: Balance,
}

impl<A: Decode, B: Default> Default for StakeInfo<A, B> {
    fn default() -> StakeInfo<A, B> {
        StakeInfo {
            owner: A::decode(&mut sp_runtime::traits::TrailingZeroInput::zeroes())
                .expect("infinite length input; no invalid inputs for type; qed"),
            free_amount: B::default(),
            reserved_amount: B::default(),
        }
    }
}

impl<A, B: Default> StakeInfo<A, B> {
    pub fn new(owner: A, free_amount: B, reserved_amount: B) -> Self {
        StakeInfo { owner, free_amount, reserved_amount }
    }
}
