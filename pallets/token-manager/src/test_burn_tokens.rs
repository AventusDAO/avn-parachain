// This file is part of Aventus.
// Copyright (C) 2022 Aventus Network Services (UK) Ltd.

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![cfg(test)]
use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};

type Curr = <TestRuntime as crate::Config>::Currency;

fn any_burn_funds_requested_event() -> bool {
    frame_system::Pallet::<TestRuntime>::events().iter().any(|r| {
        matches!(
            r.event,
            mock::RuntimeEvent::TokenManager(
                crate::Event::<TestRuntime>::BurnFundsRequested { .. }
            )
        )
    })
}

fn last_pending_burn_submission() -> Option<(u32, AccountId, u128)> {
    PendingBurnSubmission::<TestRuntime>::iter()
        .max_by_key(|(tx_id, _)| *tx_id)
        .map(|(tx_id, (burner, amount))| (tx_id, burner, amount))
}

fn reserved_of(who: &<TestRuntime as frame_system::Config>::AccountId) -> u128 {
    Curr::reserved_balance(who)
}

mod burn_tests {
    use super::*;

    mod set_burn_period {
        use super::*;

        mod succeeds_when {
            use super::*;

            #[test]
            fn origin_is_sudo() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let current_block: u64 = 1;
                    frame_system::Pallet::<TestRuntime>::set_block_number(current_block);

                    let new_period: u32 = 8000;
                    assert_ok!(TokenManager::set_burn_period(RuntimeOrigin::root(), new_period,));

                    // storage updated correctly
                    assert_eq!(BurnPeriod::<TestRuntime>::get(), new_period);

                    // event emitted
                    assert!(event_emitted(&mock::RuntimeEvent::TokenManager(crate::Event::<
                        TestRuntime,
                    >::BurnPeriodUpdated {
                        burn_period: new_period
                    })));
                });
            }
        }

        mod fails_when {
            use super::*;

            #[test]
            fn origin_is_not_sudo() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let current_block: u64 = 1;
                    frame_system::Pallet::<TestRuntime>::set_block_number(current_block);

                    let new_period: u32 = 8000;
                    assert_noop!(
                        TokenManager::set_burn_period(
                            RuntimeOrigin::signed(account_id_with_seed_item(1)),
                            new_period,
                        ),
                        sp_runtime::DispatchError::BadOrigin,
                    );
                });
            }

            #[test]
            fn burn_period_is_below_minimum() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let current_block: u64 = 1;
                    frame_system::Pallet::<TestRuntime>::set_block_number(current_block);

                    let min_burn_period = <TestRuntime as crate::Config>::MinBurnPeriod::get();
                    let invalid_period = min_burn_period.saturating_sub(1);
                    assert_noop!(
                        TokenManager::set_burn_period(RuntimeOrigin::root(), invalid_period,),
                        Error::<TestRuntime>::InvalidBurnPeriod,
                    );
                })
            }
        }
    }

    mod on_initialize {
        use super::*;

        mod succeeds_when {
            use super::*;

            #[test]
            fn burn_is_due_and_burn_pot_is_not_empty() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let current_block: u64 = 1;
                    frame_system::Pallet::<TestRuntime>::set_block_number(current_block);

                    // Burn is due at this block
                    NextBurnAt::<TestRuntime>::put(current_block);

                    // Put funds into burn pot
                    let burn_pot = TokenManager::burn_pot_account();
                    let amount = 1_000u128;
                    pallet_balances::Pallet::<TestRuntime>::make_free_balance_be(&burn_pot, amount);

                    // call hook
                    TokenManager::on_initialize(current_block);

                    let (tx_id, _burner, stored_amount) =
                        last_pending_burn_submission().expect("PendingBurnSubmission should exist");
                    assert_eq!(stored_amount, amount);

                    assert!(event_emitted(&mock::RuntimeEvent::TokenManager(crate::Event::<
                        TestRuntime,
                    >::BurnFundsRequested {
                        burner: burn_pot,
                        amount,
                        tx_id,
                    })));
                });
            }
        }

        mod fails_when {
            use super::*;

            #[test]
            fn burn_is_not_due() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let current_block: u64 = 1;
                    frame_system::Pallet::<TestRuntime>::set_block_number(current_block);

                    // Not due yet
                    NextBurnAt::<TestRuntime>::put(current_block + 10);

                    // Put funds into burn pot
                    let burn_pot = TokenManager::burn_pot_account();
                    let amount = 1_000u128;
                    pallet_balances::Pallet::<TestRuntime>::make_free_balance_be(&burn_pot, amount);

                    // call hook
                    TokenManager::on_initialize(current_block);

                    // event NOT emitted
                    assert!(last_pending_burn_submission().is_none());
                    assert!(!any_burn_funds_requested_event());
                });
            }

            #[test]
            fn burn_is_due_but_burn_pot_is_empty() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let current_block: u64 = 1;
                    frame_system::Pallet::<TestRuntime>::set_block_number(current_block);

                    // Due
                    NextBurnAt::<TestRuntime>::put(current_block);

                    // Empty burn pot
                    let burn_pot = TokenManager::burn_pot_account();
                    let amount = 0u128;
                    pallet_balances::Pallet::<TestRuntime>::make_free_balance_be(&burn_pot, amount);

                    // call hook
                    TokenManager::on_initialize(current_block);

                    // event NOT emitted (because amount is zero)
                    assert!(last_pending_burn_submission().is_none());
                    assert!(!any_burn_funds_requested_event());
                });
            }
        }
    }

    mod burn_funds {
        use super::*;

        mod succeeds_when {
            use super::*;

            #[test]
            fn caller_has_enough_funds_and_publish_succeeds() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let burner = account_id_with_100_avt();

                    let free_before = Curr::free_balance(&burner);
                    let reserved_before = reserved_of(&burner);

                    let amount: u128 = 1_000u128;

                    assert_ok!(TokenManager::burn_funds(
                        RuntimeOrigin::signed(burner.clone()),
                        amount
                    ));

                    // Reserved increased (funds locked until ETH confirmation)
                    assert_eq!(reserved_of(&burner), reserved_before + amount);
                    // Free reduced accordingly
                    assert_eq!(Curr::free_balance(&burner), free_before - amount);

                    // Pending burn submission exists (tx_id comes from publish mock)
                    let (tx_id, stored_burner, stored_amount) =
                        last_pending_burn_submission().expect("PendingBurnSubmission should exist");
                    assert_eq!(stored_burner, burner);
                    assert_eq!(stored_amount, amount);

                    // Event emitted
                    assert!(event_emitted(&mock::RuntimeEvent::TokenManager(crate::Event::<
                        TestRuntime,
                    >::BurnFundsRequested {
                        burner,
                        amount,
                        tx_id,
                    })));
                });
            }
        }

        mod fails_when {
            use super::*;

            #[test]
            fn amount_is_zero() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let burner = account_id_with_100_avt();

                    assert_noop!(
                        TokenManager::burn_funds(RuntimeOrigin::signed(burner), 0u128),
                        Error::<TestRuntime>::AmountIsZero
                    );
                });
            }

            #[test]
            fn reserve_fails_due_to_insufficient_balance() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    // Use some random account with 0 balance
                    let burner = account_id_with_seed_item(55);

                    // Try reserving something non-zero
                    let amount: u128 = 1_000u128;

                    assert_noop!(
                        TokenManager::burn_funds(RuntimeOrigin::signed(burner), amount),
                        Error::<TestRuntime>::InsufficientSenderBalance
                    );
                });
            }
        }
    }
}
