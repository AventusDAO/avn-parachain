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
    *,
};
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::RawOrigin;
use hex_literal::hex;
use pallet_balances::Error as BalancesError;
use sp_runtime::DispatchError;

const USE_RECEIVER_WITH_EXISTING_AMOUNT: bool = true;
const USE_RECEIVER_WITH_0_AMOUNT: bool = false;

fn schedule_lower(
    from: AccountId,
    amount: u128,
    t1_recipient: H160,
    expected_lower_id: u32,
    burn_acc: AccountId,
) {
    assert_ok!(TokenManager::schedule_direct_lower(
        RuntimeOrigin::signed(from),
        from,
        AVT_TOKEN_CONTRACT,
        amount,
        t1_recipient
    ));

    // execute lower
    fast_forward_to_block(get_expected_execution_block());

    // Event emitted
    assert!(System::events().iter().any(|a| a.event ==
        RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AvtLowered {
            sender: from,
            recipient: burn_acc,
            amount,
            t1_recipient,
            lower_id: expected_lower_id
        })));
}

fn perform_lower_setup(lower_id: u32) {
    let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
    let pre_lower_balance = Balances::free_balance(from);
    let amount = pre_lower_balance;

    let expected_lower_id = lower_id;
    schedule_lower(from, amount, t1_recipient, expected_lower_id, burn_acc);
    assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

    let test_proof_data: Vec<u8> = "lowerProofReady".to_string().into();
    // Simulate the response from eth-bridge
    assert_ok!(TokenManager::process_lower_proof_result(
        expected_lower_id,
        PALLET_ID.to_vec(),
        Ok(test_proof_data.clone())
    ));

    // Generated proof should be stored in LowerReadyToClaim
    assert_eq!(
        <LowersReadyToClaim<TestRuntime>>::get(expected_lower_id)
            .unwrap()
            .encoded_lower_data,
        test_proof_data
    );
}

#[test]
fn avn_test_lift_to_zero_balance_account_should_succeed() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let mock_data = MockData::setup(AMOUNT_123_TOKEN, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        // check that TokenManager.balance for AVT contract is 0
        assert_eq!(TokenManager::balance((AVT_TOKEN_CONTRACT, mock_data.receiver_account_id)), 0);

        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), 0);
        assert_ok!(TokenManager::on_event_processed(&mock_event));
        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), AMOUNT_123_TOKEN);

        // check that TokenManager.balance for AVT contract is still 0
        assert_eq!(TokenManager::balance((AVT_TOKEN_CONTRACT, mock_data.receiver_account_id)), 0);

        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTLifted {
                recipient: mock_data.receiver_account_id,
                amount: AMOUNT_123_TOKEN,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lift_to_non_zero_balance_account_should_succeed() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let mock_data = MockData::setup(AMOUNT_123_TOKEN, USE_RECEIVER_WITH_EXISTING_AMOUNT);
        let mock_event = &mock_data.avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        // check that TokenManager.balance for AVT contract is 0
        assert_eq!(TokenManager::balance((AVT_TOKEN_CONTRACT, mock_data.receiver_account_id)), 0);

        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), AMOUNT_100_TOKEN);
        let new_balance = Balances::free_balance(mock_data.receiver_account_id) + AMOUNT_123_TOKEN;

        assert_ok!(TokenManager::on_event_processed(&mock_event));
        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), new_balance);

        // check that TokenManager.balance for AVT contract is still 0
        assert_eq!(TokenManager::balance((AVT_TOKEN_CONTRACT, mock_data.receiver_account_id)), 0);

        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTLifted {
                recipient: mock_data.receiver_account_id,
                amount: AMOUNT_123_TOKEN,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lift_max_balance_to_zero_balance_account_should_succeed() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let u128_max_amount: u128 = u128::max_value();
        let mock_data = MockData::setup(u128_max_amount, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), 0);
        assert_ok!(TokenManager::on_event_processed(&mock_event));
        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), u128_max_amount);

        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTLifted {
                recipient: mock_data.receiver_account_id,
                amount: u128_max_amount,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lift_max_balance_to_non_zero_balance_account_should_return_deposit_failed_error() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let u128_max_amount = u128::max_value();
        let mock_data = MockData::setup(u128_max_amount, USE_RECEIVER_WITH_EXISTING_AMOUNT);
        let mock_event = &mock_data.avt_token_lift_event;
        insert_to_mock_processed_events(&mock_event.event_id);
        let balance_before = Balances::free_balance(mock_data.receiver_account_id);

        assert_noop!(
            TokenManager::on_event_processed(&mock_event),
            Error::<TestRuntime>::DepositFailed
        );
        assert_eq!(Balances::free_balance(mock_data.receiver_account_id), balance_before);

        assert!(!System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTLifted {
                recipient: mock_data.receiver_account_id,
                amount: u128_max_amount,
                eth_tx_hash: mock_event.event_id.transaction_hash
            })));
    });
}

#[test]
fn avn_test_lower_all_avt_token_succeed() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, to_account_id, t1_recipient) =
            MockData::setup_lower_request_data();
        let from_account_balance_before = Balances::free_balance(from_account_id);
        let amount = from_account_balance_before;

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            AVT_TOKEN_CONTRACT,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(Balances::free_balance(from_account_id), from_account_balance_before - amount);
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::Balances(pallet_balances::Event::<TestRuntime>::Withdraw {
                who: from_account_id,
                amount
            })));
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AvtLowered {
                sender: from_account_id,
                recipient: to_account_id,
                amount,
                t1_recipient,
                lower_id: 0
            })));
    });
}

#[test]
fn avn_test_lower_some_avt_token_succeed() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, to_account_id, t1_recipient) =
            MockData::setup_lower_request_data();
        let from_account_balance_before = Balances::free_balance(from_account_id);
        let amount = from_account_balance_before / 2;

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            AVT_TOKEN_CONTRACT,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(Balances::free_balance(from_account_id), from_account_balance_before - amount);
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::Balances(pallet_balances::Event::<TestRuntime>::Withdraw {
                who: from_account_id,
                amount
            })));
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AvtLowered {
                sender: from_account_id,
                recipient: to_account_id,
                amount,
                t1_recipient,
                lower_id: 0
            })));
    });
}

#[test]
fn avn_test_reverted_avt_lower_refunds_sender_removes_claim_and_emits_event() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let lower_id: u32 = 0;
        let (_, from, _burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let amount = Balances::free_balance(from);

        perform_lower_setup(lower_id);
        assert!(LowersReadyToClaim::<TestRuntime>::contains_key(lower_id));

        let balance_before = Balances::free_balance(from);
        let receiver_address = H256::from_slice(&receiver_topic_with_100_avt());

        let mock_event = sp_avn_common::event_types::EthEvent {
            event_id: sp_avn_common::event_types::EthEventId {
                signature: sp_avn_common::event_types::ValidEvents::LowerReverted.signature(),
                transaction_hash: H256::random(),
            },
            event_data: sp_avn_common::event_types::EventData::LogLowerReverted(
                sp_avn_common::event_types::LowerRevertedData {
                    token_contract: AVT_TOKEN_CONTRACT,
                    receiver_address,
                    amount,
                    lower_id,
                    t1_recipient,
                },
            ),
        };

        insert_to_mock_processed_events(&mock_event.event_id);

        assert_ok!(TokenManager::on_event_processed(&mock_event));
        assert_eq!(Balances::free_balance(from), balance_before + amount);
        assert!(!LowersReadyToClaim::<TestRuntime>::contains_key(lower_id));
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTLowerReverted {
                t2_refunded_sender: from,
                amount,
                eth_tx_hash: mock_event.event_id.transaction_hash,
                lower_id,
                t1_reverted_recipient: t1_recipient,
            })));
    });
}

#[test]
fn avn_test_lower_avt_token_should_fail_when_sender_does_not_have_enough_avt_token() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, _, _, t1_recipient) = MockData::setup_lower_request_data();
        let from_account = H256::random();
        let from_account_id =
            <TestRuntime as frame_system::Config>::AccountId::decode(&mut from_account.as_bytes())
                .unwrap();
        let amount = 1;

        assert_eq!(Balances::free_balance(from_account_id), 0);
        // Even if the user has no money, the scheduling will pass
        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            AVT_TOKEN_CONTRACT,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(Balances::free_balance(from_account_id), 0);

        let dispatch_result = System::events().iter().find_map(|a| match a.event {
            RuntimeEvent::Scheduler(pallet_scheduler::Event::<TestRuntime>::Dispatched {
                task: _,
                id: _,
                result,
            }) => Some(result),
            _ => None,
        });

        assert!(dispatch_result.is_some());
        assert_err!(dispatch_result.unwrap(), BalancesError::<TestRuntime, _>::InsufficientBalance);
    });
}

// Note: This test prevents the implementation of lower function to use a t2 destination account to
// receive all the tokens which may cause an overflow of the t2 destination account token balance
#[test]
fn avn_test_avt_token_total_lowered_amount_greater_than_balance_max_value_ok() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let (_, from_account_id, to_account_id, _) = MockData::setup_lower_request_data();
        let mut from_account_balance_before = Balances::free_balance(from_account_id);
        let mut amount = from_account_balance_before;
        let t1_recipient = H160(hex!("0000000000000000000000000000000000000001"));

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            AVT_TOKEN_CONTRACT,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(Balances::free_balance(from_account_id), from_account_balance_before - amount);
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::Balances(pallet_balances::Event::<TestRuntime>::Withdraw {
                who: from_account_id,
                amount
            })));
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AvtLowered {
                sender: from_account_id,
                recipient: to_account_id,
                amount,
                t1_recipient,
                lower_id: 0
            })));

        // Lift and lower AVT tokens again
        amount = u128::max_value();
        MockData::set_avt_balance(from_account_id, amount);
        from_account_balance_before = Balances::free_balance(from_account_id);

        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from_account_id),
            from_account_id,
            AVT_TOKEN_CONTRACT,
            amount,
            t1_recipient
        ));

        // move a few blocks to trigger the execution
        fast_forward_to_block(get_expected_execution_block());

        assert_eq!(Balances::free_balance(from_account_id), from_account_balance_before - amount);
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::Balances(pallet_balances::Event::<TestRuntime>::Withdraw {
                who: from_account_id,
                amount
            })));
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AvtLowered {
                sender: from_account_id,
                recipient: to_account_id,
                amount,
                t1_recipient,
                lower_id: 1
            })));
    });
}

#[test]
fn avt_lower_claimed_succesfully() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let mock_data = MockData::setup(AMOUNT_123_TOKEN, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.lower_claimed_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        let lower_id = 0;

        perform_lower_setup(lower_id);

        assert_ok!(TokenManager::on_event_processed(&mock_event));

        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AvtLowerClaimed {
                lower_id
            })));
    });
}

#[test]
fn avt_lower_claimed_fails_due_with_invalid_lower_id() {
    let mut ext = ExtBuilder::build_default()
        .with_genesis_config()
        .with_balances()
        .as_externality();

    ext.execute_with(|| {
        let mock_data = MockData::setup(AMOUNT_123_TOKEN, USE_RECEIVER_WITH_0_AMOUNT);
        let mock_event = &mock_data.lower_claimed_event;
        insert_to_mock_processed_events(&mock_event.event_id);

        assert_noop!(
            TokenManager::on_event_processed(&mock_event),
            Error::<TestRuntime>::InvalidLowerId
        );
    });
}

mod set_native_token_eth_address {
    use super::*;

    #[test]
    fn works() {
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_balances()
            .as_externality();

        ext.execute_with(|| {
            let old_address = <AVTTokenContract<TestRuntime>>::get();
            let new_address = H160(hex_literal::hex!("dadB0d80178819F2319190D340ce9A924f783711"));

            assert_ok!(TokenManager::set_native_token_eth_address(
                RuntimeOrigin::root(),
                new_address
            ));

            assert!(System::events().iter().any(|a| a.event ==
                RuntimeEvent::TokenManager(
                    crate::Event::<TestRuntime>::NativeTokenEthAddressUpdated {
                        old_address,
                        new_address
                    }
                )));
        });
    }

    #[test]
    fn fails_when_new_address_is_zero() {
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_balances()
            .as_externality();
        ext.execute_with(|| {
            let new_token_address = H160::zero();

            assert_noop!(
                TokenManager::set_native_token_eth_address(
                    RuntimeOrigin::root(),
                    new_token_address
                ),
                Error::<TestRuntime>::InvalidEthAddress
            );
        });
    }

    #[test]
    fn fails_for_non_root() {
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_balances()
            .as_externality();
        ext.execute_with(|| {
            let new_token_address =
                H160(hex_literal::hex!("dadB0d80178819F2319190D340ce9A924f783711"));
            let random_signer = TestAccount::new([26u8; 32]).account_id();
            assert_noop!(
                TokenManager::set_native_token_eth_address(
                    RuntimeOrigin::signed(random_signer),
                    new_token_address
                ),
                DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn fails_for_unsigned() {
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_balances()
            .as_externality();
        ext.execute_with(|| {
            let new_token_address =
                H160(hex_literal::hex!("dadB0d80178819F2319190D340ce9A924f783711"));

            assert_noop!(
                TokenManager::set_native_token_eth_address(
                    RawOrigin::None.into(),
                    new_token_address
                ),
                DispatchError::BadOrigin
            );
        });
    }
}
