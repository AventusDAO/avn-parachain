#![cfg(test)]
use crate::{
    mock::{RuntimeEvent, *},
    Balances as TokenManagerBalances, *,
};
use frame_support::{assert_noop, assert_ok};
use pallet_scheduler::Agenda;
use sp_runtime::DispatchError;

fn schedule_lower(from: AccountId, amount: u128, t1_recipient: H160) {
    assert_ok!(TokenManager::schedule_direct_lower(
        RuntimeOrigin::signed(from),
        from,
        NON_AVT_TOKEN_ID,
        amount,
        t1_recipient
    ));
}

#[test]
fn simple_non_avt_token_lower_works() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        let expected_lower_id = 0;
        let expected_schedule_name =
            ("Lower", &expected_lower_id).using_encoded(sp_io::hashing::blake2_256);
        let expected_execution_block = get_expected_execution_block();
        schedule_lower(from, amount, t1_recipient);

        // The lower has been successfully scheduled in the scheduler pallet
        assert_eq!(1, Agenda::<TestRuntime>::get(expected_execution_block).len());

        // Event emitted
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::LowerRequested {
                token_id: NON_AVT_TOKEN_ID,
                from,
                amount,
                t1_recipient,
                sender_nonce: None,
                lower_id: expected_lower_id,
                schedule_name: expected_schedule_name,
            })));

        // No tokens have been burned
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
            pre_lower_balance
        );

        // move to 1 block less than the expected execution block
        fast_forward_to_block(expected_execution_block - 1);

        // Still no tokens have been burned because of the delay
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
            pre_lower_balance
        );

        // move to the next block which should be the expected execution block
        forward_to_next_block();

        // Tokens have been burned
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
            pre_lower_balance - amount
        );

        // Event emitted
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                token_id: NON_AVT_TOKEN_ID,
                sender: from,
                recipient: burn_acc,
                amount,
                t1_recipient,
                lower_id: 0
            })));

        // There is nothing scheduled
        assert_eq!(0, Agenda::<TestRuntime>::get(expected_execution_block).len());
    });
}

#[test]
fn lower_id_is_unique() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, sender, recipient, t1_recipient) = MockData::setup_lower_request_data();
        let amount = 1;

        schedule_lower(sender, amount, t1_recipient);
        schedule_lower(sender, amount + 1, t1_recipient);
        schedule_lower(sender, amount + 2, t1_recipient);

        // move to the expected execution block
        fast_forward_to_block(get_expected_execution_block());

        // Event emitted
        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                token_id: NON_AVT_TOKEN_ID,
                sender,
                recipient,
                amount,
                t1_recipient,
                lower_id: 0
            })));

        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                token_id: NON_AVT_TOKEN_ID,
                sender,
                recipient,
                amount: amount + 1,
                t1_recipient,
                lower_id: 1
            })));

        assert!(System::events().iter().any(|a| a.event
            == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                token_id: NON_AVT_TOKEN_ID,
                sender,
                recipient,
                amount: amount + 2,
                t1_recipient,
                lower_id: 2
            })));
    });
}

#[test]
fn multiple_requests_in_the_same_block_works() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, _, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = 1;

        let expected_execution_block = get_expected_execution_block();

        schedule_lower(from, amount, t1_recipient);
        schedule_lower(from, amount, t1_recipient);
        schedule_lower(from, amount, t1_recipient);

        // The lower has been successfully scheduled in the scheduler pallet
        assert_eq!(3, Agenda::<TestRuntime>::get(expected_execution_block).len());

        // move to the expected execution block
        fast_forward_to_block(expected_execution_block);

        // Token balance has reduced
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
            pre_lower_balance - 3 * amount
        );

        // The lower has been removed from the scheduler pallet
        assert_eq!(0, Agenda::<TestRuntime>::get(expected_execution_block).len());
    });
}

#[test]
fn multiple_requests_in_different_blocks_works() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, _, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = 1;

        let expected_execution_block = get_expected_execution_block();

        schedule_lower(from, amount, t1_recipient);
        forward_to_next_block();
        schedule_lower(from, amount + 1, t1_recipient);
        forward_to_next_block();
        schedule_lower(from, amount + 2, t1_recipient);

        // The lower has been successfully scheduled in the scheduler pallet for each block
        assert_eq!(1, Agenda::<TestRuntime>::get(expected_execution_block).len());
        assert_eq!(1, Agenda::<TestRuntime>::get(expected_execution_block + 1).len());
        assert_eq!(1, Agenda::<TestRuntime>::get(expected_execution_block + 2).len());

        // move to the first expected execution block
        fast_forward_to_block(expected_execution_block);

        // Token balance has reduced by the first lower
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
            pre_lower_balance - amount
        );

        let balance_after_first_lower =
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        forward_to_next_block();

        // The second lower has been executed
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
            balance_after_first_lower - (amount + 1)
        );

        let balance_after_second_lower =
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        forward_to_next_block();

        // The third lower has been executed
        assert_eq!(
            TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
            balance_after_second_lower - (amount + 2)
        );

        // All lowers have been removed from the scheduler pallet
        assert_eq!(0, Agenda::<TestRuntime>::get(expected_execution_block).len());
    });
}

mod cancelling {
    use super::*;

    #[test]
    fn cancelling_works() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

        ext.execute_with(|| {
            let (_, from, _, t1_recipient) = MockData::setup_lower_request_data();
            let pre_lower_balance =
                TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
            let amount = pre_lower_balance;

            let expected_lower_id = 0;
            let expected_schedule_name =
                ("Lower", &expected_lower_id).using_encoded(sp_io::hashing::blake2_256);
            let expected_execution_block = get_expected_execution_block();

            schedule_lower(from, amount, t1_recipient);

            // The lower has been successfully scheduled in the scheduler pallet
            assert_eq!(1, Agenda::<TestRuntime>::get(expected_execution_block).len());

            // Event emitted
            assert!(System::events().iter().any(|a| a.event
                == RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::LowerRequested {
                    token_id: NON_AVT_TOKEN_ID,
                    from,
                    amount,
                    t1_recipient,
                    sender_nonce: None,
                    lower_id: expected_lower_id,
                    schedule_name: expected_schedule_name,
                })));

            // Cancel the lower
            assert_ok!(Scheduler::cancel_named(RuntimeOrigin::root(), expected_schedule_name));

            // move to the expected execution block
            fast_forward_to_block(expected_execution_block);

            // No tokens have been burned even when the expected execution block is reached
            assert_eq!(
                TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from)),
                pre_lower_balance
            );

            // The lower has been removed from the scheduler pallet
            assert_eq!(0, Agenda::<TestRuntime>::get(expected_execution_block).len());
        });
    }

    #[test]
    fn non_root_cancellation_fails() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

        ext.execute_with(|| {
            let (_, from, _, t1_recipient) = MockData::setup_lower_request_data();
            let pre_lower_balance =
                TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
            let amount = pre_lower_balance;

            let expected_lower_id = 0;
            let expected_schedule_name =
                ("Lower", &expected_lower_id).using_encoded(sp_io::hashing::blake2_256);
            let expected_execution_block = get_expected_execution_block();

            schedule_lower(from, amount, t1_recipient);

            // The lower has been successfully scheduled in the scheduler pallet
            assert_eq!(1, Agenda::<TestRuntime>::get(expected_execution_block).len());

            // Only root can cancel the lower
            assert_noop!(
                Scheduler::cancel_named(RuntimeOrigin::signed(from), expected_schedule_name),
                DispatchError::BadOrigin
            );
        });
    }
}

#[test]
fn toggling_lowering_works() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        // Disable lower
        let mut enabled = false;
        assert_ok!(TokenManager::toggle_lowering(RuntimeOrigin::root(), enabled));

        // Lowering has been successfully disabled
        assert_eq!(LowersDisabled::<TestRuntime>::get(), true);

        let (_, from, _, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        //schedule_lower(from, amount, t1_recipient);
        // Only root can cancel the lower
        assert_noop!(
            TokenManager::schedule_direct_lower(
                RuntimeOrigin::signed(from),
                from,
                NON_AVT_TOKEN_ID,
                amount,
                t1_recipient
            ),
            Error::<TestRuntime>::LoweringDisabled
        );

        enabled = true;
        assert_ok!(TokenManager::toggle_lowering(RuntimeOrigin::root(), enabled));

        // Lowering has been successfully enabled
        assert_eq!(LowersDisabled::<TestRuntime>::get(), false);

        // Prove it now works
        assert_ok!(TokenManager::schedule_direct_lower(
            RuntimeOrigin::signed(from),
            from,
            NON_AVT_TOKEN_ID,
            amount,
            t1_recipient
        ));
    });
}
