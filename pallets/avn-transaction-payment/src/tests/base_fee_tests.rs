use super::*;
use crate::mock::{
    event_emitted, new_test_ext, AccountId, AvnTransactionPayment, RuntimeOrigin, System,
    TestAccount, TestRuntime,
};
use frame_support::{assert_noop, assert_ok, traits::WithdrawReasons};

type Balance = <TestRuntime as pallet_balances::Config>::Balance;
type NegativeImbalance = pallet_balances::NegativeImbalance<mock::TestRuntime>;
use crate::mock::test_collator;
use frame_support::{
    pallet_prelude::{DispatchClass, Weight},
    traits::{OnFinalize, OnInitialize},
};
use pallet_authorship::Pallet as Authorship;
use sp_runtime::{traits::Zero, Perbill};

const SENDER_BALANCE: u128 = 10_000_000;

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id()
}

fn simulate_fee_from(who: &AccountId, fee: Balance) {
    let imbalance: NegativeImbalance = pallet_balances::Pallet::<mock::TestRuntime>::withdraw(
        who,
        fee,
        WithdrawReasons::FEE,
        frame_support::traits::ExistenceRequirement::KeepAlive,
    )
    .expect("withdraw should not fail in test");

    mock::DealWithFeesForTest::on_unbalanceds(core::iter::once(imbalance));
}

fn simulate_tip_from(who: &AccountId, tip: Balance) {
    let imbalance: NegativeImbalance = pallet_balances::Pallet::<mock::TestRuntime>::withdraw(
        who,
        tip,
        WithdrawReasons::TIP,
        frame_support::traits::ExistenceRequirement::KeepAlive,
    )
    .expect("withdraw should not fail in test");

    mock::DealWithFeesForTest::on_unbalanceds(core::iter::once(imbalance));
}

/// Simulate some block usage by registering extra weight for this block.
fn set_block_fullness(percent: f64) -> (u128, u128) {
    assert!(percent >= 0.0);
    assert!(percent <= 100.0);

    let max_weight = <TestRuntime as frame_system::Config>::BlockWeights::get().max_block;
    let max_ref_time: u64 = max_weight.ref_time();

    let fraction = percent / 100.0;

    let extra_ref_time: u64 = ((max_ref_time as f64) * fraction).round() as u64;

    System::register_extra_weight_unchecked(
        Weight::from_parts(extra_ref_time, 0),
        DispatchClass::Normal,
    );

    let used_weight = System::block_weight().total().ref_time() as u128;

    (used_weight, max_ref_time as u128)
}

mod base_fee_tests {
    use super::*;

    mod set_base_fee_usd {
        use super::*;

        mod succeeds_when {
            use super::*;

            #[test]
            fn origin_is_sudo() {
                new_test_ext().execute_with(|| {
                    let new_fee: u128 = 11_000_000; // $0.11 with 8 decimals
                    assert_ok!(AvnTransactionPayment::set_base_gas_fee_usd(
                        RuntimeOrigin::root(),
                        new_fee,
                    ));

                    // storage updated
                    assert_eq!(BaseGasFeeUsd::<TestRuntime>::get(), new_fee);

                    // event emitted
                    assert!(event_emitted(&mock::RuntimeEvent::AvnTransactionPayment(
                        crate::Event::<TestRuntime>::BaseGasFeeUsdSet { new_base_gas_fee: new_fee }
                    )));
                })
            }
        }

        mod fails_when {
            use super::*;

            #[test]
            fn origin_is_not_sudo() {
                new_test_ext().execute_with(|| {
                    let new_fee: u128 = 11_000_000; // $0.11 with 8 decimals
                    assert_noop!(
                        AvnTransactionPayment::set_base_gas_fee_usd(
                            RuntimeOrigin::signed(to_acc_id(1)),
                            new_fee,
                        ),
                        sp_runtime::DispatchError::BadOrigin,
                    );
                })
            }

            #[test]
            fn new_base_fee_is_zero() {
                new_test_ext().execute_with(|| {
                    let new_fee: u128 = 0;
                    assert_noop!(
                        AvnTransactionPayment::set_base_gas_fee_usd(
                            RuntimeOrigin::root(),
                            new_fee,
                        ),
                        Error::<TestRuntime>::BaseGasFeeZero,
                    );
                })
            }
        }
    }

    mod get_min_avt_fee_function {
        use super::*;

        mod succeeds_when {
            use super::*;
            #[test]
            fn returns_fallback_fee_when_base_fee_is_zero() {
                new_test_ext().execute_with(|| {
                    BaseGasFeeUsd::<TestRuntime>::put(0u128);

                    let min_fee = AvnTransactionPayment::get_min_avt_fee();

                    assert_eq!(min_fee, FALLBACK_MIN_FEE);
                });
            }

            #[test]
            fn computes_min_fee_from_base_usd_and_price() {
                new_test_ext().execute_with(|| {
                    // given:
                    // base_gas_fee_usd = 50_000_000 ($0.50 with 8 decimals) (minimum gas fee set by
                    // user) NativeRateProvider (in mock) returns price =
                    // 25_000_000 ($0.25) ( 1 Native token worth $0.25 returned by oracle)
                    // => usd_min_fee = 0.50 / 0.25 = 2 Native tokens (minimum calculated fee based
                    // on USD)

                    BaseGasFeeUsd::<TestRuntime>::put(50_000_000u128);

                    let min_fee = AvnTransactionPayment::get_min_avt_fee();

                    assert_eq!(min_fee, 2u128);
                });
            }
        }
    }

    mod fee_calculation_tests {
        use super::*;
        use codec::Encode;
        use pallet_transaction_payment::Pallet as TransactionPayment;

        #[test]
        fn simple_transfer_fee_is_at_least_get_min_avt_fee() {
            new_test_ext().execute_with(|| {
                // Arrange:
                // Set base fee USD and rely on TestRateProvider price.
                // For example:
                //   BaseGasFeeUsd = 5_000_000_000 ($50.00)
                //   price = 25_000_000 ($0.25)
                // => usd_min_fee = 200 Native tokens
                BaseGasFeeUsd::<TestRuntime>::put(5_000_000_000u128);

                // A simple transfer call
                let call = pallet_balances::Call::<TestRuntime>::transfer_allow_death {
                    dest: to_acc_id(2),
                    value: 1_000_000_000_000,
                };

                let info = call.get_dispatch_info();
                let len = call.encode().len() as u32;

                // Compute the fee as transaction-payment would
                let fee = TransactionPayment::<TestRuntime>::compute_fee(len, &info, 0u128.into());
                let min_fee = AvnTransactionPayment::get_min_avt_fee() as u128;

                // Fee is atleast min_fee
                assert!(
                    fee >= min_fee.into(),
                    "computed fee {:?} should be at least get_min_avt_fee {:?}",
                    fee,
                    min_fee,
                );
            });
        }
    }
}

mod fee_pot {
    use super::*;

    mod deal_with_fees {
        use super::*;

        mod succeeds_when {
            use super::*;

            #[test]
            fn single_fee_goes_to_fee_pot() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1);
                    // Fund the sender
                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &sender,
                        SENDER_BALANCE,
                    );

                    let fee_pot = AvnTransactionPayment::fee_pot_account();
                    let initial_pot_balance =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot);
                    assert_eq!(initial_pot_balance, Balance::zero());

                    let fee: Balance = 1_000u128.into();
                    simulate_fee_from(&sender, fee);

                    let final_pot_balance =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot);
                    assert_eq!(final_pot_balance - initial_pot_balance, fee);

                    let final_sender_balance =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&sender);
                    assert_eq!(SENDER_BALANCE - final_sender_balance, fee);
                })
            }

            #[test]
            fn multiple_fees_in_same_block_accumulate_in_fee_pot() {
                mock::new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1);
                    // Fund the sender
                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &sender,
                        SENDER_BALANCE,
                    );

                    let fee_pot = AvnTransactionPayment::fee_pot_account();
                    let initial_pot_balance =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot);
                    assert_eq!(initial_pot_balance, Balance::zero());

                    let fee1: Balance = 500u128.into();
                    let fee2: Balance = 1_200u128.into();
                    let fee3: Balance = 300u128.into();
                    let total_fees: Balance = (500u128 + 1_200u128 + 300u128).into();

                    // Simulate mulitple fees in the same block
                    simulate_fee_from(&sender, fee1);
                    simulate_fee_from(&sender, fee2);
                    simulate_fee_from(&sender, fee3);

                    // fee pot increased by the sum of all the "fees"
                    let final_pot_balance =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot);
                    let pot_delta = final_pot_balance - initial_pot_balance;
                    assert_eq!(pot_delta, total_fees);

                    // And the payer lost exactly the sum of fees
                    let final_who =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&sender);
                    assert_eq!(SENDER_BALANCE - final_who, total_fees);
                });
            }

            #[test]
            fn fees_and_tips_go_to_fee_pot() {
                mock::new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1);
                    // Fund the sender
                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &sender,
                        SENDER_BALANCE,
                    );

                    let fee_pot = AvnTransactionPayment::fee_pot_account();
                    let initial_pot_balance =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot);
                    assert_eq!(initial_pot_balance, Balance::zero());

                    let fee: Balance = 1_000u128.into();
                    let tip: Balance = 500u128.into();
                    let total: Balance = (1_000u128 + 500u128).into();

                    simulate_fee_from(&sender, fee);
                    simulate_tip_from(&sender, tip);

                    // Fee pot should now contain fee + tip
                    let final_pot_balance =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot);
                    assert_eq!(final_pot_balance - initial_pot_balance, total);

                    let final_sender_balance =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&sender);
                    assert_eq!(SENDER_BALANCE - final_sender_balance, total);
                });
            }
        }
    }

    mod on_finalize {
        use super::*;

        mod succeeds_when {
            use super::*;
            #[test]
            fn block_with_no_fees_does_nothing() {
                new_test_ext().execute_with(|| {
                    let fee_pot = AvnTransactionPayment::fee_pot_account();
                    let burn_pot = AvnTransactionPayment::burn_pot_account();
                    let collator = test_collator();

                    // no fees in pot
                    assert_eq!(
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot),
                        Balance::zero()
                    );

                    let collator_balance_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&collator);
                    let burn_pot_balance_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&burn_pot);

                    AvnTransactionPayment::on_finalize(1);

                    assert_eq!(
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot),
                        Balance::zero()
                    );
                    assert_eq!(
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&collator),
                        collator_balance_before
                    );
                    assert_eq!(
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&burn_pot),
                        burn_pot_balance_before
                    );
                });
            }

            #[test]
            fn block_at_point_five_percent_full_sends_all_fees_to_collator() {
                new_test_ext().execute_with(|| {
                    let burn_pot = AvnTransactionPayment::burn_pot_account();
                    let fee_pot = AvnTransactionPayment::fee_pot_account();
                    let collator = test_collator();

                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &collator,
                        SENDER_BALANCE,
                    );

                    // Put some fees in the fee pot
                    let total_fees: Balance = 10_000u128.into();
                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &fee_pot, total_fees,
                    );

                    // Set block fullness to 0.5%
                    let (used_weight, max_weight) = set_block_fullness(0.5);

                    // Check that the internal ratio at this fullness is 100% to collator
                    let ratio =
                        AvnTransactionPayment::collator_ratio_from_weights(used_weight, max_weight);
                    assert_eq!(
                        ratio,
                        Perbill::one().into(),
                        "at 0.5% fullness, all fees should go to the collator (ratio should be 1)"
                    );

                    // Ensure the author for this block is our collator
                    Authorship::<TestRuntime>::on_initialize(1);
                    let author = Authorship::<TestRuntime>::author()
                        .expect("author should be set by FindAuthor");
                    assert_eq!(author, collator);

                    // Snapshot balances before finalize
                    let collator_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&collator);
                    let burn_pot_balance_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&burn_pot);
                    let pot_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot);
                    assert_eq!(pot_before, total_fees);

                    AvnTransactionPayment::on_finalize(1);

                    // Check balances after finalize
                    let collator_after =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&collator);
                    let burn_pot_balance_after =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&burn_pot);
                    let pot_after =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&fee_pot);

                    // Fee pot must be drained
                    assert_eq!(
                        pot_after,
                        Balance::zero(),
                        "fee pot should be drained after on_finalize"
                    );

                    let collator_gain = collator_after - collator_before;
                    let burn_gain = burn_pot_balance_after - burn_pot_balance_before;

                    // All fees must go to collator
                    assert_eq!(
                        collator_gain, total_fees,
                        "at 0.5% fullness, collator should get all the fees"
                    );
                    assert_eq!(
                        burn_gain,
                        Balance::zero(),
                        "at 0.5% fullness, nothing should be burned"
                    );
                });
            }

            #[test]
            fn block_at_point_six_percent_splits_fees_correctly() {
                new_test_ext().execute_with(|| {
                    let collator = test_collator();
                    let burn_pot = AvnTransactionPayment::burn_pot_account();
                    let fee_pot = AvnTransactionPayment::fee_pot_account();

                    // Initial balances
                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &collator,
                        SENDER_BALANCE,
                    );

                    let total_fees: Balance = 10_000u128.into();
                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &fee_pot, total_fees,
                    );

                    // Set block usage to 0.6%
                    let (used_weight, max_weight) = set_block_fullness(0.6);

                    // Compute expected ratio
                    let expected_ratio =
                        AvnTransactionPayment::collator_ratio_from_weights(used_weight, max_weight);

                    // sanity checks
                    assert!(expected_ratio < FixedU128::one());
                    assert!(expected_ratio > FixedU128::zero());

                    let expected_collator_share: Balance =
                        expected_ratio.saturating_mul_int(total_fees);
                    let expected_burn_share: Balance = total_fees - expected_collator_share;

                    // Ensure the author for this block is our collator
                    Authorship::<TestRuntime>::on_initialize(1);
                    let author = Authorship::<TestRuntime>::author()
                        .expect("author should be set by FindAuthor");
                    assert_eq!(author, collator);

                    let collator_balance_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&collator);
                    let burn_balance_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&burn_pot);

                    AvnTransactionPayment::on_finalize(1);

                    let collator_balance_after =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&collator);
                    let burn_balance_after =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&burn_pot);

                    let collator_gain = collator_balance_after - collator_balance_before;
                    let burn_gain = burn_balance_after - burn_balance_before;

                    assert_eq!(collator_gain + burn_gain, total_fees);
                    assert_eq!(collator_gain, expected_collator_share);
                    assert_eq!(burn_gain, expected_burn_share);
                });
            }

            #[test]
            fn block_at_full_capacity_splits_fees_correctly() {
                new_test_ext().execute_with(|| {
                    let collator = test_collator();
                    let burn_pot = AvnTransactionPayment::burn_pot_account();
                    let fee_pot = AvnTransactionPayment::fee_pot_account();

                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &collator,
                        SENDER_BALANCE,
                    );

                    let total_fees: Balance = 10_000u128.into();
                    pallet_balances::Pallet::<mock::TestRuntime>::make_free_balance_be(
                        &fee_pot, total_fees,
                    );

                    // Full block usage = 100%
                    let (used_weight, max_weight) = set_block_fullness(100.0);

                    let expected_ratio =
                        AvnTransactionPayment::collator_ratio_from_weights(used_weight, max_weight);

                    // sanity
                    assert!(expected_ratio < FixedU128::saturating_from_rational(1u128, 100u128)); // < 1%
                    assert!(expected_ratio > FixedU128::zero());

                    let expected_collator_share: Balance =
                        expected_ratio.saturating_mul_int(total_fees);
                    let expected_burn_share: Balance = total_fees - expected_collator_share;

                    // Ensure the author for this block is our collator
                    Authorship::<TestRuntime>::on_initialize(1);
                    let author = Authorship::<TestRuntime>::author()
                        .expect("author should be set by FindAuthor");
                    assert_eq!(author, collator);

                    let collator_balance_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&collator);
                    let burn_balance_before =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&burn_pot);

                    AvnTransactionPayment::on_finalize(1);

                    let collator_balance_after =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&collator);
                    let burn_balance_after =
                        pallet_balances::Pallet::<mock::TestRuntime>::free_balance(&burn_pot);

                    let collator_gain = collator_balance_after - collator_balance_before;
                    let burn_gain = burn_balance_after - burn_balance_before;

                    assert_eq!(collator_gain + burn_gain, total_fees);
                    assert_eq!(collator_gain, expected_collator_share);
                    assert_eq!(burn_gain, expected_burn_share);
                });
            }
        }
    }
}
