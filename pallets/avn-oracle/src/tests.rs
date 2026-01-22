#![cfg(test)]
use super::{AVN, *};
use crate::mock::*;
use frame_support::{assert_err, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use serde_json::json;
use sp_core::U256;

fn submit_price_for_x_validators(num_validators: u64, rates: Rates) {
    for i in 1..=num_validators {
        let submitter = create_validator(i);
        let signature = generate_signature(&submitter, b"test context");

        assert_ok!(AvnOracle::submit_price(
            RuntimeOrigin::none(),
            rates.clone(),
            submitter.clone(),
            signature
        ));
    }
}

fn register_max_currencies() {
    let max_currencies: u32 = <TestRuntime as Config>::MaxCurrencies::get();
    for i in 1..=max_currencies {
        let currency_symbol = format!("us{}", i).into_bytes();
        let currency = create_currency(currency_symbol.clone());

        assert_ok!(AvnOracle::register_currency(RuntimeOrigin::root(), currency_symbol.clone(),));
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

        assert_ok!(AvnOracle::submit_price(
            RuntimeOrigin::none(),
            rates,
            submitter.clone(),
            signature
        ));
    }
}

pub fn scale_rate(rate: f64) -> u128 {
    (rate * 1e8) as u128
}

fn sort_rates(r: Rates) -> Rates {
    let mut v: Vec<(Currency, u128)> = r.into_inner();
    v.sort_by(|(a, _), (b, _)| a.cmp(b));

    Rates::try_from(v).expect("bounds unchanged")
}

#[cfg(test)]
mod price_submission {
    use super::*;

    #[cfg(test)]
    mod succeeds_if {
        use super::*;

        #[test]
        fn first_submission_by_validator() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let submitter = create_validator(1);
                let signature = generate_signature(&submitter, b"test context");

                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());
                register_currency(currency_symbol);

                let rates = create_rates(vec![(currency, 1000 as u128)]);

                let current_voting_id = VotingRoundId::<TestRuntime>::get();

                assert_ok!(AvnOracle::submit_price(
                    RuntimeOrigin::none(),
                    rates.clone(),
                    submitter.clone(),
                    signature
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
        fn second_submission_by_another_validator() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let submitter = create_validator(1);
                let submitter_2 = create_validator(2);
                let signature = generate_signature(&submitter, b"test context");
                let signature_2 = generate_signature(&submitter_2, b"test context");

                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());
                register_currency(currency_symbol);

                let rates = create_rates(vec![(currency, 1000 as u128)]);

                let current_voting_id = VotingRoundId::<TestRuntime>::get();

                assert_ok!(AvnOracle::submit_price(
                    RuntimeOrigin::none(),
                    rates.clone(),
                    submitter.clone(),
                    signature
                ));
                assert_ok!(AvnOracle::submit_price(
                    RuntimeOrigin::none(),
                    rates.clone(),
                    submitter_2.clone(),
                    signature_2
                ));

                assert!(PriceReporters::<TestRuntime>::contains_key(
                    current_voting_id,
                    &submitter.account_id
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
        fn submission_with_multiple_currencies() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let submitter = create_validator(1);
                let signature = generate_signature(&submitter, b"test context");

                let usd_symbol = b"usd".to_vec();
                let usd = create_currency(usd_symbol.clone());
                register_currency(usd_symbol);

                let eur_symbol = b"eur".to_vec();
                let eur = create_currency(eur_symbol.clone());
                register_currency(eur_symbol);

                let rates = create_rates(vec![(usd, 1000 as u128), (eur, 1000 as u128)]);

                let current_voting_id = VotingRoundId::<TestRuntime>::get();

                assert_ok!(AvnOracle::submit_price(
                    RuntimeOrigin::none(),
                    rates.clone(),
                    submitter.clone(),
                    signature
                ));

                assert!(PriceReporters::<TestRuntime>::contains_key(
                    current_voting_id,
                    &submitter.account_id
                ));
                let count = ReportedRates::<TestRuntime>::get(current_voting_id, rates);
                assert_eq!(count, 1);
            });
        }
    }

    #[cfg(test)]
    mod fails_if {
        use super::*;

        #[test]
        fn submitter_is_not_validator() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let submitter = create_validator(11);
                let signature = generate_signature(&submitter, b"test context");
                let currency = create_currency(b"usd".to_vec().clone());
                let rates = create_rates(vec![(currency, 1000 as u128)]);

                assert_err!(
                    AvnOracle::submit_price(
                        RuntimeOrigin::none(),
                        rates.clone(),
                        submitter.clone(),
                        signature
                    ),
                    Error::<TestRuntime>::SubmitterNotAValidator
                );
            });
        }

        #[test]
        fn second_submission_by_same_validator() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let submitter = create_validator(1);
                let signature = generate_signature(&submitter, b"test context");

                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());
                register_currency(currency_symbol);

                let rates = create_rates(vec![(currency, 1000 as u128)]);

                assert_ok!(AvnOracle::submit_price(
                    RuntimeOrigin::none(),
                    rates.clone(),
                    submitter.clone(),
                    signature.clone()
                ));
                assert_err!(
                    AvnOracle::submit_price(
                        RuntimeOrigin::none(),
                        rates.clone(),
                        submitter.clone(),
                        signature
                    ),
                    Error::<TestRuntime>::ValidatorAlreadySubmitted
                );
            });
        }

        #[test]
        fn currency_is_not_registered() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let submitter = create_validator(1);
                let signature = generate_signature(&submitter, b"test context");
                let currency = create_currency(b"usd".to_vec().clone());
                let rates = create_rates(vec![(currency, 1000 as u128)]);

                assert_err!(
                    AvnOracle::submit_price(
                        RuntimeOrigin::none(),
                        rates.clone(),
                        submitter.clone(),
                        signature
                    ),
                    Error::<TestRuntime>::UnregisteredCurrency
                );
            });
        }
    }

    #[cfg(test)]
    mod when_quorum_is_reached {
        use super::*;

        #[test]
        fn rates_event_is_emitted_and_storage_is_updated_correctly() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let number_of_validators = AVN::<TestRuntime>::quorum();

                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());
                register_currency(currency_symbol);

                let rates = create_rates(vec![(currency, 1000 as u128)]);

                let current_voting_id = VotingRoundId::<TestRuntime>::get();

                // add enough votes, just before quorum is reached
                submit_price_for_x_validators(number_of_validators.into(), rates.clone());

                // verify count
                let count = ReportedRates::<TestRuntime>::get(current_voting_id, rates.clone());
                assert_eq!(count, number_of_validators);

                // verify voting_round_id
                let voting_round_id = VotingRoundId::<TestRuntime>::get();
                assert_eq!(voting_round_id, 0);

                // add the fifth validator that will trigger consensus
                let submitter = create_validator(5);
                let signature = generate_signature(&submitter, b"test context");
                assert_ok!(AvnOracle::submit_price(
                    RuntimeOrigin::none(),
                    rates.clone(),
                    submitter.clone(),
                    signature
                ));

                // check that voting_round_id increments by currency
                assert_eq!(VotingRoundId::<TestRuntime>::get(), 1);

                // check that price is updated
                for (symbol, value) in &rates {
                    assert_eq!(
                        NativeTokenRateByCurrency::<TestRuntime>::get(symbol),
                        Some(value.clone())
                    );
                }

                // check that voting_round_id has been processed for currency
                assert_eq!(ProcessedVotingRoundIds::<TestRuntime>::get(), 0);

                // check that lastPriceSubmission updates correctly
                let current_block = <frame_system::Pallet<TestRuntime>>::block_number();
                assert_eq!(LastPriceSubmission::<TestRuntime>::get(), current_block);

                // event is emitted
                assert_eq!(
                    true,
                    System::events().iter().any(|a| a.event ==
                        mock::RuntimeEvent::AvnOracle(
                            crate::Event::<TestRuntime>::RatesUpdated {
                                rates: rates.clone(),
                                round_id: voting_round_id,
                            }
                        ))
                );
            });
        }
    }
}

#[cfg(test)]
mod register_currency {
    use super::*;

    #[cfg(test)]
    mod succeeds_if {
        use super::*;

        #[test]
        fn origin_is_sudo() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());

                // Ensure currency is not registered initially
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

                // Register currency
                assert_ok!(AvnOracle::register_currency(
                    RuntimeOrigin::root(),
                    currency_symbol.clone(),
                ));

                // Ensure currency is added
                assert!(Currencies::<TestRuntime>::contains_key(&currency));

                // event is emitted
                assert_eq!(
                    true,
                    System::events().iter().any(|a| a.event ==
                        mock::RuntimeEvent::AvnOracle(
                            crate::Event::<TestRuntime>::CurrencyRegistered {
                                currency: currency_symbol.clone(),
                            }
                        ))
                );
            });
        }

        #[test]
        fn duplicate_symbols_will_replace_existing() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());

                // Ensure currency is not registered initially
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

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

                // make sure only one entry exists
                let count = Currencies::<TestRuntime>::iter().count();
                assert_eq!(
                    count, 1,
                    "Expected only one currency entry after duplicate registration"
                );
            });
        }
    }

    #[cfg(test)]
    mod fails_if {
        use super::*;

        #[test]
        fn origin_is_not_sudo() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());

                // Ensure currency is not registered initially
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

                // Register currency
                assert_err!(
                    AvnOracle::register_currency(RuntimeOrigin::signed(1), currency_symbol.clone(),),
                    sp_runtime::DispatchError::BadOrigin
                );

                // Ensure currency is not registered
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

                // Ensure no CurrencyRegistered event was emitted
                assert!(!System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::AvnOracle(
                        crate::Event::<TestRuntime>::CurrencyRegistered {
                            currency: currency_symbol.clone(),
                        }
                    )));
            });
        }

        #[test]
        fn max_fiat_rates_reached() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                register_max_currencies();
                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());

                // Ensure currency is not registered initially
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

                // Register currency
                assert_err!(
                    AvnOracle::register_currency(RuntimeOrigin::root(), currency_symbol.clone(),),
                    Error::<TestRuntime>::TooManyCurrencies
                );
            });
        }

        #[test]
        fn currency_symbol_too_long() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let long_currency_symbol = b"usdusd".to_vec();

                // Register currency
                assert_err!(
                    AvnOracle::register_currency(
                        RuntimeOrigin::root(),
                        long_currency_symbol.clone(),
                    ),
                    Error::<TestRuntime>::InvalidCurrency
                );
            });
        }
    }
}

#[cfg(test)]
mod remove_currency {
    use super::*;

    #[cfg(test)]
    mod succeeds_if {
        use super::*;

        #[test]
        fn origin_is_sudo() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());

                // Ensure currency is not registered initially
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

                // Register currency
                assert_ok!(AvnOracle::register_currency(
                    RuntimeOrigin::root(),
                    currency_symbol.clone(),
                ));

                // Ensure currency is added
                assert!(Currencies::<TestRuntime>::contains_key(&currency));

                // Remove currency
                assert_ok!(AvnOracle::remove_currency(
                    RuntimeOrigin::root(),
                    currency_symbol.clone(),
                ));

                // Ensure currency is removed
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

                // event is emitted
                assert_eq!(
                    true,
                    System::events().iter().any(|a| a.event ==
                        mock::RuntimeEvent::AvnOracle(
                            crate::Event::<TestRuntime>::CurrencyRemoved {
                                currency: currency_symbol.clone(),
                            }
                        ))
                );
            });
        }
    }

    #[cfg(test)]
    mod fails_if {
        use super::*;

        #[test]
        fn origin_is_not_sudo() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());

                // Ensure currency is not registered initially
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

                // Register currency
                assert_ok!(AvnOracle::register_currency(
                    RuntimeOrigin::root(),
                    currency_symbol.clone(),
                ));

                // Ensure currency is added
                assert!(Currencies::<TestRuntime>::contains_key(&currency));

                // Remove currency
                assert_err!(
                    AvnOracle::remove_currency(RuntimeOrigin::signed(1), currency_symbol.clone(),),
                    sp_runtime::DispatchError::BadOrigin
                );

                // Ensure currency is not removed
                assert!(Currencies::<TestRuntime>::contains_key(&currency));

                // Ensure no CurrencyRemoved event was emitted
                assert!(!System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::AvnOracle(
                        crate::Event::<TestRuntime>::CurrencyRemoved {
                            currency: currency_symbol.clone()
                        }
                    )));
            });
        }

        #[test]
        fn currency_not_found() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                register_max_currencies();
                let currency_symbol = b"usd".to_vec();
                let currency = create_currency(currency_symbol.clone());

                // Ensure currency is not registered initially
                assert!(!Currencies::<TestRuntime>::contains_key(&currency));

                // Remove currency
                assert_err!(
                    AvnOracle::remove_currency(RuntimeOrigin::root(), currency_symbol.clone(),),
                    Error::<TestRuntime>::CurrencyNotFound
                );
            });
        }
    }
}

#[cfg(test)]
mod clear_consensus {
    use super::*;

    #[cfg(test)]
    mod succeeds_if {
        use super::*;

        #[test]
        fn round_hasnt_finish_in_grace_period_blocks() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let number_of_validators = AVN::<TestRuntime>::quorum() + 1;
                let voting_round_id = VotingRoundId::<TestRuntime>::get();

                // add votes to all validators with different rates so consensus is never reached
                submit_different_rates_for_x_validators(number_of_validators.into());

                // round started at block 0
                assert_eq!(
                    LastPriceSubmission::<TestRuntime>::get(),
                    BlockNumberFor::<TestRuntime>::from(0u64)
                );

                // in grace period blocks, it should reset round
                let current_block: u64 = <frame_system::Pallet<TestRuntime>>::block_number();
                let rates_refresh_range: u32 = RatesRefreshRangeBlocks::<TestRuntime>::get();
                let grace: u32 = <TestRuntime as Config>::ConsensusGracePeriod::get();
                let new_block_number = current_block
                    .saturating_add(grace.into())
                    .saturating_add(rates_refresh_range.into());
                System::set_block_number(new_block_number);

                let submitter = create_validator(1);
                let signature = generate_signature(&submitter, b"clear consensus");

                assert_ok!(Pallet::<TestRuntime>::clear_consensus(
                    RuntimeOrigin::none(),
                    submitter.clone(),
                    signature,
                ));

                // new submission round is set to rates_refresh_range blocks ago so it begins the
                // round right after
                let new_last_submission_block =
                    new_block_number.saturating_sub(rates_refresh_range.into());
                assert_eq!(LastPriceSubmission::<TestRuntime>::get(), new_last_submission_block);
                assert_eq!(VotingRoundId::<TestRuntime>::get(), voting_round_id + 1);
            })
        }
    }

    #[cfg(test)]
    mod fails_if {
        use super::*;

        #[test]
        fn submitter_is_not_a_validator() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let number_of_validators = AVN::<TestRuntime>::quorum() + 1;

                // add votes to all validators with different rates so consensus is never reached
                submit_different_rates_for_x_validators(number_of_validators.into());

                // round started at block 0
                assert_eq!(
                    LastPriceSubmission::<TestRuntime>::get(),
                    BlockNumberFor::<TestRuntime>::from(0u64)
                );

                // in grace period blocks, it should reset round
                let current_block: u64 = <frame_system::Pallet<TestRuntime>>::block_number();
                let rates_refresh_range: u32 = RatesRefreshRangeBlocks::<TestRuntime>::get();
                let grace: u32 = <TestRuntime as Config>::ConsensusGracePeriod::get();
                let new_block_number = current_block
                    .saturating_add(grace.into())
                    .saturating_add(rates_refresh_range.into());
                System::set_block_number(new_block_number);

                let submitter = create_validator(11);
                let signature = generate_signature(&submitter, b"clear consensus");

                assert_err!(
                    Pallet::<TestRuntime>::clear_consensus(
                        RuntimeOrigin::none(),
                        submitter.clone(),
                        signature,
                    ),
                    Error::<TestRuntime>::SubmitterNotAValidator
                );
            })
        }

        #[test]
        fn grace_period_has_not_passed() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let number_of_validators = AVN::<TestRuntime>::quorum() + 1;

                // add votes to all validators with different rates so consensus is never reached
                submit_different_rates_for_x_validators(number_of_validators.into());

                // round started at block 0
                assert_eq!(
                    LastPriceSubmission::<TestRuntime>::get(),
                    BlockNumberFor::<TestRuntime>::from(0u64)
                );

                let current_block: u64 = <frame_system::Pallet<TestRuntime>>::block_number();
                let incomplete_grace: u32 =
                    <TestRuntime as Config>::ConsensusGracePeriod::get() - 5;
                System::set_block_number(current_block.saturating_add(incomplete_grace.into()));

                let submitter = create_validator(1);
                let signature = generate_signature(&submitter, b"clear rate");

                assert_err!(
                    Pallet::<TestRuntime>::clear_consensus(
                        RuntimeOrigin::none(),
                        submitter.clone(),
                        signature,
                    ),
                    Error::<TestRuntime>::GracePeriodNotPassed
                );
            })
        }
    }
}

#[cfg(test)]
mod format_rates {
    use super::*;

    #[cfg(test)]
    mod succeeds_if {
        use super::*;

        #[test]
        fn rate_with_one_currencies() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let usd_rate: f64 = 100.1;
                let prices_json: Vec<u8> = json!({
                    "usd": usd_rate,
                })
                .to_string()
                .into_bytes();

                let formatted_rates = Pallet::<TestRuntime>::format_rates(prices_json);

                let usd = create_currency(b"usd".to_vec().clone());
                let rates = create_rates(vec![(usd, scale_rate(usd_rate))]);

                assert_eq!(sort_rates(formatted_rates.expect("ok")), sort_rates(rates));
            });
        }

        #[test]
        fn rate_with_multiple_currencies() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let usd_rate: f64 = 100.1;
                let eur_rate: f64 = 200.2;
                let prices_json: Vec<u8> = json!({
                    "usd": usd_rate,
                    "eur": eur_rate,
                })
                .to_string()
                .into_bytes();

                let formatted_rates = Pallet::<TestRuntime>::format_rates(prices_json);

                let usd = create_currency(b"usd".to_vec().clone());
                let eur = create_currency(b"eur".to_vec().clone());
                let rates =
                    create_rates(vec![(usd, scale_rate(usd_rate)), (eur, scale_rate(eur_rate))]);

                assert_eq!(sort_rates(formatted_rates.expect("ok")), sort_rates(rates));
            });
        }
    }

    #[cfg(test)]
    mod fails_if {
        use super::*;

        #[test]
        fn any_rate_with_price_zero() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let usd_rate: f64 = 100.1;
                let eur_rate: f64 = 0.0;
                let zero_price_json: Vec<u8> = json!({
                    "usd": usd_rate,
                    "eur": eur_rate,
                })
                .to_string()
                .into_bytes();

                assert_err!(
                    Pallet::<TestRuntime>::format_rates(zero_price_json),
                    Error::<TestRuntime>::PriceMustBeGreaterThanZero
                );
            });
        }

        #[test]
        fn any_invalid_currency() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let usd_rate: f64 = 100.1;
                let invalid_currency_json: Vec<u8> = json!({
                    "usdfsdfs": usd_rate,
                })
                .to_string()
                .into_bytes();

                assert_err!(
                    Pallet::<TestRuntime>::format_rates(invalid_currency_json),
                    Error::<TestRuntime>::InvalidCurrency
                );
            });
        }

        #[test]
        fn any_invalid_format() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let invalid_format_json: Vec<u8> = json!({
                    "usd": "usd",
                })
                .to_string()
                .into_bytes();

                assert_err!(
                    Pallet::<TestRuntime>::format_rates(invalid_format_json),
                    Error::<TestRuntime>::InvalidRateFormat
                );
            });
        }
    }
}

mod set_rates_refresh_range {
    use super::*;

    #[cfg(test)]
    mod succeeds_if {
        use super::*;

        #[test]
        fn range_is_valid() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let valid_rates_range: u32 = <TestRuntime as Config>::MinRatesRefreshRange::get();

                assert_ok!(AvnOracle::set_rates_refresh_range(
                    RuntimeOrigin::root(),
                    valid_rates_range
                ));
            });
        }
    }

    #[cfg(test)]
    mod fails_if {
        use super::*;

        #[test]
        fn range_is_invalid() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let min_rates_range: u32 = <TestRuntime as Config>::MinRatesRefreshRange::get();
                let invalid_rates_range: u32 = min_rates_range.saturating_sub(1);

                assert_err!(
                    AvnOracle::set_rates_refresh_range(RuntimeOrigin::root(), invalid_rates_range),
                    Error::<TestRuntime>::RateRangeTooLow
                );
            });
        }
    }
}
