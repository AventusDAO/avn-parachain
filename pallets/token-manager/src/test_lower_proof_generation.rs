#![cfg(test)]
use crate::{
    mock::{RuntimeEvent, *},
    Balances as TokenManagerBalances, *,
};
use frame_support::{assert_noop, assert_ok};

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
        NON_AVT_TOKEN_ID,
        amount,
        t1_recipient
    ));

    // execute lower
    fast_forward_to_block(get_expected_execution_block());

    // Event emitted
    assert!(System::events().iter().any(|a| a.event ==
        RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
            token_id: NON_AVT_TOKEN_ID,
            sender: from,
            recipient: burn_acc,
            amount,
            t1_recipient,
            lower_id: expected_lower_id
        })));
}

#[test]
fn lower_proof_generation_works() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        let expected_lower_id = 0;
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

        // Pending lower should be removed
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_none());

        // Event should be emitted
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::LowerReadyToClaim {
                lower_id: expected_lower_id,
            })));
    });
}

#[test]
fn failed_lower_proofs_are_handled() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        let expected_lower_id = 0;
        schedule_lower(from, amount, t1_recipient, expected_lower_id, burn_acc);
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

        // Simulate the response from eth-bridge
        assert_ok!(TokenManager::process_lower_proof_result(
            expected_lower_id,
            PALLET_ID.to_vec(),
            Err(())
        ));

        // Failed proof should be stored
        assert!(<FailedLowerProofs<TestRuntime>>::get(expected_lower_id).is_some());

        // Pending lower should be removed
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_none());

        // Event should be emitted
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(
                crate::Event::<TestRuntime>::FailedToGenerateLowerProof {
                    lower_id: expected_lower_id,
                }
            )));
    });
}

#[test]
fn unknown_caller_id_is_ignored() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        let expected_lower_id = 0;
        schedule_lower(from, amount, t1_recipient, expected_lower_id, burn_acc);
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

        let bad_caller_id = vec![];
        // Simulate the response from eth-bridge
        assert_ok!(TokenManager::process_lower_proof_result(
            expected_lower_id,
            bad_caller_id,
            Ok("lowerProofReady".to_string().into())
        ));

        // No changes to the storage
        assert!(<FailedLowerProofs<TestRuntime>>::get(expected_lower_id).is_none());
        assert!(<LowersReadyToClaim<TestRuntime>>::get(expected_lower_id).is_none());
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());
    });
}

#[test]
fn unknown_lower_id_is_ignored() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        let expected_lower_id = 0;
        schedule_lower(from, amount, t1_recipient, expected_lower_id, burn_acc);
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

        let bad_lower_id = 7;
        // Simulate the response from eth-bridge
        assert_ok!(TokenManager::process_lower_proof_result(
            bad_lower_id,
            PALLET_ID.to_vec(),
            Ok("lowerProofReady".to_string().into())
        ));

        // No changes to the storage
        assert!(<FailedLowerProofs<TestRuntime>>::get(expected_lower_id).is_none());
        assert!(<LowersReadyToClaim<TestRuntime>>::get(expected_lower_id).is_none());
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());
    });
}

#[test]
fn successfull_proof_can_be_regenerated() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        let expected_lower_id = 0;
        schedule_lower(from, amount, t1_recipient, expected_lower_id, burn_acc);
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

        let test_proof_data: Vec<u8> = "lowerProofReady".to_string().into();
        // Simulate the response from eth-bridge
        assert_ok!(TokenManager::process_lower_proof_result(
            expected_lower_id,
            PALLET_ID.to_vec(),
            Ok(test_proof_data.clone())
        ));

        // Pending lower should be empty
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_none());

        assert_ok!(TokenManager::regenerate_lower_proof(
            RuntimeOrigin::signed(from),
            expected_lower_id
        ));

        // Proof should be removed from ReadyToClaim
        assert!(<LowersReadyToClaim<TestRuntime>>::get(expected_lower_id).is_none());

        // We have a new pending lower
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

        // Event should be emitted
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::RegeneratingLowerProof {
                lower_id: expected_lower_id,
                requester: from,
            })));
    });
}

#[test]
fn failed_proof_can_be_regenerated() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        let expected_lower_id = 0;
        schedule_lower(from, amount, t1_recipient, expected_lower_id, burn_acc);
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

        // Simulate the response from eth-bridge
        assert_ok!(TokenManager::process_lower_proof_result(
            expected_lower_id,
            PALLET_ID.to_vec(),
            Err(())
        ));

        // Pending lower should be empty
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_none());

        assert_ok!(TokenManager::regenerate_lower_proof(
            RuntimeOrigin::signed(from),
            expected_lower_id
        ));

        // Proof should be removed from FailedLowerProofs
        assert!(<FailedLowerProofs<TestRuntime>>::get(expected_lower_id).is_none());

        // We have a new pending lower
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

        // Event should be emitted
        assert!(System::events().iter().any(|a| a.event ==
            RuntimeEvent::TokenManager(
                crate::Event::<TestRuntime>::RegeneratingFailedLowerProof {
                    lower_id: expected_lower_id,
                    requester: from,
                }
            )));
    });
}

#[test]
fn proof_cannot_be_regenerated_if_lower_disabled() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

    ext.execute_with(|| {
        let (_, from, burn_acc, t1_recipient) = MockData::setup_lower_request_data();
        let pre_lower_balance = TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, from));
        let amount = pre_lower_balance;

        let expected_lower_id = 0;
        schedule_lower(from, amount, t1_recipient, expected_lower_id, burn_acc);
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_some());

        let test_proof_data: Vec<u8> = "lowerProofReady".to_string().into();
        // Simulate the response from eth-bridge
        assert_ok!(TokenManager::process_lower_proof_result(
            expected_lower_id,
            PALLET_ID.to_vec(),
            Ok(test_proof_data.clone())
        ));

        // Pending lower should be empty
        assert!(<LowersPendingProof<TestRuntime>>::get(expected_lower_id).is_none());

        // Disable lowering
        assert_ok!(TokenManager::toggle_lowering(RuntimeOrigin::root(), false));

        assert_noop!(
            TokenManager::regenerate_lower_proof(RuntimeOrigin::signed(from), expected_lower_id),
            Error::<TestRuntime>::LoweringDisabled
        );
    });
}
