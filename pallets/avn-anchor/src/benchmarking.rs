#![cfg(feature = "runtime-benchmarks")]

use super::*;
use crate::{
    encode_signed_register_chain_handler_params, encode_signed_submit_checkpoint_params,
    encode_signed_update_chain_handler_params,
};
use codec::{Decode, Encode};
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_support::{traits::Currency, BoundedVec};
use frame_system::RawOrigin;
use sp_application_crypto::KeyTypeId;
use sp_avn_common::Proof;
use sp_core::H256;
use sp_runtime::{RuntimeAppPublic, Saturating};

pub const BENCH_KEY_TYPE_ID: KeyTypeId = KeyTypeId(*b"test");

mod app_sr25519 {
    use super::BENCH_KEY_TYPE_ID;
    use sp_application_crypto::{app_crypto, sr25519};
    app_crypto!(sr25519, BENCH_KEY_TYPE_ID);
}

type SignerId = app_sr25519::Public;

const SEED: u32 = 0;

pub fn setup_balance<T: Config>(account: &T::AccountId) {
    let min_balance = T::Currency::minimum_balance();
    // Convert default checkpoint fee to the correct balance type
    let default_fee: BalanceOf<T> = T::DefaultCheckpointFee::get();

    // Calculate a large initial balance
    // Use saturating operations to prevent overflow
    let large_multiplier: BalanceOf<T> = 1000u32.into();
    let fee_component = default_fee.saturating_mul(large_multiplier);
    let existential_component = min_balance.saturating_mul(large_multiplier);

    // Add the components together for total initial balance
    let initial_balance = fee_component.saturating_add(existential_component);

    // Set the balance
    T::Currency::make_free_balance_be(account, initial_balance);

    // Ensure the account has enough free balance
    assert!(
        T::Currency::free_balance(account) >= initial_balance,
        "Failed to set up sufficient balance"
    );
}

pub fn ensure_fee_payment_possible<T: Config>(
    chain_id: ChainId,
    account: &T::AccountId,
) -> Result<(), &'static str> {
    let fee = Pallet::<T>::checkpoint_fee(chain_id);
    let balance = T::Currency::free_balance(account);
    if balance < fee {
        return Err("Insufficient balance for fee payment")
    }
    Ok(())
}

fn create_account_id<T: Config>(seed: u32) -> T::AccountId {
    account("account", seed, SEED)
}

fn create_proof<T: Config>(
    signature: sp_core::sr25519::Signature,
    signer: T::AccountId,
    relayer: T::AccountId,
) -> Proof<T::Signature, T::AccountId> {
    Proof { signer, relayer, signature: T::Signature::decode(&mut &signature[..]).unwrap() }
}

fn setup_chain<T: Config>(
    handler: &T::AccountId,
) -> Result<(ChainId, BoundedVec<u8, ConstU32<32>>), &'static str> {
    let name: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(vec![0u8; 32]).unwrap();
    Pallet::<T>::register_chain_handler(RawOrigin::Signed(handler.clone()).into(), name.clone())?;
    let chain_id = ChainHandlers::<T>::get(handler).ok_or("Chain not registered")?;
    Ok((chain_id, name))
}

benchmarks! {
    register_chain_handler {
        let caller: T::AccountId = create_account_id::<T>(0);
        let name: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(vec![0u8; 32]).unwrap();
        setup_balance::<T>(&caller);
    }: _(RawOrigin::Signed(caller.clone()), name.clone())
    verify {
        assert!(ChainHandlers::<T>::contains_key(&caller));
        let chain_data = ChainData::<T>::get(ChainHandlers::<T>::get(&caller).unwrap()).unwrap();
        assert_eq!(chain_data.name, name);
    }

    update_chain_handler {
        let old_handler: T::AccountId = create_account_id::<T>(0);
        let new_handler: T::AccountId = create_account_id::<T>(1);
        setup_balance::<T>(&old_handler);
        setup_balance::<T>(&new_handler);
        let (chain_id, name) = setup_chain::<T>(&old_handler)?;
    }: _(RawOrigin::Signed(old_handler.clone()), new_handler.clone())
    verify {
        assert!(!ChainHandlers::<T>::contains_key(&old_handler));
        assert!(ChainHandlers::<T>::contains_key(&new_handler));
        let chain_data = ChainData::<T>::get(ChainHandlers::<T>::get(&new_handler).unwrap()).unwrap();
        assert_eq!(chain_data.name, name);
    }

    submit_checkpoint_with_identity {
        let handler: T::AccountId = create_account_id::<T>(0);
        let chain_id = setup_chain::<T>(&handler)?.0;
        setup_balance::<T>(&handler);
        ensure_fee_payment_possible::<T>(chain_id, &handler)?;

        let checkpoint = H256::from([0u8; 32]);
        let origin_id = 42u64;
        let initial_checkpoint_id = NextCheckpointId::<T>::get(chain_id);

        let initial_balance = T::Currency::free_balance(&handler);
        let fee = Pallet::<T>::checkpoint_fee(chain_id);
        assert!(initial_balance >= fee, "Insufficient initial balance");
    }: _(RawOrigin::Signed(handler.clone()), checkpoint, origin_id)
    verify {
        assert_eq!(
            Checkpoints::<T>::get(chain_id, initial_checkpoint_id).unwrap(),
            CheckpointData { hash: checkpoint, origin_id }
        );
        assert_eq!(
            OriginIdToCheckpoint::<T>::get(chain_id, origin_id).unwrap(),
            initial_checkpoint_id
        );

        // Verify checkpoint ID increment
        assert_eq!(NextCheckpointId::<T>::get(chain_id), initial_checkpoint_id + 1);
        // Verify fee was paid
        assert!(T::Currency::free_balance(&handler) < initial_balance, "Fee was not deducted");
    }

    signed_register_chain_handler {
        let signer_pair = SignerId::generate_pair(None);
        let handler: T::AccountId = T::AccountId::decode(&mut signer_pair.encode().as_slice())
            .expect("Valid account id");
        let relayer: T::AccountId = create_account_id::<T>(1);
        let name: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(vec![0u8; 32]).unwrap();
        setup_balance::<T>(&handler);
        setup_balance::<T>(&relayer);

        let payload = encode_signed_register_chain_handler_params::<T>(&relayer, &handler, &name);
        let signature = signer_pair.sign(&payload).ok_or("Error signing proof")?;
        let proof = create_proof::<T>(signature.into(), handler.clone(), relayer);
    }: _(RawOrigin::Signed(handler.clone()), proof, handler.clone(), name.clone())
    verify {
        assert!(ChainHandlers::<T>::contains_key(&handler));
        let chain_data = ChainData::<T>::get(ChainHandlers::<T>::get(&handler).unwrap()).unwrap();
        assert_eq!(chain_data.name, name);
    }

    signed_update_chain_handler {
        let old_signer_pair = SignerId::generate_pair(None);
        let old_handler: T::AccountId = T::AccountId::decode(&mut old_signer_pair.encode().as_slice())
            .expect("Valid account id");
        let new_handler: T::AccountId = create_account_id::<T>(1);
        let relayer: T::AccountId = create_account_id::<T>(2);
        setup_balance::<T>(&old_handler);
        setup_balance::<T>(&new_handler);
        setup_balance::<T>(&relayer);
        let (chain_id, _) = setup_chain::<T>(&old_handler)?;
        let nonce = Nonces::<T>::get(chain_id);

        let payload = encode_signed_update_chain_handler_params::<T>(
            &relayer,
            &old_handler,
            &new_handler,
            chain_id,
            nonce
        );
        let signature = old_signer_pair.sign(&payload).ok_or("Error signing proof")?;
        let proof = create_proof::<T>(signature.into(), old_handler.clone(), relayer);
    }: _(RawOrigin::Signed(old_handler.clone()), proof, old_handler.clone(), new_handler.clone())
    verify {
        assert!(!ChainHandlers::<T>::contains_key(&old_handler));
        assert!(ChainHandlers::<T>::contains_key(&new_handler));
    }

    signed_submit_checkpoint_with_identity {
        let signer_pair = SignerId::generate_pair(None);
        let handler: T::AccountId = T::AccountId::decode(&mut Encode::encode(&signer_pair).as_slice())
            .expect("valid account id");
        let relayer: T::AccountId = create_account_id::<T>(1);

        setup_balance::<T>(&handler);
        setup_balance::<T>(&relayer);

        let (chain_id, _) = setup_chain::<T>(&handler)?;
        ensure_fee_payment_possible::<T>(chain_id, &handler)?;

        let checkpoint = H256::from([0u8; 32]);
        let origin_id = 42u64;
        let nonce = Nonces::<T>::get(chain_id);
        let initial_checkpoint_id = NextCheckpointId::<T>::get(chain_id);

        let payload = encode_signed_submit_checkpoint_params::<T>(
            &relayer,
            &handler,
            &checkpoint,
            chain_id,
            nonce,
            &origin_id
        );
        let signature = signer_pair.sign(&payload).ok_or("Error signing proof")?;
        let proof = create_proof::<T>(signature.into(), handler.clone(), relayer);

        let initial_balance = T::Currency::free_balance(&handler);
        let fee = Pallet::<T>::checkpoint_fee(chain_id);
        assert!(initial_balance >= fee, "Insufficient initial balance");
    }: _(RawOrigin::Signed(handler.clone()), proof, handler.clone(), checkpoint, origin_id)
    verify {
        assert_eq!(
            Checkpoints::<T>::get(chain_id, initial_checkpoint_id).unwrap(),
            CheckpointData { hash: checkpoint, origin_id }
        );
        assert_eq!(
            OriginIdToCheckpoint::<T>::get(chain_id, origin_id).unwrap(),
            initial_checkpoint_id
        );
        assert_eq!(NextCheckpointId::<T>::get(chain_id), initial_checkpoint_id + 1);
        assert!(T::Currency::free_balance(&handler) < initial_balance, "Fee was not deducted");
    }

    set_checkpoint_fee {
        let chain_id = 0;
        let new_fee = BalanceOf::<T>::from(100u32);
    }: _(RawOrigin::Root, chain_id, new_fee)
    verify {
        assert_eq!(CheckpointFee::<T>::get(chain_id), new_fee);
    }
}

impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::TestRuntime);
