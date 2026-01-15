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
use crate::{
    mock::{Balances, RuntimeEvent, *},
    EventData::LogAvtGrowthLifted,
    *,
};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_avn_common::event_types::{EthEventId, ValidEvents};
use sp_runtime::{DispatchError, Perbill};

fn generate_growth_event(lifted_amount: u128) -> (EthEvent, AvtGrowthLiftedData) {
    let growth_data = AvtGrowthLiftedData { amount: lifted_amount, period: 1 };

    let growth_eth_event = EthEvent {
        event_id: EthEventId {
            signature: ValidEvents::AvtGrowthLifted.signature(),
            transaction_hash: H256::random(),
        },
        event_data: LogAvtGrowthLifted(growth_data.clone()),
    };

    return (growth_eth_event, growth_data)
}

mod lifted_growth_processed_correctly {
    use super::*;

    #[test]
    fn with_good_values() {
        let lifted_amount = 1000;
        let treasury_account_id = TokenManager::compute_treasury_account_id();
        let expected_treasury_share = TreasuryGrowthPercentage::get() * lifted_amount;
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            let (growth_eth_event, growth_data) = generate_growth_event(lifted_amount);
            insert_to_mock_processed_events(&growth_eth_event.event_id);

            // we start with an empty balance
            assert_eq!(Balances::free_balance(&treasury_account_id), 0);

            assert_ok!(TokenManager::process_avt_growth_lift(&growth_eth_event, &growth_data));

            // Check that the correct amount has been paid to the treasury
            assert_eq!(Balances::free_balance(treasury_account_id), expected_treasury_share);

            // Check expected event has been emitted
            assert!(System::events().iter().any(|a| a.event ==
                RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTGrowthLifted {
                    treasury_share: expected_treasury_share,
                    collators_share: lifted_amount - expected_treasury_share,
                    eth_tx_hash: growth_eth_event.event_id.transaction_hash
                })));
        });
    }

    #[test]
    fn with_max_amount() {
        let lifted_amount = u128::max_value();
        let treasury_account_id = TokenManager::compute_treasury_account_id();
        let expected_treasury_share = TreasuryGrowthPercentage::get() * lifted_amount;
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            let (growth_eth_event, growth_data) = generate_growth_event(lifted_amount);
            insert_to_mock_processed_events(&growth_eth_event.event_id);

            // we start with an empty balance
            assert_eq!(Balances::free_balance(&treasury_account_id), 0);

            assert_ok!(TokenManager::process_avt_growth_lift(&growth_eth_event, &growth_data));

            // Check that the correct amount has been paid to the treasury
            assert_eq!(Balances::free_balance(treasury_account_id), expected_treasury_share);

            // Check expected event has been emitted
            assert!(System::events().iter().any(|a| a.event ==
                RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::AVTGrowthLifted {
                    treasury_share: expected_treasury_share,
                    collators_share: lifted_amount - expected_treasury_share,
                    eth_tx_hash: growth_eth_event.event_id.transaction_hash
                })));
        });
    }

    #[test]
    fn and_accumulates_treasury_amount() {
        let lifted_amount = 500;
        let treasury_account_id = TokenManager::compute_treasury_account_id();
        let expected_treasury_share = TreasuryGrowthPercentage::get() * lifted_amount;
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            let (growth_eth_event, mut growth_data) = generate_growth_event(lifted_amount);
            insert_to_mock_processed_events(&growth_eth_event.event_id);

            // we start with an empty balance
            assert_eq!(Balances::free_balance(&treasury_account_id), 0);
            let initial_issuance = Balances::total_issuance();

            // Process growth twice
            assert_ok!(TokenManager::process_avt_growth_lift(&growth_eth_event, &growth_data));

            growth_data.period += 1;
            assert_ok!(TokenManager::process_avt_growth_lift(&growth_eth_event, &growth_data));

            // Any dust after paying collators is added back to treasury, so check for >= instead
            assert!(Balances::free_balance(treasury_account_id) >= expected_treasury_share * 2);
            assert_eq!(Balances::total_issuance(), initial_issuance + lifted_amount * 2);
        });
    }

    #[test]
    fn and_dust_amount_is_handled_correctly() {
        let lifted_amount = 700;
        let treasury_account_id = TokenManager::compute_treasury_account_id();
        let expected_treasury_share = TreasuryGrowthPercentage::get() * lifted_amount;
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            let (growth_eth_event, growth_data) = generate_growth_event(lifted_amount);
            insert_to_mock_processed_events(&growth_eth_event.event_id);

            // we start with an empty balance
            assert_eq!(Balances::free_balance(&treasury_account_id), 0);
            let initial_issuance = Balances::total_issuance();

            assert_ok!(TokenManager::process_avt_growth_lift(&growth_eth_event, &growth_data));

            // Any dust after paying collators is added back to treasury
            let number_of_collators = genesis_collators().len() as u32;
            let total_collator_share = lifted_amount - expected_treasury_share;
            let amount_per_collator =
                Perbill::from_rational::<u32>(1, number_of_collators) * total_collator_share;

            let dust: u128 =
                total_collator_share - (amount_per_collator * number_of_collators as u128);

            assert_eq!(Balances::free_balance(treasury_account_id), expected_treasury_share + dust);
            assert_eq!(Balances::total_issuance(), initial_issuance + lifted_amount);
        });
    }
}

mod lifted_growth_fails_to_be_processed {
    use super::*;

    #[test]
    fn when_amount_is_zero() {
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            let bad_lifted_amount = 0u128;
            let (growth_eth_event, growth_data) = generate_growth_event(bad_lifted_amount);
            insert_to_mock_processed_events(&growth_eth_event.event_id);

            assert_noop!(
                TokenManager::process_avt_growth_lift(&growth_eth_event, &growth_data),
                Error::<TestRuntime>::AmountIsZero
            );
        });
    }

    #[test]
    fn when_amount_overflows() {
        let treasury_account_id = TokenManager::compute_treasury_account_id();

        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            Balances::make_free_balance_be(&treasury_account_id, u128::max_value());

            // Treasury hold u128::max amount at this point, any additional growth will overflow
            let overflow_amount = 1u128;
            let (growth_eth_event, growth_data) = generate_growth_event(overflow_amount);
            insert_to_mock_processed_events(&growth_eth_event.event_id);

            assert_noop!(
                TokenManager::process_avt_growth_lift(&growth_eth_event, &growth_data),
                Error::<TestRuntime>::DepositFailed
            );
        });
    }
}

mod transfering_from_treasury_works {
    use super::*;

    fn process_lifted_growth(amount: u128) -> u128 {
        let (growth_eth_event, growth_data) = generate_growth_event(amount);
        insert_to_mock_processed_events(&growth_eth_event.event_id);

        assert_ok!(TokenManager::process_avt_growth_lift(&growth_eth_event, &growth_data));
        return TreasuryGrowthPercentage::get() * amount
    }

    #[test]
    fn with_good_values() {
        let lifted_amount = 1000;
        let treasury_account_id = TokenManager::compute_treasury_account_id();
        let recipient = TestAccount::new([56u8; 32]).account_id();
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            // we start with an empty balance
            assert_eq!(Balances::free_balance(&treasury_account_id), 0);
            assert_eq!(Balances::free_balance(&recipient), 0);

            let new_treasury_balance = process_lifted_growth(lifted_amount);

            let transfer_amount = new_treasury_balance - 1;
            assert_ok!(TokenManager::transfer_from_treasury(
                RuntimeOrigin::root(),
                recipient,
                transfer_amount
            ));

            assert_eq!(
                Balances::free_balance(&treasury_account_id),
                new_treasury_balance - transfer_amount
            );
            assert_eq!(Balances::free_balance(&recipient), transfer_amount);

            // Check expected event has been emitted
            assert!(System::events().iter().any(|a| a.event ==
                RuntimeEvent::TokenManager(
                    crate::Event::<TestRuntime>::TransferFromTreasury {
                        recipient,
                        amount: transfer_amount,
                    }
                )));
        });
    }
}

mod transfering_from_treasury_fails {
    use sp_runtime::TokenError;

    use super::*;

    #[test]
    fn when_origin_is_not_signed() {
        let treasury_account_id = TokenManager::compute_treasury_account_id();
        let recipient = TestAccount::new([56u8; 32]).account_id();
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            let treasury_balance = 1000;
            Balances::make_free_balance_be(&treasury_account_id, treasury_balance);

            assert_noop!(
                TokenManager::transfer_from_treasury(
                    RawOrigin::None.into(),
                    recipient,
                    treasury_balance - 1
                ),
                DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn when_origin_is_not_root() {
        let treasury_account_id = TokenManager::compute_treasury_account_id();
        let recipient = TestAccount::new([56u8; 32]).account_id();
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            let treasury_balance = 1000;
            Balances::make_free_balance_be(&treasury_account_id, treasury_balance);

            assert_noop!(
                TokenManager::transfer_from_treasury(
                    RuntimeOrigin::signed(recipient),
                    recipient,
                    treasury_balance - 1
                ),
                DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn when_not_enough_funds() {
        let treasury_account_id = TokenManager::compute_treasury_account_id();
        let recipient = TestAccount::new([56u8; 32]).account_id();
        let mut ext = ExtBuilder::build_default()
            .with_genesis_config()
            .with_validators()
            .as_externality();

        ext.execute_with(|| {
            let treasury_balance = 1000;
            Balances::make_free_balance_be(&treasury_account_id, treasury_balance);

            assert_noop!(
                TokenManager::transfer_from_treasury(
                    RuntimeOrigin::root(),
                    recipient,
                    treasury_balance + 1
                ),
                TokenError::FundsUnavailable
            );
        });
    }
}
