use crate::{
    encode_signed_submit_checkpoint_params, mock::*, tests::RuntimeCall, AssetIdToChainId,
    ChainHandlers, CheckpointData, CheckpointId, Error, Event, NextCheckpointId, Nonces,
    RegisteredAppchains, SUBMIT_CHECKPOINT, UPDATE_CHAIN_HANDLER,
};
use codec::Encode;
use frame_support::{assert_noop, assert_ok, BoundedVec};
use pallet_avn_proxy::Error as avn_proxy_error;
use sp_avn_common::{primitives::CurrencyId, Asset, Proof};
use sp_core::{sr25519, ConstU32, Pair, H256};
use sp_runtime::{traits::Hash, DispatchError};

fn create_account_id(seed: u8) -> AccountId {
    create_account_pair(seed).public()
}

fn create_account_pair(seed: u8) -> sr25519::Pair {
    sr25519::Pair::from_seed(&[seed; 32])
}

fn bounded_vec(input: &[u8]) -> BoundedVec<u8, ConstU32<32>> {
    BoundedVec::<u8, ConstU32<32>>::try_from(input.to_vec()).unwrap()
}

/// Directly inserts a chain handler into storage, bypassing the deprecated extrinsic.
fn setup_chain(handler: AccountId) -> u32 {
    use crate::NextChainId;
    let chain_id = AvnAnchor::next_chain_id();
    NextChainId::<TestRuntime>::mutate(|id| *id = id.saturating_add(1));
    ChainHandlers::<TestRuntime>::insert(handler, chain_id);
    Nonces::<TestRuntime>::insert(chain_id, 0u64);
    chain_id
}

fn create_proof(
    signer_pair: &sr25519::Pair,
    relayer: &AccountId,
    payload: &[u8],
) -> Proof<Signature, AccountId> {
    let signature = Signature::from(signer_pair.sign(payload));
    Proof { signer: signer_pair.public(), relayer: relayer.clone(), signature }
}

#[test]
fn register_chain_handler_is_deprecated() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let name = bounded_vec(b"Test Chain");

        assert_noop!(
            AvnAnchor::register_chain_handler(RuntimeOrigin::signed(handler), name),
            Error::<TestRuntime>::CallDeprecated
        );
        assert!(AvnAnchor::chain_handlers(handler).is_none());
    });
}

#[test]
fn signed_register_chain_handler_is_deprecated() {
    new_test_ext().execute_with(|| {
        let handler_pair = create_account_pair(1);
        let handler = handler_pair.public();
        let relayer = create_account_id(2);
        let name = bounded_vec(b"Test Chain");

        let proof = Proof {
            signer: handler,
            relayer,
            signature: sp_core::sr25519::Signature::default().into(),
        };
        assert_noop!(
            AvnAnchor::signed_register_chain_handler(
                RuntimeOrigin::signed(handler),
                proof,
                handler,
                name
            ),
            Error::<TestRuntime>::CallDeprecated
        );
        assert!(AvnAnchor::chain_handlers(handler).is_none());
    });
}

#[test]
fn update_chain_handler_works() {
    new_test_ext().execute_with(|| {
        let old_handler = create_account_id(1);
        let new_handler = create_account_id(2);

        setup_chain(old_handler);
        assert_ok!(AvnAnchor::update_chain_handler(
            RuntimeOrigin::signed(old_handler),
            new_handler
        ));

        assert!(AvnAnchor::chain_handlers(old_handler).is_none());
        let chain_id = AvnAnchor::chain_handlers(new_handler).unwrap();
        assert_eq!(chain_id, 0);

        System::assert_last_event(Event::ChainHandlerUpdated(old_handler, new_handler, 0).into());
    });
}

#[test]
fn update_chain_handler_fails_for_non_existent_handler() {
    new_test_ext().execute_with(|| {
        let old_handler = create_account_id(1);
        let new_handler = create_account_id(2);

        assert_noop!(
            AvnAnchor::update_chain_handler(RuntimeOrigin::signed(old_handler), new_handler),
            Error::<TestRuntime>::ChainNotRegistered
        );
    });
}

#[test]
fn update_chain_handler_fails_for_already_registered_new_handler() {
    new_test_ext().execute_with(|| {
        let handler1 = create_account_id(1);
        let handler2 = create_account_id(2);

        setup_chain(handler1);
        setup_chain(handler2);

        assert_noop!(
            AvnAnchor::update_chain_handler(RuntimeOrigin::signed(handler1), handler2),
            Error::<TestRuntime>::HandlerAlreadyRegistered
        );
    });
}

#[test]
fn update_chain_handler_fails_for_non_handler() {
    new_test_ext().execute_with(|| {
        let current_handler = create_account_id(1);
        let new_handler = create_account_id(2);
        let unauthorized_account = create_account_id(3);

        setup_chain(current_handler);

        assert_noop!(
            AvnAnchor::update_chain_handler(
                RuntimeOrigin::signed(unauthorized_account),
                new_handler
            ),
            Error::<TestRuntime>::ChainNotRegistered
        );

        assert_eq!(AvnAnchor::chain_handlers(current_handler), Some(0));

        assert_ok!(AvnAnchor::update_chain_handler(
            RuntimeOrigin::signed(current_handler),
            new_handler
        ));

        assert!(AvnAnchor::chain_handlers(current_handler).is_none());
        assert_eq!(AvnAnchor::chain_handlers(new_handler), Some(0));

        System::assert_last_event(
            Event::ChainHandlerUpdated(current_handler, new_handler, 0).into(),
        );
    });
}

#[test]
fn submit_checkpoint_with_identity_works() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let checkpoint = H256::random();
        let origin_id = 42u64;

        let chain_id = setup_chain(handler);
        let default_fee = DefaultCheckpointFee::get();

        // Submit checkpoint
        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler),
            checkpoint,
            origin_id
        ));

        let stored_checkpoint_id = AvnAnchor::origin_id_to_checkpoint(chain_id, origin_id)
            .expect("Origin ID mapping should exist");
        assert_eq!(stored_checkpoint_id, 0); // First checkpoint should have ID 0

        let stored_checkpoint = AvnAnchor::checkpoints(chain_id, stored_checkpoint_id)
            .expect("Checkpoint should exist");
        assert_eq!(stored_checkpoint.hash, checkpoint);
        assert_eq!(stored_checkpoint.origin_id, origin_id);

        System::assert_has_event(
            Event::CheckpointSubmitted(handler, chain_id, stored_checkpoint_id, checkpoint).into(),
        );
        System::assert_has_event(
            Event::CheckpointFeeCharged { handler, chain_id, fee: default_fee }.into(),
        );
    });
}

#[test]
fn submit_checkpoint_with_identity_fails_for_unregistered_handler() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let checkpoint = H256::random();
        let origin_id = 42u64;

        assert_noop!(
            AvnAnchor::submit_checkpoint_with_identity(
                RuntimeOrigin::signed(handler),
                checkpoint,
                origin_id
            ),
            Error::<TestRuntime>::ChainNotRegistered
        );
    });
}

#[test]
fn submit_multiple_checkpoints_increments_checkpoint_id() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let checkpoint1 = H256::random();
        let origin_id1 = 42u64;
        let checkpoint2 = H256::random();
        let origin_id2 = 43u64;

        let chain_id = setup_chain(handler);

        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler),
            checkpoint1,
            origin_id1
        ));
        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler),
            checkpoint2,
            origin_id2
        ));

        assert_eq!(AvnAnchor::origin_id_to_checkpoint(chain_id, origin_id1), Some(0));
        assert_eq!(AvnAnchor::origin_id_to_checkpoint(chain_id, origin_id2), Some(1));

        assert_eq!(
            AvnAnchor::checkpoints(chain_id, 0),
            Some(CheckpointData { hash: checkpoint1, origin_id: origin_id1 })
        );
        assert_eq!(
            AvnAnchor::checkpoints(chain_id, 1),
            Some(CheckpointData { hash: checkpoint2, origin_id: origin_id2 })
        );
        assert_eq!(AvnAnchor::next_checkpoint_id(chain_id), 2);

        System::assert_has_event(
            Event::CheckpointSubmitted(handler, chain_id, 0, checkpoint1).into(),
        );
        System::assert_has_event(
            Event::CheckpointSubmitted(handler, chain_id, 1, checkpoint2).into(),
        );
    });
}

#[test]
fn submit_checkpoints_for_multiple_chains() {
    new_test_ext().execute_with(|| {
        let handler1 = create_account_id(1);
        let handler2 = create_account_id(2);
        let checkpoint1 = H256::random();
        let origin_id1 = 42u64;
        let checkpoint2 = H256::random();
        let origin_id2 = 43u64;
        let checkpoint3 = H256::random();
        let origin_id3 = 44u64;

        setup_chain(handler1);
        setup_chain(handler2);

        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler1),
            checkpoint1,
            origin_id1
        ));
        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler2),
            checkpoint2,
            origin_id2
        ));
        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler1),
            checkpoint3,
            origin_id3
        ));

        assert_eq!(
            AvnAnchor::checkpoints(0, 0),
            Some(CheckpointData { hash: checkpoint1, origin_id: origin_id1 })
        );
        assert_eq!(
            AvnAnchor::checkpoints(1, 0),
            Some(CheckpointData { hash: checkpoint2, origin_id: origin_id2 })
        );
        assert_eq!(
            AvnAnchor::checkpoints(0, 1),
            Some(CheckpointData { hash: checkpoint3, origin_id: origin_id3 })
        );

        assert_eq!(AvnAnchor::next_checkpoint_id(0), 2);
        assert_eq!(AvnAnchor::next_checkpoint_id(1), 1);

        System::assert_has_event(Event::CheckpointSubmitted(handler1, 0, 0, checkpoint1).into());
        System::assert_has_event(Event::CheckpointSubmitted(handler2, 1, 0, checkpoint2).into());
        System::assert_has_event(Event::CheckpointSubmitted(handler1, 0, 1, checkpoint3).into());
    });
}

#[test]
fn register_multiple_chains_increments_chain_id() {
    new_test_ext().execute_with(|| {
        let handler1 = create_account_id(1);
        let handler2 = create_account_id(2);

        let chain_id1 = setup_chain(handler1);
        let chain_id2 = setup_chain(handler2);

        assert_eq!(chain_id1, 0);
        assert_eq!(chain_id2, 1);
        assert_eq!(AvnAnchor::chain_handlers(handler1), Some(0));
        assert_eq!(AvnAnchor::chain_handlers(handler2), Some(1));
    });
}

#[test]
fn proxy_signed_register_chain_handler_returns_deprecated() {
    new_test_ext().execute_with(|| {
        let handler_pair = create_account_pair(1);
        let handler_account = handler_pair.public();
        let relayer = create_account_id(2);
        let name = bounded_vec(b"Test Chain");

        let proof = Proof {
            signer: handler_account,
            relayer: relayer.clone(),
            signature: sp_core::sr25519::Signature::default().into(),
        };
        let call = Box::new(RuntimeCall::AvnAnchor(
            super::Call::<TestRuntime>::signed_register_chain_handler {
                proof,
                handler: handler_account.clone(),
                name,
            },
        ));

        assert_ok!(AvnProxy::proxy(RuntimeOrigin::signed(relayer.clone()), call.clone(), None));

        // The inner call should have failed with CallDeprecated and left no state
        assert!(AvnAnchor::chain_handlers(handler_account).is_none());
        assert!(inner_call_failed_event_emitted(Error::<TestRuntime>::CallDeprecated.into()));
    });
}

#[test]
fn signed_update_chain_handler_works() {
    new_test_ext().execute_with(|| {
        let old_handler_pair = create_account_pair(1);
        let old_handler = old_handler_pair.public();
        let new_handler = create_account_id(2);
        let relayer = create_account_id(3);

        let chain_id = setup_chain(old_handler);
        let nonce = AvnAnchor::nonces(chain_id);
        let payload = (
            UPDATE_CHAIN_HANDLER,
            relayer.clone(),
            old_handler.clone(),
            new_handler.clone(),
            chain_id,
            nonce,
        )
            .encode();
        let proof = create_proof(&old_handler_pair, &relayer, &payload);

        let call = Box::new(RuntimeCall::AvnAnchor(
            super::Call::<TestRuntime>::signed_update_chain_handler {
                proof: proof.clone(),
                old_handler: old_handler.clone(),
                new_handler: new_handler.clone(),
            },
        ));

        assert_ok!(AvnProxy::proxy(RuntimeOrigin::signed(relayer.clone()), call.clone(), None));

        assert!(AvnAnchor::chain_handlers(old_handler).is_none());
        let updated_chain_id = AvnAnchor::chain_handlers(new_handler).unwrap();
        assert_eq!(updated_chain_id, chain_id);

        assert!(proxy_event_emitted(
            relayer.clone(),
            <TestRuntime as frame_system::Config>::Hashing::hash_of(&call)
        ));
    });
}

#[test]
fn signed_submit_checkpoint_with_identity_works() {
    new_test_ext().execute_with(|| {
        let handler_pair = create_account_pair(1);
        let handler = handler_pair.public();
        let relayer = create_account_id(2);
        let checkpoint = H256::random();
        let origin_id = 42u64;

        setup_balance::<TestRuntime>(&handler);
        setup_balance::<TestRuntime>(&relayer);

        let chain_id = setup_chain(handler);
        let nonce = AvnAnchor::nonces(chain_id);
        let initial_balance = Balances::free_balance(&handler);

        let payload = encode_signed_submit_checkpoint_params::<TestRuntime>(
            &relayer,
            &handler,
            &checkpoint,
            chain_id,
            nonce,
            &origin_id,
        );
        let proof = create_proof(&handler_pair, &relayer, &payload);

        let call = Box::new(RuntimeCall::AvnAnchor(
            super::Call::<TestRuntime>::signed_submit_checkpoint_with_identity {
                proof: proof.clone(),
                handler: handler.clone(),
                checkpoint,
                origin_id,
            },
        ));

        assert_ok!(AvnProxy::proxy(RuntimeOrigin::signed(relayer.clone()), call.clone(), None));

        assert_eq!(AvnAnchor::origin_id_to_checkpoint(chain_id, origin_id), Some(0));
        let final_balance = Balances::free_balance(&handler);
        let actual_checkpoint = AvnAnchor::checkpoints(chain_id, 0).unwrap();
        assert_eq!(actual_checkpoint.hash, checkpoint);
        assert_eq!(actual_checkpoint.origin_id, origin_id);
        assert_eq!(AvnAnchor::next_checkpoint_id(chain_id), 1);

        System::assert_has_event(
            Event::CheckpointSubmitted(handler.clone(), chain_id, 0, checkpoint).into(),
        );

        assert!(proxy_event_emitted(
            relayer.clone(),
            <TestRuntime as frame_system::Config>::Hashing::hash_of(&call)
        ));

        assert!(final_balance < initial_balance, "Fee was not deducted");
    });
}

#[test]
fn proxy_signed_register_chain_handler_fails_with_wrong_relayer() {
    new_test_ext().execute_with(|| {
        let handler_pair = create_account_pair(1);
        let handler = handler_pair.public();
        let relayer = create_account_id(2);
        let wrong_relayer = create_account_id(3);
        let name = bounded_vec(b"Test Chain");

        let proof = Proof {
            signer: handler,
            relayer: relayer.clone(),
            signature: sp_core::sr25519::Signature::default().into(),
        };
        let call = Box::new(RuntimeCall::AvnAnchor(
            super::Call::<TestRuntime>::signed_register_chain_handler {
                proof,
                handler: handler.clone(),
                name,
            },
        ));

        assert_noop!(
            AvnProxy::proxy(RuntimeOrigin::signed(wrong_relayer), call.clone(), None),
            avn_proxy_error::<TestRuntime>::UnauthorizedProxyTransaction
        );
    });
}

#[test]
fn proxy_signed_update_chain_handler_fails_with_invalid_signature() {
    new_test_ext().execute_with(|| {
        let old_handler_pair = create_account_pair(1);
        let old_handler = old_handler_pair.public();
        let new_handler = create_account_id(2);
        let relayer = create_account_id(3);

        setup_chain(old_handler);

        let invalid_payload = b"invalid payload";
        let invalid_signature = old_handler_pair.sign(invalid_payload);

        let proof = Proof {
            signer: old_handler.clone(),
            relayer: relayer.clone(),
            signature: invalid_signature,
        };

        let call = Box::new(RuntimeCall::AvnAnchor(
            super::Call::<TestRuntime>::signed_update_chain_handler {
                proof,
                old_handler: old_handler.clone(),
                new_handler: new_handler.clone(),
            },
        ));

        assert_ok!(AvnProxy::proxy(RuntimeOrigin::signed(relayer.clone()), call.clone(), None),);

        assert_eq!(
            true,
            inner_call_failed_event_emitted(
                avn_proxy_error::<TestRuntime>::UnauthorizedProxyTransaction.into()
            )
        );
    });
}

#[test]
fn proxy_signed_submit_checkpoint_with_identity_fails_with_unregistered_handler() {
    new_test_ext().execute_with(|| {
        let registered_handler = create_account_id(1);
        setup_chain(registered_handler);

        let unauthorized_handler_pair = create_account_pair(2);
        let unauthorized_handler = unauthorized_handler_pair.public();
        let relayer = create_account_id(3);
        let checkpoint = H256::random();
        let origin_id: u64 = 0;

        let chain_id = 0;
        let nonce: u64 = AvnAnchor::nonces(chain_id);
        let payload = (
            SUBMIT_CHECKPOINT,
            relayer.clone(),
            unauthorized_handler.clone(),
            checkpoint,
            chain_id,
            nonce,
        )
            .encode();
        let proof = create_proof(&unauthorized_handler_pair, &relayer, &payload);

        let call = Box::new(RuntimeCall::AvnAnchor(
            super::Call::<TestRuntime>::signed_submit_checkpoint_with_identity {
                proof,
                handler: unauthorized_handler.clone(),
                checkpoint,
                origin_id,
            },
        ));

        assert_ok!(AvnProxy::proxy(RuntimeOrigin::signed(relayer.clone()), call.clone(), None));

        assert!(inner_call_failed_event_emitted(
            avn_proxy_error::<TestRuntime>::UnauthorizedProxyTransaction.into()
        ));
    });
}

#[test]
fn checkpoint_id_overflow_fails() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let checkpoint = H256::random();
        let origin_id = 0;

        setup_chain(handler);

        NextCheckpointId::<TestRuntime>::insert(0, CheckpointId::MAX);

        assert_noop!(
            AvnAnchor::submit_checkpoint_with_identity(
                RuntimeOrigin::signed(handler),
                checkpoint,
                origin_id
            ),
            Error::<TestRuntime>::NoAvailableCheckpointId
        );
    });
}

// Fees
#[test]
fn set_checkpoint_fee_works() {
    new_test_ext().execute_with(|| {
        let chain_id = 0;
        let new_fee = 100;

        assert_ok!(AvnAnchor::set_checkpoint_fee(RuntimeOrigin::root(), chain_id, new_fee));

        assert_eq!(AvnAnchor::checkpoint_fee(chain_id), new_fee);
        System::assert_last_event(Event::CheckpointFeeUpdated { chain_id, new_fee }.into());
    });
}

#[test]
fn set_checkpoint_fee_fails_for_non_root() {
    new_test_ext().execute_with(|| {
        let non_root = create_account_id(1);
        let chain_id = 0;
        let new_fee = 100;

        assert_noop!(
            AvnAnchor::set_checkpoint_fee(RuntimeOrigin::signed(non_root), chain_id, new_fee),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn charge_fee_works() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let chain_id = 0;
        let fee = 100;

        assert_ok!(AvnAnchor::set_checkpoint_fee(RuntimeOrigin::root(), chain_id, fee));

        assert_ok!(AvnAnchor::charge_fee(handler.clone(), chain_id));

        System::assert_last_event(
            Event::CheckpointFeeCharged { handler: handler.clone(), chain_id, fee }.into(),
        );
    });
}

#[test]
fn submit_checkpoint_charges_correct_fee() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let checkpoint = H256::random();
        let origin_id = 0;

        let chain_id = setup_chain(handler);
        let fee = 100;
        assert_ok!(AvnAnchor::set_checkpoint_fee(RuntimeOrigin::root(), chain_id, fee));

        let balance_before = get_balance(&handler);

        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler),
            checkpoint,
            origin_id
        ));

        let balance_after = get_balance(&handler);
        assert_eq!(
            balance_before - fee,
            balance_after,
            "Handler balance should be reduced by exactly the checkpoint fee amount"
        );

        System::assert_has_event(
            Event::CheckpointSubmitted(handler, chain_id, 0, checkpoint).into(),
        );
        System::assert_has_event(Event::CheckpointFeeCharged { handler, chain_id, fee }.into());
    });
}

#[test]
fn submit_checkpoint_charges_zero_fee() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let checkpoint = H256::random();
        let origin_id = 0;

        let chain_id = setup_chain(handler);
        let fee = 0;
        assert_ok!(AvnAnchor::set_checkpoint_fee(RuntimeOrigin::root(), chain_id, fee));

        let balance_before = get_balance(&handler);

        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler),
            checkpoint,
            origin_id
        ));

        let balance_after = get_balance(&handler);
        assert_eq!(
            balance_before, balance_after,
            "Handler balance should remain unchanged when checkpoint fee is zero"
        );

        System::assert_has_event(
            Event::CheckpointSubmitted(handler, chain_id, 0, checkpoint).into(),
        );
        System::assert_has_event(Event::CheckpointFeeCharged { handler, chain_id, fee }.into());
    });
}

#[test]
fn different_chains_can_have_different_fees() {
    new_test_ext().execute_with(|| {
        let handler1 = create_account_id(1);
        let handler2 = create_account_id(2);
        let fee1 = 100;
        let fee2 = 200;

        setup_chain(handler1);
        setup_chain(handler2);

        let chain_id1 = AvnAnchor::chain_handlers(handler1).unwrap();
        let chain_id2 = AvnAnchor::chain_handlers(handler2).unwrap();

        assert_ok!(AvnAnchor::set_checkpoint_fee(RuntimeOrigin::root(), chain_id1, fee1));
        assert_ok!(AvnAnchor::set_checkpoint_fee(RuntimeOrigin::root(), chain_id2, fee2));

        assert_eq!(AvnAnchor::checkpoint_fee(chain_id1), fee1);
        assert_eq!(AvnAnchor::checkpoint_fee(chain_id2), fee2);
    });
}

#[test]
fn default_fee_applies_when_no_override() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);

        let chain_id = setup_chain(handler);

        assert_eq!(AvnAnchor::checkpoint_fee(chain_id), DefaultCheckpointFee::get());
    });
}

#[test]
fn fee_override_works() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let override_fee = 500u128;

        let chain_id = setup_chain(handler);

        assert_eq!(AvnAnchor::checkpoint_fee(chain_id), DefaultCheckpointFee::get());

        assert_ok!(AvnAnchor::set_checkpoint_fee(RuntimeOrigin::root(), chain_id, override_fee));

        assert_eq!(AvnAnchor::checkpoint_fee(chain_id), override_fee);

        let other_chain_id = chain_id + 1;
        assert_eq!(AvnAnchor::checkpoint_fee(other_chain_id), DefaultCheckpointFee::get());
    });
}

#[test]
fn submit_checkpoint_fails_with_duplicate_origin_id() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let checkpoint1 = H256::random();
        let checkpoint2 = H256::random();
        let origin_id = 42u64; // Same origin_id for both submissions

        let chain_id = setup_chain(handler);

        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler),
            checkpoint1,
            origin_id
        ));

        assert_eq!(AvnAnchor::origin_id_to_checkpoint(chain_id, origin_id), Some(0));

        assert_noop!(
            AvnAnchor::submit_checkpoint_with_identity(
                RuntimeOrigin::signed(handler),
                checkpoint2,
                origin_id
            ),
            Error::<TestRuntime>::CheckpointOriginAlreadyExists
        );
    });
}

#[test]
fn origin_id_uniqueness_is_per_chain() {
    new_test_ext().execute_with(|| {
        let handler1 = create_account_id(1);
        let handler2 = create_account_id(2);
        let checkpoint1 = H256::random();
        let checkpoint2 = H256::random();
        let origin_id = 42u64;

        let chain_id1 = setup_chain(handler1);
        let chain_id2 = setup_chain(handler2);

        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler1),
            checkpoint1,
            origin_id
        ));
        assert_ok!(AvnAnchor::submit_checkpoint_with_identity(
            RuntimeOrigin::signed(handler2),
            checkpoint2,
            origin_id
        ));

        assert_eq!(AvnAnchor::origin_id_to_checkpoint(chain_id1, origin_id), Some(0));
        assert_eq!(AvnAnchor::origin_id_to_checkpoint(chain_id2, origin_id), Some(0));
    });
}

// register_appchain
fn make_token(seed: u8) -> sp_core::H160 {
    sp_core::H160::from([seed; 20])
}

fn register_appchain_call(
    handler: AccountId,
    name: &[u8],
    symbol: &[u8],
    token: sp_core::H160,
    asset_id: CurrencyId,
) -> frame_support::dispatch::DispatchResult {
    AvnAnchor::register_appchain(
        RuntimeOrigin::root(),
        handler,
        bounded_vec(name),
        bounded_vec(symbol),
        token,
        asset_id,
        18,
    )
}

#[test]
fn register_appchain_works() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let token = make_token(1);
        let asset_id = Asset::ForeignAsset(1);

        assert_ok!(register_appchain_call(handler, b"Test Chain", b"TST", token, asset_id));

        let chain_id = AvnAnchor::chain_handlers(handler).expect("handler should be registered");
        assert_eq!(AvnAnchor::asset_chain_id(asset_id), Some(chain_id));
        assert_eq!(RegisteredAppchains::<TestRuntime>::get().to_vec(), vec![asset_id]);

        System::assert_last_event(
            Event::AppChainRegistered { chain_id, handler, token, asset_id }.into(),
        );
    });
}

#[test]
fn register_appchain_increments_chain_id_and_updates_both_storage_items() {
    new_test_ext().execute_with(|| {
        let handler1 = create_account_id(1);
        let handler2 = create_account_id(2);
        let token1 = make_token(1);
        let token2 = make_token(2);
        let asset_id1 = Asset::ForeignAsset(1);
        let asset_id2 = Asset::ForeignAsset(2);

        assert_ok!(register_appchain_call(handler1, b"Chain 1", b"C1", token1, asset_id1));
        assert_ok!(register_appchain_call(handler2, b"Chain 2", b"C2", token2, asset_id2));

        let chain_id1 = AvnAnchor::chain_handlers(handler1).unwrap();
        let chain_id2 = AvnAnchor::chain_handlers(handler2).unwrap();
        assert_eq!(chain_id1, 0);
        assert_eq!(chain_id2, 1);

        assert_eq!(AvnAnchor::asset_chain_id(asset_id1), Some(chain_id1));
        assert_eq!(AvnAnchor::asset_chain_id(asset_id2), Some(chain_id2));
        assert_eq!(RegisteredAppchains::<TestRuntime>::get().to_vec(), vec![asset_id1, asset_id2]);
    });
}

#[test]
fn register_appchain_fails_for_non_root() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        assert_noop!(
            AvnAnchor::register_appchain(
                RuntimeOrigin::signed(handler),
                handler,
                bounded_vec(b"Test Chain"),
                bounded_vec(b"TST"),
                make_token(1),
                Asset::ForeignAsset(1),
                18,
            ),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn register_appchain_fails_for_already_registered_handler() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        assert_ok!(register_appchain_call(
            handler,
            b"Chain 1",
            b"C1",
            make_token(1),
            Asset::ForeignAsset(1)
        ));

        assert_noop!(
            register_appchain_call(
                handler,
                b"Chain 2",
                b"C2",
                make_token(2),
                Asset::ForeignAsset(2)
            ),
            Error::<TestRuntime>::HandlerAlreadyRegistered
        );
    });
}

#[test]
fn register_appchain_fails_for_empty_name() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            register_appchain_call(
                create_account_id(1),
                b"",
                b"TST",
                make_token(1),
                Asset::ForeignAsset(1)
            ),
            Error::<TestRuntime>::EmptyChainName
        );
    });
}

#[test]
fn register_appchain_fails_for_empty_symbol() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            register_appchain_call(
                create_account_id(1),
                b"Test Chain",
                b"",
                make_token(1),
                Asset::ForeignAsset(1)
            ),
            Error::<TestRuntime>::EmptyTokenSymbol
        );
    });
}

#[test]
fn register_appchain_fails_for_duplicate_token_location() {
    new_test_ext().execute_with(|| {
        let token = make_token(1);
        assert_ok!(register_appchain_call(
            create_account_id(1),
            b"Chain 1",
            b"C1",
            token,
            Asset::ForeignAsset(1)
        ));

        assert_noop!(
            register_appchain_call(
                create_account_id(2),
                b"Chain 2",
                b"C2",
                token,
                Asset::ForeignAsset(2)
            ),
            Error::<TestRuntime>::TokenLocationAlreadyRegistered
        );
    });
}

#[test]
fn register_appchain_fails_when_capacity_is_reached() {
    new_test_ext().execute_with(|| {
        // Fill RegisteredAppchains to capacity by direct storage mutation
        RegisteredAppchains::<TestRuntime>::put(
            sp_runtime::BoundedVec::try_from(
                (0u32..256).map(Asset::ForeignAsset).collect::<Vec<_>>(),
            )
            .unwrap(),
        );

        assert_noop!(
            register_appchain_call(
                create_account_id(1),
                b"Chain X",
                b"CX",
                make_token(1),
                Asset::ForeignAsset(256)
            ),
            Error::<TestRuntime>::MaxAppChainsReached
        );
    });
}

#[test]
fn register_appchain_does_not_update_storage_on_failure() {
    new_test_ext().execute_with(|| {
        let handler = create_account_id(1);
        let token = make_token(1);
        // Attempt with empty name — should leave no state
        let _ = register_appchain_call(handler, b"", b"TST", token, Asset::ForeignAsset(1));

        assert!(AvnAnchor::chain_handlers(handler).is_none());
        assert!(AssetIdToChainId::<TestRuntime>::get(Asset::ForeignAsset(1)).is_none());
        assert!(RegisteredAppchains::<TestRuntime>::get().is_empty());
    });
}

// ---------------------------------------------------------------------------
// App-chain rewards
// ---------------------------------------------------------------------------
mod app_chain_rewards {
    use super::*;
    use crate::{
        NextRewardAmountPerPeriod, PeriodChainReward, RewardRecord, SweepCursor, UnpaidByNode,
        UnpaidByPeriod,
    };
    use frame_support::weights::{Weight, WeightMeter};
    use orml_traits::MultiCurrency;
    use sp_avn_common::{primitives::Balance, AppChainInterface};
    use sp_core::H160;
    use sp_runtime::Perquintill;

    type Anchor = AvnAnchor;

    fn register_rewardable_chain(handler: AccountId, seed: u8, asset_id: CurrencyId) -> H160 {
        let token = make_token(seed);
        assert_ok!(register_appchain_call(handler, b"Chain", b"TKN", token, asset_id));
        token
    }

    fn fund_pot(asset_id: CurrencyId, amount: Balance) {
        assert_ok!(<Tokens as MultiCurrency<AccountId>>::deposit(
            asset_id,
            &reward_pot_account(),
            amount
        ));
    }

    fn token_balance(asset_id: CurrencyId, who: &AccountId) -> Balance {
        <Tokens as MultiCurrency<AccountId>>::free_balance(asset_id, who)
    }

    pub fn event_emitted(event: &RuntimeEvent) -> bool {
        return System::events().iter().any(|a| a.event == *event)
    }

    #[test]
    fn set_appchain_period_reward_works() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            let token = register_rewardable_chain(handler, 1, asset_id);

            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));

            assert_eq!(
                NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id),
                Some((token, 1_000))
            );
            assert!(event_emitted(
                &Event::AppChainRewardAmountPerPeriodUpdated { asset_id, amount: 1_000 }.into()
            ));

            System::assert_last_event(Event::AppChainEnabled { asset_id }.into());
        });
    }

    #[test]
    fn set_appchain_period_reward_rejects_zero_and_bad_callers() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let stranger = create_account_id(9);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);

            assert_noop!(
                Anchor::set_appchain_period_reward(RuntimeOrigin::signed(handler), asset_id, 0),
                Error::<TestRuntime>::ZeroRewardAmount
            );
            assert_noop!(
                Anchor::set_appchain_period_reward(
                    RuntimeOrigin::signed(handler),
                    Asset::ForeignAsset(99),
                    10
                ),
                Error::<TestRuntime>::AppChainAssetNotRegistered
            );
            assert_noop!(
                Anchor::set_appchain_period_reward(RuntimeOrigin::signed(stranger), asset_id, 10),
                Error::<TestRuntime>::NotAppChainHandler
            );
        });
    }

    #[test]
    fn disable_appchain_removes_entry_and_emits_event() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));

            assert_ok!(Anchor::disable_appchain(RuntimeOrigin::signed(handler), asset_id));

            // Disabling removes the entry entirely so the chain is inactive, and the event fires.
            assert!(NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id).is_none());
            System::assert_last_event(Event::AppChainDisabled { asset_id }.into());
        });
    }

    #[test]
    fn disable_appchain_rejects_inactive_chain() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);
            // Registered but no rate ever set, so it is already inactive.
            assert!(NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id).is_none());

            assert_noop!(
                Anchor::disable_appchain(RuntimeOrigin::signed(handler), asset_id),
                Error::<TestRuntime>::AppChainNotActive
            );
        });
    }

    #[test]
    fn disable_appchain_rejects_unregistered_and_non_handler() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let stranger = create_account_id(9);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);

            // Unknown asset id.
            assert_noop!(
                Anchor::disable_appchain(RuntimeOrigin::signed(handler), Asset::ForeignAsset(99)),
                Error::<TestRuntime>::AppChainAssetNotRegistered
            );
            // Caller is not the registered handler.
            assert_noop!(
                Anchor::disable_appchain(RuntimeOrigin::signed(stranger), asset_id),
                Error::<TestRuntime>::NotAppChainHandler
            );
        });
    }

    #[test]
    fn disabled_appchain_is_not_snapshotted_for_new_period() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));

            assert_ok!(Anchor::disable_appchain(RuntimeOrigin::signed(handler), asset_id));

            // A disabled chain (entry removed) must not be snapshotted when a new period starts.
            <Anchor as AppChainInterface>::on_new_reward_period(&7);
            assert!(PeriodChainReward::<TestRuntime>::get(7, asset_id).is_none());
        });
    }

    #[test]
    fn disabled_appchain_can_be_reenabled() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            let token = register_rewardable_chain(handler, 1, asset_id);
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));
            assert_ok!(Anchor::disable_appchain(RuntimeOrigin::signed(handler), asset_id));

            // Re-enabling is just setting a non-zero rate again; it emits `AppChainEnabled`.
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                2_000
            ));
            assert_eq!(
                NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id),
                Some((token, 2_000))
            );
            System::assert_last_event(Event::AppChainEnabled { asset_id }.into());

            // And it is snapshotted again on the next period.
            <Anchor as AppChainInterface>::on_new_reward_period(&8);
            assert_eq!(PeriodChainReward::<TestRuntime>::get(8, asset_id), Some((token, 2_000)));
        });
    }

    #[test]
    fn deregister_appchain_full_unregister_works() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);
            let chain_id =
                Anchor::asset_chain_id(asset_id).expect("registered chain has a chain id");
            // Must be disabled first (set a rate so it is active, then disable).
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));
            assert_ok!(Anchor::disable_appchain(RuntimeOrigin::signed(handler), asset_id));

            assert_ok!(Anchor::deregister_appchain(RuntimeOrigin::root(), handler, asset_id));

            // Routing + reward state removed.
            assert!(Anchor::asset_chain_id(asset_id).is_none());
            assert!(Anchor::chain_handlers(handler).is_none());
            assert!(NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id).is_none());
            assert!(!RegisteredAppchains::<TestRuntime>::get().contains(&asset_id));
            // Asset marked non-native: it is no longer resolvable as an app-chain payout token.
            assert_noop!(
                Anchor::resolve_payout_token(&asset_id),
                Error::<TestRuntime>::AssetNotAppChainNative
            );
            System::assert_last_event(Event::AppChainDeregistered { chain_id, asset_id }.into());
        });
    }

    #[test]
    fn deregister_appchain_rejects_active_chain() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);
            // Active (non-zero rate), not disabled.
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));

            assert_noop!(
                Anchor::deregister_appchain(RuntimeOrigin::root(), handler, asset_id),
                Error::<TestRuntime>::AppChainNotDisabled
            );
        });
    }

    #[test]
    fn deregister_appchain_rejects_unregistered_and_wrong_handler() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let stranger = create_account_id(9);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));
            assert_ok!(Anchor::disable_appchain(RuntimeOrigin::signed(handler), asset_id));

            // Unknown asset id.
            assert_noop!(
                Anchor::deregister_appchain(
                    RuntimeOrigin::root(),
                    handler,
                    Asset::ForeignAsset(99)
                ),
                Error::<TestRuntime>::AppChainAssetNotRegistered
            );
            // Handler does not match this chain.
            assert_noop!(
                Anchor::deregister_appchain(RuntimeOrigin::root(), stranger, asset_id),
                Error::<TestRuntime>::NotAppChainHandler
            );
        });
    }

    #[test]
    fn deregister_appchain_requires_root() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            register_rewardable_chain(handler, 1, asset_id);
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));
            assert_ok!(Anchor::disable_appchain(RuntimeOrigin::signed(handler), asset_id));

            assert_noop!(
                Anchor::deregister_appchain(RuntimeOrigin::signed(handler), handler, asset_id),
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }

    // Deregistering does NOT strand already-accrued rewards: the snapshot captured the token and
    // the asset's registry location is retained, so a claim still pays out after
    // deregistration.
    #[test]
    fn claim_still_pays_after_deregister() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let owner = create_account_id(2);
            let node = create_account_id(3);
            let asset_id = Asset::ForeignAsset(1);
            let period = 4u64;

            register_rewardable_chain(handler, 1, asset_id);
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));
            // Snapshot the period and record an unpaid reward for the node.
            <Anchor as AppChainInterface>::on_new_reward_period(&period);
            <Anchor as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node,
                0,
                Perquintill::from_percent(30),
            );
            fund_pot(asset_id, 10_000);

            // Disable then fully deregister while the snapshot + unpaid record are still live.
            assert_ok!(Anchor::disable_appchain(RuntimeOrigin::signed(handler), asset_id));
            assert_ok!(Anchor::deregister_appchain(RuntimeOrigin::root(), handler, asset_id));

            // The claim still settles: 30% of 1_000 = 300 paid in the (deregistered) chain's token.
            assert_ok!(Anchor::claim(RuntimeOrigin::signed(owner), node));
            assert_eq!(token_balance(asset_id, &owner), 300);
            assert!(UnpaidByPeriod::<TestRuntime>::get(period, node).is_none());
        });
    }

    #[test]
    fn root_can_set_and_disable_appchain_reward() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            let token = register_rewardable_chain(handler, 1, asset_id);

            // Root (not the handler) can set the rate...
            assert_ok!(Anchor::set_appchain_period_reward(RuntimeOrigin::root(), asset_id, 1_000));
            assert_eq!(
                NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id),
                Some((token, 1_000))
            );

            // ...and disable it, which removes the entry.
            assert_ok!(Anchor::disable_appchain(RuntimeOrigin::root(), asset_id));
            assert!(NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id).is_none());
        });
    }

    #[test]
    fn set_appchain_period_reward_authorizes_root_and_handler_only() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let stranger = create_account_id(9);
            let asset_id = Asset::ForeignAsset(1);
            let token = register_rewardable_chain(handler, 1, asset_id);

            // Registered handler: allowed.
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));
            assert_eq!(
                NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id),
                Some((token, 1_000))
            );

            // Root: allowed.
            assert_ok!(Anchor::set_appchain_period_reward(RuntimeOrigin::root(), asset_id, 2_000));
            assert_eq!(
                NextRewardAmountPerPeriod::<TestRuntime>::get(asset_id),
                Some((token, 2_000))
            );

            // Any other signed account: rejected.
            assert_noop!(
                Anchor::set_appchain_period_reward(
                    RuntimeOrigin::signed(stranger),
                    asset_id,
                    3_000
                ),
                Error::<TestRuntime>::NotAppChainHandler
            );

            // Unsigned origin: rejected.
            assert_noop!(
                Anchor::set_appchain_period_reward(RuntimeOrigin::none(), asset_id, 3_000),
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn reclaim_period_clears_a_stuck_snapshot() {
        new_test_ext().execute_with(|| {
            let asset_id = Asset::ForeignAsset(1);
            let period = 4u64;
            // Simulate a completed period whose snapshot was left behind (marker set, no unpaid
            // records) — the state a partial reclaim would leave.
            PeriodChainReward::<TestRuntime>::insert(period, asset_id, (make_token(1), 1_000));
            crate::PeriodPayoutCompleted::<TestRuntime>::insert(period, ());

            assert_ok!(Anchor::reclaim_period(RuntimeOrigin::root(), period));

            assert!(PeriodChainReward::<TestRuntime>::iter_prefix(period).next().is_none());
            assert!(!crate::PeriodPayoutCompleted::<TestRuntime>::contains_key(period));
            System::assert_last_event(
                Event::AppChainRewardPayoutCompleted { reward_period: period }.into(),
            );
        });
    }

    #[test]
    fn reclaim_period_requires_root() {
        new_test_ext().execute_with(|| {
            let caller = create_account_id(1);
            assert_noop!(
                Anchor::reclaim_period(RuntimeOrigin::signed(caller), 4u64),
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn on_new_reward_period_snapshots_rate_and_token() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            let token = register_rewardable_chain(handler, 1, asset_id);
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                5_000
            ));

            <Anchor as AppChainInterface>::on_new_reward_period(&7);

            assert_eq!(PeriodChainReward::<TestRuntime>::get(7, asset_id), Some((token, 5_000)));
        });
    }

    #[test]
    fn on_reward_paid_records_share_and_skips_zero() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(9);
            let owner = create_account_id(1);
            let node = create_account_id(2);
            let zero_node = create_account_id(3);

            // A pool must exist for the period, otherwise recording is skipped (see other test).
            accrue(7, handler, Asset::ForeignAsset(1), 1, 1_000);

            <Anchor as AppChainInterface>::on_reward_paid(
                &7,
                &owner,
                &node,
                0,
                Perquintill::from_percent(25),
            );
            <Anchor as AppChainInterface>::on_reward_paid(
                &7,
                &owner,
                &zero_node,
                0,
                Perquintill::zero(),
            );

            assert_eq!(
                UnpaidByPeriod::<TestRuntime>::get(7, node),
                Some(RewardRecord {
                    owner,
                    share: Perquintill::from_percent(25),
                    auto_stake_expiry: 0
                })
            );
            assert!(UnpaidByNode::<TestRuntime>::contains_key(node, 7));
            // Zero share is not recorded.
            assert!(UnpaidByPeriod::<TestRuntime>::get(7, zero_node).is_none());
        });
    }

    #[test]
    fn on_reward_paid_skips_when_no_pool_for_period() {
        new_test_ext().execute_with(|| {
            let owner = create_account_id(1);
            let node = create_account_id(2);

            // No app chain funded a pool for period 7 (no `on_new_reward_period` snapshot).
            <Anchor as AppChainInterface>::on_reward_paid(
                &7,
                &owner,
                &node,
                0,
                Perquintill::from_percent(25),
            );

            assert!(UnpaidByPeriod::<TestRuntime>::get(7, node).is_none());
            assert!(!UnpaidByNode::<TestRuntime>::contains_key(node, 7));
        });
    }

    fn accrue(
        period: u64,
        handler: AccountId,
        asset_id: CurrencyId,
        seed: u8,
        rate: Balance,
    ) -> H160 {
        let token = register_rewardable_chain(handler, seed, asset_id);
        assert_ok!(Anchor::set_appchain_period_reward(
            RuntimeOrigin::signed(handler),
            asset_id,
            rate
        ));
        <Anchor as AppChainInterface>::on_new_reward_period(&period);
        token
    }

    #[test]
    fn claim_pays_owner_and_clears_records() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let owner = create_account_id(2);
            let node = create_account_id(3);
            let asset_id = Asset::ForeignAsset(1);
            let period = 4u64;

            accrue(period, handler, asset_id, 1, 1_000);
            fund_pot(asset_id, 10_000);
            <Anchor as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node,
                0,
                Perquintill::from_percent(30),
            );
            // node-manager finished recording this period, so the snapshot may be reclaimed.
            <Anchor as AppChainInterface>::on_reward_period_completed(&period);

            assert_ok!(Anchor::claim(RuntimeOrigin::signed(owner), node));

            // 30% of 1_000 = 300 paid to owner in the chain's token.
            assert_eq!(token_balance(asset_id, &owner), 300);
            assert!(UnpaidByPeriod::<TestRuntime>::get(period, node).is_none());
            assert!(!UnpaidByNode::<TestRuntime>::contains_key(node, period));
            // Snapshot reclaimed once the last record for the (completed) period is gone.
            assert!(PeriodChainReward::<TestRuntime>::get(period, asset_id).is_none());
            System::assert_has_event(
                Event::AppChainRewardPayoutCompleted { reward_period: period }.into(),
            );
        });
    }

    // Regression: node-manager records nodes across batched payouts. A claim that drains
    // `UnpaidByPeriod` between batches must NOT clear the snapshot, or a later `on_reward_paid`
    // would hit the "no pool" short-circuit and silently drop that node's reward.
    #[test]
    fn snapshot_not_cleared_mid_payout_until_period_completed() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let owner = create_account_id(2);
            let node_a = create_account_id(3);
            let node_b = create_account_id(4);
            let asset_id = Asset::ForeignAsset(1);
            let period = 4u64;

            accrue(period, handler, asset_id, 1, 1_000);
            fund_pot(asset_id, 10_000);

            // node-manager's first batch records node A.
            <Anchor as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node_a,
                0,
                Perquintill::from_percent(10),
            );

            // A claim drains node A — transiently emptying UnpaidByPeriod — but the period is NOT
            // yet completed, so the snapshot must survive.
            assert_ok!(Anchor::claim(RuntimeOrigin::signed(owner), node_a));
            assert!(UnpaidByPeriod::<TestRuntime>::iter_prefix(period).next().is_none());
            assert_eq!(
                PeriodChainReward::<TestRuntime>::get(period, asset_id),
                Some((make_token(1), 1_000)),
                "snapshot must not be cleared mid-payout"
            );

            // node-manager's second batch records node B — must still be recorded (not skipped).
            <Anchor as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node_b,
                0,
                Perquintill::from_percent(20),
            );
            assert!(UnpaidByPeriod::<TestRuntime>::get(period, node_b).is_some());

            // node-manager finishes the period; the final claim now reclaims the snapshot.
            <Anchor as AppChainInterface>::on_reward_period_completed(&period);
            assert_ok!(Anchor::claim(RuntimeOrigin::signed(owner), node_b));

            // Both nodes paid (10% + 20% of 1_000) and the snapshot is finally cleared.
            assert_eq!(token_balance(asset_id, &owner), 300);
            assert!(PeriodChainReward::<TestRuntime>::get(period, asset_id).is_none());
            System::assert_has_event(
                Event::AppChainRewardPayoutCompleted { reward_period: period }.into(),
            );
        });
    }

    // A period that completes with no recorded rewards (e.g. every share was zero) must still have
    // its snapshot reclaimed by `on_reward_period_completed`.
    #[test]
    fn empty_period_snapshot_reclaimed_on_completion() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let asset_id = Asset::ForeignAsset(1);
            let token = register_rewardable_chain(handler, 1, asset_id);
            assert_ok!(Anchor::set_appchain_period_reward(
                RuntimeOrigin::signed(handler),
                asset_id,
                1_000
            ));
            let period = 4u64;
            <Anchor as AppChainInterface>::on_new_reward_period(&period);
            assert_eq!(
                PeriodChainReward::<TestRuntime>::get(period, asset_id),
                Some((token, 1_000))
            );

            // No nodes recorded; completing the period reclaims the snapshot immediately.
            <Anchor as AppChainInterface>::on_reward_period_completed(&period);
            assert!(PeriodChainReward::<TestRuntime>::get(period, asset_id).is_none());
        });
    }

    #[test]
    fn claim_without_rewards_fails() {
        new_test_ext().execute_with(|| {
            let caller = create_account_id(1);
            let node = create_account_id(2);
            assert_noop!(
                Anchor::claim(RuntimeOrigin::signed(caller), node),
                Error::<TestRuntime>::NoUnpaidRewards
            );
        });
    }

    #[test]
    fn double_claim_is_a_noop() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let owner = create_account_id(2);
            let node = create_account_id(3);
            let asset_id = Asset::ForeignAsset(1);
            let period = 4u64;

            accrue(period, handler, asset_id, 1, 1_000);
            fund_pot(asset_id, 10_000);
            <Anchor as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node,
                0,
                Perquintill::from_percent(30),
            );

            assert_ok!(Anchor::claim(RuntimeOrigin::signed(owner), node));
            assert_eq!(token_balance(asset_id, &owner), 300);
            // Nothing left to claim.
            assert_noop!(
                Anchor::claim(RuntimeOrigin::signed(owner), node),
                Error::<TestRuntime>::NoUnpaidRewards
            );
            assert_eq!(token_balance(asset_id, &owner), 300);
        });
    }

    #[test]
    fn underfunded_pot_leaves_record_for_retry() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let owner = create_account_id(2);
            let node = create_account_id(3);
            let asset_id = Asset::ForeignAsset(1);
            let period = 4u64;

            accrue(period, handler, asset_id, 1, 1_000);
            // Pot NOT funded.
            <Anchor as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node,
                0,
                Perquintill::from_percent(30),
            );

            // claim_node attempts the payout; it fails internally and emits the failure event.
            assert_ok!(Anchor::claim(RuntimeOrigin::signed(owner), node));
            assert_eq!(token_balance(asset_id, &owner), 0);
            // Record retained for a later retry, snapshot intact.
            assert!(UnpaidByPeriod::<TestRuntime>::get(period, node).is_some());
            assert!(PeriodChainReward::<TestRuntime>::get(period, asset_id).is_some());

            // Once funded, a retry pays out.
            fund_pot(asset_id, 10_000);
            assert_ok!(Anchor::claim(RuntimeOrigin::signed(owner), node));
            assert_eq!(token_balance(asset_id, &owner), 300);
        });
    }

    #[test]
    fn process_outstanding_rewards_sweeps_all_nodes() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let owner = create_account_id(2);
            let node_a = create_account_id(3);
            let node_b = create_account_id(4);
            let caller = create_account_id(5);
            let asset_id = Asset::ForeignAsset(1);
            let period = 4u64;

            accrue(period, handler, asset_id, 1, 1_000);
            fund_pot(asset_id, 10_000);
            <Anchor as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node_a,
                0,
                Perquintill::from_percent(10),
            );
            <Anchor as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node_b,
                0,
                Perquintill::from_percent(20),
            );
            // node-manager finished recording this period, so the snapshot may be reclaimed.
            <Anchor as AppChainInterface>::on_reward_period_completed(&period);

            assert_ok!(Anchor::process_outstanding_rewards(RuntimeOrigin::signed(caller)));

            // 10% + 20% of 1_000 = 300 total to the shared owner.
            assert_eq!(token_balance(asset_id, &owner), 300);
            assert!(UnpaidByPeriod::<TestRuntime>::iter_prefix(period).next().is_none());
            assert!(PeriodChainReward::<TestRuntime>::get(period, asset_id).is_none());

            assert_noop!(
                Anchor::process_outstanding_rewards(RuntimeOrigin::signed(caller)),
                Error::<TestRuntime>::NoUnpaidRewards
            );
        });
    }

    // A record whose payout always fails (underfunded asset) must not block records behind it.
    // With one payout per call, the cursor steps past the failure so the funded record is still
    // paid within a single pass, regardless of hash-iteration order.
    #[test]
    fn sweep_cursor_does_not_starve_funded_records() {
        new_test_ext().execute_with(|| {
            let handler_a = create_account_id(10);
            let handler_b = create_account_id(11);
            let owner_fail = create_account_id(1);
            let owner_ok = create_account_id(2);
            let node_fail = create_account_id(3);
            let node_ok = create_account_id(4);
            let asset_a = Asset::ForeignAsset(1);
            let asset_b = Asset::ForeignAsset(2);

            // Register both chains so payouts route through the asset registry.
            let token_a = register_rewardable_chain(handler_a, 1, asset_a);
            let token_b = register_rewardable_chain(handler_b, 2, asset_b);

            // Period 1 pays asset A, but the pot is never funded for A -> always fails.
            PeriodChainReward::<TestRuntime>::insert(1, asset_a, (token_a, 1_000));
            UnpaidByPeriod::<TestRuntime>::insert(
                1,
                node_fail,
                RewardRecord {
                    owner: owner_fail,
                    share: Perquintill::from_percent(100),
                    auto_stake_expiry: 0,
                },
            );
            UnpaidByNode::<TestRuntime>::insert(node_fail, 1, ());

            // Period 2 pays asset B and the pot is funded -> succeeds.
            PeriodChainReward::<TestRuntime>::insert(2, asset_b, (token_b, 1_000));
            UnpaidByPeriod::<TestRuntime>::insert(
                2,
                node_ok,
                RewardRecord {
                    owner: owner_ok,
                    share: Perquintill::from_percent(100),
                    auto_stake_expiry: 0,
                },
            );
            UnpaidByNode::<TestRuntime>::insert(node_ok, 2, ());
            fund_pot(asset_b, 1_000_000);

            // One payout per call: a front-positioned failure would starve node_ok without a
            // cursor.
            let mut meter = WeightMeter::with_limit(Weight::MAX);
            Anchor::sweep(&mut meter, 1);
            let mut meter = WeightMeter::with_limit(Weight::MAX);
            Anchor::sweep(&mut meter, 1);

            // Funded record is paid within one pass; failed record is retained for retry.
            assert_eq!(token_balance(asset_b, &owner_ok), 1_000);
            assert!(UnpaidByPeriod::<TestRuntime>::get(2, node_ok).is_none());
            assert!(UnpaidByPeriod::<TestRuntime>::get(1, node_fail).is_some());
        });
    }

    // The cursor advances one record per call and wraps to `None` once the pass is exhausted.
    #[test]
    fn sweep_cursor_advances_then_wraps() {
        new_test_ext().execute_with(|| {
            let handler = create_account_id(1);
            let owner = create_account_id(2);
            let n1 = create_account_id(3);
            let n2 = create_account_id(4);
            let asset_id = Asset::ForeignAsset(1);
            let period = 5u64;

            accrue(period, handler, asset_id, 1, 1_000);
            fund_pot(asset_id, 1_000_000);
            for node in [n1, n2] {
                UnpaidByPeriod::<TestRuntime>::insert(
                    period,
                    node,
                    RewardRecord {
                        owner,
                        share: Perquintill::from_percent(10),
                        auto_stake_expiry: 0,
                    },
                );
                UnpaidByNode::<TestRuntime>::insert(node, period, ());
            }

            // First two calls pay one record each and leave a resume cursor.
            let mut meter = WeightMeter::with_limit(Weight::MAX);
            assert_eq!(Anchor::sweep(&mut meter, 1), 1);
            assert!(SweepCursor::<TestRuntime>::get().is_some());
            let mut meter = WeightMeter::with_limit(Weight::MAX);
            assert_eq!(Anchor::sweep(&mut meter, 1), 1);

            // Third call finds nothing after the cursor, so it wraps to None.
            let mut meter = WeightMeter::with_limit(Weight::MAX);
            assert_eq!(Anchor::sweep(&mut meter, 1), 0);
            assert!(SweepCursor::<TestRuntime>::get().is_none());
            assert!(UnpaidByPeriod::<TestRuntime>::iter_prefix(period).next().is_none());
        });
    }
}
