//! # Validators Manager pallet
// Copyright 2020 Artos Systems (UK) Ltd.

// validators manager pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;

use crate::{Pallet as ValidatorManager, *};
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_system::{EventRecord, Pallet as System, RawOrigin};
use hex_literal::hex;
use libsecp256k1::{PublicKey, SecretKey};
use pallet_avn::{self as avn};
use pallet_parachain_staking::{Currency, Pallet as ParachainStaking};
use pallet_session::Pallet as Session;
use sp_avn_common::eth_key_actions::decompress_eth_public_key;
use sp_core::{ecdsa::Public, H512};
use sp_runtime::{RuntimeAppPublic, WeakBoundedVec};

// Resigner keys derived from [6u8; 32] private key
const RESIGNING_COLLATOR_PUBLIC_KEY_BYTES: [u8; 32] =
    hex!["ea3021db7da7831e0d5ed7e60a8102d2d721bcca88adb03ee992f4dec3baee3e"];
const RESIGNING_COLLATOR_ETHEREUM_PUBLIC_KEY: [u8; 33] =
    hex!["03f006a18d5653c4edf5391ff23a61f03ff83d237e880ee61187fa9f379a028e0a"];

// Vote sender keys derived from [7u8; 32] private key
const VOTING_COLLATOR_PUBLIC_KEY_BYTES: [u8; 32] =
    hex!["7c0f469d3bd340bae718203fa30ca071a5e37c751e891dbded837b213d45d91d"];
const VOTING_COLLATOR_ETHEREUM_PUBLIC_KEY: [u8; 33] =
    hex!["02989c0b76cb563971fdc9bef31ec06c3560f3249d6ee9e5d83c57625596e05f6f"];

const NEW_COLLATOR_ETHEREUM_PUBLIC_KEY: [u8; 33] =
    hex!["03f171af36531200540b2badee5ed581b0a51f4e4a1a995025e149b9721b050074"];

const MINIMUM_ADDITIONAL_BENCHMARKS_VALIDATORS: usize = 2;

fn generate_resigning_collator_account_details<T: Config>(
) -> (T::AccountId, <T as pallet_avn::Config>::AuthorityId, Public) {
    let authority_id =
        <T as avn::Config>::AuthorityId::generate_pair(Some("//avn_resigner".as_bytes().to_vec()));
    let eth_public_key = Public::from_raw(RESIGNING_COLLATOR_ETHEREUM_PUBLIC_KEY);
    let account_id =
        T::AccountId::decode(&mut RESIGNING_COLLATOR_PUBLIC_KEY_BYTES.as_slice()).unwrap();

    (account_id, authority_id, eth_public_key)
}

fn generate_sender_collator_account_details<T: Config>(
) -> (T::AccountId, <T as pallet_avn::Config>::AuthorityId, Public) {
    let authority_id =
        <T as avn::Config>::AuthorityId::generate_pair(Some("//avn_sender".as_bytes().to_vec()));
    let eth_public_key = Public::from_raw(VOTING_COLLATOR_ETHEREUM_PUBLIC_KEY);
    let account_id =
        T::AccountId::decode(&mut VOTING_COLLATOR_PUBLIC_KEY_BYTES.as_slice()).unwrap();

    (account_id, authority_id, eth_public_key)
}

// Add additional collators, on top of genesis configuration
fn setup_additional_validators<T: Config>(number_of_additional_validators: u32) {
    assert!(number_of_additional_validators >= MINIMUM_ADDITIONAL_BENCHMARKS_VALIDATORS as u32);

    let mut avn_validators: Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>> =
        Vec::new();

    let mut validators: Vec<(T::AccountId, Public)> = Vec::new();
    let vote_sender_index = number_of_additional_validators - (1 as u32);

    for i in 0..number_of_additional_validators {
        let (account, avn_authority_id, eth_key) = match i {
            0 => generate_resigning_collator_account_details::<T>(),
            i if i == vote_sender_index => generate_sender_collator_account_details::<T>(),
            _ => (
                account("dummy_validator", i, i),
                <T as avn::Config>::AuthorityId::generate_pair(None),
                generate_collator_eth_public_key_from_seed::<T>(i as u64),
            ),
        };

        avn_validators.push(Validator::new(account.clone(), avn_authority_id));
        validators.push((account, eth_key));
    }

    // Setup validators in avn pallet
    let new_avn_validators = avn::Validators::<T>::get();
    // new_avn_validators.append(&mut avn_validators.clone());
    let combined_avn_validators: Vec<_> =
        new_avn_validators.iter().chain(avn_validators.iter()).cloned().collect();
    avn::Validators::<T>::put(WeakBoundedVec::force_from(
        combined_avn_validators,
        Some("Too many validators for session"),
    ));

    validators.iter().enumerate().for_each(|(i, (account_id, eth_public_key))| {
        force_add_collator::<T>(&account_id, i as u64, &eth_public_key)
    });
}

fn setup_resignation_action_data<T: Config>(sender: T::AccountId, ingress_counter: IngressCounter) {
    let (action_account_id, _, t1_eth_public_key) =
        generate_resigning_collator_account_details::<T>();

    let eth_transaction_id: EthereumId = 0;
    let decompressed_eth_public_key = decompress_eth_public_key(t1_eth_public_key)
        .map_err(|_| Error::<T>::InvalidPublicKey)
        .unwrap();

    ValidatorActions::<T>::insert(
        action_account_id,
        ingress_counter,
        ValidatorsActionData::new(
            ValidatorsActionStatus::AwaitingConfirmation,
            eth_transaction_id,
            ValidatorsActionType::Resignation,
        ),
    )
}

fn generate_signature<T: pallet_avn::Config>(
) -> <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature {
    let encoded_data = 0.encode();
    let authority_id = T::AuthorityId::generate_pair(None);
    let signature = authority_id.sign(&encoded_data).expect("able to make signature");
    return signature
}

fn generate_mock_ecdsa_signature<T: pallet_avn::Config>(msg: u8) -> ecdsa::Signature {
    let signature_bytes: [u8; 65] = [msg; 65];
    return ecdsa::Signature::from_slice(&signature_bytes).unwrap().into()
}

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    assert_last_nth_event::<T>(generic_event, 1);
}

fn assert_last_nth_event<T: Config>(generic_event: <T as Config>::RuntimeEvent, n: u32) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // Compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(n as usize)];
    assert_eq!(event, &system_event);
}

fn advance_session<T: Config>() {
    use frame_support::traits::{OnFinalize, OnInitialize};

    let now = System::<T>::block_number().max(1u32.into());
    pallet_parachain_staking::ForceNewEra::<T>::put(true);

    System::<T>::on_finalize(System::<T>::block_number());
    System::<T>::set_block_number(now + 1u32.into());
    System::<T>::on_initialize(System::<T>::block_number());
    Session::<T>::on_initialize(System::<T>::block_number());
    ParachainStaking::<T>::on_initialize(System::<T>::block_number());
}

fn set_session_keys<T: Config>(collator_id: &T::AccountId, index: u64) {
    use rand::{RngCore, SeedableRng};

    frame_system::Pallet::<T>::inc_providers(collator_id);

    let keys = {
        let mut keys = [0u8; 128];
        // We keep the keys for the first validator as 0x00000...
        let mut rng = rand::rngs::StdRng::seed_from_u64(index);
        rng.fill_bytes(&mut keys);
        keys
    };

    let keys: T::Keys = Decode::decode(&mut &keys[..]).unwrap();

    pallet_session::Pallet::<T>::set_keys(
        RawOrigin::<T::AccountId>::Signed(collator_id.clone()).into(),
        keys,
        Vec::new(),
    )
    .unwrap();
}

fn generate_collator_eth_public_key_from_seed<T: Config>(seed: u64) -> Public {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let secret_key = SecretKey::random(&mut rng);
    let public_key = PublicKey::from_secret_key(&secret_key);

    return ValidatorManager::<T>::compress_eth_public_key(H512::from_slice(
        &public_key.serialize()[1..],
    ))
}

fn simulate_t1_callback_success<T: Config>(tx_id: EthereumId) {
    const PALLET_ID: &[u8; 14] = b"author_manager";
    ValidatorManager::<T>::process_result(tx_id, PALLET_ID.to_vec(), true).unwrap();
}

fn get_tx_id_for_validator<T: Config>(account_id: &T::AccountId) -> Option<EthereumId> {
    // Find the ValidatorActions entry for this validator
    for (acc_id, _ingress_counter, validators_action_data) in <ValidatorActions<T>>::iter() {
        if &acc_id == account_id {
            return Some(validators_action_data.eth_transaction_id)
        }
    }
    None
}

fn force_add_collator<T: Config>(collator_id: &T::AccountId, index: u64, eth_public_key: &Public) {
    set_session_keys::<T>(collator_id, index);
    <T as pallet_parachain_staking::Config>::Currency::make_free_balance_be(
        &collator_id,
        ParachainStaking::<T>::min_collator_stake() * 2u32.into(),
    );
    ValidatorManager::<T>::add_collator(
        RawOrigin::Root.into(),
        collator_id.clone(),
        eth_public_key.clone(),
        None,
    )
    .unwrap();

    // Simulate T1 callback to complete registration
    let tx_id = get_tx_id_for_validator::<T>(collator_id).unwrap();
    simulate_t1_callback_success::<T>(tx_id);

    //Advance 2 session to add the collator to the session
    advance_session::<T>();
    advance_session::<T>();

    // Clean up the action entry to prevent interference with subsequent operations
    let ingress_counter = <TotalIngresses<T>>::get();
    <ValidatorActions<T>>::remove(collator_id, ingress_counter);
}

benchmarks! {
    add_collator {
        let candidate = account("collator_candidate", 1, 1);
        <T as pallet_parachain_staking::Config>::Currency::make_free_balance_be(&candidate, ParachainStaking::<T>::min_collator_stake() * 2u32.into());
        let eth_public_key: ecdsa::Public = Public::from_raw(NEW_COLLATOR_ETHEREUM_PUBLIC_KEY);
        set_session_keys::<T>(&candidate, 20u64);

        assert_eq!(false, pallet_parachain_staking::CandidateInfo::<T>::contains_key(&candidate));
    }: _(RawOrigin::Root, candidate.clone(), eth_public_key, None)
    verify {
        // After extrinsic, ValidatorActions entry is created but candidate not yet joined
        // Need to simulate T1 callback to complete
        let tx_id = get_tx_id_for_validator::<T>(&candidate).unwrap();
        simulate_t1_callback_success::<T>(tx_id);
        assert!(pallet_parachain_staking::CandidateInfo::<T>::contains_key(&candidate));

        // Clean up action to allow subsequent benchmarks to run
        let ingress_counter = <TotalIngresses<T>>::get();
        <ValidatorActions<T>>::remove(&candidate, ingress_counter);
    }

    remove_validator {
        let v in (MINIMUM_ADDITIONAL_BENCHMARKS_VALIDATORS as u32 + 1) .. MAX_VALIDATOR_ACCOUNTS;

        setup_additional_validators::<T>(v);
        let (caller_account, caller_id, _) = generate_sender_collator_account_details::<T>();
        let caller = Validator::new(caller_account.clone(), caller_id.clone());

    }: remove_validator(RawOrigin::Root, caller_account.clone())
    verify {
        // Verify ValidatorActions entry was created with Resignation type
        let ingress_counter = <TotalIngresses<T>>::get();
        assert_eq!(true, ValidatorActions::<T>::contains_key(&caller_account, ingress_counter));

        // After extrinsic, validator is still in ValidatorAccountIds (removed by session handler later)
        assert!(ValidatorAccountIds::<T>::get().unwrap().contains(&caller_account));

        // Clean up action to allow subsequent benchmarks to run
        <ValidatorActions<T>>::remove(&caller_account, ingress_counter);
    }

    rotate_validator_ethereum_key {
        setup_additional_validators::<T>(2);
        let (account_id, _, rotating_eth_key) = generate_resigning_collator_account_details::<T>();
        advance_session::<T>();
        advance_session::<T>();

        let eth_new_public_key: ecdsa::Public = Public::from_raw(NEW_COLLATOR_ETHEREUM_PUBLIC_KEY);

        assert!(EthereumPublicKeys::<T>::get(&rotating_eth_key).is_some());
        assert!(EthereumPublicKeys::<T>::get(&eth_new_public_key).is_none());

    }: _(RawOrigin::Root, account_id, rotating_eth_key.clone(), eth_new_public_key.clone())
    verify {
        assert!(EthereumPublicKeys::<T>::get(&eth_new_public_key).is_some());
        assert!(EthereumPublicKeys::<T>::get(&rotating_eth_key).is_none());
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().with_validators().as_externality(),
    crate::mock::TestRuntime,
);
