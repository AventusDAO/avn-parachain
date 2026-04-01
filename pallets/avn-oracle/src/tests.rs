#![cfg(test)]

use super::*;
use crate::mock::*;
use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use serde_json::json;

fn submit_price_for_x_validators(num_validators: u64, rates: Rates) {
    for i in 1..=num_validators {
        let submitter = create_validator(i);
        let signature = generate_signature(&submitter, b"test context");

        assert_ok!(AvnOracle::submit_price(
            RuntimeOrigin::none(),
            rates.clone(),
            submitter,
            signature,
        ));
    }
}

fn register_max_currencies() {
    let max_currencies: u32 = <TestRuntime as Config>::MaxCurrencies::get();

    for i in 1..=max_currencies {
        let currency_symbol = format!("us{}", i).into_bytes();
        let currency = create_currency(currency_symbol.clone());

        assert_ok!(AvnOracle::register_currency(RuntimeOrigin::root(), currency_symbol));
        assert!(Currencies::<TestRuntime>::contains_key(&currency));
    }
}

fn submit_different_rates_for_x_validators(num_validators: u64) {
    for i in 1..=num_validators {
        let submitter = create_validator(i);
        let signature = generate_signature(&submitter, b"test context");

        let currency_symbol = b"usd".to_vec();
        let currency = create_currency(currency_symbol.clone());
        register_currency(currency_symbol);

        let rates = create_rates(vec![(currency, i as u128)]);

        assert_ok!(AvnOracle::submit_price(RuntimeOrigin::none(), rates, submitter, signature,));
    }
}

pub fn scale_rate(rate: f64) -> u128 {
    (rate * 1e8) as u128
}

fn sort_rates(rates: Rates) -> Rates {
    let mut inner: Vec<(Currency, u128)> = rates.into_inner();
    inner.sort_by(|(a, _), (b, _)| a.cmp(b));
    Rates::try_from(inner).expect("bounds unchanged")
}

mod submit_price {
    use super::*;

    #[test]
    fn first_submission_by_validator_succeeds() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let submitter = create_validator(1);
            let signature = generate_signature(&submitter, b"test context");

            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());
            register_currency(currency_symbol);

            let rates = create_rates(vec![(currency, 1000u128)]);
            let current_voting_id = VotingRoundId::<TestRuntime>::get();

            assert_ok!(AvnOracle::submit_price(
                RuntimeOrigin::none(),
                rates.clone(),
                submitter.clone(),
                signature,
            ));

            assert!(PriceReporters::<TestRuntime>::contains_key(
                current_voting_id,
                &submitter.account_id
            ));

            let count = ReportedRates::<TestRuntime>::get(current_voting_id, rates);
            assert_eq!(count, 1);
        });
    }

    #[test]
    fn second_submission_by_another_validator_succeeds() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let submitter_1 = create_validator(1);
            let submitter_2 = create_validator(2);

            let signature_1 = generate_signature(&submitter_1, b"test context");
            let signature_2 = generate_signature(&submitter_2, b"test context");

            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());
            register_currency(currency_symbol);

            let rates = create_rates(vec![(currency, 1000u128)]);
            let current_voting_id = VotingRoundId::<TestRuntime>::get();

            assert_ok!(AvnOracle::submit_price(
                RuntimeOrigin::none(),
                rates.clone(),
                submitter_1.clone(),
                signature_1,
            ));

            assert_ok!(AvnOracle::submit_price(
                RuntimeOrigin::none(),
                rates.clone(),
                submitter_2.clone(),
                signature_2,
            ));

            assert!(PriceReporters::<TestRuntime>::contains_key(
                current_voting_id,
                &submitter_1.account_id
            ));
            assert!(PriceReporters::<TestRuntime>::contains_key(
                current_voting_id,
                &submitter_2.account_id
            ));

            let count = ReportedRates::<TestRuntime>::get(current_voting_id, rates);
            assert_eq!(count, 2);
        });
    }

    #[test]
    fn submission_with_multiple_currencies_succeeds() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let submitter = create_validator(1);
            let signature = generate_signature(&submitter, b"test context");

            let usd_symbol = b"usd".to_vec();
            let usd = create_currency(usd_symbol.clone());
            register_currency(usd_symbol);

            let eur_symbol = b"eur".to_vec();
            let eur = create_currency(eur_symbol.clone());
            register_currency(eur_symbol);

            let rates = create_rates(vec![(usd, 1000u128), (eur, 1000u128)]);
            let current_voting_id = VotingRoundId::<TestRuntime>::get();

            assert_ok!(AvnOracle::submit_price(
                RuntimeOrigin::none(),
                rates.clone(),
                submitter.clone(),
                signature,
            ));

            assert!(PriceReporters::<TestRuntime>::contains_key(
                current_voting_id,
                &submitter.account_id
            ));

            let count = ReportedRates::<TestRuntime>::get(current_voting_id, rates);
            assert_eq!(count, 1);
        });
    }

    #[test]
    fn fails_if_submitter_is_not_validator() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let submitter = create_validator(11);
            let signature = generate_signature(&submitter, b"test context");
            let currency = create_currency(b"usd".to_vec());
            let rates = create_rates(vec![(currency, 1000u128)]);

            assert_err!(
                AvnOracle::submit_price(RuntimeOrigin::none(), rates, submitter, signature,),
                Error::<TestRuntime>::SubmitterNotAValidator
            );
        });
    }

    #[test]
    fn fails_if_validator_submits_twice_in_same_round() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let submitter = create_validator(1);
            let signature = generate_signature(&submitter, b"test context");

            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());
            register_currency(currency_symbol);

            let rates = create_rates(vec![(currency, 1000u128)]);

            assert_ok!(AvnOracle::submit_price(
                RuntimeOrigin::none(),
                rates.clone(),
                submitter.clone(),
                signature.clone(),
            ));

            assert_err!(
                AvnOracle::submit_price(RuntimeOrigin::none(), rates, submitter, signature,),
                Error::<TestRuntime>::ValidatorAlreadySubmitted
            );
        });
    }

    #[test]
    fn fails_if_currency_is_not_registered() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let submitter = create_validator(1);
            let signature = generate_signature(&submitter, b"test context");
            let currency = create_currency(b"usd".to_vec());
            let rates = create_rates(vec![(currency, 1000u128)]);

            assert_err!(
                AvnOracle::submit_price(RuntimeOrigin::none(), rates, submitter, signature,),
                Error::<TestRuntime>::UnregisteredCurrency
            );
        });
    }

    #[test]
    fn reaching_quorum_emits_event_and_updates_storage() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let quorum = <TestRuntime as Config>::Quorum::get_quorum();

            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());
            register_currency(currency_symbol);

            let rates = create_rates(vec![(currency, 1000u128)]);
            let current_voting_id = VotingRoundId::<TestRuntime>::get();

            submit_price_for_x_validators(quorum.into(), rates.clone());

            let count = ReportedRates::<TestRuntime>::get(current_voting_id, rates.clone());
            assert_eq!(count, quorum);

            assert_eq!(VotingRoundId::<TestRuntime>::get(), current_voting_id + 1);

            for (symbol, value) in &rates {
                assert_eq!(NativeTokenRateByCurrency::<TestRuntime>::get(symbol), Some(*value));
            }

            assert_eq!(ProcessedVotingRoundIds::<TestRuntime>::get(), current_voting_id);

            let current_block = frame_system::Pallet::<TestRuntime>::block_number();
            assert_eq!(LastPriceSubmission::<TestRuntime>::get(), current_block);

            assert!(System::events().iter().any(|event_record| event_record.event ==
                mock::RuntimeEvent::AvnOracle(crate::Event::<TestRuntime>::RatesUpdated {
                    rates: rates.clone(),
                    round_id: current_voting_id,
                })));
        });
    }
}

mod register_currency_tests {
    use super::*;

    #[test]
    fn root_can_register_currency() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());

            assert!(!Currencies::<TestRuntime>::contains_key(&currency));

            assert_ok!(AvnOracle::register_currency(
                RuntimeOrigin::root(),
                currency_symbol.clone(),
            ));

            assert!(Currencies::<TestRuntime>::contains_key(&currency));

            assert!(System::events().iter().any(|event_record| event_record.event ==
                mock::RuntimeEvent::AvnOracle(
                    crate::Event::<TestRuntime>::CurrencyRegistered {
                        currency: currency_symbol.clone(),
                    }
                )));
        });
    }

    #[test]
    fn duplicate_symbol_replaces_existing_entry() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());

            assert_ok!(AvnOracle::register_currency(
                RuntimeOrigin::root(),
                currency_symbol.clone(),
            ));

            assert!(Currencies::<TestRuntime>::contains_key(&currency));

            assert_ok!(AvnOracle::register_currency(
                RuntimeOrigin::root(),
                currency_symbol.clone(),
            ));

            assert!(Currencies::<TestRuntime>::contains_key(&currency));
            assert_eq!(Currencies::<TestRuntime>::iter().count(), 1);
        });
    }

    #[test]
    fn fails_if_origin_is_not_root() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());

            assert!(!Currencies::<TestRuntime>::contains_key(&currency));

            assert_err!(
                AvnOracle::register_currency(RuntimeOrigin::signed(1), currency_symbol.clone()),
                sp_runtime::DispatchError::BadOrigin
            );

            assert!(!Currencies::<TestRuntime>::contains_key(&currency));
        });
    }

    #[test]
    fn fails_if_max_currencies_reached() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            register_max_currencies();

            let currency_symbol = b"usd".to_vec();

            assert_err!(
                AvnOracle::register_currency(RuntimeOrigin::root(), currency_symbol),
                Error::<TestRuntime>::TooManyCurrencies
            );
        });
    }

    #[test]
    fn fails_if_currency_symbol_too_long() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let long_currency_symbol = b"usdusd".to_vec();

            assert_err!(
                AvnOracle::register_currency(RuntimeOrigin::root(), long_currency_symbol),
                Error::<TestRuntime>::InvalidCurrency
            );
        });
    }
}

mod remove_currency_tests {
    use super::*;

    #[test]
    fn root_can_remove_currency() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());

            assert_ok!(AvnOracle::register_currency(
                RuntimeOrigin::root(),
                currency_symbol.clone(),
            ));
            assert!(Currencies::<TestRuntime>::contains_key(&currency));

            assert_ok!(AvnOracle::remove_currency(RuntimeOrigin::root(), currency_symbol.clone(),));

            assert!(!Currencies::<TestRuntime>::contains_key(&currency));

            assert!(System::events().iter().any(|event_record| event_record.event ==
                mock::RuntimeEvent::AvnOracle(crate::Event::<TestRuntime>::CurrencyRemoved {
                    currency: currency_symbol.clone(),
                })));
        });
    }

    #[test]
    fn fails_if_origin_is_not_root() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let currency_symbol = b"usd".to_vec();
            let currency = create_currency(currency_symbol.clone());

            assert_ok!(AvnOracle::register_currency(
                RuntimeOrigin::root(),
                currency_symbol.clone(),
            ));
            assert!(Currencies::<TestRuntime>::contains_key(&currency));

            assert_err!(
                AvnOracle::remove_currency(RuntimeOrigin::signed(1), currency_symbol),
                sp_runtime::DispatchError::BadOrigin
            );

            assert!(Currencies::<TestRuntime>::contains_key(&currency));
        });
    }

    #[test]
    fn fails_if_currency_not_found() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let currency_symbol = b"usd".to_vec();

            assert_err!(
                AvnOracle::remove_currency(RuntimeOrigin::root(), currency_symbol),
                Error::<TestRuntime>::CurrencyNotFound
            );
        });
    }
}

mod clear_consensus_tests {
    use super::*;

    #[test]
    fn succeeds_if_round_has_not_finished_within_grace_period() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let number_of_validators = <TestRuntime as Config>::Quorum::get_quorum() + 1;
            let voting_round_id = VotingRoundId::<TestRuntime>::get();

            submit_different_rates_for_x_validators(number_of_validators.into());

            assert_eq!(
                LastPriceSubmission::<TestRuntime>::get(),
                BlockNumberFor::<TestRuntime>::from(0u64)
            );

            let current_block: u64 = frame_system::Pallet::<TestRuntime>::block_number();
            let rates_refresh_range = RatesRefreshRangeBlocks::<TestRuntime>::get();
            let grace = <TestRuntime as Config>::ConsensusGracePeriod::get();

            let new_block_number = current_block
                .saturating_add(grace.into())
                .saturating_add(rates_refresh_range.into());

            System::set_block_number(new_block_number);

            let submitter = create_validator(1);
            let signature = generate_signature(&submitter, b"clear consensus");

            assert_ok!(Pallet::<TestRuntime>::clear_consensus(
                RuntimeOrigin::none(),
                submitter,
                signature,
            ));

            let new_last_submission_block =
                new_block_number.saturating_sub(rates_refresh_range.into());

            assert_eq!(LastPriceSubmission::<TestRuntime>::get(), new_last_submission_block);
            assert_eq!(VotingRoundId::<TestRuntime>::get(), voting_round_id + 1);
        });
    }

    #[test]
    fn fails_if_submitter_is_not_validator() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let number_of_validators = <TestRuntime as Config>::Quorum::get_quorum() + 1;

            submit_different_rates_for_x_validators(number_of_validators.into());

            let current_block: u64 = frame_system::Pallet::<TestRuntime>::block_number();
            let rates_refresh_range = RatesRefreshRangeBlocks::<TestRuntime>::get();
            let grace = <TestRuntime as Config>::ConsensusGracePeriod::get();

            let new_block_number = current_block
                .saturating_add(grace.into())
                .saturating_add(rates_refresh_range.into());

            System::set_block_number(new_block_number);

            let submitter = create_validator(11);
            let signature = generate_signature(&submitter, b"clear consensus");

            assert_err!(
                Pallet::<TestRuntime>::clear_consensus(
                    RuntimeOrigin::none(),
                    submitter,
                    signature,
                ),
                Error::<TestRuntime>::SubmitterNotAValidator
            );
        });
    }

    #[test]
    fn fails_if_grace_period_has_not_passed() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let number_of_validators = <TestRuntime as Config>::Quorum::get_quorum() + 1;

            submit_different_rates_for_x_validators(number_of_validators.into());

            let current_block: u64 = frame_system::Pallet::<TestRuntime>::block_number();
            let incomplete_grace = <TestRuntime as Config>::ConsensusGracePeriod::get() - 5;

            System::set_block_number(current_block.saturating_add(incomplete_grace.into()));

            let submitter = create_validator(1);
            let signature = generate_signature(&submitter, b"clear rate");

            assert_err!(
                Pallet::<TestRuntime>::clear_consensus(
                    RuntimeOrigin::none(),
                    submitter,
                    signature,
                ),
                Error::<TestRuntime>::GracePeriodNotPassed
            );
        });
    }
}

mod format_rates_tests {
    use super::*;

    #[test]
    fn parses_flat_json_rates() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let usd_rate = 100.1f64;

            let prices_json = json!({
                "usd": usd_rate,
            })
            .to_string()
            .into_bytes();

            let formatted_rates = Pallet::<TestRuntime>::format_rates(prices_json);

            let usd = create_currency(b"usd".to_vec());
            let expected = create_rates(vec![(usd, scale_rate(usd_rate))]);

            assert_eq!(sort_rates(formatted_rates.expect("ok")), sort_rates(expected));
        });
    }

    #[test]
    fn parses_nested_json_rates() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let usd_rate = 100.1f64;
            let eur_rate = 200.2f64;

            let prices_json = json!({
                "aventus": {
                    "usd": usd_rate,
                    "eur": eur_rate,
                }
            })
            .to_string()
            .into_bytes();

            let formatted_rates = Pallet::<TestRuntime>::format_rates(prices_json);

            let usd = create_currency(b"usd".to_vec());
            let eur = create_currency(b"eur".to_vec());
            let expected =
                create_rates(vec![(usd, scale_rate(usd_rate)), (eur, scale_rate(eur_rate))]);

            assert_eq!(sort_rates(formatted_rates.expect("ok")), sort_rates(expected));
        });
    }

    #[test]
    fn fails_if_any_rate_is_zero() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let prices_json = json!({
                "usd": 100.1,
                "eur": 0.0,
            })
            .to_string()
            .into_bytes();

            assert_err!(
                Pallet::<TestRuntime>::format_rates(prices_json),
                Error::<TestRuntime>::PriceMustBeGreaterThanZero
            );
        });
    }

    #[test]
    fn fails_if_any_currency_is_invalid() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let prices_json = json!({
                "usdfsdfs": 100.1,
            })
            .to_string()
            .into_bytes();

            assert_err!(
                Pallet::<TestRuntime>::format_rates(prices_json),
                Error::<TestRuntime>::InvalidCurrency
            );
        });
    }

    #[test]
    fn fails_if_any_rate_format_is_invalid() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let prices_json = json!({
                "usd": "usd",
            })
            .to_string()
            .into_bytes();

            assert_err!(
                Pallet::<TestRuntime>::format_rates(prices_json),
                Error::<TestRuntime>::InvalidRateFormat
            );
        });
    }
}

mod set_rates_refresh_range_tests {
    use super::*;

    #[test]
    fn succeeds_if_range_is_valid() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let valid_rates_range = <TestRuntime as Config>::MinRatesRefreshRange::get();

            assert_ok!(AvnOracle::set_rates_refresh_range(
                RuntimeOrigin::root(),
                valid_rates_range,
            ));
        });
    }

    #[test]
    fn fails_if_range_is_invalid() {
        ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
            let min_rates_range = <TestRuntime as Config>::MinRatesRefreshRange::get();
            let invalid_rates_range = min_rates_range.saturating_sub(1);

            assert_err!(
                AvnOracle::set_rates_refresh_range(RuntimeOrigin::root(), invalid_rates_range),
                Error::<TestRuntime>::RateRangeTooLow
            );
        });
    }
}
