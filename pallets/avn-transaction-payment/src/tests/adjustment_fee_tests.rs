use super::*;
use crate::mock::{
    event_emitted, new_test_ext, roll_one_block, AccountId, AvnTransactionPayment, Balances,
    RuntimeCall, RuntimeEvent, RuntimeOrigin, System, TestAccount, TestRuntime, BASE_FEE,
};

use frame_support::{
    dispatch::{DispatchInfo, PostDispatchInfo},
    pallet_prelude::Weight,
    traits::Currency,
};
use pallet_transaction_payment::ChargeTransactionPayment;
use sp_runtime::{
    traits::{transaction_extension::TxBaseImplication, TransactionExtension},
    transaction_validity::TransactionSource,
};

use frame_support::assert_ok;

pub const TX_LEN: usize = 1;
pub const INITIAL_SENDER_BALANCE: u128 = 200;
pub const WEIGHT_FEE: u64 = 100;
pub const FIXED_FEE: u128 = 10;
pub const PERCENTAGE_FEE: u32 = 25;
pub const NO_TIP: u128 = 0u128;
pub const ONE_ATTO_TIP: u128 = 1u128;

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id()
}

/// create a transaction info struct from weight. Handy to avoid building the whole struct.
pub fn info_from_weight(w: Weight) -> DispatchInfo {
    DispatchInfo { call_weight: w, ..Default::default() }
}

fn pay_gas_and_call_remark_post_info(
    sender: &AccountId,
    tip: u128,
    post_dispatch_weight: Option<Weight>,
) {
    let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
    let info = info_from_weight(Weight::from_parts(WEIGHT_FEE, 0));
    let len = TX_LEN;
    let ext = ChargeTransactionPayment::<TestRuntime>::from(tip);
    let implicit = ext.implicit().expect("implicit data available");
    let implication = TxBaseImplication(call.clone());
    let origin = RuntimeOrigin::signed(*sender);
    let (_valid, val, origin) = ext
        .validate(origin, &call, &info, len, implicit, &implication, TransactionSource::Local)
        .expect("validation should succeed");
    let pre = ext
        .prepare(val, &origin, &call, &info, len)
        .expect("prepare should charge the fee");

    System::inc_account_nonce(sender); // please don't move this line

    assert_ok!(System::remark(RuntimeOrigin::signed(*sender), vec![])
        .map_err(|_e| Error::<TestRuntime>::InvalidFeeConfig));

    assert_ok!(ChargeTransactionPayment::<TestRuntime>::post_dispatch(
        pre,
        &DispatchInfo { call_weight: Weight::from_parts(WEIGHT_FEE, 0), ..Default::default() },
        &mut PostDispatchInfo { actual_weight: post_dispatch_weight, pays_fee: Default::default() },
        TX_LEN,
        &Ok(())
    ));
}

fn expected_fee() -> u128 {
    BASE_FEE.saturating_add(WEIGHT_FEE as u128).saturating_add(TX_LEN as u128)
}

fn pay_gas_and_call_remark(sender: &AccountId, tip: u128) {
    pay_gas_and_call_remark_post_info(sender, tip, None);
}

fn set_initial_sender_balance(sender: &AccountId) {
    Balances::make_free_balance_be(&sender, INITIAL_SENDER_BALANCE);
    assert_eq!(INITIAL_SENDER_BALANCE, Balances::free_balance(sender));
}

fn get_percentage_fee_paid(percentage_fee: u32) -> u128 {
    let expected_fee = expected_fee();
    expected_fee.saturating_sub((expected_fee * u128::from(percentage_fee)) / 100)
}

fn check_sender_paid_fee(sender: &AccountId, paid_fee: u128) {
    assert_eq!(INITIAL_SENDER_BALANCE.saturating_sub(paid_fee), Balances::free_balance(sender));
    assert!(event_emitted(&mock::RuntimeEvent::AvnTransactionPayment(
        crate::Event::<TestRuntime>::AdjustedTransactionFeePaid { who: *sender, fee: paid_fee }
    )));
}

fn check_sender_fee_and_event_emitted(sender: &AccountId, paid_fee: u128) {
    assert_eq!(INITIAL_SENDER_BALANCE.saturating_sub(paid_fee), Balances::free_balance(sender));
    assert_eq!(fee_adjusted_event_emitted(), false);
}

fn set_known_sender(sender: &AccountId, config: AdjustmentInput<TestRuntime>) {
    assert_ok!(AvnTransactionPayment::set_known_sender(RuntimeOrigin::root(), *sender, config,));
    assert_eq!(AvnTransactionPayment::is_known_sender(*sender), true);
}

pub(crate) fn fee_adjusted_event_emitted() -> bool {
    let transaction_payment_event_missing = System::events()
        .into_iter()
        .map(|r| r.event)
        .filter_map(|e| {
            if let RuntimeEvent::AvnTransactionPayment(inner) = e {
                Some(inner)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .is_empty();

    return transaction_payment_event_missing == false
}

/// Rolls desired block number of times.
pub(crate) fn roll_blocks(n: u64) {
    let mut block = System::block_number();
    let target_block = block + n;
    while block < target_block {
        block = roll_one_block();
    }
}

fn pay_gas_with_tip_and_check(sender: &AccountId, tip: u128, expected_base_amount: u128) {
    set_initial_sender_balance(sender);
    pay_gas_and_call_remark(sender, tip);
    check_sender_paid_fee(sender, expected_base_amount + tip);
}

mod adjustment_fee_tests {
    use super::*;

    mod fees_are_paid_correctly_when {
        use super::*;

        mod call_is_set_without_adjustment_type {
            use super::*;

            #[test]
            fn and_valid_fixed_fee() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::FixedFee(FixedFeeConfig { fee: FIXED_FEE }),
                        adjustment_type: AdjustmentType::None,
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);
                    check_sender_paid_fee(&sender, FIXED_FEE);

                    pay_gas_with_tip_and_check(&sender, ONE_ATTO_TIP, FIXED_FEE);
                })
            }

            #[test]
            fn and_fixed_fee_higher_than_base_fee() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let higher_fixed_fee = BASE_FEE + WEIGHT_FEE as u128 + TX_LEN as u128 + 1;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::FixedFee(FixedFeeConfig { fee: higher_fixed_fee }),
                        adjustment_type: AdjustmentType::None,
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);
                    check_sender_paid_fee(&sender, expected_fee());

                    pay_gas_with_tip_and_check(&sender, ONE_ATTO_TIP, expected_fee());
                })
            }

            #[test]
            fn and_valid_percentage_fee() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                            percentage: PERCENTAGE_FEE,
                            _marker: sp_std::marker::PhantomData::<TestRuntime>,
                        }),
                        adjustment_type: AdjustmentType::None,
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);

                    let paid_fee = get_percentage_fee_paid(PERCENTAGE_FEE);
                    check_sender_paid_fee(&sender, paid_fee);

                    pay_gas_with_tip_and_check(&sender, ONE_ATTO_TIP, paid_fee);
                })
            }

            #[test]
            fn and_percentage_fee_is_100() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let high_percentage_fee = 100;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                            percentage: high_percentage_fee,
                            _marker: sp_std::marker::PhantomData::<TestRuntime>,
                        }),
                        adjustment_type: AdjustmentType::None,
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);
                    check_sender_paid_fee(&sender, 0);

                    pay_gas_with_tip_and_check(&sender, ONE_ATTO_TIP, 0);
                })
            }
        }

        mod call_is_set_with_valid_transaction_based_adjustment_type {
            use super::*;

            #[test]
            fn and_valid_fixed_fee() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let number_of_transactions = 1;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::FixedFee(FixedFeeConfig { fee: FIXED_FEE }),
                        adjustment_type: AdjustmentType::TransactionBased(NumberOfTransactions {
                            number_of_transactions,
                        }),
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_paid_fee(&sender, FIXED_FEE);

                    <frame_system::Pallet<TestRuntime>>::reset_events();

                    set_initial_sender_balance(&sender);
                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_fee_and_event_emitted(&sender, expected_fee());
                })
            }

            #[test]
            fn and_valid_percentage_fee() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let number_of_transactions = 2;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                            percentage: PERCENTAGE_FEE,
                            _marker: sp_std::marker::PhantomData::<TestRuntime>,
                        }),
                        adjustment_type: AdjustmentType::TransactionBased(NumberOfTransactions {
                            number_of_transactions,
                        }),
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);

                    let paid_fee = get_percentage_fee_paid(PERCENTAGE_FEE);
                    check_sender_paid_fee(&sender, paid_fee);

                    // this works because number_of_transactions is set to 2
                    pay_gas_with_tip_and_check(&sender, ONE_ATTO_TIP, paid_fee);

                    <frame_system::Pallet<TestRuntime>>::reset_events();

                    set_initial_sender_balance(&sender);
                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_fee_and_event_emitted(&sender, expected_fee());
                })
            }

            #[test]
            fn and_initial_sender_nonce_is_not_zero() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);

                    System::inc_account_nonce(sender);

                    set_initial_sender_balance(&sender);

                    let number_of_transactions = 1;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                            percentage: PERCENTAGE_FEE,
                            _marker: sp_std::marker::PhantomData::<TestRuntime>,
                        }),
                        adjustment_type: AdjustmentType::TransactionBased(NumberOfTransactions {
                            number_of_transactions,
                        }),
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);

                    let paid_fee = get_percentage_fee_paid(PERCENTAGE_FEE);
                    check_sender_paid_fee(&sender, paid_fee);

                    <frame_system::Pallet<TestRuntime>>::reset_events();

                    set_initial_sender_balance(&sender);
                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_fee_and_event_emitted(&sender, expected_fee());
                })
            }
        }

        mod call_is_set_with_valid_block_based_adjustment_type {
            use super::*;

            #[test]
            fn and_valid_fixed_fee() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let duration = 1;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::FixedFee(FixedFeeConfig { fee: FIXED_FEE }),
                        adjustment_type: AdjustmentType::TimeBased(Duration { duration }),
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);
                    check_sender_paid_fee(&sender, FIXED_FEE);

                    // we are still in the same block
                    pay_gas_with_tip_and_check(&sender, ONE_ATTO_TIP, FIXED_FEE);

                    roll_one_block();

                    <frame_system::Pallet<TestRuntime>>::reset_events();

                    set_initial_sender_balance(&sender);
                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_fee_and_event_emitted(&sender, expected_fee());
                })
            }

            #[test]
            fn and_valid_percentage_fee() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let duration = 1;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                            percentage: PERCENTAGE_FEE,
                            _marker: sp_std::marker::PhantomData::<TestRuntime>,
                        }),
                        adjustment_type: AdjustmentType::TimeBased(Duration { duration }),
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);
                    let paid_fee = get_percentage_fee_paid(PERCENTAGE_FEE);
                    check_sender_paid_fee(&sender, paid_fee);

                    roll_one_block();

                    <frame_system::Pallet<TestRuntime>>::reset_events();

                    set_initial_sender_balance(&sender);
                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_fee_and_event_emitted(&sender, expected_fee());
                })
            }

            #[test]
            fn and_can_do_multiple_transactions_with_adjusted_fee_in_one_block() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let duration = 1;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::FixedFee(FixedFeeConfig { fee: FIXED_FEE }),
                        adjustment_type: AdjustmentType::TimeBased(Duration { duration }),
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);
                    check_sender_paid_fee(&sender, FIXED_FEE);

                    set_initial_sender_balance(&sender);

                    pay_gas_and_call_remark(&sender, NO_TIP);
                    check_sender_paid_fee(&sender, FIXED_FEE);

                    roll_one_block();

                    <frame_system::Pallet<TestRuntime>>::reset_events();

                    set_initial_sender_balance(&sender);
                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_fee_and_event_emitted(&sender, expected_fee());
                })
            }

            #[test]
            fn and_initial_block_is_not_zero() {
                new_test_ext().execute_with(|| {
                    roll_one_block();
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let duration = 1;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                            percentage: PERCENTAGE_FEE,
                            _marker: sp_std::marker::PhantomData::<TestRuntime>,
                        }),
                        adjustment_type: AdjustmentType::TimeBased(Duration { duration }),
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);

                    let paid_fee = get_percentage_fee_paid(PERCENTAGE_FEE);
                    check_sender_paid_fee(&sender, paid_fee);

                    roll_one_block();

                    <frame_system::Pallet<TestRuntime>>::reset_events();

                    set_initial_sender_balance(&sender);
                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_fee_and_event_emitted(&sender, expected_fee());
                })
            }

            #[test]
            fn and_a_high_duration() {
                new_test_ext().execute_with(|| {
                    let sender = to_acc_id(1u64);
                    set_initial_sender_balance(&sender);

                    let duration = 5;
                    let config = AdjustmentInput::<TestRuntime> {
                        fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                            percentage: PERCENTAGE_FEE,
                            _marker: sp_std::marker::PhantomData::<TestRuntime>,
                        }),
                        adjustment_type: AdjustmentType::TimeBased(Duration { duration }),
                    };

                    set_known_sender(&sender, config);

                    pay_gas_and_call_remark(&sender, NO_TIP);

                    let paid_fee = get_percentage_fee_paid(PERCENTAGE_FEE);
                    check_sender_paid_fee(&sender, paid_fee);

                    roll_blocks(5);

                    <frame_system::Pallet<TestRuntime>>::reset_events();

                    set_initial_sender_balance(&sender);
                    pay_gas_and_call_remark(&sender, NO_TIP);

                    check_sender_fee_and_event_emitted(&sender, expected_fee());
                })
            }
        }
    }
}

mod refunds {
    use super::*;

    #[test]
    fn work() {
        new_test_ext().execute_with(|| {
            let sender = to_acc_id(1u64);
            let refund = 50u64;
            set_initial_sender_balance(&sender);

            pay_gas_and_call_remark_post_info(&sender, NO_TIP, Some(Weight::from_parts(refund, 0)));
            let expected_fee =
                BASE_FEE.saturating_add(refund as u128).saturating_add(TX_LEN as u128);
            assert_eq!(
                INITIAL_SENDER_BALANCE.saturating_sub(expected_fee),
                Balances::free_balance(sender)
            );
        })
    }

    #[test]
    fn work_with_fixed_fee() {
        new_test_ext().execute_with(|| {
            let sender = to_acc_id(1u64);
            let refund = 50u64;
            set_initial_sender_balance(&sender);

            let config = AdjustmentInput::<TestRuntime> {
                fee_type: FeeType::FixedFee(FixedFeeConfig { fee: FIXED_FEE }),
                adjustment_type: AdjustmentType::None,
            };

            set_known_sender(&sender, config);

            pay_gas_and_call_remark_post_info(&sender, NO_TIP, Some(Weight::from_parts(refund, 0)));
            assert_eq!(
                INITIAL_SENDER_BALANCE.saturating_sub(FIXED_FEE),
                Balances::free_balance(sender)
            );
        })
    }

    #[test]
    fn work_with_percentage() {
        new_test_ext().execute_with(|| {
            let sender = to_acc_id(1u64);
            let refund = 50u64;
            set_initial_sender_balance(&sender);

            let config = AdjustmentInput::<TestRuntime> {
                fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                    percentage: PERCENTAGE_FEE,
                    _marker: sp_std::marker::PhantomData::<TestRuntime>,
                }),
                adjustment_type: AdjustmentType::None,
            };

            set_known_sender(&sender, config);

            pay_gas_and_call_remark_post_info(&sender, NO_TIP, Some(Weight::from_parts(refund, 0)));

            let expected_fee =
                BASE_FEE.saturating_add(refund as u128).saturating_add(TX_LEN as u128);
            let percentage = u128::from(PERCENTAGE_FEE);
            let percentage_fee = (expected_fee * percentage + 99) / 100; // round up
            let total_fee = expected_fee.saturating_sub(percentage_fee);

            assert_eq!(
                INITIAL_SENDER_BALANCE.saturating_sub(total_fee),
                Balances::free_balance(sender)
            );
        })
    }
}
