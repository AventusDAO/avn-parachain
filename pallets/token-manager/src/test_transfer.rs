// This file is part of Aventus.
// Copyright 2026 Aventus DAO Ltd

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

//! Unit tests for the `transfer` extrinsic (call index 12).
//!
//! Covers three transfer paths:
//!   1. Unregistered token  — balance held in `TokenManager::Balances` storage map
//!   2. Native AVT          — registered in AssetRegistry; backed by `pallet_balances`
//!   3. Registered foreign  — registered in AssetRegistry; backed by `orml_tokens`
//!
//! Plus edge cases: zero-amount, self-transfer, insufficient balance, and
//! `NativeTokenNotRegistered` (AVT address used but not registered).

#![cfg(test)]
use crate::{
    mock::{Balances, RuntimeEvent, *},
    Balances as TokenManagerBalances, *,
};
use frame_support::{assert_noop, assert_ok};
use orml_traits::asset_registry::{AvnAssetLocation, AvnAssetMetadata};
use sp_avn_common::Asset;

/// A distinct H160 address used as the registered foreign token in these tests.
/// Must differ from AVT_TOKEN_CONTRACT and the existing NON_AVT_TOKEN_ID* constants.
const REGISTERED_FOREIGN_TOKEN_ID: H160 =
    H160(hex_literal::hex!("cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"));

/// The `Asset::ForeignAsset(1)` currency id we assign to `REGISTERED_FOREIGN_TOKEN_ID`.
const FOREIGN_ASSET_CURRENCY_ID: Asset = Asset::ForeignAsset(1);

/// Register `token_id` in the asset registry under `currency_id`.
/// Uses `do_register_asset` which is the same path exercised by the runtime migration.
fn register_foreign_token(token_id: H160, currency_id: Asset) {
    let metadata = orml_traits::asset_registry::AssetMetadata {
        decimals: 18,
        name: b"Foreign Test Token".to_vec().try_into().unwrap(),
        symbol: b"FTT".to_vec().try_into().unwrap(),
        existential_deposit: 1,
        location: Some(AvnAssetLocation::Ethereum(token_id)),
        additional: AvnAssetMetadata { appchain_native: false },
    };
    assert_ok!(orml_asset_registry::Pallet::<TestRuntime>::do_register_asset(
        metadata,
        Some(currency_id)
    ));
}

// Unregistered token  (TokenManager::Balances storage)

#[test]
fn transfer_unregistered_token_succeeds_and_emits_event() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_seed_item(1);
        let recipient = account_id_with_seed_item(2);
        let amount: u128 = 1_000_000;

        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID, sender), 2 * amount);

        assert_eq!(System::events().len(), 0);
        assert_ok!(TokenManager::transfer(
            RuntimeOrigin::signed(sender),
            recipient,
            NON_AVT_TOKEN_ID,
            amount,
        ));

        assert_eq!(TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)), amount);
        assert_eq!(TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient)), amount);
        // Other token balances must remain untouched
        assert_eq!(TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, sender)), 0);

        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenTransferred {
                token_id: NON_AVT_TOKEN_ID,
                sender,
                recipient,
                token_balance: amount,
            })));
    });
}

#[test]
fn transfer_unregistered_token_insufficient_balance_fails() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_seed_item(1);
        let recipient = account_id_with_seed_item(2);
        let amount: u128 = 1_000_000;

        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID, sender), amount - 1);

        assert_noop!(
            TokenManager::transfer(
                RuntimeOrigin::signed(sender),
                recipient,
                NON_AVT_TOKEN_ID,
                amount,
            ),
            Error::<TestRuntime>::InsufficientSenderBalance,
        );

        // No state change
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
            amount - 1
        );
        assert_eq!(TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient)), 0);
    });
}

#[test]
fn transfer_unregistered_token_with_zero_sender_balance_fails() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_seed_item(1);
        let recipient = account_id_with_seed_item(2);

        // sender has no balance at all (default is 0 via ValueQuery)
        assert_noop!(
            TokenManager::transfer(
                RuntimeOrigin::signed(sender),
                recipient,
                NON_AVT_TOKEN_ID,
                500,
            ),
            Error::<TestRuntime>::InsufficientSenderBalance,
        );
    });
}

// Native AVT  (registered in AssetRegistry, backed by pallet_balances)
#[test]
fn transfer_avt_succeeds_and_updates_native_balances() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_100_avt();
        let recipient = account_id2_with_100_avt();
        let amount = 10 * ONE_TOKEN;

        let sender_before = Balances::free_balance(sender);
        let recipient_before = Balances::free_balance(recipient);

        assert_ok!(TokenManager::transfer(
            RuntimeOrigin::signed(sender),
            recipient,
            AVT_TOKEN_CONTRACT,
            amount,
        ));

        assert_eq!(Balances::free_balance(sender), sender_before - amount);
        assert_eq!(Balances::free_balance(recipient), recipient_before + amount);
    });
}

#[test]
fn transfer_avt_insufficient_balance_fails() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_100_avt();
        let recipient = account_id2_with_100_avt();
        let amount = AMOUNT_100_TOKEN + ONE_TOKEN; // more than sender has

        let sender_before = Balances::free_balance(sender);

        assert_noop!(
            TokenManager::transfer(
                RuntimeOrigin::signed(sender),
                recipient,
                AVT_TOKEN_CONTRACT,
                amount,
            ),
            sp_runtime::TokenError::FundsUnavailable,
        );

        // Balance unchanged
        assert_eq!(Balances::free_balance(sender), sender_before);
    });
}

#[test]
fn transfer_avt_when_not_registered_in_asset_registry_fails() {
    // build_default() does NOT call with_genesis_config(), so AVT is not in AssetRegistry.
    // We manually set the AVT contract address so is_native_token() returns true,
    // which triggers NativeTokenNotRegistered.
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_seed_item(1);
        let recipient = account_id_with_seed_item(2);

        AVTTokenContract::<TestRuntime>::put(AVT_TOKEN_CONTRACT);

        assert_noop!(
            TokenManager::transfer(
                RuntimeOrigin::signed(sender),
                recipient,
                AVT_TOKEN_CONTRACT,
                ONE_TOKEN,
            ),
            Error::<TestRuntime>::NativeTokenNotRegistered,
        );
    });
}

// Registered foreign token  (ORML / orml_tokens)
#[test]
fn transfer_registered_foreign_token_succeeds_and_updates_orml_balances() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_seed_item(1);
        let recipient = account_id_with_seed_item(2);
        let amount: u128 = 500_000;

        register_foreign_token(REGISTERED_FOREIGN_TOKEN_ID, FOREIGN_ASSET_CURRENCY_ID);

        // Fund sender via credit_user_balance (goes through AssetManager::deposit)
        assert_ok!(TokenManager::credit_user_balance(
            REGISTERED_FOREIGN_TOKEN_ID,
            &sender,
            amount * 2,
        ));

        let sender_before =
            orml_tokens::Accounts::<TestRuntime>::get(sender, FOREIGN_ASSET_CURRENCY_ID).free;
        let recipient_before =
            orml_tokens::Accounts::<TestRuntime>::get(recipient, FOREIGN_ASSET_CURRENCY_ID).free;

        assert_ok!(TokenManager::transfer(
            RuntimeOrigin::signed(sender),
            recipient,
            REGISTERED_FOREIGN_TOKEN_ID,
            amount,
        ));

        assert_eq!(
            orml_tokens::Accounts::<TestRuntime>::get(sender, FOREIGN_ASSET_CURRENCY_ID).free,
            sender_before - amount
        );
        assert_eq!(
            orml_tokens::Accounts::<TestRuntime>::get(recipient, FOREIGN_ASSET_CURRENCY_ID).free,
            recipient_before + amount
        );
    });
}

#[test]
fn transfer_registered_foreign_token_does_not_emit_custom_token_transferred_event() {
    // ORML emits its own Transferred event; the pallet must NOT also emit TokenTransferred.
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_seed_item(1);
        let recipient = account_id_with_seed_item(2);
        let amount: u128 = 100_000;

        register_foreign_token(REGISTERED_FOREIGN_TOKEN_ID, FOREIGN_ASSET_CURRENCY_ID);
        assert_ok!(TokenManager::credit_user_balance(
            REGISTERED_FOREIGN_TOKEN_ID,
            &sender,
            amount * 2,
        ));

        assert_ok!(TokenManager::transfer(
            RuntimeOrigin::signed(sender),
            recipient,
            REGISTERED_FOREIGN_TOKEN_ID,
            amount,
        ));

        assert!(!System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenTransferred {
                token_id: REGISTERED_FOREIGN_TOKEN_ID,
                sender,
                recipient,
                token_balance: amount,
            })));
    });
}

#[test]
fn transfer_registered_foreign_token_insufficient_balance_fails() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_seed_item(1);
        let recipient = account_id_with_seed_item(2);
        let amount: u128 = 500_000;

        register_foreign_token(REGISTERED_FOREIGN_TOKEN_ID, FOREIGN_ASSET_CURRENCY_ID);
        // Sender not funded — zero balance

        assert_noop!(
            TokenManager::transfer(
                RuntimeOrigin::signed(sender),
                recipient,
                REGISTERED_FOREIGN_TOKEN_ID,
                amount,
            ),
            orml_tokens::Error::<TestRuntime>::BalanceTooLow,
        );
    });
}

// Edge cases: zero amount and self-transfer
#[test]
fn transfer_zero_amount_for_unregistered_token_is_noop_and_succeeds() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_seed_item(1);
        let recipient = account_id_with_seed_item(2);
        let initial_balance: u128 = 1_000_000;

        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID, sender), initial_balance);

        assert_ok!(TokenManager::transfer(
            RuntimeOrigin::signed(sender),
            recipient,
            NON_AVT_TOKEN_ID,
            0,
        ));

        // No state change
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
            initial_balance
        );
        assert_eq!(TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient)), 0);
        // No TokenTransferred event
        assert!(!System::events().iter().any(|a| matches!(
            &a.event,
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenTransferred { .. })
        )));
    });
}

#[test]
fn transfer_zero_amount_for_avt_is_noop_and_succeeds() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let sender = account_id_with_100_avt();
        let recipient = account_id2_with_100_avt();

        let sender_before = Balances::free_balance(sender);
        let recipient_before = Balances::free_balance(recipient);

        assert_ok!(TokenManager::transfer(
            RuntimeOrigin::signed(sender),
            recipient,
            AVT_TOKEN_CONTRACT,
            0,
        ));

        assert_eq!(Balances::free_balance(sender), sender_before);
        assert_eq!(Balances::free_balance(recipient), recipient_before);
    });
}

#[test]
fn transfer_to_self_for_unregistered_token_is_noop_and_succeeds() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let account = account_id_with_seed_item(1);
        let initial_balance: u128 = 1_000_000;

        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID, account), initial_balance);

        assert_ok!(TokenManager::transfer(
            RuntimeOrigin::signed(account),
            account, // same as sender
            NON_AVT_TOKEN_ID,
            initial_balance,
        ));

        // Balance unchanged
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, account)),
            initial_balance
        );
        assert!(!System::events().iter().any(|a| matches!(
            &a.event,
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenTransferred { .. })
        )));
    });
}

#[test]
fn transfer_to_self_for_avt_is_noop_and_succeeds() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let account = account_id_with_100_avt();
        let balance_before = Balances::free_balance(account);

        assert_ok!(TokenManager::transfer(
            RuntimeOrigin::signed(account),
            account, // same as sender
            AVT_TOKEN_CONTRACT,
            50 * ONE_TOKEN,
        ));

        assert_eq!(Balances::free_balance(account), balance_before);
    });
}

// Origin guard
#[test]
fn transfer_requires_signed_origin() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let recipient = account_id_with_seed_item(2);

        assert_noop!(
            TokenManager::transfer(RuntimeOrigin::none(), recipient, NON_AVT_TOKEN_ID, 1_000_000,),
            sp_runtime::traits::BadOrigin,
        );

        assert_noop!(
            TokenManager::transfer(RuntimeOrigin::root(), recipient, NON_AVT_TOKEN_ID, 1_000_000,),
            sp_runtime::traits::BadOrigin,
        );
    });
}
