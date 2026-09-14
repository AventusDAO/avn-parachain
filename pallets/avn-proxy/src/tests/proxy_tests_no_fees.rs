//Copyright 2022 Aventus Network Systems (UK) Ltd.

#![cfg(test)]
use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_avn_common::{recover_ethereum_address_from_ecdsa_signature, HashMessageFormat};
use sp_core::ByteArray;
use sp_runtime::traits::{BadOrigin, Hash};

mod proxy_without_fees {
    use super::*;

    mod succeeds_when {
        use super::*;

        #[test]
        fn call_targets_a_whitelisted_pallet() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let inner_call = context.create_valid_inner_call();
                let call_hash = Hashing::hash_of(&inner_call);

                assert_eq!(false, proxy_event_emitted(context.relayer.account_id(), call_hash));
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call,
                    None
                ));
                assert_eq!(true, proxy_event_emitted(context.relayer.account_id(), call_hash));
            })
        }

        #[test]
        fn call_is_proxied_with_good_parameters() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let inner_call = create_signed_mint_single_nft_call(&context);

                assert_eq!(false, single_nft_minted_events_emitted());
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call,
                    None
                ));
                assert_eq!(true, single_nft_minted_events_emitted());
            })
        }

        #[test]
        fn ecdsa_hash_works() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                use sp_avn_common::{hash_string_data_with_ethereum_prefix, verify_multi_signature, verify_signature as verify_sig, Proof};
                use hex_literal::hex;
                use sp_runtime::MultiSignature;

                let signed_transfer_context: &'static [u8] = b"authorization for transfer operation";
                let public_key_bytes: [u8; 32] = hex!("e66ebc6f8820f1d27706ae812f208e5dfbf741e9463892471a24abf6a4d4ab4b");
                let public_key = sp_core::sr25519::Public::from_raw(public_key_bytes);
                let relayer: AccountId = AccountId::decode(&mut public_key.encode().as_slice()).unwrap();

                let public_key_bytes: [u8; 32] = hex!("6fc436cd5273e4ecfd09079c982e2824825d5903cb2cf6cfd02586fbf6faa9a3");
                let public_key = sp_core::sr25519::Public::from_raw(public_key_bytes);
                let from: AccountId = AccountId::decode(&mut public_key.encode().as_slice()).unwrap();

                let public_key_bytes: [u8; 32] = hex!("ea055d6f2e2280ecfd691e28f3062047c3904273ea699ec5d05c43a5b8413e55");
                let public_key = sp_core::sr25519::Public::from_raw(public_key_bytes);
                let to: AccountId = AccountId::decode(&mut public_key.encode().as_slice()).unwrap();

                let token_id = H160::from_slice(&hex!("bfaffd8001493dfeb51c26748d2aff53c2984190"));
                let amount: u128 = 22u128;
                let sender_nonce: u64 = 2u64;

                let encoded_data = (signed_transfer_context, relayer.clone(), from, to, token_id, amount, sender_nonce, ).encode();

                // make sure we can hash
                assert_ok!(hash_string_data_with_ethereum_prefix(&encoded_data));

                let sig_hex = hex!("1cd1bd9e23eee5df0d0a7e21d88775a5d9020eb946fb89c8fc649d3a1c1e4c330f9474ece85b49813370dc249c98175337f0aee0108415c6f2988c0e37b554671c");
                let sig = sp_core::ecdsa::Signature::from_slice(&sig_hex).unwrap();
                let result = recover_ethereum_address_from_ecdsa_signature(&sig, &encoded_data, HashMessageFormat::String).unwrap();

                let expected_eth_address = H160::from_slice(&hex!("6E43697Ca52437e76743ad0B932189872F9612E6"));

                assert_eq!(result, expected_eth_address[..]);

                let public_key_bytes: [u8; 32] = hex!("844da93c328fc981b1c507a926bb64055f01ec6a7b4d1ab9dc79e086655d368d");
                let public_key = sp_core::sr25519::Public::from_raw(public_key_bytes);

                let account_id: AccountId = AccountId::decode(&mut public_key.encode().as_slice()).unwrap();
                assert_ok!(verify_multi_signature::<Signature, AccountId>(&account_id, &MultiSignature::from(sig.clone()), &encoded_data));

                let proof = Proof::<Signature, AccountId> {
                    signer: account_id.clone(),
                    relayer: account_id,
                    signature: MultiSignature::from(sig),
                };

                assert_ok!(verify_sig::<Signature, AccountId>(&proof, &encoded_data));
            })
        }

        #[test]
        fn call_is_proxied_with_good_parameters_ecdsa() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let inner_call = create_signed_mint_single_nft_call_ecdsa(&context);

                assert_eq!(false, single_nft_minted_events_emitted());
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call,
                    None
                ));
                assert_eq!(true, single_nft_minted_events_emitted());
            })
        }

        #[test]
        fn inner_call_fails_to_execute() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let inner_call = create_signed_mint_single_nft_call(&context);

                assert_eq!(false, single_nft_minted_events_emitted());
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call,
                    None
                ));
                assert_eq!(single_nft_minted_events_count(), 1);

                let inner_call_with_duplicate_external_ref =
                    create_signed_mint_single_nft_call(&context);
                let call_hash = Hashing::hash_of(&inner_call_with_duplicate_external_ref);

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call_with_duplicate_external_ref,
                    None
                ));

                assert_eq!(
                    true,
                    inner_call_failed_event_emitted(context.relayer.account_id(), call_hash)
                );
            })
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let inner_call = context.create_valid_inner_call();

                let unsigned_origin = RawOrigin::None.into();

                assert_noop!(AvnProxy::proxy(unsigned_origin, inner_call, None), BadOrigin);
            });
        }

        #[test]
        fn call_is_not_allowed_to_be_proxied() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();

                let invalid_inner_call = context.create_invalid_inner_call();

                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        invalid_inner_call,
                        None
                    ),
                    Error::<TestRuntime>::TransactionNotSupported
                );

                assert_eq!(System::events().len(), 0);
            })
        }

        #[test]
        fn sender_has_not_signed_relayer() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let inner_call = context.create_valid_inner_call();
                let call_hash = Hashing::hash_of(&inner_call);

                let invalid_relayer = context.signer.account_id();

                assert_eq!(false, proxy_event_emitted(context.relayer.account_id(), call_hash));
                assert_noop!(
                    AvnProxy::proxy(RuntimeOrigin::signed(invalid_relayer), inner_call, None),
                    Error::<TestRuntime>::UnauthorizedProxyTransaction
                );
            })
        }

        #[test]
        fn call_is_proxying_the_proxy_extrinsic() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();

                let invalid_inner_call = context.create_proxy_call();

                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        invalid_inner_call,
                        None
                    ),
                    Error::<TestRuntime>::TransactionNotSupported
                );

                assert_eq!(System::events().len(), 0);
            })
        }
    }
}
