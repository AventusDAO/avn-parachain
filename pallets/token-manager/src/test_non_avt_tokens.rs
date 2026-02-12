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
    mock::{RuntimeEvent, *},
    Balances as TokenManagerBalances, *,
};
use frame_support::{assert_err, assert_noop, assert_ok};

const USE_RECEIVER_WITH_EXISTING_AMOUNT: bool = true;
const USE_RECEIVER_WITH_0_AMOUNT: bool = false;

fn schedule_lower_token(
    from: AccountId,
    amount: u128,
    t1_recipient: H160,
    expected_lower_id: u32,
    burn_acc: AccountId,
) {
    assert_ok!(TokenManager::schedule_direct_lower(
        RuntimeOrigin::signed(from),
        from,
        NON_AVT_TOKEN_ID,
        amount,
        t1_recipient
    ));

    fast_forward_to_block(get_expected_execution_block());

    assert!(System::events().iter().any(|a| a.event
        == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
            token_id: NON_AVT_TOKEN_ID,
            sender: from,
            recipient: burn_acc,
            amount,
            t1_recipient,
            lower_id: expected_lower_id
        })));
}

fn perform_lower_setup_token(lower_id: u32) {
    let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
    let amount = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));

    schedule_lower_token(from, amount, t1_recipient, lower_id, burn_acc);
    assert!(LowersPendingProof::<TestRuntime>::get(lower_id).is_some());

    let proof: Vec<u8> = b"lowerProofReady".to_vec();
    assert_ok!(TokenManager::process_lower_proof_result(
        lower_id,
        PALLET_ID.to_vec(),
        Ok(proof.clone())
    ));

    assert_eq!(LowersReadyToClaim::<TestRuntime>::get(lower_id).unwrap().encoded_lower_data, proof);
}

#[test]
fn avn_test_lift_to_zero_balance_account_should_succeed() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let mock_data = MockData::setup(AMOUNT_123_TOKEN, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.non_avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            0
        );
        assert_ok!(TokenManager::on_event_processed(&mock_event));
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            mock_data.token_balance_123_tokens
        );

        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLifted {
                token_id: NON_AVT_TOKEN_ID,
                recipient: mock_data.receiver_account_id,
                token_balance: mock_data.token_balance_123_tokens,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lift_to_non_zero_balance_account_should_succeed() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let mock_data = MockData::setup(AMOUNT_123_TOKEN, USE_RECEIVER_WITH_EXISTING_AMOUNT);
        let mock_event = &mock_data.non_avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        let token_balance_before = TokenManagerBalances::<TestRuntime>::get((
            NON_AVT_TOKEN_ID,
            mock_data.receiver_account_id,
        ));
        assert_eq!(token_balance_before, AMOUNT_100_TOKEN);
        let expected_token_balance = token_balance_before + AMOUNT_123_TOKEN;
        assert_ok!(TokenManager::on_event_processed(&mock_event));
        let new_token_balance = TokenManagerBalances::<TestRuntime>::get((
            NON_AVT_TOKEN_ID,
            mock_data.receiver_account_id,
        ));
        assert_eq!(new_token_balance, expected_token_balance);

        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLifted {
                token_id: NON_AVT_TOKEN_ID,
                recipient: mock_data.receiver_account_id,
                token_balance: mock_data.token_balance_123_tokens,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lift_max_balance_to_zero_balance_account_should_succeed() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let u128_max_amount: u128 = u128::max_value();
        let mock_data = MockData::setup(u128_max_amount, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.non_avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            0
        );
        assert_ok!(TokenManager::on_event_processed(&mock_event));
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            u128_max_amount
        );

        let token_balance_u128_max_amount = MockData::get_token_balance(u128_max_amount);
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLifted {
                token_id: NON_AVT_TOKEN_ID,
                recipient: mock_data.receiver_account_id,
                token_balance: token_balance_u128_max_amount,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lift_max_balance_to_non_zero_balance_account_should_fail_with_overflow() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let u128_max_amount = u128::max_value();
        let mock_data = MockData::setup(u128_max_amount, USE_RECEIVER_WITH_EXISTING_AMOUNT);
        let mock_event = &mock_data.non_avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);
        let token_balance_before = TokenManagerBalances::<TestRuntime>::get((
            NON_AVT_TOKEN_ID,
            mock_data.receiver_account_id,
        ));

        assert_noop!(
            TokenManager::on_event_processed(&mock_event),
            Error::<TestRuntime>::AmountOverflow
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((
                NON_AVT_TOKEN_ID,
                mock_data.receiver_account_id
            )),
            token_balance_before
        );

        let token_balance_u128_max_amount = MockData::get_token_balance(u128_max_amount);
        assert!(!System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLifted {
                token_id: NON_AVT_TOKEN_ID,
                recipient: mock_data.receiver_account_id,
                token_balance: token_balance_u128_max_amount,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_signed_transfer_with_valid_input_should_succeed() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender_keys = sp_core::Pair::from_seed_slice(&[1u8; 32]).unwrap();

        let sender_account_id = get_account_id(&sender_keys);
        let relayer_account_id = AccountId::from_raw([17; 32]); // just some arbitrary account id
        let recipient_account_id = AccountId::from_raw([0; 32]); // just some arbitrary account id

        let amount: u128 = 1_000_000;
        let nonce: u64 = 15;

        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID, sender_account_id),
            2 * amount,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID_2, sender_account_id),
            3 * amount,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID_2, recipient_account_id),
            4 * amount,
        );
        Nonces::<TestRuntime>::insert(sender_account_id, nonce);

        let authorization_signature = create_valid_signature_for_signed_transfer(
            &relayer_account_id,
            &sender_account_id,
            &recipient_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            nonce,
            &sender_keys,
        );

        let proof = Proof {
            signer: sender_account_id,
            relayer: relayer_account_id,
            signature: authorization_signature,
        };

        assert_eq!(System::events().len(), 0);
        assert_ok!(TokenManager::signed_transfer(
            RuntimeOrigin::signed(sender_account_id),
            proof,
            sender_account_id,
            recipient_account_id,
            NON_AVT_TOKEN_ID,
            amount,
        ));

        assert_eq!(Nonces::<TestRuntime>::get(sender_account_id), nonce + 1);
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender_account_id)),
            amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient_account_id)),
            amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, sender_account_id)),
            3 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, recipient_account_id)),
            4 * amount
        );

        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenTransferred {
                token_id: NON_AVT_TOKEN_ID,
                sender: sender_account_id,
                recipient: recipient_account_id,
                token_balance: amount
            })));
    });
}

#[test]
fn avn_test_signed_transfer_of_0_token_should_succeed() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender_keys = sp_core::Pair::from_seed_slice(&[1u8; 32]).unwrap();

        let sender_account_id = get_account_id(&sender_keys);
        let relayer_account_id = AccountId::from_raw([17; 32]); // just some arbitrary account id
        let recipient_account_id = AccountId::from_raw([0; 32]); // just some arbitrary account id

        let zero_amount: u128 = 0;
        let amount: u128 = 1_000_000;
        let nonce: u64 = 15;

        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID, sender_account_id),
            2 * amount,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID_2, sender_account_id),
            3 * amount,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID_2, recipient_account_id),
            4 * amount,
        );
        Nonces::<TestRuntime>::insert(sender_account_id, nonce);

        let authorization_signature = create_valid_signature_for_signed_transfer(
            &relayer_account_id,
            &sender_account_id,
            &recipient_account_id,
            NON_AVT_TOKEN_ID,
            zero_amount,
            nonce,
            &sender_keys,
        );

        let proof = Proof {
            signer: sender_account_id,
            relayer: relayer_account_id,
            signature: authorization_signature,
        };

        assert_eq!(System::events().len(), 0);
        assert_ok!(TokenManager::signed_transfer(
            RuntimeOrigin::signed(sender_account_id),
            proof,
            sender_account_id,
            recipient_account_id,
            NON_AVT_TOKEN_ID,
            zero_amount,
        ));

        assert_eq!(Nonces::<TestRuntime>::get(sender_account_id), nonce + 1);
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender_account_id)),
            2 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient_account_id)),
            0
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, sender_account_id)),
            3 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, recipient_account_id)),
            4 * amount
        );

        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenTransferred {
                token_id: NON_AVT_TOKEN_ID,
                sender: sender_account_id,
                recipient: recipient_account_id,
                token_balance: zero_amount
            })));
    });
}

#[test]
fn avn_test_self_signed_transfer_should_succeed() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender_keys = sp_core::Pair::from_seed_slice(&[1u8; 32]).unwrap();

        let sender_account_id = get_account_id(&sender_keys);
        let relayer_account_id = AccountId::from_raw([17; 32]); // just some arbitrary account id
        let recipient_account_id = sender_account_id.clone();

        let amount: u128 = 1_000_000;
        let nonce: u64 = 15;

        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID, sender_account_id),
            2 * amount,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID_2, sender_account_id),
            3 * amount,
        );
        Nonces::<TestRuntime>::insert(sender_account_id, nonce);

        let authorization_signature = create_valid_signature_for_signed_transfer(
            &relayer_account_id,
            &sender_account_id,
            &recipient_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            nonce,
            &sender_keys,
        );

        let proof = Proof {
            signer: sender_account_id,
            relayer: relayer_account_id,
            signature: authorization_signature,
        };

        assert_eq!(System::events().len(), 0);
        assert_ok!(TokenManager::signed_transfer(
            RuntimeOrigin::signed(sender_account_id),
            proof,
            sender_account_id,
            recipient_account_id,
            NON_AVT_TOKEN_ID,
            amount,
        ));

        assert_eq!(Nonces::<TestRuntime>::get(sender_account_id), nonce + 1);
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender_account_id)),
            2 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient_account_id)),
            2 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, sender_account_id)),
            3 * amount
        );

        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenTransferred {
                token_id: NON_AVT_TOKEN_ID,
                sender: sender_account_id,
                recipient: recipient_account_id,
                token_balance: amount
            })));
    });
}

#[test]
fn avn_test_self_signed_transfer_of_0_token_should_succeed() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender_keys = sp_core::Pair::from_seed_slice(&[1u8; 32]).unwrap();

        let sender_account_id = get_account_id(&sender_keys);
        let relayer_account_id = AccountId::from_raw([17; 32]); // just some arbitrary account id
        let recipient_account_id = sender_account_id.clone();

        let zero_amount: u128 = 0;
        let amount: u128 = 1_000_000;
        let nonce: u64 = 15;

        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID, sender_account_id),
            2 * amount,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID_2, sender_account_id),
            3 * amount,
        );
        Nonces::<TestRuntime>::insert(sender_account_id, nonce);

        let authorization_signature = create_valid_signature_for_signed_transfer(
            &relayer_account_id,
            &sender_account_id,
            &recipient_account_id,
            NON_AVT_TOKEN_ID,
            zero_amount,
            nonce,
            &sender_keys,
        );

        let proof = Proof {
            signer: sender_account_id,
            relayer: relayer_account_id,
            signature: authorization_signature,
        };

        assert_eq!(System::events().len(), 0);
        assert_ok!(TokenManager::signed_transfer(
            RuntimeOrigin::signed(sender_account_id),
            proof,
            sender_account_id,
            recipient_account_id,
            NON_AVT_TOKEN_ID,
            zero_amount,
        ));

        assert_eq!(Nonces::<TestRuntime>::get(sender_account_id), nonce + 1);
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender_account_id)),
            2 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient_account_id)),
            2 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, sender_account_id)),
            3 * amount
        );

        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenTransferred {
                token_id: NON_AVT_TOKEN_ID,
                sender: sender_account_id,
                recipient: recipient_account_id,
                token_balance: zero_amount
            })));
    });
}

#[test]
fn avn_test_signed_transfer_fails_when_nonce_is_less_than_account_nonce() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender_keys = sp_core::Pair::from_seed_slice(&[1u8; 32]).unwrap();

        let sender_account_id = get_account_id(&sender_keys);
        let relayer_account_id = AccountId::from_raw([17; 32]); // just some arbitrary account id
        let recipient_account_id = AccountId::from_raw([0; 32]); // just some arbitrary account id

        let amount: u128 = 1_000_000;
        let nonce: u64 = 15;

        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID, sender_account_id),
            2 * amount,
        );
        Nonces::<TestRuntime>::insert(sender_account_id, nonce);

        let authorization_signature = create_valid_signature_for_signed_transfer(
            &relayer_account_id,
            &sender_account_id,
            &recipient_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            nonce - 1,
            &sender_keys,
        );

        let proof = Proof {
            signer: sender_account_id,
            relayer: relayer_account_id,
            signature: authorization_signature,
        };

        assert_eq!(System::events().len(), 0);
        assert_noop!(
            TokenManager::signed_transfer(
                RuntimeOrigin::signed(sender_account_id),
                proof,
                sender_account_id,
                recipient_account_id,
                NON_AVT_TOKEN_ID,
                amount,
            ),
            Error::<TestRuntime>::UnauthorizedSignedTransferTransaction
        );
    });
}

#[test]
fn avn_test_signed_transfer_fails_when_nonce_is_more_than_account_nonce() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender_keys = sp_core::Pair::from_seed_slice(&[1u8; 32]).unwrap();

        let sender_account_id = get_account_id(&sender_keys);
        let relayer_account_id = AccountId::from_raw([17; 32]); // just some arbitrary account id
        let recipient_account_id = AccountId::from_raw([0; 32]); // just some arbitrary account id

        let amount: u128 = 1_000_000;
        let nonce: u64 = 15;

        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID, sender_account_id),
            2 * amount,
        );
        Nonces::<TestRuntime>::insert(sender_account_id, nonce);

        let authorization_signature = create_valid_signature_for_signed_transfer(
            &relayer_account_id,
            &sender_account_id,
            &recipient_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            nonce + 1,
            &sender_keys,
        );

        let proof = Proof {
            signer: sender_account_id,
            relayer: relayer_account_id,
            signature: authorization_signature,
        };

        assert_eq!(System::events().len(), 0);
        assert_noop!(
            TokenManager::signed_transfer(
                RuntimeOrigin::signed(sender_account_id),
                proof,
                sender_account_id,
                recipient_account_id,
                NON_AVT_TOKEN_ID,
                amount,
            ),
            Error::<TestRuntime>::UnauthorizedSignedTransferTransaction
        );
    });
}

#[test]
fn avn_test_signed_transfer_fails_when_sender_has_insufficient_fund() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender_keys = sp_core::Pair::from_seed_slice(&[1u8; 32]).unwrap();

        let sender_account_id = get_account_id(&sender_keys);
        let relayer_account_id = AccountId::from_raw([17; 32]); // just some arbitrary account id
        let recipient_account_id = AccountId::from_raw([0; 32]); // just some arbitrary account id

        let amount: u128 = 1_000_000;
        let nonce: u64 = 15;

        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID, sender_account_id),
            amount - 1,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID, recipient_account_id),
            2 * amount,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID_2, sender_account_id),
            3 * amount,
        );
        TokenManagerBalances::<TestRuntime>::insert(
            (NON_AVT_TOKEN_ID_2, recipient_account_id),
            4 * amount,
        );
        Nonces::<TestRuntime>::insert(sender_account_id, nonce);

        let authorization_signature = create_valid_signature_for_signed_transfer(
            &relayer_account_id,
            &sender_account_id,
            &recipient_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            nonce,
            &sender_keys,
        );

        let proof = Proof {
            signer: sender_account_id,
            relayer: relayer_account_id,
            signature: authorization_signature,
        };

        assert_noop!(
            TokenManager::signed_transfer(
                RuntimeOrigin::signed(sender_account_id),
                proof,
                sender_account_id,
                recipient_account_id,
                NON_AVT_TOKEN_ID,
                amount
            ),
            Error::<TestRuntime>::InsufficientSenderBalance
        );

        // Check the nonce is not updated
        assert_eq!(Nonces::<TestRuntime>::get(sender_account_id), nonce);

        // Check account balances are not changed
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender_account_id)),
            amount - 1
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient_account_id)),
            2 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, sender_account_id)),
            3 * amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, recipient_account_id)),
            4 * amount
        );

        // Check the event is not emitted
        assert_eq!(System::events().len(), 0);
    });
}

#[test]
fn avn_test_signed_transfer_fails_when_amount_causes_balance_overflow() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let sender_keys = sp_core::Pair::from_seed_slice(&[1u8; 32]).unwrap();

        let sender_account_id = get_account_id(&sender_keys);
        let relayer_account_id = AccountId::from_raw([17; 32]); // just some arbitrary account id
        let recipient_account_id = AccountId::from_raw([0; 32]); // just some arbitrary account id

        let amount: u128 = u128::max_value();
        let nonce: u64 = 15;

        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID, sender_account_id), amount);
        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID, recipient_account_id), 1);
        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID_2, sender_account_id), 3);
        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID_2, recipient_account_id), 4);
        Nonces::<TestRuntime>::insert(sender_account_id, nonce);

        let authorization_signature = create_valid_signature_for_signed_transfer(
            &relayer_account_id,
            &sender_account_id,
            &recipient_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            nonce,
            &sender_keys,
        );

        let proof = Proof {
            signer: sender_account_id,
            relayer: relayer_account_id,
            signature: authorization_signature,
        };

        assert_noop!(
            TokenManager::signed_transfer(
                RuntimeOrigin::signed(sender_account_id),
                proof,
                sender_account_id,
                recipient_account_id,
                NON_AVT_TOKEN_ID,
                amount
            ),
            Error::<TestRuntime>::AmountOverflow
        );

        // Check the nonce is not updated
        assert_eq!(Nonces::<TestRuntime>::get(sender_account_id), nonce);

        // Check account balances are not changed
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender_account_id)),
            amount
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, recipient_account_id)),
            1
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, sender_account_id)),
            3
        );
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID_2, recipient_account_id)),
            4
        );

        // Check the event is not emitted
        assert_eq!(System::events().len(), 0);
    });
}

#[test]
fn avn_test_lower_all_non_avt_token_succeed() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, to_account_id, t1_recipient) =
            MockData::setup_lower_request_data();
        let from_account_balance_before =
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id));
        let amount = from_account_balance_before;

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id)),
            from_account_balance_before - amount
        );
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                token_id: NON_AVT_TOKEN_ID,
                sender: from_account_id,
                recipient: to_account_id,
                amount,
                t1_recipient,
                lower_id: 0
            })));
    });
}

#[test]
fn avn_test_lower_some_non_avt_token_succeed() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, to_account_id, t1_recipient) =
            MockData::setup_lower_request_data();
        let from_account_balance_before =
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id));
        let amount = from_account_balance_before / 2;

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id)),
            from_account_balance_before - amount
        );
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                token_id: NON_AVT_TOKEN_ID,
                sender: from_account_id,
                recipient: to_account_id,
                amount,
                t1_recipient,
                lower_id: 0
            })));
    });
}

#[test]
fn avn_test_reverted_non_avt_token_lower_refunds_sender_removes_claim_and_emits_event() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let lower_id: u32 = 0;

        let (_, from, _burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let amount = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));

        perform_lower_setup_token(lower_id);
        assert!(LowersReadyToClaim::<TestRuntime>::contains_key(lower_id));

        let balance_before = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let receiver_address = H256::from_slice(&receiver_topic_with_100_avt());

        let mock_event = sp_avn_common::event_types::EthEvent {
            event_id: sp_avn_common::event_types::EthEventId {
                signature: sp_avn_common::event_types::ValidEvents::LowerReverted.signature(),
                transaction_hash: H256::random(),
            },
            event_data: sp_avn_common::event_types::EventData::LogLowerReverted(
                sp_avn_common::event_types::LowerRevertedData {
                    token_contract: NON_AVT_TOKEN_ID,
                    receiver_address,
                    amount,
                    lower_id,
                    t1_recipient,
                },
            ),
        };

        insert_to_mock_processed_events(&mock_event.event_id);

        assert_ok!(TokenManager::on_event_processed(&mock_event));
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
            balance_before + amount
        );
        assert!(!LowersReadyToClaim::<TestRuntime>::contains_key(lower_id));
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowerReverted {
                token_id: NON_AVT_TOKEN_ID,
                t2_refunded_sender: from,
                amount,
                eth_tx_hash: mock_event.event_id.transaction_hash,
                lower_id,
                t1_reverted_recipient: t1_recipient,
            })));
    });
}

#[test]
fn avn_test_lower_non_avt_token_should_fail_when_sender_does_not_have_enough_token() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, _, _, t1_recipient) = MockData::setup_lower_request_data();
        let from_account = H256::random();
        let from_account_id =
            <TestRuntime as frame_system::Config>::AccountId::decode(&mut from_account.as_bytes())
                .unwrap();
        let amount = 1;

        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id)),
            0
        );

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id)),
            0
        );

        let dispatch_result = System::events().iter().find_map(|a| match a.event {
            RuntimeEvent::Scheduler(pallet_scheduler::Event::<TestRuntime>::Dispatched {
                task: _,
                id: _,
                result,
            }) => Some(result),
            _ => None,
        });

        assert!(dispatch_result.is_some());
        assert_err!(dispatch_result.unwrap(), Error::<TestRuntime>::InsufficientSenderBalance);
    });
}

// Note: This test prevents the implementation of lower function from using a t2 destination account
// to receive all the tokens, which may cause an overflow of the t2 destination account token
// balance
#[test]
fn avn_test_non_avt_token_total_lowered_amount_greater_than_balance_max_value_ok() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, to_account_id, t1_recipient) =
            MockData::setup_lower_request_data();
        let mut from_account_balance_before =
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id));
        let mut amount = from_account_balance_before;

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id)),
            from_account_balance_before - amount
        );
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                token_id: NON_AVT_TOKEN_ID,
                sender: from_account_id,
                recipient: to_account_id,
                amount,
                t1_recipient,
                lower_id: 0
            })));

        // Lift and lower non-AVT tokens again
        amount = u128::max_value();
        TokenManager::initialise_non_avt_tokens_to_account(from_account_id, amount);
        from_account_balance_before =
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id));

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            NON_AVT_TOKEN_ID,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from_account_id)),
            from_account_balance_before - amount
        );
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                token_id: NON_AVT_TOKEN_ID,
                sender: from_account_id,
                recipient: to_account_id,
                amount,
                t1_recipient,
                lower_id: 1
            })));
    });
}
