#![cfg(test)]
use crate::{mock::*, Call, *};
use codec::Decode;
use frame_system::pallet_prelude::BlockNumberFor;
use sp_avn_common::event_types::EthEvent;
use sp_runtime::{generic::Preamble, testing::UintAuthorityId};

fn mock_event() -> EthEvent {
    EthEvent {
        event_id: EthEventId { signature: H256::zero(), transaction_hash: H256::zero() },
        event_data: EventData::EmptyEvent,
    }
}

fn mock_event_result() -> EthEventCheckResult<BlockNumberFor<TestRuntime>, AccountId> {
    let event = mock_event();
    return EthEventCheckResult::new(
        10,
        CheckResult::Ok,
        &event.event_id,
        &EventData::EmptyEvent,
        account_id_0(),
        14,
        20,
    )
}

#[test]
fn test_try_check_event_no_change_when_no_events() {
    let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
        .with_validators()
        .for_offchain_worker()
        .as_externality_with_state();

    ext.execute_with(|| {
        let validator = keys_setup_return_good_validator();
        // when
        EthereumEvents::try_check_event(1u64, &validator, 0u32.into());
        // then
        assert!(pool_state.read().transactions.is_empty());
    });
}

#[test]
fn test_check_event_and_submit_result_status_bad() {
    check_event_and_submit_result(
        "banana",
        GOOD_BLOCK_CONFIRMATIONS,
        CheckResult::Invalid,
        DEFAULT_INGRESS_COUNTER,
    );
}

#[test]
fn test_check_event_and_submit_result_status_failed() {
    check_event_and_submit_result(
        "0x0",
        GOOD_BLOCK_CONFIRMATIONS,
        CheckResult::Invalid,
        DEFAULT_INGRESS_COUNTER,
    );
}

#[test]
fn test_check_event_and_submit_result_ok() {
    check_event_and_submit_result(
        GOOD_STATUS,
        GOOD_BLOCK_CONFIRMATIONS,
        CheckResult::Ok,
        DEFAULT_INGRESS_COUNTER,
    );
}

#[test]
fn test_check_event_and_submit_result_ok_ignores_not_enough_confirmations() {
    check_event_and_submit_result(
        GOOD_STATUS,
        GOOD_BLOCK_CONFIRMATIONS - 1,
        CheckResult::Unknown,
        DEFAULT_INGRESS_COUNTER,
    );
}

fn check_event_and_submit_result(
    status: &str,
    confirmations: u64,
    expected_result: CheckResult,
    ingress_counter: u64,
) {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();

    ext.execute_with(|| {
        let block_number = 1;
        //TODO [TYPE: test][PRI: medium][NOTE: clarify]: investigate why these keys are allowed,
        // although they haven't been added in the genesis config have we done this yet? Do
        // we instead need to change to a better way of representing Validators,
        // in line with later refactorings in other pallets?
        let validator =
            Validator::<UintAuthorityId, AccountId>::new(account_id_1(), UintAuthorityId(1));

        let unchecked_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::random(),
        };
        // TODO: Following line is only needed if we are calling try_check_event; if we call
        // check_event_and_submit_result directly, it is not needed. Which is the intent of
        // this test? EthereumEvents::insert_to_unchecked_events(unchecked_event);

        let log_data = "0x0000000000000000000000000000000000000000000000000000000005f5e100";
        let event_topics = "0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303";
        inject_ethereum_node_response(
            &mut offchain_state.write(),
            &unchecked_event.transaction_hash,
            Some(test_json(
                &unchecked_event.transaction_hash,
                &unchecked_event.signature,
                &AVN::<TestRuntime>::get_bridge_contract_address(),
                log_data,
                event_topics,
                status,
                confirmations,
            )),
        );

        let result = EthereumEvents::check_event_and_submit_result(
            block_number,
            unchecked_event,
            ingress_counter,
            &validator,
        );
        assert!(result.is_ok(), "Check of valid event with valid data failed");

        let tx = pool_state.write().transactions.pop();
        match tx {
            None => assert_eq!(expected_result, CheckResult::Unknown),
            Some(tx) => {
                assert!(expected_result != CheckResult::Unknown);
                // Only one Tx submitted
                assert!(pool_state.read().transactions.is_empty());
                let tx = Extrinsic::decode(&mut &*tx).unwrap();
                assert!(matches!(tx.preamble, Preamble::Bare(_)));
                match tx.function {
                    mock::RuntimeCall::EthereumEvents(crate::Call::submit_checkevent_result {
                        result: check_result,
                        ingress_counter: call_counter,
                        signature: _,
                        validator: _,
                    }) => {
                        assert_eq!(check_result.result, expected_result);
                        assert_eq!(call_counter, ingress_counter);
                    },
                    _ => assert!(false),
                };
            },
        }
    });
}

#[test]
fn test_check_event_and_submit_result_not_found() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();

    ext.execute_with(|| {
        let block_number = 1;
        let ingress_counter = DEFAULT_INGRESS_COUNTER;
        //TODO [TYPE: test][PRI: medium][NOTE: clarify]: investigate why these keys are allowed,
        // although they haven't been added in the genesis config see TODO above
        let validator =
            Validator::<UintAuthorityId, AccountId>::new(account_id_1(), UintAuthorityId(1));

        let not_existing_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::random(),
        };

        let expected_response = Some(
            "{{
            \"id\": 1,
            \"jsonrpc\": \"2.0\",
            \"result\": null
        }}"
            .into(),
        );
        inject_ethereum_node_response(
            &mut offchain_state.write(),
            &not_existing_event.transaction_hash,
            expected_response,
        );

        let result = EthereumEvents::check_event_and_submit_result(
            block_number,
            not_existing_event,
            ingress_counter,
            &validator,
        );
        assert!(result.is_ok(), "Check of event with empty result set was flagged as error.");

        let tx = pool_state.write().transactions.pop().unwrap();
        // Only one Tx submitted
        assert!(pool_state.read().transactions.is_empty());
        let tx = Extrinsic::decode(&mut &*tx).unwrap();
        assert!(matches!(tx.preamble, Preamble::Bare(_)));
        match tx.function {
            mock::RuntimeCall::EthereumEvents(crate::Call::submit_checkevent_result {
                result,
                ingress_counter: call_counter,
                signature: _,
                validator: _,
            }) => {
                assert_eq!(result.result, CheckResult::Invalid);
                assert_eq!(call_counter, ingress_counter)
            },
            _ => assert!(false),
        };
    });
}

// ==================== test process_event() ====================

#[test]
fn test_try_process_event_no_change_when_no_events() {
    let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
        .with_validators()
        .for_offchain_worker()
        .as_externality_with_state();

    ext.execute_with(|| {
        let validator = keys_setup_return_good_validator();
        // when
        EthereumEvents::try_process_event(1u64, &validator, 0u32.into());
        // then
        assert!(pool_state.read().transactions.is_empty());
    });
}

#[test]
fn test_send_event_ok() {
    let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
        .with_validators()
        .for_offchain_worker()
        .as_externality_with_state();

    ext.execute_with(|| {
        let validator = keys_setup_return_good_validator();
        let event = mock_event();
        let expected_signature = validator
            .key
            .sign(&(PROCESS_EVENT_CONTEXT, event.event_id, DEFAULT_INGRESS_COUNTER).encode())
            .unwrap();
        let event_result = mock_event_result();

        // no transactions in the pool before sending
        assert!(pool_state.read().transactions.is_empty());

        // when
        let _ =
            EthereumEvents::send_event(event_result.clone(), DEFAULT_INGRESS_COUNTER, &validator);
        // then
        let tx = pool_state.write().transactions.pop().unwrap();
        assert!(pool_state.read().transactions.is_empty());

        let tx = Extrinsic::decode(&mut &*tx).unwrap();
        assert!(matches!(tx.preamble, Preamble::Bare(_)));

        assert_eq!(
            tx.function,
            mock::RuntimeCall::EthereumEvents(crate::Call::process_event {
                event_id: event_result.event.event_id,
                ingress_counter: DEFAULT_INGRESS_COUNTER,
                validator,
                signature: expected_signature
            })
        );
    });
}

// ==================== test validate_event() ====================

#[test]
fn test_try_validate_event_no_change_when_no_events() {
    let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
        .with_validators()
        .for_offchain_worker()
        .as_externality_with_state();

    ext.execute_with(|| {
        let validator = keys_setup_return_good_validator();
        // when
        EthereumEvents::try_validate_event(1u64, &validator, 0u32.into());
        // then
        assert!(pool_state.read().transactions.is_empty());
    });
}

#[test]
fn test_validate_event_ok() {
    let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
        .with_validators()
        .with_genesis_config()
        .for_offchain_worker()
        .as_externality_with_state();

    ext.execute_with(|| {
        let block_number = 1;

        let validator = keys_setup_return_good_validator();
        let event = mock_event();
        let event_result = mock_event_result();
        let expected_counter = DEFAULT_INGRESS_COUNTER;

        let expected_challenge = Challenge::new(
            event.event_id.clone(),
            ChallengeReason::IncorrectResult,
            validator.account_id.clone(),
        );
        let expected_signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, &expected_challenge, expected_counter).encode())
            .unwrap();

        let log_data = "0x0000000000000000000000000000000000000000000000000000000005f5e100";
        let event_topics = "0x00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303";
        inject_ethereum_node_response(
            &mut offchain_state.write(),
            &event.event_id.transaction_hash,
            Some(test_json(
                &event.event_id.transaction_hash,
                &event.event_id.signature,
                &AVN::<TestRuntime>::get_bridge_contract_address(),
                log_data,
                event_topics,
                GOOD_STATUS,
                GOOD_BLOCK_CONFIRMATIONS,
            )),
        );

        let result = EthereumEvents::validate_event(
            block_number,
            event_result,
            DEFAULT_INGRESS_COUNTER,
            &validator,
        );
        assert!(result.is_ok(), "Validation of valid event with valid data failed");

        let tx = pool_state.write().transactions.pop().unwrap();
        // Only one Tx submitted
        assert!(pool_state.read().transactions.is_empty());
        let tx = Extrinsic::decode(&mut &*tx).unwrap();

        assert!(matches!(tx.preamble, Preamble::Bare(_)));

        assert_eq!(
            tx.function,
            mock::RuntimeCall::EthereumEvents(Call::challenge_event {
                challenge: expected_challenge,
                ingress_counter: DEFAULT_INGRESS_COUNTER,
                signature: expected_signature,
                validator
            })
        );
    });
}
