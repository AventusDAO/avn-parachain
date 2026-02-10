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

//! Scheduled requests functionality for nominators

use crate::{
    BalanceOf, CandidateInfo, Config, Delay, Era, EraIndex, Error, Event, MinTotalNominatorStake,
    NominationScheduledRequests, Nominator, NominatorState, Pallet, Total,
};
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{dispatch::DispatchResultWithPostInfo, ensure, traits::Get};
use scale_info::TypeInfo;
use sp_runtime::{traits::Saturating, BoundedVec, RuntimeDebug};
use sp_std::vec;

/// An action that can be performed upon a nomination
#[derive(
    Clone,
    Eq,
    PartialEq,
    Encode,
    Decode,
    RuntimeDebug,
    TypeInfo,
    PartialOrd,
    Ord,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
pub enum NominationAction<Balance> {
    Revoke(Balance),
    Decrease(Balance),
}

impl<Balance: Copy> NominationAction<Balance> {
    /// Returns the wrapped amount value.
    pub fn amount(&self) -> Balance {
        match self {
            NominationAction::Revoke(amount) => *amount,
            NominationAction::Decrease(amount) => *amount,
        }
    }
}

/// Represents a scheduled request that define a [NominationAction]. The request is executable
/// iff the provided [EraIndex] is achieved.
#[derive(
    Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug, TypeInfo, PartialOrd, Ord, MaxEncodedLen,
)]
pub struct ScheduledRequest<AccountId, Balance> {
    pub nominator: AccountId,
    pub when_executable: EraIndex,
    pub action: NominationAction<Balance>,
}

/// Represents a cancelled scheduled request for emitting an event.
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug, TypeInfo, DecodeWithMemTracking)]
pub struct CancelledScheduledRequest<Balance> {
    pub when_executable: EraIndex,
    pub action: NominationAction<Balance>,
}

impl<A, B> From<ScheduledRequest<A, B>> for CancelledScheduledRequest<B> {
    fn from(request: ScheduledRequest<A, B>) -> Self {
        CancelledScheduledRequest {
            when_executable: request.when_executable,
            action: request.action,
        }
    }
}

impl<T: Config> Pallet<T> {
    /// Schedules a [NominationAction::Revoke] for the nominator, towards a given collator.
    pub(crate) fn nomination_schedule_revoke(
        collator: T::AccountId,
        nominator: T::AccountId,
    ) -> DispatchResultWithPostInfo {
        let mut state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
        let mut scheduled_requests = <NominationScheduledRequests<T>>::get(&collator);

        ensure!(
            !scheduled_requests.iter().any(|req| req.nominator == nominator),
            <Error<T>>::PendingNominationRequestAlreadyExists,
        );

        let bonded_amount = state.get_bond_amount(&collator).ok_or(<Error<T>>::NominationDNE)?;
        let now = <Era<T>>::get().current;
        let when = now.saturating_add(<Delay<T>>::get());
        match scheduled_requests.try_push(ScheduledRequest {
            nominator: nominator.clone(),
            action: NominationAction::Revoke(bonded_amount),
            when_executable: when,
        }) {
            Ok(()) => {
                state.less_total = state.less_total.saturating_add(bonded_amount);
                <NominationScheduledRequests<T>>::insert(collator.clone(), scheduled_requests);
                <NominatorState<T>>::insert(nominator.clone(), state);

                Self::deposit_event(Event::NominationRevocationScheduled {
                    era: now,
                    nominator,
                    candidate: collator,
                    scheduled_exit: when,
                });
            },
            Err(_) => {
                ();
            },
        }

        Ok(().into())
    }

    /// Schedules a [NominationAction::Decrease] for the nominator, towards a given collator.
    pub(crate) fn nomination_schedule_bond_decrease(
        collator: T::AccountId,
        nominator: T::AccountId,
        decrease_amount: BalanceOf<T>,
    ) -> DispatchResultWithPostInfo {
        let mut state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
        let mut scheduled_requests = <NominationScheduledRequests<T>>::get(&collator);

        ensure!(
            !scheduled_requests.iter().any(|req| req.nominator == nominator),
            <Error<T>>::PendingNominationRequestAlreadyExists,
        );

        let bonded_amount = state.get_bond_amount(&collator).ok_or(<Error<T>>::NominationDNE)?;
        ensure!(bonded_amount > decrease_amount, <Error<T>>::NominatorBondBelowMin);
        let new_amount: BalanceOf<T> = (bonded_amount - decrease_amount).into();
        ensure!(new_amount >= T::MinNominationPerCollator::get(), <Error<T>>::NominationBelowMin);

        // Net Total is total after pending orders are executed
        let net_total = state.total().saturating_sub(state.less_total);
        // Net Total is always >= MinTotalNominatorStake
        let max_subtracted_amount =
            net_total.saturating_sub(<MinTotalNominatorStake<T>>::get().into());
        ensure!(decrease_amount <= max_subtracted_amount, <Error<T>>::NominatorBondBelowMin);

        let now = <Era<T>>::get().current;
        let when = now.saturating_add(<Delay<T>>::get());
        match scheduled_requests.try_push(ScheduledRequest {
            nominator: nominator.clone(),
            action: NominationAction::Decrease(decrease_amount),
            when_executable: when,
        }) {
            Ok(()) => {
                state.less_total = state.less_total.saturating_add(decrease_amount);
                <NominationScheduledRequests<T>>::insert(collator.clone(), scheduled_requests);
                <NominatorState<T>>::insert(nominator.clone(), state);

                Self::deposit_event(Event::NominationDecreaseScheduled {
                    nominator,
                    candidate: collator,
                    amount_to_decrease: decrease_amount,
                    execute_era: when,
                });
            },
            Err(_) => (),
        }
        Ok(().into())
    }

    /// Cancels the nominator's existing [ScheduledRequest] towards a given collator.
    pub(crate) fn nomination_cancel_request(
        collator: T::AccountId,
        nominator: T::AccountId,
    ) -> DispatchResultWithPostInfo {
        let mut state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
        let mut scheduled_requests = <NominationScheduledRequests<T>>::get(&collator);

        let request =
            Self::cancel_request_with_state(&nominator, &mut state, &mut scheduled_requests)
                .ok_or(<Error<T>>::PendingNominationRequestDNE)?;

        <NominationScheduledRequests<T>>::insert(collator.clone(), scheduled_requests);
        <NominatorState<T>>::insert(nominator.clone(), state);

        Self::deposit_event(Event::CancelledNominationRequest {
            nominator,
            collator,
            cancelled_request: request.into(),
        });
        Ok(().into())
    }

    fn cancel_request_with_state(
        nominator: &T::AccountId,
        state: &mut Nominator<T::AccountId, BalanceOf<T>>,
        scheduled_requests: &mut BoundedVec<
            ScheduledRequest<T::AccountId, BalanceOf<T>>,
            T::MaxNominationsPerNominator,
        >,
    ) -> Option<ScheduledRequest<T::AccountId, BalanceOf<T>>> {
        let request_idx = scheduled_requests.iter().position(|req| &req.nominator == nominator)?;

        let request = scheduled_requests.remove(request_idx);
        let amount = request.action.amount();
        state.less_total = state.less_total.saturating_sub(amount);
        Some(request)
    }

    /// Executes the nominator's existing [ScheduledRequest] towards a given collator.
    pub(crate) fn nomination_execute_scheduled_request(
        collator: T::AccountId,
        nominator: T::AccountId,
    ) -> DispatchResultWithPostInfo {
        let mut state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
        let mut scheduled_requests = <NominationScheduledRequests<T>>::get(&collator);
        let request_idx = scheduled_requests
            .iter()
            .position(|req| req.nominator == nominator)
            .ok_or(<Error<T>>::PendingNominationRequestDNE)?;
        let request = &scheduled_requests[request_idx];

        let now = <Era<T>>::get().current;
        ensure!(request.when_executable <= now, <Error<T>>::PendingNominationRequestNotDueYet);

        match request.action {
            NominationAction::Revoke(amount) => {
                // revoking last nomination => leaving set of nominators
                let leaving = if state.nominations.0.len() == 1usize {
                    true
                } else {
                    ensure!(
                        state.total().saturating_sub(<MinTotalNominatorStake<T>>::get().into()) >=
                            amount,
                        <Error<T>>::NominatorBondBelowMin
                    );
                    false
                };

                // remove from pending requests
                let amount = scheduled_requests.remove(request_idx).action.amount();
                state.less_total = state.less_total.saturating_sub(amount);

                // remove nomination from nominator state
                state.rm_nomination::<T>(&collator);

                // remove nomination from collator state nominations
                Self::nominator_leaves_candidate(collator.clone(), nominator.clone(), amount)?;
                Self::deposit_event(Event::NominationRevoked {
                    nominator: nominator.clone(),
                    candidate: collator.clone(),
                    unstaked_amount: amount,
                });

                <NominationScheduledRequests<T>>::insert(collator, scheduled_requests);
                if leaving {
                    <NominatorState<T>>::remove(&nominator);
                    Self::deposit_event(Event::NominatorLeft {
                        nominator,
                        unstaked_amount: amount,
                    });
                } else {
                    <NominatorState<T>>::insert(&nominator, state);
                }
                Ok(().into())
            },
            NominationAction::Decrease(_) => {
                // remove from pending requests
                let amount = scheduled_requests.remove(request_idx).action.amount();
                state.less_total = state.less_total.saturating_sub(amount);

                // decrease nomination
                for bond in &mut state.nominations.0 {
                    if bond.owner == collator {
                        return if bond.amount > amount {
                            let amount_before: BalanceOf<T> = bond.amount.into();
                            bond.amount = bond.amount.saturating_sub(amount);
                            let mut collator_info = <CandidateInfo<T>>::get(&collator)
                                .ok_or(<Error<T>>::CandidateDNE)?;

                            state.total_sub_if::<T, _>(amount, |total| {
                                let new_total: BalanceOf<T> = total.into();
                                ensure!(
                                    new_total >= T::MinNominationPerCollator::get(),
                                    <Error<T>>::NominationBelowMin
                                );
                                ensure!(
                                    new_total >= <MinTotalNominatorStake<T>>::get(),
                                    <Error<T>>::NominatorBondBelowMin
                                );

                                Ok(())
                            })?;

                            // need to go into decrease_nomination
                            let in_top = collator_info.decrease_nomination::<T>(
                                &collator,
                                nominator.clone(),
                                amount_before,
                                amount,
                            )?;
                            <CandidateInfo<T>>::insert(&collator, collator_info);
                            let new_total_staked = <Total<T>>::get().saturating_sub(amount);
                            <Total<T>>::put(new_total_staked);

                            <NominationScheduledRequests<T>>::insert(
                                collator.clone(),
                                scheduled_requests,
                            );
                            <NominatorState<T>>::insert(nominator.clone(), state);
                            Self::deposit_event(Event::NominationDecreased {
                                nominator,
                                candidate: collator.clone(),
                                amount,
                                in_top,
                            });
                            Ok(().into())
                        } else {
                            // must rm entire nomination if bond.amount <= less or cancel request
                            Err(<Error<T>>::NominationBelowMin.into())
                        }
                    }
                }
                Err(<Error<T>>::NominationDNE.into())
            },
        }
    }

    /// Schedules [NominationAction::Revoke] for the nominator, towards all nominated collator.
    /// The last fulfilled request causes the nominator to leave the set of nominators.
    pub(crate) fn nominator_schedule_revoke_all(
        nominator: T::AccountId,
    ) -> DispatchResultWithPostInfo {
        let mut state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
        let mut updated_scheduled_requests = vec![];
        let now = <Era<T>>::get().current;
        let when = now.saturating_add(<Delay<T>>::get());

        // it is assumed that a multiple nominations to the same collator does not exist, else this
        // will cause a bug - the last duplicate nomination update will be the only one applied.
        let mut existing_revoke_count = 0;
        for bond in state.nominations.0.clone() {
            let collator = bond.owner;
            let bonded_amount = bond.amount;
            let mut scheduled_requests = <NominationScheduledRequests<T>>::get(&collator);

            // cancel any existing requests
            let request =
                Self::cancel_request_with_state(&nominator, &mut state, &mut scheduled_requests);
            let request = match request {
                Some(revoke_req) if matches!(revoke_req.action, NominationAction::Revoke(_)) => {
                    existing_revoke_count += 1;
                    revoke_req // re-insert the same Revoke request
                },
                _ => ScheduledRequest {
                    nominator: nominator.clone(),
                    action: NominationAction::Revoke(bonded_amount.clone()),
                    when_executable: when,
                },
            };

            match scheduled_requests.try_push(request) {
                Ok(()) => {
                    state.less_total = state.less_total.saturating_add(bonded_amount);
                    updated_scheduled_requests.push((collator, scheduled_requests));
                },
                Err(_) => (),
            }
        }

        if existing_revoke_count == state.nominations.0.len() {
            return Err(<Error<T>>::NominatorAlreadyLeaving.into())
        }

        updated_scheduled_requests
            .into_iter()
            .for_each(|(collator, scheduled_requests)| {
                <NominationScheduledRequests<T>>::insert(collator, scheduled_requests);
            });

        <NominatorState<T>>::insert(nominator.clone(), state);
        Self::deposit_event(Event::NominatorExitScheduled {
            era: now,
            nominator,
            scheduled_exit: when,
        });
        Ok(().into())
    }

    /// Cancels every [NominationAction::Revoke] request for a nominator towards a collator.
    /// Each nomination must have a [NominationAction::Revoke] scheduled that must be allowed to be
    /// executed in the current era, for this function to succeed.
    pub(crate) fn nominator_cancel_scheduled_revoke_all(
        nominator: T::AccountId,
    ) -> DispatchResultWithPostInfo {
        let mut state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
        let mut updated_scheduled_requests = vec![];

        // pre-validate that all nominations have a Revoke request.
        for bond in &state.nominations.0 {
            let collator = bond.owner.clone();
            let scheduled_requests = <NominationScheduledRequests<T>>::get(&collator);
            scheduled_requests
                .iter()
                .find(|req| {
                    req.nominator == nominator && matches!(req.action, NominationAction::Revoke(_))
                })
                .ok_or(<Error<T>>::NominatorNotLeaving)?;
        }

        // cancel all requests
        for bond in state.nominations.0.clone() {
            let collator = bond.owner.clone();
            let mut scheduled_requests = <NominationScheduledRequests<T>>::get(&collator);
            Self::cancel_request_with_state(&nominator, &mut state, &mut scheduled_requests);
            updated_scheduled_requests.push((collator, scheduled_requests));
        }

        updated_scheduled_requests
            .into_iter()
            .for_each(|(collator, scheduled_requests)| {
                <NominationScheduledRequests<T>>::insert(collator, scheduled_requests);
            });

        <NominatorState<T>>::insert(nominator.clone(), state);
        Self::deposit_event(Event::NominatorExitCancelled { nominator });

        Ok(().into())
    }

    /// Executes every [NominationAction::Revoke] request for a nominator towards a collator.
    /// Each nomination must have a [NominationAction::Revoke] scheduled that must be allowed to be
    /// executed in the current era, for this function to succeed.
    pub(crate) fn nominator_execute_scheduled_revoke_all(
        nominator: T::AccountId,
        nomination_count: u32,
    ) -> DispatchResultWithPostInfo {
        let mut state = <NominatorState<T>>::get(&nominator).ok_or(<Error<T>>::NominatorDNE)?;
        ensure!(
            nomination_count >= (state.nominations.0.len() as u32),
            Error::<T>::TooLowNominationCountToLeaveNominators
        );
        let now = <Era<T>>::get().current;
        let mut validated_scheduled_requests = vec![];
        // pre-validate that all nominations have a Revoke request that can be executed now.
        for bond in &state.nominations.0 {
            let scheduled_requests = <NominationScheduledRequests<T>>::get(&bond.owner);
            let request_idx = scheduled_requests
                .iter()
                .position(|req| {
                    req.nominator == nominator && matches!(req.action, NominationAction::Revoke(_))
                })
                .ok_or(<Error<T>>::NominatorNotLeaving)?;
            let request = &scheduled_requests[request_idx];

            ensure!(request.when_executable <= now, <Error<T>>::NominatorCannotLeaveYet);

            validated_scheduled_requests.push((bond.clone(), scheduled_requests, request_idx))
        }

        let mut updated_scheduled_requests = vec![];
        // we do not update the nominator state, since the it will be completely removed
        for (bond, mut scheduled_requests, request_idx) in validated_scheduled_requests {
            let collator = bond.owner;

            if let Err(error) =
                Self::nominator_leaves_candidate(collator.clone(), nominator.clone(), bond.amount)
            {
                log::warn!(
                    "STORAGE CORRUPTED \nNominator {:?} leaving collator failed with error: {:?}",
                    nominator,
                    error
                );
            }

            // remove the scheduled request, since it is fulfilled
            scheduled_requests.remove(request_idx).action.amount();
            updated_scheduled_requests.push((collator, scheduled_requests));
        }

        // set state.total so that state.adjust_bond_lock will remove lock
        let unstaked_amount = state.total();
        state.total_sub::<T>(unstaked_amount)?;

        updated_scheduled_requests
            .into_iter()
            .for_each(|(collator, scheduled_requests)| {
                <NominationScheduledRequests<T>>::insert(collator, scheduled_requests);
            });

        Self::deposit_event(Event::NominatorLeft { nominator: nominator.clone(), unstaked_amount });
        <NominatorState<T>>::remove(&nominator);

        Ok(().into())
    }

    /// Removes the nominator's existing [ScheduledRequest] towards a given collator, if exists.
    /// The state needs to be persisted by the caller of this function.
    pub(crate) fn nomination_remove_request_with_state(
        collator: &T::AccountId,
        nominator: &T::AccountId,
        state: &mut Nominator<T::AccountId, BalanceOf<T>>,
    ) {
        let mut scheduled_requests = <NominationScheduledRequests<T>>::get(collator);

        let maybe_request_idx =
            scheduled_requests.iter().position(|req| &req.nominator == nominator);

        if let Some(request_idx) = maybe_request_idx {
            let request = scheduled_requests.remove(request_idx);
            let amount = request.action.amount();
            state.less_total = state.less_total.saturating_sub(amount);
            <NominationScheduledRequests<T>>::insert(collator, scheduled_requests);
        }
    }

    /// Returns true if a [ScheduledRequest] exists for a given nomination
    pub fn nomination_request_exists(collator: &T::AccountId, nominator: &T::AccountId) -> bool {
        <NominationScheduledRequests<T>>::get(collator)
            .iter()
            .any(|req| &req.nominator == nominator)
    }

    /// Returns true if a [NominationAction::Revoke] [ScheduledRequest] exists for a given
    /// nomination
    pub fn nomination_request_revoke_exists(
        collator: &T::AccountId,
        nominator: &T::AccountId,
    ) -> bool {
        <NominationScheduledRequests<T>>::get(collator).iter().any(|req| {
            &req.nominator == nominator && matches!(req.action, NominationAction::Revoke(_))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mock::{Test, TestAccount},
        set::BoundedOrderedSet,
        Bond,
    };

    #[test]
    fn test_cancel_request_with_state_removes_request_for_correct_nominator_and_updates_state() {
        let nominator1_account_id = TestAccount::new(1u64).account_id();
        let collator_account_id = TestAccount::new(2u64).account_id();

        let mut state = Nominator {
            id: nominator1_account_id,
            nominations: BoundedOrderedSet::from(BoundedVec::truncate_from(vec![Bond {
                amount: 100,
                owner: collator_account_id,
            }])),
            total: 100,
            less_total: 100,
        };
        let mut scheduled_requests = BoundedVec::truncate_from(vec![
            ScheduledRequest {
                nominator: nominator1_account_id,
                when_executable: 1,
                action: NominationAction::Revoke(100),
            },
            ScheduledRequest {
                nominator: collator_account_id,
                when_executable: 1,
                action: NominationAction::Decrease(50),
            },
        ]);
        let removed_request = <Pallet<Test>>::cancel_request_with_state(
            &nominator1_account_id,
            &mut state,
            &mut scheduled_requests,
        );

        assert_eq!(
            removed_request,
            Some(ScheduledRequest {
                nominator: nominator1_account_id,
                when_executable: 1,
                action: NominationAction::Revoke(100),
            })
        );
        assert_eq!(
            scheduled_requests,
            vec![ScheduledRequest {
                nominator: collator_account_id,
                when_executable: 1,
                action: NominationAction::Decrease(50),
            },]
        );
        assert_eq!(
            state,
            Nominator {
                id: nominator1_account_id,
                nominations: BoundedOrderedSet::from(BoundedVec::truncate_from(vec![Bond {
                    amount: 100,
                    owner: collator_account_id
                }])),
                total: 100,
                less_total: 0,
            }
        );
    }

    #[test]
    fn test_cancel_request_with_state_does_nothing_when_request_does_not_exist() {
        let nominator1_account_id = TestAccount::new(1u64).account_id();
        let collator_account_id = TestAccount::new(2u64).account_id();

        let mut state = Nominator {
            id: nominator1_account_id,
            nominations: BoundedOrderedSet::from(BoundedVec::truncate_from(vec![Bond {
                amount: 100,
                owner: collator_account_id,
            }])),
            total: 100,
            less_total: 100,
        };
        let mut scheduled_requests = BoundedVec::truncate_from(vec![ScheduledRequest {
            nominator: collator_account_id,
            when_executable: 1,
            action: NominationAction::Decrease(50),
        }]);
        let removed_request = <Pallet<Test>>::cancel_request_with_state(
            &nominator1_account_id,
            &mut state,
            &mut scheduled_requests,
        );

        assert_eq!(removed_request, None,);
        assert_eq!(
            scheduled_requests,
            vec![ScheduledRequest {
                nominator: collator_account_id,
                when_executable: 1,
                action: NominationAction::Decrease(50),
            },]
        );
        assert_eq!(
            state,
            Nominator {
                id: nominator1_account_id,
                nominations: BoundedOrderedSet::from(BoundedVec::truncate_from(vec![Bond {
                    amount: 100,
                    owner: collator_account_id
                }])),
                total: 100,
                less_total: 100,
            }
        );
    }
}
