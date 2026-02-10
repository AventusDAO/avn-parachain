#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_system::RawOrigin;
use libsecp256k1::{Message, PublicKey, SecretKey};
use sp_core::{ecdsa, H160};

use crate::Pallet as CrossChainVoting;

fn eth_address_from_seed(seed: [u8; 32]) -> H160 {
    let sk = SecretKey::parse(&seed).expect("32 bytes; within curve order for benchmarks");
    let pk = PublicKey::from_secret_key(&sk);
    let uncompressed = pk.serialize();
    let hash = sp_io::hashing::keccak_256(&uncompressed[1..]);
    H160::from_slice(&hash[12..])
}

fn sign_payload_from_seed<AccountId: Encode>(
    seed: [u8; 32],
    payload: &LinkPayload<AccountId>,
) -> ecdsa::Signature {
    let msg = payload.signing_bytes();

    let hash = sp_avn_common::hash_string_data_with_ethereum_prefix(&msg)
        .expect("hashing should succeed in benchmarks");

    let sk = SecretKey::parse(&seed).expect("valid secret key for benchmarks");
    let m = Message::parse(&hash);
    let (sig, recid) = libsecp256k1::sign(&m, &sk);

    let mut sig65 = [0u8; 65];
    sig65[0..64].copy_from_slice(&sig.serialize());
    sig65[64] = recid.serialize(); // 0 or 1

    ecdsa::Signature::from_raw(sig65)
}

benchmarks! {
    link_account {
        let caller: T::AccountId = account("t2", 0, 0);

        let seed = [1u8; 32];
        let t1 = eth_address_from_seed(seed);

        let payload = LinkPayload::<T::AccountId> {
            action: Action::Link,
            t1_identity_account: t1,
            t2_linked_account: caller.clone(),
            chain_id: 1u64,
        };

        let sig = sign_payload_from_seed(seed, &payload);

    }: _(RawOrigin::Signed(caller.clone()), payload, sig)
    verify {
        assert_eq!(LinkedAccountToIdentity::<T>::get(&caller), Some(t1));
        let linked = LinkedAccounts::<T>::get(t1);
        assert!(linked.contains(&caller));
    }

    unlink_account {
        let caller: T::AccountId = account("t2", 0, 0);

        let seed = [1u8; 32];
        let t1 = eth_address_from_seed(seed);

        let link_payload = LinkPayload::<T::AccountId> {
            action: Action::Link,
            t1_identity_account: t1,
            t2_linked_account: caller.clone(),
            chain_id: 1u64,
        };
        let sig = sign_payload_from_seed(seed, &link_payload);
        CrossChainVoting::<T>::link_account(
            RawOrigin::Signed(caller.clone()).into(),
            link_payload,
            sig
        )?;

        let unlink_payload = LinkPayload::<T::AccountId> {
            action: Action::Unlink,
            t1_identity_account: t1,
            t2_linked_account: caller.clone(),
            chain_id: 1u64,
        };

    }: _(RawOrigin::Signed(caller.clone()), unlink_payload)
    verify {
        assert_eq!(LinkedAccountToIdentity::<T>::get(&caller), None);
        let linked = LinkedAccounts::<T>::get(t1);
        assert!(!linked.contains(&caller));
    }
}

impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::TestRuntime,);
