// Copyright 2022 Aventus Systems (UK) Ltd.

#![cfg(test)]

use crate::{
    mock::{RuntimeCall as Call, *},
    *,
};
use codec::Decode;
use frame_support::{assert_ok, unsigned::ValidateUnsigned};
use sp_avn_common::event_types::{EthEventId, ValidEvents};
use sp_core::hash::H256;
use sp_runtime::{testing::UintAuthorityId, BoundedVec};

mod test_get_contract_address_for {
    use super::*;

    fn get_contract_for_event_from_genesis(event_type: &ValidEvents) -> H160 {
        return match event_type {
            ValidEvents::AddedValidator => H160::from(BRIDGE_CONTRACT),
            ValidEvents::Lifted => H160::from(BRIDGE_CONTRACT),
            ValidEvents::NftMint => H160::from(NFT_CONTRACT),
            ValidEvents::NftTransferTo => H160::from(NFT_CONTRACT),
            ValidEvents::NftCancelListing => H160::from(NFT_CONTRACT),
            ValidEvents::NftEndBatchListing => H160::from(NFT_CONTRACT),
            ValidEvents::AvtGrowthLifted => H160::from(BRIDGE_CONTRACT),
            ValidEvents::AvtLowerClaimed => H160::from(BRIDGE_CONTRACT),
            ValidEvents::LiftedToPredictionMarket => H160::from(BRIDGE_CONTRACT),
            ValidEvents::Erc20DirectTransfer => H160::from(BRIDGE_CONTRACT),
            ValidEvents::LowerReverted => H160::from(BRIDGE_CONTRACT),
        }
    }

    #[test]
    fn add_validator_event() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let event = ValidEvents::AddedValidator;
            assert_eq!(
                AVN::<TestRuntime>::get_bridge_contract_address(),
                get_contract_for_event_from_genesis(&event),
            );
        });
    }

    #[test]
    fn lift_event() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let event = ValidEvents::Lifted;
            assert_eq!(
                AVN::<TestRuntime>::get_bridge_contract_address(),
                get_contract_for_event_from_genesis(&event),
            );
        });
    }
}

mod test_nft_contract {
    use super::*;

    #[test]
    fn nft_mint_event() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let event = ValidEvents::NftMint;
            assert_eq!(true, event.is_nft_event());
        });
    }

    #[test]
    fn nft_transfer_to_event() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let event = ValidEvents::NftTransferTo;
            assert_eq!(true, event.is_nft_event());
        });
    }

    #[test]
    fn nft_cancel_single_listing() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let event = ValidEvents::NftCancelListing;
            assert_eq!(true, event.is_nft_event());
        });
    }

    #[test]
    fn nft_cancel_batch_listing() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let event = ValidEvents::NftEndBatchListing;
            assert_eq!(true, event.is_nft_event());
        });
    }
}

/*
fn submit_checkevent_result(
    origin,
    result: EthEventCheckResult<T::BlockNumber>,
    authority_index: AuthIndex,
    // Signature and structural validation is already done in validate unsigned so no need to do it here
    _signature: <T::AuthorityId as RuntimeAppPublic>::Signature) -> DispatchResult
{
    * test good case:
        * return type is ok
        * correct event is deposited
        * event added to correct list
        * check event removed from UncheckedEvents list
        * check event added to PendingChallenges list
        * include different cases with different  CheckResults (even when the event Check failed)
    * test bad cases:
        * any problem with origin?
        * non existing event in unchecked list
        * bad validator index
        * bad signature
}
*/

#[test]
fn test_local_authority_keys_empty() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let current_node_validator = EthereumEvents::get_validator_for_current_node();
        assert!(current_node_validator.is_none());
    });
}

#[test]
fn test_local_authority_keys_valid() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        UintAuthorityId::set_all_keys(vec![1, 2, 3]);
        let current_node_validator = EthereumEvents::get_validator_for_current_node().unwrap();
        assert_eq!(current_node_validator.account_id, validator_id_2());
        assert_eq!(current_node_validator.key, UintAuthorityId(1));
    });
}

/*

fn parse_tier1_event(event_id: EthEventId, data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<EventData, Error<T>> {
    * main test of parsing logic is done in another file

    Here:
    * test good case:
        * pass a proper AddedValidator event
        * check we get an Ok(...) with the correct type
        * no need to check format of actual data of the event (tested somewhere else)
    * test bad case:
        * pass an invalid event signature
        * check it returns an error of the proper type
}

*/

#[test]
fn test_event_exists_in_system_with_no_entries() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::random(),
        };
        // test with an event that does not exist in any queue
        assert!(!EthereumEvents::event_exists_in_system(&event_id));
        assert!(!EthereumEvents::has_events_to_check());
        assert!(!EthereumEvents::has_events_to_validate());
    });
}

#[test]
fn test_event_exists_in_system_entry_in_unchecked_queue() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::random(),
        };
        EthereumEvents::insert_to_unchecked_events(&event_id, DEFAULT_INGRESS_COUNTER);
        // test with an event that exists in Unchecked Events only
        assert!(EthereumEvents::event_exists_in_system(&event_id));
        assert!(EthereumEvents::has_events_to_check());
        assert!(!EthereumEvents::has_events_to_validate());
    });
}

#[test]
fn test_is_primary_blocknumber_3() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let block_number = 3;
        let expected_primary = account_id_1();
        let result =
            EthereumEvents::is_primary_validator_for_block(block_number, &expected_primary);
        assert!(result.is_ok(), "Getting primary validator failed");
        assert_eq!(result.unwrap(), true);
    });
}

#[test]
fn test_event_exists_in_system_entry_in_pending_queue() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::random(),
        };
        EthereumEvents::insert_to_events_pending_challenge(
            DEFAULT_BLOCK,
            CheckResult::Unknown,
            &event_id,
            DEFAULT_INGRESS_COUNTER,
            &EventData::EmptyEvent,
            checked_by(),
            CHECKED_AT_BLOCK,
            MIN_CHALLENGE_VOTES,
        );
        // test with an event that exists in Pending Events only
        assert!(EthereumEvents::event_exists_in_system(&event_id));
        assert!(!EthereumEvents::has_events_to_check());
        assert!(EthereumEvents::has_events_to_validate());
    });
}

#[test]
fn test_event_exists_in_system_entry_in_processed_queue() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::random(),
        };
        assert_ok!(EthereumEvents::add_processed_event(&event_id, true));
        // Test with an event that exists in Processed Events only
        assert!(EthereumEvents::event_exists_in_system(&event_id));
        assert!(!EthereumEvents::has_events_to_check());
        assert!(!EthereumEvents::has_events_to_validate());
    });
}

#[test]
fn test_multiple_events_in_system() {
    let mut unchecked_events: BoundedVec<EthEventId, MaxUncheckedEvents> = Default::default();
    let mut pending_challenge_events: BoundedVec<EthEventId, MaxEventsPendingChallenges> =
        Default::default();

    for i in 0..10 {
        unchecked_events
            .try_push(EthEventId {
                signature: ValidEvents::Lifted.signature(),
                transaction_hash: H256::from([i; 32]),
            })
            .expect("Cannot push");
        pending_challenge_events
            .try_push(EthEventId {
                signature: ValidEvents::Lifted.signature(),
                transaction_hash: H256::from([i + 10; 32]),
            })
            .expect("Cannot push");
    }

    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        // No events exist in the system yet
        assert!(!EthereumEvents::has_events_to_check());
        assert!(!EthereumEvents::has_events_to_validate());
        for event in &unchecked_events {
            assert!(!EthereumEvents::event_exists_in_system(&event));
        }
        for event in &pending_challenge_events {
            assert!(!EthereumEvents::event_exists_in_system(&event));
        }

        // Insert unchecked event
        for event_id in &unchecked_events {
            EthereumEvents::insert_to_unchecked_events(event_id, DEFAULT_INGRESS_COUNTER);
        }
        assert!(EthereumEvents::has_events_to_check());
        assert!(!EthereumEvents::has_events_to_validate());

        for event in &unchecked_events {
            assert!(EthereumEvents::event_exists_in_system(&event));
        }
        for event in &pending_challenge_events {
            assert!(!EthereumEvents::event_exists_in_system(&event));
        }

        // Insert Pending Events
        let mut ingress_counter = DEFAULT_INGRESS_COUNTER;
        for event_id in &pending_challenge_events {
            EthereumEvents::insert_to_events_pending_challenge(
                DEFAULT_BLOCK,
                CheckResult::Unknown,
                &event_id,
                ingress_counter,
                &EventData::EmptyEvent,
                checked_by(),
                CHECKED_AT_BLOCK,
                MIN_CHALLENGE_VOTES,
            );
            ingress_counter += 1;
        }
        assert!(EthereumEvents::has_events_to_check());
        assert!(EthereumEvents::has_events_to_validate());
        for event in &pending_challenge_events {
            assert!(EthereumEvents::event_exists_in_system(&event));
        }
    });
}

#[test]
fn is_primary_fails_with_no_validators() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let block_number = 1;
        let result = EthereumEvents::is_primary_validator_for_block(block_number, &account_id_1());
        assert!(result.is_err(), "Getting primary validator should have failed");
    });
}

#[test]
fn test_compute_result_ok() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        EthereumEvents::setup_mock_ethereum_contracts_address();
        let block_number = 1;
        let tx_validator_hash: H256 = H256::random();
        let unchecked_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: tx_validator_hash,
        };

        let log_data = "0x0000000000000000000000000000000000000000000000000000000005f5e100";
        let event_topics = "0x00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303";
        let json = test_json(
            &unchecked_event.transaction_hash,
            &unchecked_event.signature,
            &AVN::<TestRuntime>::get_bridge_contract_address(),
            log_data,
            event_topics,
            GOOD_STATUS,
            GOOD_BLOCK_CONFIRMATIONS,
        );
        let result = EthereumEvents::compute_result(
            block_number,
            Ok(json),
            unchecked_event,
            &account_id_1(),
        );

        let expected_signature = ValidEvents::AddedValidator.signature();
        let expected_transaction_hash = tx_validator_hash;
        let expected_event_data_valid = true;
        let expected_result = CheckResult::Ok;
        let expected_authority_account_id = account_id_1();
        let expected_blocknumber = 1;

        assert_eq!(result.event.event_id.signature, expected_signature);
        assert_eq!(result.event.event_id.transaction_hash, expected_transaction_hash);
        assert_eq!(result.event.event_data.is_valid(), expected_event_data_valid);
        assert_eq!(result.result, expected_result);
        assert_eq!(result.checked_by, expected_authority_account_id);
        // ready_for_processing_at_block value should not be set here, so we skip the
        // checks.event_topics It should be set after the offchain worker finishes the tasks
        // and re-enters as a new Tx to the system
        assert_eq!(result.checked_at_block, expected_blocknumber);
    });
}

#[test]
fn test_compute_result_invalid_event_signature() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let block_number = 1;
        let tx_validator_hash: H256 = H256::random();
        let unchecked_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: tx_validator_hash,
        };
        let log_data = "0x0000000000000000000000000000000000000000000000000000000005f5e100";
        let event_topics = "0x00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303";
        let invalid_event_signature = H256::from([1; 32]);
        let json = test_json(
            &unchecked_event.transaction_hash,
            &invalid_event_signature,
            &AVN::<TestRuntime>::get_bridge_contract_address(),
            log_data,
            event_topics,
            GOOD_STATUS,
            GOOD_BLOCK_CONFIRMATIONS,
        );

        let result = EthereumEvents::compute_result(
            block_number,
            Ok(json),
            unchecked_event,
            &account_id_1(),
        );

        let expected_result = CheckResult::Invalid;

        assert_eq!(result.event.event_data, EventData::EmptyEvent);
        assert_eq!(result.result, expected_result);
    });
}

#[test]
fn test_compute_result_invalid_contract_address() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let block_number = 1;
        let tx_validator_hash: H256 = H256::random();
        let unchecked_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: tx_validator_hash,
        };
        let log_data = "0x0000000000000000000000000000000000000000000000000000000005f5e100";
        let event_topics = "0x00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303";

        let invalid_contract_address = H160::from([1; 20]);
        let json = test_json(
            &unchecked_event.transaction_hash,
            &unchecked_event.signature,
            &invalid_contract_address,
            log_data,
            event_topics,
            GOOD_STATUS,
            GOOD_BLOCK_CONFIRMATIONS,
        );
        let result = EthereumEvents::compute_result(
            block_number,
            Ok(json),
            unchecked_event,
            &account_id_1(),
        );

        let expected_result = CheckResult::Invalid;

        assert_eq!(result.event.event_data, EventData::EmptyEvent);
        assert_eq!(result.result, expected_result);
    });
}

#[test]
fn test_compute_result_invalid_log_data() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let block_number = 1;
        let tx_validator_hash: H256 = H256::random();
        let unchecked_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: tx_validator_hash,
        };
        let invalid_log_data = "0xblah";
        let event_topics = "0x00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3\",
                          \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303";

        let json = test_json(
            &unchecked_event.transaction_hash,
            &unchecked_event.signature,
            &AVN::<TestRuntime>::get_bridge_contract_address(),
            invalid_log_data,
            event_topics,
            GOOD_STATUS,
            GOOD_BLOCK_CONFIRMATIONS,
        );
        let result = EthereumEvents::compute_result(
            block_number,
            Ok(json),
            unchecked_event,
            &account_id_1(),
        );

        let expected_result = CheckResult::Invalid;

        assert_eq!(result.event.event_data, EventData::EmptyEvent);
        assert_eq!(result.result, expected_result);
    });
}

#[test]
fn test_compute_result_invalid_event_topics() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let block_number = 1;
        let tx_validator_hash: H256 = H256::random();
        let unchecked_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: tx_validator_hash,
        };
        let log_data = "0x0000000000000000000000000000000000000000000000000000000005f5e100";
        let invalid_event_topics = "0xblah";

        let json = test_json(
            &unchecked_event.transaction_hash,
            &unchecked_event.signature,
            &AVN::<TestRuntime>::get_bridge_contract_address(),
            log_data,
            invalid_event_topics,
            GOOD_STATUS,
            GOOD_BLOCK_CONFIRMATIONS,
        );
        let result = EthereumEvents::compute_result(
            block_number,
            Ok(json),
            unchecked_event,
            &account_id_1(),
        );

        let expected_result = CheckResult::Invalid;

        assert_eq!(result.event.event_data, EventData::EmptyEvent);
        assert_eq!(result.result, expected_result);
    });
}

#[test]
fn test_compute_result_empty_json() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let block_number = 1;
        let tx_validator_hash: H256 = H256::random();
        let unchecked_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: tx_validator_hash,
        };
        let json = String::from("{}").into_bytes();
        let result = EthereumEvents::compute_result(
            block_number,
            Ok(json),
            unchecked_event,
            &account_id_1(),
        );

        let expected_result = CheckResult::Invalid;

        assert_eq!(result.event.event_data, EventData::EmptyEvent);
        assert_eq!(result.result, expected_result);
    });
}

#[test]
fn test_compute_result_invalid_json() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let block_number = 1;
        let tx_validator_hash: H256 = H256::random();
        let unchecked_event = &EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: tx_validator_hash,
        };
        let json = String::from("bad").into_bytes();
        let result = EthereumEvents::compute_result(
            block_number,
            Ok(json),
            unchecked_event,
            &account_id_1(),
        );

        let expected_result = CheckResult::Invalid;

        assert_eq!(result.event.event_data, EventData::EmptyEvent);
        assert_eq!(result.result, expected_result);
    });
}

/*
fn fetch_event(unchecked_event: &EthEventId) -> Result<Vec<u8>, http::Error> {
    test good cases:
        * check return type is Ok
        * check content of Ok is correct
    test bad cases:
        * mock reply to create some IO Errors
        * mock reply to create different return types
        * check return type is Err(_)
}

fn get_post_body(transaction_hash: H256) -> String {
    [do nothing]
    [implicit testing as part of the previous one]
}

fn initialize_keys(keys: &[T::AuthorityId]) {
    [possibly implicit test somewhere else, as part of the session Config]
    [don't test for now]
}
*/

mod signature_in {
    use super::*;
    // the intention of this module is to invoke unsigned transactions from the lib code,
    // so that we can use the signature as they are calculated (and not as the tests compute them)
    // and check they are accepted in validate_unsigned

    mod submit_checkevent_result {
        use super::*;

        struct Context {
            block_number: u64,
            ingress_counter: u64,
            event_id: EthEventId,
            validator: Validator<AuthorityId, AccountId>,
        }

        fn setup() -> Context {
            let tx_hash: H256 = H256::from_slice(&[1u8; 32]);
            let block_number = 1u64;
            let ingress_counter = DEFAULT_INGRESS_COUNTER;
            let event_id = EthEventId {
                signature: ValidEvents::AddedValidator.signature(),
                transaction_hash: tx_hash.clone(),
            };

            let validator =
                prepare_to_invoke_function(block_number as usize, &event_id, ingress_counter);

            Context { block_number, ingress_counter, event_id, validator }
        }

        fn prepare_to_invoke_function(
            block_number: usize,
            event_id: &EthEventId,
            ingress_counter: u64,
        ) -> Validator<AuthorityId, AccountId> {
            UintAuthorityId::set_all_keys(vec![1, 2, 3]);

            let val_length = EthereumEvents::validators().len();
            let index_of_primary_validator = block_number % val_length;
            let validator = &EthereumEvents::validators()[index_of_primary_validator as usize];

            EthereumEvents::insert_to_unchecked_events(&event_id, ingress_counter);
            assert_eq!(EthereumEvents::unchecked_events().len(), 1);

            return validator.clone()
        }

        #[test]
        fn is_correctly_validated() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup();

                simulate_http_response(
                    &offchain_state,
                    &context.event_id,
                    GOOD_STATUS,
                    GOOD_BLOCK_CONFIRMATIONS,
                );

                let result = EthereumEvents::check_event_and_submit_result(
                    context.block_number,
                    &context.event_id,
                    context.ingress_counter,
                    &context.validator,
                );
                assert_ok!(result);

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    Call::EthereumEvents(inner_tx) => {
                        assert_ok!(EthereumEvents::validate_unsigned(
                            TransactionSource::Local,
                            &inner_tx
                        ));
                    },
                    _ => unreachable!(),
                }
            });
        }

        // Note: the previous test guarantees that validate_unsigned is aligned to the signatures
        // produced in the code This test guarantees that the signed data includes the
        // fields we want This could be made clearer with more tests, one testing each
        // field, but for now this will do
        #[test]
        fn includes_all_relevant_fields() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup();

                simulate_http_response(
                    &offchain_state,
                    &context.event_id,
                    GOOD_STATUS,
                    GOOD_BLOCK_CONFIRMATIONS,
                );

                let result = EthereumEvents::check_event_and_submit_result(
                    context.block_number,
                    &context.event_id,
                    context.ingress_counter,
                    &context.validator,
                );
                assert_ok!(result);

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    Call::EthereumEvents(crate::Call::submit_checkevent_result {
                        result,
                        ingress_counter: counter,
                        signature,
                        validator,
                    }) => {
                        let data: &[u8] = &(SUBMIT_CHECKEVENT_RESULT_CONTEXT, result, counter)
                            .using_encoded(|encoded_data| encoded_data.to_vec());
                        assert!(signature.verify(data, &validator.key.0));
                    },
                    _ => assert!(false),
                };
            });
        }
    }

    mod challenge_event {
        use super::*;

        struct Context {
            block_number: u64,
            ingress_counter: u64,
            event_id: EthEventId,
            result: EthEventCheckResult<BlockNumber, AccountId>,
            validator: Validator<AuthorityId, AccountId>,
        }

        fn setup() -> Context {
            let tx_hash: H256 = H256::from_slice(&[1u8; 32]);
            let block_number = 1u64;
            let ingress_counter = DEFAULT_INGRESS_COUNTER;
            let event_id = EthEventId {
                signature: ValidEvents::AddedValidator.signature(),
                transaction_hash: tx_hash.clone(),
            };

            let result = EthEventCheckResult::new(
                DEFAULT_BLOCK,
                CheckResult::Unknown,
                &event_id,
                &EventData::EmptyEvent,
                checked_by(),
                CHECKED_AT_BLOCK,
                MIN_CHALLENGE_VOTES,
            );

            let validator =
                prepare_to_invoke_function(block_number as usize, &event_id, ingress_counter);

            Context { block_number, ingress_counter, event_id, result, validator }
        }

        fn prepare_to_invoke_function(
            block_number: usize,
            event_id: &EthEventId,
            ingress_counter: u64,
        ) -> Validator<AuthorityId, AccountId> {
            UintAuthorityId::set_all_keys(vec![1, 2, 3]);

            let val_length = EthereumEvents::validators().len();
            let index_of_primary_validator = block_number % val_length;
            let validator = &EthereumEvents::validators()[index_of_primary_validator];

            let other_validator_account_id = account_id_0();

            // Insert filler challenges
            assert!(!EthereumEvents::has_events_to_validate());
            EthereumEvents::populate_events_pending_challenge(&validator.account_id, 2);
            EthereumEvents::populate_events_pending_challenge(&other_validator_account_id, 2);
            assert_eq!(EthereumEvents::events_pending_challenge().len(), 4);

            // Insert target challenge
            EthereumEvents::insert_to_events_pending_challenge(
                DEFAULT_BLOCK,
                CheckResult::Unknown,
                event_id,
                ingress_counter,
                &EventData::EmptyEvent,
                checked_by(),
                CHECKED_AT_BLOCK,
                MIN_CHALLENGE_VOTES,
            );

            return validator.clone()
        }

        #[test]
        fn is_correctly_validated() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();
            ext.execute_with(|| {
                let context = setup();

                simulate_http_response(
                    &offchain_state,
                    &context.event_id,
                    GOOD_STATUS,
                    GOOD_BLOCK_CONFIRMATIONS,
                );

                let result = EthereumEvents::validate_event(
                    context.block_number,
                    context.result,
                    context.ingress_counter,
                    &context.validator,
                );

                assert_ok!(result);

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    Call::EthereumEvents(inner_tx) => {
                        assert_ok!(EthereumEvents::validate_unsigned(
                            TransactionSource::Local,
                            &inner_tx
                        ));
                    },
                    _ => unreachable!(),
                }
            });
        }

        // Note: the previous test guarantees that validate_unsigned is aligned to the signatures
        // produced in the code This test guarantees that the signed data includes the
        // fields we want This could be made clearer with more tests, one testing each
        // field, but for now this will do
        #[test]
        fn includes_all_relevant_fields() {
            let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();
            ext.execute_with(|| {
                let context = setup();

                simulate_http_response(
                    &offchain_state,
                    &context.event_id,
                    GOOD_STATUS,
                    GOOD_BLOCK_CONFIRMATIONS,
                );

                let result = EthereumEvents::validate_event(
                    context.block_number,
                    context.result,
                    context.ingress_counter,
                    &context.validator,
                );

                assert_ok!(result);

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    Call::EthereumEvents(crate::Call::challenge_event {
                        challenge,
                        ingress_counter: counter,
                        signature,
                        validator,
                    }) => {
                        let data: &[u8] = &(CHALLENGE_EVENT_CONTEXT, challenge, counter)
                            .using_encoded(|encoded_data| encoded_data.to_vec());
                        assert!(signature.verify(data, &validator.key.0));
                    },
                    _ => assert!(false),
                };
            });
        }
    }

    mod process_event {
        use super::*;

        struct Context {
            ingress_counter: u64,
            result: EthEventCheckResult<BlockNumber, AccountId>,
            validator: Validator<AuthorityId, AccountId>,
        }

        fn setup() -> Context {
            let tx_hash: H256 = H256::from_slice(&[1u8; 32]);
            let block_number = 1u64;
            let ingress_counter = DEFAULT_INGRESS_COUNTER;
            let event_id = EthEventId {
                signature: ValidEvents::AddedValidator.signature(),
                transaction_hash: tx_hash.clone(),
            };

            let result = EthEventCheckResult::new(
                DEFAULT_BLOCK,
                CheckResult::Unknown,
                &event_id,
                &EventData::EmptyEvent,
                checked_by(),
                CHECKED_AT_BLOCK,
                MIN_CHALLENGE_VOTES,
            );

            let validator =
                prepare_to_invoke_function(block_number as usize, &event_id, ingress_counter);

            Context { ingress_counter, result, validator }
        }

        fn prepare_to_invoke_function(
            block_number: usize,
            event_id: &EthEventId,
            ingress_counter: u64,
        ) -> Validator<AuthorityId, AccountId> {
            UintAuthorityId::set_all_keys(vec![1, 2, 3]);

            let val_length = EthereumEvents::validators().len();
            let index_of_primary_validator = block_number % val_length;
            let validator = &EthereumEvents::validators()[index_of_primary_validator];

            EthereumEvents::insert_to_events_pending_challenge(
                DEFAULT_BLOCK,
                CheckResult::Unknown,
                event_id,
                ingress_counter,
                &EventData::EmptyEvent,
                checked_by(),
                CHECKED_AT_BLOCK,
                MIN_CHALLENGE_VOTES,
            );

            assert_eq!(EthereumEvents::events_pending_challenge().len(), 1);

            return validator.clone()
        }

        #[test]
        fn is_correctly_validated() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup();

                let result = EthereumEvents::send_event(
                    context.result,
                    context.ingress_counter,
                    &context.validator,
                );
                assert_ok!(result);

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    Call::EthereumEvents(inner_tx) => {
                        assert_ok!(EthereumEvents::validate_unsigned(
                            TransactionSource::Local,
                            &inner_tx
                        ));
                    },
                    _ => unreachable!(),
                }
            });
        }

        // Note: the previous test guarantees that validate_unsigned is aligned to the signatures
        // produced in the code This test guarantees that the signed data includes the
        // fields we want This could be made clearer with more tests, one testing each
        // field, but for now this will do
        #[test]
        fn includes_all_relevant_fields() {
            let (mut ext, pool_state, _offchain_state) = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .for_offchain_worker()
                .as_externality_with_state();

            ext.execute_with(|| {
                let context = setup();

                let result = EthereumEvents::send_event(
                    context.result,
                    context.ingress_counter,
                    &context.validator,
                );
                assert_ok!(result);

                let tx = pool_state.write().transactions.pop().unwrap();
                let tx = Extrinsic::decode(&mut &*tx).unwrap();

                match tx.function {
                    Call::EthereumEvents(crate::Call::process_event {
                        event_id,
                        ingress_counter: counter,
                        validator,
                        signature,
                    }) => {
                        let data: &[u8] = &(PROCESS_EVENT_CONTEXT, event_id, counter)
                            .using_encoded(|encoded_data| encoded_data.to_vec());
                        assert!(signature.verify(data, &validator.key.0));
                    },
                    _ => assert!(false),
                };
            });
        }
    }
}
