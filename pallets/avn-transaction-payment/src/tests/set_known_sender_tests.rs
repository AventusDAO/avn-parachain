use super::*;
use crate::mock::{
    event_emitted, new_test_ext, AccountId, AvnTransactionPayment, RuntimeOrigin, TestAccount,
    TestRuntime,
};

use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::DispatchError;

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id();
}

mod set_known_senders {
    use super::*;

    mod succeeds_when {
        use super::*;

        #[test]
        fn call_is_set_with_correct_percentage_fee() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                        percentage: 10,
                        _marker: sp_std::marker::PhantomData::<TestRuntime>,
                    }),
                    adjustment_type: AdjustmentType::None,
                };

                assert_ok!(AvnTransactionPayment::set_known_sender(
                    RuntimeOrigin::root(),
                    account_1,
                    config,
                ));

                assert!(event_emitted(&mock::RuntimeEvent::AvnTransactionPayment(
                    crate::Event::<TestRuntime>::KnownSenderAdded {
                        known_sender: account_1,
                        adjustment: FeeAdjustmentConfig::<TestRuntime>::PercentageFee(
                            PercentageFeeConfig {
                                percentage: 10,
                                _marker: sp_std::marker::PhantomData::<TestRuntime>,
                            }
                        )
                    }
                )));
            })
        }

        #[test]
        fn call_is_set_with_correct_fixed_fee() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::FixedFee(FixedFeeConfig { fee: 10 }),
                    adjustment_type: AdjustmentType::None,
                };

                assert_ok!(AvnTransactionPayment::set_known_sender(
                    RuntimeOrigin::root(),
                    account_1,
                    config,
                ));

                assert!(event_emitted(&mock::RuntimeEvent::AvnTransactionPayment(
                    crate::Event::<TestRuntime>::KnownSenderAdded {
                        known_sender: account_1,
                        adjustment: FeeAdjustmentConfig::<TestRuntime>::FixedFee(FixedFeeConfig {
                            fee: 10,
                        })
                    }
                )));
            })
        }

        #[test]
        fn call_is_set_with_correct_transaction_based_adjustment_type() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::FixedFee(FixedFeeConfig { fee: 1 }),
                    adjustment_type: AdjustmentType::TransactionBased(NumberOfTransactions {
                        number_of_transactions: 5,
                    }),
                };

                assert_ok!(AvnTransactionPayment::set_known_sender(
                    RuntimeOrigin::root(),
                    account_1,
                    config,
                ));

                assert!(event_emitted(&mock::RuntimeEvent::AvnTransactionPayment(
                    crate::Event::<TestRuntime>::KnownSenderAdded {
                        known_sender: account_1,
                        adjustment: FeeAdjustmentConfig::<TestRuntime>::TransactionBased(
                            TransactionBasedConfig::new(config.fee_type, account_1, 5)
                        )
                    }
                )));
            })
        }

        #[test]
        fn call_is_set_with_correct_time_based_adjustment_type() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::FixedFee(FixedFeeConfig { fee: 1 }),
                    adjustment_type: AdjustmentType::TimeBased(Duration { duration: 1 }),
                };

                assert_ok!(AvnTransactionPayment::set_known_sender(
                    RuntimeOrigin::root(),
                    account_1,
                    config,
                ));

                assert!(event_emitted(&mock::RuntimeEvent::AvnTransactionPayment(
                    crate::Event::<TestRuntime>::KnownSenderAdded {
                        known_sender: account_1,
                        adjustment: FeeAdjustmentConfig::<TestRuntime>::TimeBased(
                            TimeBasedConfig::new(config.fee_type, 1)
                        )
                    }
                )));
            })
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn call_is_set_with_zero_percentage_fee() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                        percentage: 0,
                        _marker: sp_std::marker::PhantomData::<TestRuntime>,
                    }),
                    adjustment_type: AdjustmentType::None,
                };

                assert_noop!(
                    AvnTransactionPayment::set_known_sender(
                        RuntimeOrigin::root(),
                        account_1,
                        config,
                    ),
                    Error::<TestRuntime>::InvalidFeeConfig
                );
            })
        }

        #[test]
        fn call_is_set_with_percentage_fee_higher_than_100() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                        percentage: 101,
                        _marker: sp_std::marker::PhantomData::<TestRuntime>,
                    }),
                    adjustment_type: AdjustmentType::None,
                };

                assert_noop!(
                    AvnTransactionPayment::set_known_sender(
                        RuntimeOrigin::root(),
                        account_1,
                        config,
                    ),
                    Error::<TestRuntime>::InvalidFeeConfig
                );
            })
        }

        #[test]
        fn call_is_set_with_zero_fixed_fee() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::FixedFee(FixedFeeConfig { fee: 0 }),
                    adjustment_type: AdjustmentType::None,
                };

                assert_noop!(
                    AvnTransactionPayment::set_known_sender(
                        RuntimeOrigin::root(),
                        account_1,
                        config,
                    ),
                    Error::<TestRuntime>::InvalidFeeConfig
                );
            })
        }

        #[test]
        fn call_is_set_with_zero_number_of_transactions() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::FixedFee(FixedFeeConfig { fee: 1 }),
                    adjustment_type: AdjustmentType::TransactionBased(NumberOfTransactions {
                        number_of_transactions: 0,
                    }),
                };

                assert_noop!(
                    AvnTransactionPayment::set_known_sender(
                        RuntimeOrigin::root(),
                        account_1,
                        config,
                    ),
                    Error::<TestRuntime>::InvalidFeeConfig
                );
            })
        }

        #[test]
        fn call_is_set_with_zero_duration() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::FixedFee(FixedFeeConfig { fee: 1 }),
                    adjustment_type: AdjustmentType::TimeBased(Duration { duration: 0 }),
                };

                assert_noop!(
                    AvnTransactionPayment::set_known_sender(
                        RuntimeOrigin::root(),
                        account_1,
                        config,
                    ),
                    Error::<TestRuntime>::InvalidFeeConfig
                );
            })
        }

        #[test]
        fn call_has_bad_origin_signed() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::FixedFee(FixedFeeConfig { fee: 1 }),
                    adjustment_type: AdjustmentType::TimeBased(Duration { duration: 0 }),
                };

                let bad_signer = to_acc_id(11u64);
                assert_noop!(
                    AvnTransactionPayment::set_known_sender(
                        RuntimeOrigin::signed(bad_signer),
                        account_1,
                        config,
                    ),
                    DispatchError::BadOrigin
                );
            })
        }

        #[test]
        fn call_has_bad_origin_unsigned() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::FixedFee(FixedFeeConfig { fee: 1 }),
                    adjustment_type: AdjustmentType::TimeBased(Duration { duration: 0 }),
                };

                assert_noop!(
                    AvnTransactionPayment::set_known_sender(
                        RawOrigin::None.into(),
                        account_1,
                        config,
                    ),
                    DispatchError::BadOrigin
                );
            })
        }
    }
}

mod remove_known_senders {
    use super::*;

    mod succeeds_when {
        use super::*;

        #[test]
        fn call_is_set_with_correct_information() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                let config = AdjustmentInput::<TestRuntime> {
                    fee_type: FeeType::PercentageFee(PercentageFeeConfig {
                        percentage: 10,
                        _marker: sp_std::marker::PhantomData::<TestRuntime>,
                    }),
                    adjustment_type: AdjustmentType::None,
                };

                assert_ok!(AvnTransactionPayment::set_known_sender(
                    RuntimeOrigin::root(),
                    account_1,
                    config,
                ));
                assert_eq!(AvnTransactionPayment::is_known_sender(account_1), true);

                assert_ok!(AvnTransactionPayment::remove_known_sender(
                    RuntimeOrigin::root(),
                    account_1
                ));

                assert_eq!(AvnTransactionPayment::is_known_sender(account_1), false);

                assert!(event_emitted(&mock::RuntimeEvent::AvnTransactionPayment(
                    crate::Event::<TestRuntime>::KnownSenderRemoved { known_sender: account_1 }
                )));
            })
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn call_is_set_with_an_unknown_sender() {
            new_test_ext().execute_with(|| {
                let account_1 = to_acc_id(1u64);

                assert_noop!(
                    AvnTransactionPayment::remove_known_sender(RuntimeOrigin::root(), account_1),
                    Error::<TestRuntime>::KnownSenderMissing
                );
            })
        }
    }

    #[test]
    fn call_has_bad_origin_signed() {
        new_test_ext().execute_with(|| {
            let account_1 = to_acc_id(1u64);
            let bad_signer = to_acc_id(11u64);
            assert_noop!(
                AvnTransactionPayment::remove_known_sender(
                    RuntimeOrigin::signed(bad_signer),
                    account_1
                ),
                DispatchError::BadOrigin
            );
        })
    }

    #[test]
    fn call_has_bad_origin_unsigned() {
        new_test_ext().execute_with(|| {
            let account_1 = to_acc_id(1u64);
            assert_noop!(
                AvnTransactionPayment::remove_known_sender(RawOrigin::None.into(), account_1),
                DispatchError::BadOrigin
            );
        })
    }
}
