// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{
    eth::generate_encoded_lower_proof, mock::*, request::*, ActiveRequest, Request, RequestQueue,
    SettledTransactions,
};
use codec::{alloc::sync::Arc, Decode, Encode};
use frame_support::traits::Hooks;
use parking_lot::RwLock;
use sp_avn_common::{http_data_codec::encode_to_http_data, BridgeContractMethod, QuorumPolicy};
use sp_core::{
    ecdsa,
    offchain::testing::{OffchainState, PendingRequest, PoolState},
    H160, H256,
};
use sp_runtime::{
    testing::{TestSignature, UintAuthorityId},
    traits::Dispatchable,
};

pub fn mock_get_finalised_block(state: &mut OffchainState, response: &Option<Vec<u8>>) {
    let url = "http://127.0.0.1:2020/latest_finalised_block".to_string();

    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: url.into(),
        response: response.clone(),
        sent: true,
        ..Default::default()
    });
}

pub fn mock_ecdsa_sign(
    state: &mut OffchainState,
    proof: TestSignature,
    body: Vec<u8>,
    response: Option<Vec<u8>>,
) {
    let url = "http://127.0.0.1:2020/eth/sign_hashed_data".to_string();
    let proof_data = encode_to_http_data(&proof);
    state.expect_request(PendingRequest {
        method: "POST".into(),
        uri: url.into(),
        response,
        headers: vec![("X-Auth".to_owned(), proof_data)],
        sent: true,
        body: hex::encode(body).into_bytes(),
        ..Default::default()
    });
}

fn add_confirmations(count: u32) {
    let mut active_request = ActiveRequest::<TestRuntime>::get().unwrap();

    for (index, _) in (0..count).enumerate() {
        active_request
            .confirmation
            .confirmations
            .try_push(ecdsa::Signature::try_from(&[(index + 2) as u8; 65][0..65]).unwrap())
            .unwrap();
    }

    ActiveRequest::<TestRuntime>::put(active_request);
}

fn complete_send_request(context: &Context) {
    let mut active_request = ActiveRequest::<TestRuntime>::get().unwrap();

    active_request.tx_data.as_mut().unwrap().eth_tx_hash = context.eth_tx_hash;

    for (index, _) in (1..<TestRuntime as crate::Config>::Quorum::get_quorum()).enumerate() {
        active_request
            .tx_data
            .as_mut()
            .unwrap()
            .success_corroborations
            .try_push(index as u64)
            .unwrap();

        active_request
            .tx_data
            .as_mut()
            .unwrap()
            .valid_tx_hash_corroborations
            .try_push(index as u64)
            .unwrap();
    }

    ActiveRequest::<TestRuntime>::put(active_request.clone());

    EthBridge::add_corroboration(
        RuntimeOrigin::none(),
        active_request.as_active_tx::<TestRuntime, ()>().unwrap().request.tx_id,
        true,
        true,
        context.confirming_author.clone(),
        context.replay_attempt,
        context.test_signature.clone(),
    )
    .unwrap();
}

fn call_ocw(
    context: &Context,
    offchain_state: Arc<RwLock<OffchainState>>,
    author: AccountId,
    block_number: BlockNumber,
) {
    let h256 = H256::from_slice(
        &hex::decode(context.expected_lower_msg_hash.clone()).expect("failed to decode hex"),
    );
    mock_get_finalised_block(&mut offchain_state.write(), &context.finalised_block_vec);
    mock_ecdsa_sign(
        &mut offchain_state.write(),
        context.create_sign_proof(author),
        h256.as_ref().to_vec(),
        Some(hex::encode(&context.confirmation_signature).as_bytes().to_vec()),
    );
    UintAuthorityId::set_all_keys(vec![UintAuthorityId(author)]);

    let mut account_vec: [u8; 8] = Default::default();
    account_vec.copy_from_slice(&1u64.encode()[0..8]);
    set_mock_recovered_account_id(account_vec);

    EthBridge::offchain_worker(block_number);
}

fn call_ocw_and_dispatch(
    context: &Context,
    offchain_state: Arc<RwLock<OffchainState>>,
    pool_state: &Arc<RwLock<PoolState>>,
    author: AccountId,
    block_number: BlockNumber,
) {
    call_ocw(context, offchain_state, author, block_number);
    // Dispatch the transaction from the mempool
    let tx_bytes = pool_state.write().transactions.pop().unwrap();
    let tx = Extrinsic::decode(&mut &*tx_bytes).unwrap();
    assert!(tx.is_inherent());
    tx.function.dispatch(frame_system::RawOrigin::None.into()).map(|_| ()).unwrap();
}

mod lower_proofs {
    use super::*;
    use frame_support::assert_ok;

    #[test]
    fn lower_proof_request_can_be_added() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();

            add_new_lower_proof_request::<TestRuntime, ()>(
                context.lower_id,
                &context.lower_params,
                &vec![],
            )
            .unwrap();

            // Ensure the mem pool is empty
            assert_eq!(true, pool_state.read().transactions.is_empty());
            call_ocw(&context, offchain_state, 1u64, context.block_number);

            // A new active lower request is added
            let active_lower = ActiveRequest::<TestRuntime>::get().unwrap();
            assert_eq!(true, active_lower.request.id_matches(&context.lower_id));

            // A new confirmation is added to the pool
            assert_eq!(false, pool_state.read().transactions.is_empty());

            // Make sure the transaction in the mempool is what we expect to see
            let tx = pool_state.write().transactions.pop().unwrap();
            let tx = Extrinsic::decode(&mut &*tx).unwrap();

            assert!(matches!(
                tx.function,
                RuntimeCall::EthBridge(crate::Call::add_confirmation {
                    request_id: _,
                    confirmation: _,
                    author: _,
                    signature: _
                })
            ))
        });
    }

    #[test]
    fn lower_proof_request_can_be_generated() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();

            add_new_lower_proof_request::<TestRuntime, ()>(
                context.lower_id,
                &context.lower_params,
                &vec![],
            )
            .unwrap();
            // Add enough confirmations so the last one will complete the quorum
            add_confirmations(
                <TestRuntime as crate::Config>::Quorum::get_supermajority_quorum() - 1,
            );

            // ensure there is no request in storage
            assert!(ActiveRequest::<TestRuntime>::get().is_some());
            assert_eq!(false, lower_is_ready_to_be_claimed(&context.lower_id));

            // Ensure the mem pool is empty
            assert_eq!(true, pool_state.read().transactions.is_empty());
            call_ocw(&context, offchain_state, 1u64, context.block_number);

            // Make sure the transaction in the mempool is what we expect to see
            let tx = pool_state.write().transactions.pop().unwrap();
            let tx = Extrinsic::decode(&mut &*tx).unwrap();

            // Simulate sending the tx from the mem pool. Normally this would happen as
            // part of the ocw but in tests we have to dispatch it manually.
            tx.function.dispatch(frame_system::RawOrigin::None.into()).map(|_| ()).unwrap();

            // The proof should be generated now
            assert!(ActiveRequest::<TestRuntime>::get().is_none());
            assert_eq!(true, lower_is_ready_to_be_claimed(&context.lower_id));
        });
    }

    #[test]
    fn multiple_lower_proof_can_be_processed() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();

            add_new_lower_proof_request::<TestRuntime, ()>(
                context.lower_id,
                &context.lower_params,
                &vec![],
            )
            .unwrap();

            let new_lower_id = context.lower_id + 1;
            add_new_lower_proof_request::<TestRuntime, ()>(
                new_lower_id,
                &context.lower_params,
                &vec![],
            )
            .unwrap();

            // Add enough confirmations so the last one will complete the quorum
            add_confirmations(
                <TestRuntime as crate::Config>::Quorum::get_supermajority_quorum() - 1,
            );

            // Ensure the mem pool is empty
            assert_eq!(true, pool_state.read().transactions.is_empty());
            call_ocw(&context, offchain_state, 1u64, context.block_number);

            // Make sure the transaction in the mempool is what we expect to see
            let tx = pool_state.write().transactions.pop().unwrap();
            let tx = Extrinsic::decode(&mut &*tx).unwrap();

            // Simulate sending the tx from the mem pool
            tx.function.dispatch(frame_system::RawOrigin::None.into()).map(|_| ()).unwrap();

            // The proof should be generated now
            assert_eq!(true, lower_is_ready_to_be_claimed(&context.lower_id));

            // The next active request should be lower_id + 1
            let new_active_lower = ActiveRequest::<TestRuntime>::get().unwrap();
            assert_eq!(true, new_active_lower.request.id_matches(&new_lower_id));
        });
    }

    #[ignore]
    #[test]
    fn multiple_mixed_requests_with_same_id_can_be_processed() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let mut context = setup_context();

            // add a lower request as Active
            assert_ok!(add_new_lower_proof_request::<TestRuntime, ()>(
                context.lower_id,
                &context.lower_params,
                &vec![],
            ));

            // Queue a send tx request
            let tx_id = add_new_send_request::<TestRuntime, ()>(
                &BridgeContractMethod::RemoveAuthor.name_as_bytes().to_vec(),
                &context.request_params,
                &vec![],
            )
            .unwrap();
            assert!(context.lower_id != tx_id);

            // Re-use the same Id and queue another lower request
            let duplicate_lower_id = tx_id;
            let duplicate_lower_params = create_lower_params(duplicate_lower_id);

            add_new_lower_proof_request::<TestRuntime, ()>(
                duplicate_lower_id,
                &duplicate_lower_params,
                &vec![],
            )
            .unwrap();

            // Add enough confirmations to the 1st request so the last one will complete the quorum
            add_confirmations(
                <TestRuntime as crate::Config>::Quorum::get_supermajority_quorum() - 1,
            );
            call_ocw_and_dispatch(
                &context,
                offchain_state.clone(),
                &pool_state,
                1u64,
                context.block_number,
            );

            // Ensure the lower proof is generated (quorum is met)
            assert_eq!(true, lower_is_ready_to_be_claimed(&context.lower_id));

            // The next active request should be the send request
            let new_active_send = ActiveRequest::<TestRuntime>::get().unwrap();
            assert_eq!(true, new_active_send.request.id_matches(&tx_id));

            // The request in the queue should be the lower request with lower_id == tx_id
            let req_queue = RequestQueue::<TestRuntime>::get().unwrap();
            assert_eq!(true, req_queue[0].id_matches(&tx_id));

            // Add enough confirmations so the last one will complete the quorum
            // taking into account the sender (hence why -2 instead of -1)
            add_confirmations(<TestRuntime as crate::Config>::Quorum::get_quorum() - 2);

            let original_msg_hash = context.expected_lower_msg_hash.clone();
            // Update the hash to match the second request (tx_id) and call ocw
            context.expected_lower_msg_hash =
                "f6567b5fc754d7b5ec6543e28c68373851ec1cd91a7c228c8f1e4c40f8d9fd8d".to_string();
            call_ocw_and_dispatch(&context, offchain_state.clone(), &pool_state, 1u64, 2u64);

            // Because this is a send, active request doesn't change (next phase is to send and
            // corroborate)
            assert_eq!(
                true,
                ActiveRequest::<TestRuntime>::get().unwrap().request.id_matches(&tx_id)
            );
            assert_eq!(true, RequestQueue::<TestRuntime>::get().unwrap()[0].id_matches(&tx_id));

            complete_send_request(&context);
            // Ensure the send transaction is completed
            assert!(SettledTransactions::<TestRuntime>::contains_key(tx_id));

            // The next active request should be the final lower proof request
            let new_active_lower = ActiveRequest::<TestRuntime>::get().unwrap();
            assert_eq!(true, new_active_lower.request.id_matches(&duplicate_lower_id));

            // Add enough confirmations
            add_confirmations(
                <TestRuntime as crate::Config>::Quorum::get_supermajority_quorum() - 1,
            );

            // Reset hash for the new lower id
            context.expected_lower_msg_hash = original_msg_hash;

            call_ocw_and_dispatch(&context, offchain_state.clone(), &pool_state, 1u64, 3u64);

            // Ensure the lower proof is generated (quorum is met)
            assert_eq!(true, lower_is_ready_to_be_claimed(&context.lower_id));
            assert_eq!(true, lower_is_ready_to_be_claimed(&duplicate_lower_id));

            // No active request left
            assert_eq!(true, ActiveRequest::<TestRuntime>::get().is_none());
        });
    }
}

mod lower_proof_encoding {
    use sp_avn_common::eth::concat_lower_data;

    use super::*;

    #[test]
    fn check_lower_message_hash_generation() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let lower_id = 10u32;
            let token_id = H160::from([3u8; 20]);
            let amount = 100_000_000_000_000_000_000u128;
            let t1_recipient = H160::from([2u8; 20]);
            let t2_sender = H256::from([4u8; 32]);
            let t2_timestamp = 1_000_000_000u64;

            let params = concat_lower_data(
                lower_id,
                token_id,
                &amount,
                &t1_recipient,
                t2_sender,
                t2_timestamp,
            );
            let expected_msg_hash =
                "d89f2a698b48feb1e3248027e48e853e973fbf8e090e36dc00e6fd731d9c0df5";

            add_new_lower_proof_request::<TestRuntime, ()>(lower_id, &params, &vec![]).unwrap();
            let active_req = ActiveRequest::<TestRuntime>::get().expect("is active");
            assert_eq!(true, active_req.request.id_matches(&lower_id));

            let msg_hash = hex::encode(active_req.confirmation.msg_hash);
            assert_eq!(msg_hash, expected_msg_hash);
        })
    }

    #[test]
    fn check_lower_proof_encoding() {
        let (mut ext, _pool_state, _offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let lower_id = 0u32;
            let token_id = H160(hex_literal::hex!("97d9b397189e8b771ffac3cb04cf26c780a93431"));
            let amount = 10u128;
            let t1_recipient = H160(hex_literal::hex!("de7e1091cde63c05aa4d82c62e4c54edbc701b22"));
            let t2_sender = H256(hex_literal::hex!("df527229a93a80c6d3f82c10ac618d88fec68d54fdcfa423c9483ab3b0d6bcd7"));
            let t2_timestamp = 1767225600u64;

            let params = concat_lower_data(
                lower_id,
                token_id,
                &amount,
                &t1_recipient,
                t2_sender,
                t2_timestamp,
            );

            add_new_lower_proof_request::<TestRuntime, ()>(lower_id, &params, &vec![]).unwrap();
            let active_req = ActiveRequest::<TestRuntime>::get().expect("is active");

            let expected_encoded_proof = "97d9b397189e8b771ffac3cb04cf26c780a93431000000000000000000000000000000000000000000000000000000000000000ade7e1091cde63c05aa4d82c62e4c54edbc701b2200000000df527229a93a80c6d3f82c10ac618d88fec68d54fdcfa423c9483ab3b0d6bcd7000000006955b900";
            if let Request::LowerProof(lower_req) = active_req.request {
                let encoded_proof = generate_encoded_lower_proof::<TestRuntime, ()>(&lower_req, active_req.confirmation.confirmations);
                assert_eq!(expected_encoded_proof, hex::encode(encoded_proof));
            } else {
                assert!(false, "active request is not a lower proof");
            }
        })
    }
}
