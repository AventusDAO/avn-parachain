//Copyright 2022 Aventus Network Systems (UK) Ltd.

#![cfg(test)]
use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::traits::Hash;

pub const GATEWAY_FEE: u128 = ONE_AVT;
pub const PAYMENT_AUTH_CONTEXT: &'static [u8] = b"authorization for proxy payment";

pub fn create_default_payment_authorisation(
    context: &ProxyContext,
    proxy_proof: Proof<Signature, AccountId>,
) -> PaymentInfo<AccountId, u128, Signature, Token> {
    return create_payment_authorisation(
        &context.relayer,
        &context.signer,
        proxy_proof,
        0_u64,
        AVT_TOKEN_CONTRACT,
    );
}

pub fn create_payment_authorisation_with_nonce(
    context: &ProxyContext,
    proxy_proof: Proof<Signature, AccountId>,
    nonce: u64,
    token: H160,
) -> PaymentInfo<AccountId, u128, Signature, Token> {
    let data_to_sign = (
        PAYMENT_AUTH_CONTEXT,
        &proxy_proof,
        &context.relayer.account_id(),
        &GATEWAY_FEE,
        token,
        nonce,
    );
    let signature = context.signer.sign(&data_to_sign.encode());

    let payment_info = PaymentInfo {
        payer: context.signer.account_id(),
        recipient: context.relayer.account_id(),
        amount: GATEWAY_FEE.into(),
        signature,
        token,
    };

    return payment_info;
}

pub fn create_payment_authorisation(
    relayer: &TestAccount,
    payer: &TestAccount,
    proxy_proof: Proof<Signature, AccountId>,
    nonce: u64,
    token: H160,
) -> PaymentInfo<AccountId, u128, Signature, Token> {
    let data_to_sign =
        (PAYMENT_AUTH_CONTEXT, &proxy_proof, &relayer.account_id(), &GATEWAY_FEE, token, nonce);
    let signature = payer.sign(&data_to_sign.encode());

    let payment_info = PaymentInfo {
        payer: payer.account_id(),
        recipient: relayer.account_id(),
        amount: GATEWAY_FEE.into(),
        signature,
        token,
    };

    return payment_info;
}

mod charging_fees {
    use super::*;

    mod succeeds_when {
        use super::*;

        #[test]
        fn call_is_proxied_with_good_parameters() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let single_nft_data: SingleNftContext = Default::default();
                let proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);
                let payment_authorisation =
                    Some(Box::new(create_default_payment_authorisation(&context, proxy_proof)));

                let call_hash = Hashing::hash_of(&inner_call);

                let signer_balance = Balances::free_balance(context.signer.account_id());
                let relayer_balance = Balances::free_balance(context.relayer.account_id());

                assert_eq!(false, single_nft_minted_events_emitted());
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call,
                    payment_authorisation
                ));
                assert_eq!(true, proxy_event_emitted(context.relayer.account_id(), call_hash));
                assert_eq!(true, single_nft_minted_events_emitted());

                // Check that a fee has been paid
                assert_eq!(
                    signer_balance - ONE_AVT,
                    Balances::free_balance(context.signer.account_id())
                );
                assert_eq!(
                    relayer_balance + ONE_AVT,
                    Balances::free_balance(context.relayer.account_id())
                );
            })
        }

        #[test]
        fn signature_is_valid_but_call_is_rejected() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let mut single_nft_data: SingleNftContext = Default::default();

                // This will fail the call because unique_external_ref is mandatory
                single_nft_data.unique_external_ref = String::from("").into_bytes();

                let proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);
                let payment_authorisation =
                    Some(Box::new(create_default_payment_authorisation(&context, proxy_proof)));

                let call_hash = Hashing::hash_of(&inner_call);

                let signer_balance = Balances::free_balance(context.signer.account_id());
                let relayer_balance = Balances::free_balance(context.relayer.account_id());

                // Dispatch fails
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call,
                    payment_authorisation
                ));

                assert_eq!(
                    true,
                    inner_call_failed_event_emitted(context.relayer.account_id(), call_hash)
                );
                assert_eq!(false, proxy_event_emitted(context.relayer.account_id(), call_hash));
                assert_eq!(false, single_nft_minted_events_emitted());

                // Fee has been paid by the signer
                assert_eq!(
                    signer_balance - ONE_AVT,
                    Balances::free_balance(context.signer.account_id())
                );
                assert_eq!(
                    relayer_balance + ONE_AVT,
                    Balances::free_balance(context.relayer.account_id())
                );
            })
        }

        #[test]
        fn payer_and_signer_are_different() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();

                let single_nft_data: SingleNftContext = Default::default();
                let proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);

                // Create a new payer and fund them
                let new_payment_proof_signer = &TestAccount::new([100u8; 32]);
                Balances::make_free_balance_be(&new_payment_proof_signer.account_id(), HUNDRED_AVT);

                let payment_authorisation = Some(Box::new(create_payment_authorisation(
                    &context.relayer,
                    new_payment_proof_signer,
                    proxy_proof,
                    0u64,
                    AVT_TOKEN_CONTRACT,
                )));

                let call_hash = Hashing::hash_of(&inner_call);

                let signer_balance = Balances::free_balance(context.signer.account_id());
                let relayer_balance = Balances::free_balance(context.relayer.account_id());
                let new_payer_balance =
                    Balances::free_balance(new_payment_proof_signer.account_id());

                assert_eq!(false, single_nft_minted_events_emitted());
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call,
                    payment_authorisation
                ));
                assert_eq!(true, proxy_event_emitted(context.relayer.account_id(), call_hash));
                assert_eq!(true, single_nft_minted_events_emitted());

                // The signer is not affected because `new payer` is paying the fees
                assert_eq!(signer_balance, Balances::free_balance(context.signer.account_id()));

                // The new payer pays 1 AVT
                assert_eq!(
                    new_payer_balance - ONE_AVT,
                    Balances::free_balance(new_payment_proof_signer.account_id())
                );

                // The relayer receives 1 AVT
                assert_eq!(
                    relayer_balance + ONE_AVT,
                    Balances::free_balance(context.relayer.account_id())
                );
            })
        }

        #[test]
        fn invalid_inner_call_proof_is_given() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let single_nft_data: SingleNftContext = Default::default();
                let mut proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);

                // make the proxy proof invalid
                proxy_proof.signer = context.relayer.account_id();

                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);
                let payment_authorisation =
                    create_default_payment_authorisation(&context, proxy_proof);

                let signer_balance = Balances::free_balance(context.signer.account_id());
                let signer_nonce = AvnProxy::payment_nonces(context.signer.account_id());
                let relayer_balance = Balances::free_balance(context.relayer.account_id());

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call,
                    Some(Box::new(payment_authorisation))
                ));

                // Check that fee has been paid by the signer
                assert_eq!(
                    signer_balance - ONE_AVT,
                    Balances::free_balance(context.signer.account_id())
                );
                assert_eq!(
                    relayer_balance + ONE_AVT,
                    Balances::free_balance(context.relayer.account_id())
                );

                // Check that payment nonce has increased
                assert_eq!(AvnProxy::payment_nonces(context.signer.account_id()), signer_nonce + 1);
            })
        }
    }

    mod fails_when {
        use sp_runtime::TokenError;

        use super::*;

        #[test]
        fn wrong_fee_is_signed() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let single_nft_data: SingleNftContext = Default::default();

                let proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);

                let mut payment_authorisation =
                    create_default_payment_authorisation(&context, proxy_proof);

                //bad amount
                payment_authorisation.amount = payment_authorisation.amount + 1_u128;

                let signer_balance = Balances::free_balance(context.signer.account_id());
                let relayer_balance = Balances::free_balance(context.relayer.account_id());

                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        inner_call,
                        Some(Box::new(payment_authorisation))
                    ),
                    Error::<TestRuntime>::UnauthorizedFee
                );

                // Check that a fee has not been paid
                assert_eq!(signer_balance, Balances::free_balance(context.signer.account_id()));
                assert_eq!(relayer_balance, Balances::free_balance(context.relayer.account_id()));
            })
        }

        #[test]
        fn wrong_recipient_is_signed() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let single_nft_data: SingleNftContext = Default::default();

                let proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);

                let mut payment_authorisation =
                    create_default_payment_authorisation(&context, proxy_proof);

                //bad recipient
                payment_authorisation.recipient = context.signer.account_id();

                let signer_balance = Balances::free_balance(context.signer.account_id());
                let relayer_balance = Balances::free_balance(context.relayer.account_id());

                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        inner_call,
                        Some(Box::new(payment_authorisation))
                    ),
                    Error::<TestRuntime>::UnauthorizedFee
                );

                // Check that a fee has not been paid
                assert_eq!(signer_balance, Balances::free_balance(context.signer.account_id()));
                assert_eq!(relayer_balance, Balances::free_balance(context.relayer.account_id()));
            })
        }

        #[test]
        fn wrong_nonce_is_signed() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let single_nft_data: SingleNftContext = Default::default();

                let proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);

                let bad_nonce = 10_u64;
                let payment_authorisation = create_payment_authorisation_with_nonce(
                    &context,
                    proxy_proof,
                    bad_nonce,
                    AVT_TOKEN_CONTRACT,
                );

                let signer_balance = Balances::free_balance(context.signer.account_id());
                let relayer_balance = Balances::free_balance(context.relayer.account_id());

                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        inner_call,
                        Some(Box::new(payment_authorisation))
                    ),
                    Error::<TestRuntime>::UnauthorizedFee
                );

                // Check that a fee has not been paid
                assert_eq!(signer_balance, Balances::free_balance(context.signer.account_id()));
                assert_eq!(relayer_balance, Balances::free_balance(context.relayer.account_id()));
            })
        }

        #[test]
        fn payment_authorisation_is_replayed() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let single_nft_data: SingleNftContext = Default::default();
                let proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);
                let payment_authorisation =
                    Some(Box::new(create_default_payment_authorisation(&context, proxy_proof)));

                let mut signer_balance = Balances::free_balance(context.signer.account_id());
                let mut relayer_balance = Balances::free_balance(context.relayer.account_id());

                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    inner_call.clone(),
                    payment_authorisation.clone()
                ));

                // Check mint event has been emitted
                assert_eq!(true, single_nft_minted_events_emitted());

                // Check that a fee has been paid
                assert_eq!(
                    signer_balance - ONE_AVT,
                    Balances::free_balance(context.signer.account_id())
                );
                assert_eq!(
                    relayer_balance + ONE_AVT,
                    Balances::free_balance(context.relayer.account_id())
                );

                // Update the balance after the successfull run
                signer_balance = Balances::free_balance(context.signer.account_id());
                relayer_balance = Balances::free_balance(context.relayer.account_id());

                // Replay the same fee signature
                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        inner_call,
                        payment_authorisation
                    ),
                    Error::<TestRuntime>::UnauthorizedFee
                );

                // Check that a fee has not been paid
                assert_eq!(signer_balance, Balances::free_balance(context.signer.account_id()));
                assert_eq!(relayer_balance, Balances::free_balance(context.relayer.account_id()));
            })
        }

        // This test is handled by Substrate automatically, but just adding it for completeness
        #[test]
        fn signer_doesnt_have_enough_avt_to_pay_fees() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let mut context: ProxyContext = Default::default();

                //Account with no AVT
                context.signer = TestAccount::new([25u8; 32]);
                assert_eq!(Balances::free_balance(context.signer.account_id()), 0_u128);

                let single_nft_data: SingleNftContext = Default::default();
                let proxy_proof = get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let inner_call = get_signed_mint_single_nft_call(&single_nft_data, &proxy_proof);
                let payment_authorisation =
                    Some(Box::new(create_default_payment_authorisation(&context, proxy_proof)));

                let relayer_balance = Balances::free_balance(context.relayer.account_id());

                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        inner_call,
                        payment_authorisation
                    ),
                    TokenError::FundsUnavailable
                );

                // No mint events
                assert_eq!(false, single_nft_minted_events_emitted());

                // Check that a fee has not been paid
                assert_eq!(0_u128, Balances::free_balance(context.signer.account_id()));
                assert_eq!(relayer_balance, Balances::free_balance(context.relayer.account_id()));
            })
        }

        #[test]
        fn inner_call_proof_does_not_match_fee_proof() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();
            ext.execute_with(|| {
                let context: ProxyContext = Default::default();
                let single_nft_data: SingleNftContext = Default::default();
                let original_proxy_proof =
                    get_mint_single_nft_proxy_proof(&context, &single_nft_data);
                let original_payment_authorisation =
                    create_default_payment_authorisation(&context, original_proxy_proof.clone());
                let original_inner_call =
                    get_signed_mint_single_nft_call(&single_nft_data, &original_proxy_proof);

                // Generate a new valid proof with different data
                let mut new_single_nft_data: SingleNftContext = single_nft_data.clone();
                new_single_nft_data.unique_external_ref =
                    String::from("New unique ref").into_bytes();
                let new_proxy_proof =
                    get_mint_single_nft_proxy_proof(&context, &new_single_nft_data);
                let new_payment_authorisation =
                    create_default_payment_authorisation(&context, new_proxy_proof.clone());
                let new_inner_call =
                    get_signed_mint_single_nft_call(&new_single_nft_data, &new_proxy_proof);

                let signer_balance = Balances::free_balance(context.signer.account_id());
                let relayer_balance = Balances::free_balance(context.relayer.account_id());

                // Mismatching inner call proof and payment authorisation
                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        original_inner_call.clone(),
                        Some(Box::new(new_payment_authorisation))
                    ),
                    Error::<TestRuntime>::UnauthorizedFee
                );

                // Mismatching inner call proof and payment authorisation
                assert_noop!(
                    AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        new_inner_call,
                        Some(Box::new(original_payment_authorisation.clone()))
                    ),
                    Error::<TestRuntime>::UnauthorizedFee
                );

                // Check that a fee has not been paid
                assert_eq!(signer_balance, Balances::free_balance(context.signer.account_id()));
                assert_eq!(relayer_balance, Balances::free_balance(context.relayer.account_id()));

                //Now show that the original proof and original payment authorisation are valid
                assert_ok!(AvnProxy::proxy(
                    RuntimeOrigin::signed(context.relayer.account_id()),
                    original_inner_call,
                    Some(Box::new(original_payment_authorisation))
                ));

                // Check that a fee has been paid
                assert_eq!(
                    signer_balance - ONE_AVT,
                    Balances::free_balance(context.signer.account_id())
                );
                assert_eq!(
                    relayer_balance + ONE_AVT,
                    Balances::free_balance(context.relayer.account_id())
                );
            })
        }
    }
}

mod for_token_transfer_extrinsics {
    use super::*;

    pub const SIGNED_TRANSFER_CONTEXT: &'static [u8] = b"authorization for transfer operation";

    #[derive(Clone)]
    pub struct TokenTransferContext {
        pub from: AccountId,
        pub to: AccountId,
        pub token: H160,
        pub amount: u128,
    }

    impl Default for TokenTransferContext {
        fn default() -> Self {
            let context: ProxyContext = Default::default();
            let from = context.signer.account_id();
            let to = TestAccount::new([188u8; 32]).account_id();
            let token = AVT_TOKEN_CONTRACT;
            let amount = ONE_AVT;
            TokenTransferContext { from, to, token, amount }
        }
    }

    pub fn get_signed_transfer_token_call(
        transfer_data: &TokenTransferContext,
        proof: &Proof<Signature, AccountId>,
    ) -> Box<<TestRuntime as Config>::RuntimeCall> {
        return Box::new(crate::mock::RuntimeCall::TokenManager(
            TokenManagerCall::signed_transfer {
                proof: proof.clone(),
                from: transfer_data.from.clone(),
                to: transfer_data.to.clone(),
                token_id: transfer_data.token,
                amount: transfer_data.amount,
            },
        ));
    }

    pub fn get_signed_transfer_proxy_proof(
        context: &ProxyContext,
        data: &TokenTransferContext,
    ) -> Proof<Signature, AccountId> {
        let nonce = 0_u64;
        let data_to_sign = (
            SIGNED_TRANSFER_CONTEXT,
            context.relayer.account_id(),
            &data.from,
            &data.to,
            data.token,
            data.amount,
            nonce,
        );

        let signature = context.signer.sign(&data_to_sign.encode());

        let proof = Proof::<Signature, AccountId> {
            signer: context.signer.account_id(),
            relayer: context.relayer.account_id(),
            signature,
        };

        return proof;
    }

    pub fn native_token_transfered_events_emitted() -> bool {
        return System::events()
            .into_iter()
            .map(|r| r.event)
            .filter_map(|e| if let RuntimeEvent::Balances(inner) = e { Some(inner) } else { None })
            .count()
            > 0;
    }

    pub fn non_native_token_transfered_events_emitted() -> bool {
        return System::events()
            .into_iter()
            .map(|r| r.event)
            .filter_map(
                |e| if let RuntimeEvent::TokenManager(inner) = e { Some(inner) } else { None },
            )
            .count()
            > 0;
    }

    mod charging_fees {
        use super::*;

        mod succeeds_when {
            use super::*;

            #[test]
            fn transfer_call_is_proxied_with_good_parameters() {
                let mut ext = ExtBuilder::build_default().with_balances().as_externality();
                ext.execute_with(|| {
                    let context: ProxyContext = Default::default();
                    let transfer_data: TokenTransferContext = Default::default();
                    let proxy_proof = get_signed_transfer_proxy_proof(&context, &transfer_data);
                    let inner_call = get_signed_transfer_token_call(&transfer_data, &proxy_proof);
                    let payment_authorisation =
                        Some(Box::new(create_default_payment_authorisation(&context, proxy_proof)));

                    let call_hash = Hashing::hash_of(&inner_call);

                    let signer_balance = Balances::free_balance(context.signer.account_id());
                    let relayer_balance = Balances::free_balance(context.relayer.account_id());

                    assert_eq!(false, native_token_transfered_events_emitted());
                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        inner_call,
                        payment_authorisation
                    ));

                    assert_eq!(
                        true,
                        proxy_event_emitted(context.relayer.account_id(), call_hash),
                        "Proxy failed"
                    );
                    assert_eq!(
                        true,
                        native_token_transfered_events_emitted(),
                        "Tokens not transfered"
                    );

                    // Check that a fee has been paid
                    assert_eq!(
                        signer_balance - (ONE_AVT + ONE_AVT), /* 1 for the relayer and another
                                                               * to transfer */
                        Balances::free_balance(context.signer.account_id())
                    );
                    assert_eq!(
                        relayer_balance + ONE_AVT,
                        Balances::free_balance(context.relayer.account_id())
                    );
                })
            }

            #[test]
            fn payer_and_signer_are_different() {
                let mut ext = ExtBuilder::build_default().with_balances().as_externality();
                ext.execute_with(|| {
                    let context: ProxyContext = Default::default();

                    let transfer_data: TokenTransferContext = Default::default();
                    let proxy_proof = get_signed_transfer_proxy_proof(&context, &transfer_data);
                    let inner_call = get_signed_transfer_token_call(&transfer_data, &proxy_proof);

                    // Create a new payer and fund them
                    let new_payment_proof_signer = &TestAccount::new([100u8; 32]);
                    Balances::make_free_balance_be(
                        &new_payment_proof_signer.account_id(),
                        HUNDRED_AVT,
                    );

                    let payment_authorisation = Some(Box::new(create_payment_authorisation(
                        &context.relayer,
                        new_payment_proof_signer,
                        proxy_proof,
                        0u64,
                        AVT_TOKEN_CONTRACT,
                    )));

                    let call_hash = Hashing::hash_of(&inner_call);

                    let signer_balance = Balances::free_balance(context.signer.account_id());
                    let relayer_balance = Balances::free_balance(context.relayer.account_id());
                    let new_payer_balance =
                        Balances::free_balance(new_payment_proof_signer.account_id());

                    <frame_system::Pallet<TestRuntime>>::reset_events();
                    assert_ok!(AvnProxy::proxy(
                        RuntimeOrigin::signed(context.relayer.account_id()),
                        inner_call,
                        payment_authorisation
                    ));
                    assert_eq!(
                        true,
                        proxy_event_emitted(context.relayer.account_id(), call_hash),
                        "Proxy failed"
                    );
                    assert_eq!(
                        true,
                        native_token_transfered_events_emitted(),
                        "Tokens not transfered"
                    );

                    // The signer transfers 1 AVT but is not affected by the proxy fee because `new
                    // payer` is paying the fees
                    assert_eq!(
                        signer_balance - ONE_AVT,
                        Balances::free_balance(context.signer.account_id())
                    );

                    // The new payer pays 1 AVT
                    assert_eq!(
                        new_payer_balance - ONE_AVT,
                        Balances::free_balance(new_payment_proof_signer.account_id())
                    );

                    // The relayer receives 1 AVT
                    assert_eq!(
                        relayer_balance + ONE_AVT,
                        Balances::free_balance(context.relayer.account_id())
                    );
                })
            }

            mod non_native_tokens_are_used_for_payment {
                use super::*;

                #[test]
                fn with_the_same_payer_and_signer() {
                    let mut ext = ExtBuilder::build_default().with_balances().as_externality();
                    ext.execute_with(|| {
                        let mut context: ProxyContext = Default::default();
                        let mut transfer_data: TokenTransferContext = Default::default();

                        // Make the signer the same as the payer
                        context.signer = get_default_signer();
                        transfer_data.from = context.signer.account_id();
                        let new_payment_proof_signer = get_default_signer();
                        Balances::make_free_balance_be(&transfer_data.from, HUNDRED_AVT);

                        let proxy_proof = get_signed_transfer_proxy_proof(&context, &transfer_data);
                        let inner_call =
                            get_signed_transfer_token_call(&transfer_data, &proxy_proof);

                        let payment_authorisation = Some(Box::new(create_payment_authorisation(
                            &context.relayer,
                            &new_payment_proof_signer,
                            proxy_proof,
                            0u64,
                            NON_AVT_TOKEN_CONTRACT,
                        )));

                        let call_hash = Hashing::hash_of(&inner_call);

                        let signer_avt_balance =
                            Balances::free_balance(context.signer.account_id());
                        let relayer_avt_balance =
                            Balances::free_balance(context.relayer.account_id());
                        let relayer_non_avt_balance = TokenManager::get_token_balance(
                            &context.relayer.account_id(),
                            &NON_AVT_TOKEN_CONTRACT,
                        )
                        .unwrap_or(0_u128.into());
                        let new_payer_avt_balance =
                            Balances::free_balance(new_payment_proof_signer.account_id());
                        let new_payer_non_avt_balance = TokenManager::get_token_balance(
                            &new_payment_proof_signer.account_id(),
                            &NON_AVT_TOKEN_CONTRACT,
                        )
                        .unwrap_or(0_u128.into());
                        let recipient_avt_balance =
                            Balances::free_balance(transfer_data.to.clone());

                        <frame_system::Pallet<TestRuntime>>::reset_events();
                        assert_ok!(AvnProxy::proxy(
                            RuntimeOrigin::signed(context.relayer.account_id()),
                            inner_call,
                            payment_authorisation
                        ));
                        assert_eq!(
                            true,
                            proxy_event_emitted(context.relayer.account_id(), call_hash),
                            "Proxy failed"
                        );
                        assert_eq!(
                            true,
                            native_token_transfered_events_emitted(),
                            "Tokens not transfered"
                        );
                        assert_eq!(
                            true,
                            non_native_token_transfered_events_emitted(),
                            "Proxy payment not complete"
                        );

                        // The signer transfers 1 AVT but is not affected by the proxy fee because
                        // `new payer` is paying the fees
                        assert_eq!(
                            signer_avt_balance - ONE_AVT,
                            Balances::free_balance(context.signer.account_id())
                        );

                        // The new payer's balance has decreased because they transfered 1 AVT
                        assert_eq!(
                            new_payer_avt_balance - ONE_AVT,
                            Balances::free_balance(new_payment_proof_signer.account_id())
                        );

                        // The new payer's non avt balance decreases because they paid the proxy fee
                        assert_eq!(
                            new_payer_non_avt_balance - ONE_AVT,
                            TokenManager::get_token_balance(
                                &new_payment_proof_signer.account_id(),
                                &NON_AVT_TOKEN_CONTRACT
                            )
                            .unwrap()
                        );

                        // The relayer's AVT balance doesn't change
                        assert_eq!(
                            relayer_avt_balance,
                            Balances::free_balance(context.relayer.account_id())
                        );

                        // The relayer receives 1 NON AVT TOKEN
                        assert_eq!(
                            relayer_non_avt_balance + ONE_AVT,
                            TokenManager::get_token_balance(
                                &context.relayer.account_id(),
                                &NON_AVT_TOKEN_CONTRACT
                            )
                            .unwrap()
                        );

                        // The receiver's balance goes up by 1 AVT
                        assert_eq!(
                            recipient_avt_balance + ONE_AVT,
                            Balances::free_balance(transfer_data.to.clone())
                        );
                    })
                }

                #[test]
                fn with_different_payer_and_signer() {
                    let mut ext = ExtBuilder::build_default().with_balances().as_externality();
                    ext.execute_with(|| {
                        let context: ProxyContext = Default::default();
                        let transfer_data: TokenTransferContext = Default::default();
                        let proxy_proof = get_signed_transfer_proxy_proof(&context, &transfer_data);
                        let inner_call =
                            get_signed_transfer_token_call(&transfer_data, &proxy_proof);

                        // Create a new payer
                        let new_payment_proof_signer = get_default_signer();

                        let payment_authorisation = Some(Box::new(create_payment_authorisation(
                            &context.relayer,
                            &new_payment_proof_signer,
                            proxy_proof,
                            0u64,
                            NON_AVT_TOKEN_CONTRACT,
                        )));

                        let call_hash = Hashing::hash_of(&inner_call);

                        let signer_avt_balance =
                            Balances::free_balance(context.signer.account_id());
                        let relayer_avt_balance =
                            Balances::free_balance(context.relayer.account_id());
                        let relayer_non_avt_balance = TokenManager::get_token_balance(
                            &context.relayer.account_id(),
                            &NON_AVT_TOKEN_CONTRACT,
                        )
                        .unwrap_or(0_u128.into());
                        let new_payer_avt_balance =
                            Balances::free_balance(new_payment_proof_signer.account_id());
                        let new_payer_non_avt_balance = TokenManager::get_token_balance(
                            &new_payment_proof_signer.account_id(),
                            &NON_AVT_TOKEN_CONTRACT,
                        )
                        .unwrap_or(0_u128.into());

                        assert_eq!(false, native_token_transfered_events_emitted());
                        assert_ok!(AvnProxy::proxy(
                            RuntimeOrigin::signed(context.relayer.account_id()),
                            inner_call,
                            payment_authorisation
                        ));
                        assert_eq!(
                            true,
                            proxy_event_emitted(context.relayer.account_id(), call_hash),
                            "Proxy failed"
                        );
                        assert_eq!(
                            true,
                            non_native_token_transfered_events_emitted(),
                            "Tokens not transfered"
                        );

                        // The signer transfers 1 AVT but is not affected by the proxy fee because
                        // `new payer` is paying the fees
                        assert_eq!(
                            signer_avt_balance - ONE_AVT,
                            Balances::free_balance(context.signer.account_id())
                        );

                        // The new payer doesn't pay with a native token
                        assert_eq!(
                            new_payer_avt_balance,
                            Balances::free_balance(new_payment_proof_signer.account_id())
                        );

                        // The new payer's non avt balance decreases
                        assert_eq!(
                            new_payer_non_avt_balance - ONE_AVT,
                            TokenManager::get_token_balance(
                                &new_payment_proof_signer.account_id(),
                                &NON_AVT_TOKEN_CONTRACT
                            )
                            .unwrap()
                        );

                        // The relayer's AVT balance doesn't change
                        assert_eq!(
                            relayer_avt_balance,
                            Balances::free_balance(context.relayer.account_id())
                        );

                        // The relayer receives 1 NON AVT TOKEN
                        assert_eq!(
                            relayer_non_avt_balance + ONE_AVT,
                            TokenManager::get_token_balance(
                                &context.relayer.account_id(),
                                &NON_AVT_TOKEN_CONTRACT
                            )
                            .unwrap()
                        );
                    })
                }
            }
        }
    }
}
