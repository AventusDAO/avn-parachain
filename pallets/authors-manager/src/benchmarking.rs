//! # Authors Manager pallet
// Copyright 2024 Aventus Systems (UK) Ltd.

// authors manager pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;

use crate::{Pallet as AuthorsManager, *};
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_support::traits::ValidatorSet as AuthorSet;
use frame_system::{EventRecord, Pallet as System, RawOrigin};
use hex_literal::hex;
use libsecp256k1::{PublicKey, SecretKey};
use pallet_avn::{self as avn};
use pallet_session::Pallet as Session;
use sp_avn_common::eth_key_actions::{compress_eth_public_key, decompress_eth_public_key};
use sp_core::{ecdsa::Public, ByteArray, H512};
use sp_runtime::{RuntimeAppPublic, WeakBoundedVec};

use codec::{Decode, Encode};
use sp_core::{crypto::KeyTypeId, sr25519};
use sp_runtime::traits::OpaqueKeys;

// Resigner keys derived from [6u8; 32] private key
const RESIGNING_AUTHOR_PUBLIC_KEY_BYTES: [u8; 32] =
    hex!["ea3021db7da7831e0d5ed7e60a8102d2d721bcca88adb03ee992f4dec3baee3e"];
const RESIGNING_AUTHOR_ETHEREUM_PUBLIC_KEY: [u8; 33] =
    hex!["03f006a18d5653c4edf5391ff23a61f03ff83d237e880ee61187fa9f379a028e0a"];

// Vote sender keys derived from [7u8; 32] private key
const VOTING_AUTHOR_PUBLIC_KEY_BYTES: [u8; 32] =
    hex!["7c0f469d3bd340bae718203fa30ca071a5e37c751e891dbded837b213d45d91d"];
const VOTING_AUTHOR_ETHEREUM_PUBLIC_KEY: [u8; 33] =
    hex!["02989c0b76cb563971fdc9bef31ec06c3560f3249d6ee9e5d83c57625596e05f6f"];

const NEW_AUTHOR_ETHEREUM_PUBLIC_KEY: [u8; 33] =
    hex!["03f171af36531200540b2badee5ed581b0a51f4e4a1a995025e149b9721b050074"];

const MINIMUM_ADDITIONAL_BENCHMARKS_AUTHORS: usize = 2;

fn generate_resigning_author_account_details<T: Config>(
) -> (T::AccountId, <T as pallet_avn::Config>::AuthorityId, Public) {
    let authority_id =
        <T as avn::Config>::AuthorityId::generate_pair(Some("//avn_resigner".as_bytes().to_vec()));
    let eth_public_key = Public::from_raw(RESIGNING_AUTHOR_ETHEREUM_PUBLIC_KEY);
    let account_id =
        T::AccountId::decode(&mut RESIGNING_AUTHOR_PUBLIC_KEY_BYTES.as_slice()).unwrap();

    (account_id, authority_id, eth_public_key)
}

fn generate_sender_author_account_details<T: Config>(
) -> (T::AccountId, <T as pallet_avn::Config>::AuthorityId, Public) {
    let authority_id =
        <T as avn::Config>::AuthorityId::generate_pair(Some("//avn_sender".as_bytes().to_vec()));
    let eth_public_key = Public::from_raw(VOTING_AUTHOR_ETHEREUM_PUBLIC_KEY);
    let account_id = T::AccountId::decode(&mut VOTING_AUTHOR_PUBLIC_KEY_BYTES.as_slice()).unwrap();

    (account_id, authority_id, eth_public_key)
}

// Add additional authors, on top of genesis configuration
fn setup_additional_authors<T: Config>(number_of_additional_authors: u32) {
    assert!(number_of_additional_authors >= MINIMUM_ADDITIONAL_BENCHMARKS_AUTHORS as u32);

    let mut avn_authors: Vec<Author<<T as pallet_avn::Config>::AuthorityId, T::AccountId>> =
        Vec::new();

    let mut authors: Vec<(T::AccountId, Public)> = Vec::new();
    let vote_sender_index = number_of_additional_authors - (1 as u32);

    for i in 0..number_of_additional_authors {
        let (account, avn_authority_id, eth_key) = match i {
            0 => generate_resigning_author_account_details::<T>(),
            i if i == vote_sender_index => generate_sender_author_account_details::<T>(),
            _ => (
                account("dummy_author", i, i),
                <T as avn::Config>::AuthorityId::generate_pair(None),
                generate_author_eth_public_key_from_seed::<T>(i as u64),
            ),
        };

        avn_authors.push(Author::new(account.clone(), avn_authority_id));
        authors.push((account, eth_key));
    }

    // Setup authors in avn pallet
    let new_avn_authors = avn::Validators::<T>::get();
    // new_avn_authors.append(&mut avn_authors.clone());
    let combined_avn_authors: Vec<_> =
        new_avn_authors.iter().chain(avn_authors.iter()).cloned().collect();
    avn::Validators::<T>::put(WeakBoundedVec::force_from(
        combined_avn_authors,
        Some("Too many authors for session"),
    ));

    authors.iter().enumerate().for_each(|(i, (account_id, eth_public_key))| {
        force_add_author::<T>(&account_id, i as u64, &eth_public_key)
    });
}

fn setup_resignation_action_data<T: Config>(sender: T::AccountId, ingress_counter: IngressCounter) {
    let (action_account_id, _, t1_eth_public_key) =
        generate_resigning_author_account_details::<T>();

    let eth_transaction_id: EthereumId = 0;
    let decompressed_eth_public_key = decompress_eth_public_key(t1_eth_public_key)
        .map_err(|_| Error::<T>::InvalidPublicKey)
        .unwrap();

    AuthorActions::<T>::insert(
        action_account_id,
        ingress_counter,
        AuthorsActionData::new(
            AuthorsActionStatus::AwaitingConfirmation,
            eth_transaction_id,
            AuthorsActionType::Resignation,
        ),
    )
}

fn generate_signature<T: pallet_avn::Config>(
) -> <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature {
    let encoded_data = 0.encode();
    let authority_id = T::AuthorityId::generate_pair(None);
    let signature = authority_id.sign(&encoded_data).expect("able to make signature");
    return signature;
}

fn generate_mock_ecdsa_signature<T: pallet_avn::Config>(msg: u8) -> ecdsa::Signature {
    let signature_bytes: [u8; 65] = [msg; 65];
    return ecdsa::Signature::from_slice(&signature_bytes).unwrap().into();
}

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1 as usize)];
    assert_eq!(event, &system_event);
}

fn advance_session<T: Config>() {
    use frame_support::traits::{OnFinalize, OnInitialize};

    let now = System::<T>::block_number().max(1u32.into());

    System::<T>::on_finalize(System::<T>::block_number());
    System::<T>::set_block_number(now + 1u32.into());
    System::<T>::on_initialize(System::<T>::block_number());
    Session::<T>::on_initialize(System::<T>::block_number());
}

fn create_benchmark_keys<T: Config>(rng: &mut rand::rngs::StdRng) -> T::Keys {
    use rand::RngCore;
    use sp_core::{
        crypto::{ByteArray, KeyTypeId},
        sr25519,
    };
    use sp_runtime::traits::OpaqueKeys;

    const KEY_TYPES: &[KeyTypeId] = &[
        KeyTypeId(*b"aura"), // Aura
        KeyTypeId(*b"gran"), // GRANDPA
        KeyTypeId(*b"avnk"), // Avn - keytypeid has a fixed size of 4 characters
        KeyTypeId(*b"imon"), // IMONLINE
        KeyTypeId(*b"audi"), // Authority discovery
    ];

    let mut keys = Vec::new();
    for key_type in KEY_TYPES {
        let mut key_data = [0u8; 32];
        rng.fill_bytes(&mut key_data);
        let key = sr25519::Public::from_raw(key_data);
        keys.push((*key_type, key.as_slice().to_vec()));
    }

    T::Keys::decode(&mut &keys.encode()[..]).expect("Failed to create benchmark keys")
}

fn set_session_keys<T: Config>(author_id: &T::AccountId, index: u64) {
    use rand::{RngCore, SeedableRng};
    frame_system::Pallet::<T>::inc_providers(author_id);

    let mut rng = rand::rngs::StdRng::seed_from_u64(index);

    let keys = create_benchmark_keys::<T>(&mut rng);

    pallet_session::Pallet::<T>::set_keys(
        RawOrigin::<T::AccountId>::Signed(author_id.clone()).into(),
        keys,
        Vec::new(),
    )
    .expect("Failed to set session keys");
}

fn generate_author_eth_public_key_from_seed<T: Config>(seed: u64) -> Public {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let secret_key = SecretKey::random(&mut rng);
    let public_key = PublicKey::from_secret_key(&secret_key);

    return compress_eth_public_key(H512::from_slice(&public_key.serialize()[1..]));
}

fn force_add_author<T: Config>(author_id: &T::AccountId, index: u64, eth_public_key: &Public) {
    set_session_keys::<T>(author_id, index);
    AuthorsManager::<T>::add_author(
        RawOrigin::Root.into(),
        author_id.clone(),
        eth_public_key.clone(),
    )
    .unwrap();

    let tx_id = AuthorActions::<T>::iter()
        .find(|(account_id, _, _)| account_id == author_id)
        .map(|(_, _, data)| data.eth_transaction_id)
        .expect("Action should exist");

    AuthorsManager::<T>::process_result(tx_id, PALLET_ID.to_vec(), true).unwrap();

    //Advance 2 sessions to add the author to the session
    advance_session::<T>();
    advance_session::<T>();
}

benchmarks! {
    add_author {
        let candidate: T::AccountId = account("author_candidate", 1, 1);
        let candidate_id = <pallet_session::Pallet<T> as AuthorSet<T::AccountId>>::ValidatorIdOf::convert(candidate.clone()).unwrap();
        let eth_public_key: ecdsa::Public = Public::from_raw(NEW_AUTHOR_ETHEREUM_PUBLIC_KEY);
        set_session_keys::<T>(&candidate, 20u64);
        assert_eq!(false, Session::<T>::validators().contains(&candidate_id));
    }: _(RawOrigin::Root, candidate.clone(), eth_public_key)
    verify {
        assert_last_event::<T>(Event::<T>::AuthorActionPublished{
            author_id: candidate.clone(),
            action_type: AuthorsActionType::Registration,
            tx_id: 0
        }.into());
    }

    remove_author {
        let v in (MINIMUM_ADDITIONAL_BENCHMARKS_AUTHORS as u32 + 1) .. MAX_AUTHOR_ACCOUNTS;

        setup_additional_authors::<T>(v);
        let (caller_account, caller_id, _) = generate_sender_author_account_details::<T>();
        let caller = Author::new(caller_account.clone(), caller_id.clone());

    }: remove_author(RawOrigin::Root, caller_account.clone())
    verify {
        // Author is not removed yet (only after T1 confirmation)
        assert_eq!(AuthorAccountIds::<T>::get().unwrap().iter().position(|author_account_id| *author_account_id == caller_account).is_some(), true);
        // Extract the actual tx_id from storage
        let ingress_counter = AuthorsManager::<T>::get_ingress_counter();
        let action_data = AuthorActions::<T>::get(&caller_account, ingress_counter)
            .expect("Author action should exist");
        let actual_tx_id = action_data.eth_transaction_id;
        assert_last_event::<T>(Event::<T>::AuthorActionPublished{
            author_id: caller_account.clone(),
            action_type: AuthorsActionType::Resignation,
            tx_id: actual_tx_id
        }.into());
    }

    rotate_author_ethereum_key {
    setup_additional_authors::<T>(2);
    let (account_id, _, rotating_eth_key) = generate_resigning_author_account_details::<T>();
    advance_session::<T>();
    advance_session::<T>();

    let eth_new_public_key: ecdsa::Public = Public::from_raw(NEW_AUTHOR_ETHEREUM_PUBLIC_KEY);

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
    crate::mock::ExtBuilder::build_default().with_authors().as_externality(),
    crate::mock::TestRuntime,
);
