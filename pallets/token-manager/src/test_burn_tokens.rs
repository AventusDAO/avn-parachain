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
use crate as token_manager;
use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
type Curr = <TestRuntime as token_manager::Config>::Currency;

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

mod treasury_tests {
    use super::*;
    use crate::treasury::TreasuryManager;

    mod set_treasury_burn_threshold {
        use super::*;

        mod succeeds_when {
            use super::*;

            #[test]
            fn fund_treasury_below_threshold_does_not_move_to_burn_pot() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    // TotalSupply=10_000 => threshold=1_500 (if TreasuryBurnThreshold is set at 15%)
                    let total_supply = 10_000u128;
                    TotalSupply::<TestRuntime>::put(total_supply);

                    let treasury = TokenManager::compute_treasury_account_id();
                    let burn = TokenManager::burn_pot_account();

                    // Read the threshold % from runtime config
                    let threshold =
                        <TestRuntime as token_manager::Config>::TreasuryBurnThreshold::get() * total_supply;
                    // pick an amount strictly below threshold (but >0)
                    let fund_amount = threshold.saturating_sub(1);

                    assert_eq!(Curr::free_balance(&treasury), 0u128);
                    assert_eq!(Curr::free_balance(&burn), 0u128);

                    let from = account_id_with_100_avt();

                    <crate::pallet::Pallet<TestRuntime> as TreasuryManager<TestRuntime>>::fund_treasury(from.clone(), fund_amount)
                        .unwrap();

                    assert_eq!(Curr::free_balance(&treasury), fund_amount);
                    assert_eq!(Curr::free_balance(&burn), 0u128);
                });
            }

            #[test]
            fn fund_treasury_above_threshold_moves_excess_to_burn_pot() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let total_supply = 10_000u128;
                    TotalSupply::<TestRuntime>::put(total_supply);

                    let treasury = TokenManager::compute_treasury_account_id();
                    let burn = TokenManager::burn_pot_account();

                    let threshold =
                        <TestRuntime as token_manager::Config>::TreasuryBurnThreshold::get() * total_supply;

                    let from = account_id_with_100_avt();

                    // Make treasury exceed threshold by a known amount
                    let excess = 500u128;
                    let fund_amount = threshold.saturating_add(excess);

                    <crate::pallet::Pallet<TestRuntime> as TreasuryManager<TestRuntime>>::fund_treasury(from.clone(),
                        fund_amount,
                    )
                    .unwrap();

                    // Treasury should end up capped at threshold, excess moved to burn pot
                    assert_eq!(Curr::free_balance(&treasury), threshold);
                    assert_eq!(Curr::free_balance(&burn), excess);
                });
            }

            #[test]
            fn fund_treasury_multiple_times_caps_treasury_and_accumulates_burn_pot() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    let total_supply = 10_000u128;
                    TotalSupply::<TestRuntime>::put(total_supply);

                    let treasury = TokenManager::compute_treasury_account_id();
                    let burn = TokenManager::burn_pot_account();

                    let threshold =
                        <TestRuntime as token_manager::Config>::TreasuryBurnThreshold::get() * total_supply;

                    let from = account_id_with_100_avt();

                    // 1) Fund just below threshold (no burn)
                    let first = threshold.saturating_sub(10);
                    <crate::pallet::Pallet<TestRuntime> as TreasuryManager<TestRuntime>>::fund_treasury(from.clone(),
                        first,
                    )
                    .unwrap();
                    assert_eq!(Curr::free_balance(&treasury), first);
                    assert_eq!(Curr::free_balance(&burn), 0u128);

                    // 2) Fund +50 => now 40 over threshold => 40 should move
                    let second = 50u128;
                    <crate::pallet::Pallet<TestRuntime> as TreasuryManager<TestRuntime>>::fund_treasury(from.clone(),
                        second,
                    )
                    .unwrap();

                    assert_eq!(Curr::free_balance(&treasury), threshold);
                    assert_eq!(Curr::free_balance(&burn), 40u128);

                    // 3) Fund +100 => treasury already at threshold, so all 100 is excess => all moves
                    let third = 100u128;
                    <crate::pallet::Pallet<TestRuntime> as TreasuryManager<TestRuntime>>::fund_treasury(from.clone(),
                        third,
                    )
                    .unwrap();

                    assert_eq!(Curr::free_balance(&treasury), threshold);
                    assert_eq!(Curr::free_balance(&burn), 140u128);
                });
            }

            #[test]
            fn fund_treasury_above_threshold_only_sweeps_up_to_burn_cap() {
                let mut ext = ExtBuilder::build_default()
                    .with_genesis_config()
                    .with_balances()
                    .as_externality();

                ext.execute_with(|| {
                    // In tests BalanceOf<TestRuntime> is u128, so this is fine.
                    let total_supply: BalanceOf<TestRuntime> = 10_000u128.into();
                    TotalSupply::<TestRuntime>::put(total_supply);

                    let treasury = TokenManager::compute_treasury_account_id();
                    let burn = TokenManager::burn_pot_account();

                    let threshold: BalanceOf<TestRuntime> =
                        <TestRuntime as token_manager::Config>::TreasuryBurnThreshold::get() * total_supply;

                    let cap: BalanceOf<TestRuntime> = <TestRuntime as token_manager::Config>::TreasuryBurnCap::get();
                    assert!(cap > 0u128.into());

                    // Make treasury exceed threshold by more than the cap
                    let extra_over_cap: BalanceOf<TestRuntime> = 123u128.into();
                    let excess: BalanceOf<TestRuntime> = cap.saturating_add(extra_over_cap);
                    let fund_amount: BalanceOf<TestRuntime> = threshold.saturating_add(excess);

                    let from = account_id_with_100_avt();

                    <crate::pallet::Pallet<TestRuntime> as TreasuryManager<TestRuntime>>::fund_treasury(
                        from.clone(),
                        fund_amount,
                    )
                    .unwrap();

                    // Only `cap` should be swept
                    assert_eq!(Curr::free_balance(&burn), cap);

                    // Treasury remains above threshold by (excess - cap) == extra_over_cap
                    assert_eq!(Curr::free_balance(&treasury), threshold.saturating_add(extra_over_cap));
                });
            }
        }
    }
}
