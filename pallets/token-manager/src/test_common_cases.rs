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

#![cfg(test)]
use crate::{
    mock::{Balances, RuntimeEvent, *},
    Balances as TokenManagerBalances, *,
};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::traits::BadOrigin;

const USE_RECEIVER_WITH_0_AMOUNT: bool = false;

#[test]
fn avn_test_lift_ignored_when_event_type_does_not_match() {
    let mut ext = ExtBuilder::build_default().with_balances().as_externality();

    ext.execute_with(|| {
        let mock_data = MockData::setup(AMOUNT_123_TOKEN, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.empty_data_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), 0);
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            0
        );

        assert_ok!(TokenManager::on_event_processed(&mock_data.empty_data_lift_event));

        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), 0);
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            0
        );

        assert!(!System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTLifted {
                recipient: mock_data.receiver_account_id,
                amount: AMOUNT_123_TOKEN,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
        assert!(!System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLifted {
                token_id: NON_AVT_TOKEN_ID,
                recipient: mock_data.receiver_account_id,
                token_balance: mock_data.token_balance_123_tokens,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lift_zero_amount_should_fail() {
    let mut ext = ExtBuilder::build_default().with_balances().as_externality();

    ext.execute_with(|| {
        let zero_amount = 0;
        let mock_data = MockData::setup(zero_amount, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        let avt_token_balance_before = Balances::free_balance(mock_data.receiver_account_id);
        let non_avt_token_balance_before = TokenManagerBalances::<TestRuntime>::get((
            NON_AVT_TOKEN_ID,
            mock_data.receiver_account_id,
        ));

        assert_noop!(
            TokenManager::on_event_processed(&mock_event),
            Error::<TestRuntime>::AmountIsZero
        );
        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), avt_token_balance_before);
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            non_avt_token_balance_before
        );

        let token_balance_zero_tokens = MockData::get_token_balance(zero_amount);
        assert!(!System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTLifted {
                recipient: mock_data.receiver_account_id,
                amount: zero_amount,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
        assert!(!System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLifted {
                token_id: NON_AVT_TOKEN_ID,
                recipient: mock_data.receiver_account_id,
                token_balance: token_balance_zero_tokens,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lift_should_fail_when_event_is_not_in_processed_events() {
    let mut ext = ExtBuilder::build_default().with_balances().as_externality();

    ext.execute_with(|| {
        let mock_data = MockData::setup(AMOUNT_123_TOKEN, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.avt_token_lift_event;

        let avt_token_balance_before = Balances::free_balance(mock_data.receiver_account_id);
        let non_avt_token_balance_before = TokenManagerBalances::<TestRuntime>::get((
            NON_AVT_TOKEN_ID,
            mock_data.receiver_account_id,
        ));

        assert_noop!(
            TokenManager::on_event_processed(&mock_event),
            Error::<TestRuntime>::NoTier1EventForLogLifted
        );
        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), avt_token_balance_before);
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            non_avt_token_balance_before
        );

        assert!(!System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTLifted {
                recipient: mock_data.receiver_account_id,
                amount: AMOUNT_123_TOKEN,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
        assert!(!System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLifted {
                token_id: NON_AVT_TOKEN_ID,
                recipient: mock_data.receiver_account_id,
                token_balance: mock_data.token_balance_123_tokens,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lower_should_fail_when_origin_is_not_signed() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, _, t1_recipient) = MockData::setup_lower_request_data();

        assert_noop!(
            TokenManager::schedule_direct_lower(
                RawOrigin::None.into(),
                from_account_id,
                NON_AVT_TOKEN_ID,
                100,
                t1_recipient
            ),
            BadOrigin
        );
    });
}

#[test]
fn avn_test_lower_should_fail_when_sender_does_not_own_from_account() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, _, t1_recipient) = MockData::setup_lower_request_data();
        let sender = account_id_with_seed_item(70u8);

        assert_noop!(
            TokenManager::schedule_direct_lower(
                RuntimeOrigin::signed(sender),
                from_account_id,
                NON_AVT_TOKEN_ID,
                100,
                t1_recipient
            ),
            Error::<TestRuntime>::SenderNotValid
        );
    });
}

#[test]
fn avn_test_lower_should_fail_when_amount_is_zero() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, _, t1_recipient) = MockData::setup_lower_request_data();

        assert_noop!(
            TokenManager::schedule_direct_lower(
                RuntimeOrigin::signed(from_account_id),
                from_account_id,
                NON_AVT_TOKEN_ID,
                0,
                t1_recipient
            ),
            Error::<TestRuntime>::AmountIsZero
        );
    });
}

mod on_idle {
    use super::*;
    use frame_support::traits::OnIdle;

    #[test]
    fn avn_test_on_idle_should_run() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

        ext.execute_with(|| {
            set_on_idle_run(false);
            assert!(!on_idle_has_run());

            TokenManager::on_idle(System::block_number(), 1_000_000_000.into());

            assert!(on_idle_has_run());
        });
    }
}
