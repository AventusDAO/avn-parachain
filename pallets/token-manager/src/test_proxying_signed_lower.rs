// This file is part of Aventus.
// Copyright 2026 Aventus DAO Ltd

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![cfg(test)]
use crate::{
    self as token_manager,
    mock::{Balances, RuntimeCall as MockCall, RuntimeEvent, *},
    Balances as TokenManagerBalances, Call, *,
};
use codec::Encode;
use frame_support::{assert_err, assert_noop, assert_ok};
use hex_literal::hex;
use pallet_parachain_staking::Weight;
use pallet_transaction_payment::ChargeTransactionPayment;
use sp_avn_common::verify_signature;
use sp_core::{sr25519, ByteArray, Pair};
use sp_runtime::{
    traits::{transaction_extension::TxBaseImplication, Hash, TransactionExtension},
    transaction_validity::{InvalidTransaction, TransactionSource},
};

type AccountId = <TestRuntime as frame_system::Config>::AccountId;
type Hashing = <TestRuntime as frame_system::Config>::Hashing;

const DEFAULT_AMOUNT: u128 = 1_000_000;
const DEFAULT_NONCE: u64 = 0;
const NON_ZERO_NONCE: u64 = 100;

pub static TX_LEN: usize = 1;

pub fn default_key_pair() -> sr25519::Pair {
    return sr25519::Pair::from_seed(&[70u8; 32])
}

fn default_sender() -> AccountId {
    return AccountId::decode(&mut default_key_pair().public().to_vec().as_slice()).unwrap()
}

fn default_receiver_account_id() -> AccountId {
    let receiver = H256(hex!("0000000000000000000000000000000000000000000000000000000000000000"));
    return AccountId::decode(&mut receiver.as_bytes()).expect("Valid account id")
}

fn default_relayer() -> AccountId {
    return AccountId::from_raw([10; 32])
}

fn default_t1_recipient() -> H160 {
    return H160(hex!("2222222222222222222222222222222222222222"))
}

fn pay_gas_and_proxy_call(
    relayer: &AccountId,
    outer_call: &<TestRuntime as frame_system::Config>::RuntimeCall,
    inner_call: Box<<TestRuntime as Config>::RuntimeCall>,
) -> DispatchResult {
    let info = info_from_weight(Weight::from_parts(1 as u64, 0));
    let len = TX_LEN;
    let ext = ChargeTransactionPayment::<TestRuntime>::from(0);
    let implicit = ext.implicit().map_err(|e| <&'static str>::from(e))?;
    let implication = TxBaseImplication(outer_call.clone());
    let origin = RuntimeOrigin::signed(*relayer);
    let (_valid, val, origin) = ext
        .validate(origin, outer_call, &info, len, implicit, &implication, TransactionSource::Local)
        .map_err(|e| <&'static str>::from(e))?;
    let pre = ext
        .prepare(val, &origin, outer_call, &info, len)
        .map_err(|e| <&'static str>::from(e))?;

    let result = TokenManager::proxy(RuntimeOrigin::signed(*relayer), inner_call);
    ChargeTransactionPayment::<TestRuntime>::post_dispatch(
        pre,
        &info,
        &mut Default::default(),
        len,
        &result.map(|_| ()),
    )
    .map_err(|e| <&'static str>::from(e))?;
    return result
}

fn pay_gas_and_call_lower_directly(
    sender: &AccountId,
    token_id: <TestRuntime as Config>::TokenId,
    amount: <TestRuntime as Config>::TokenBalance,
    t1_recipient: H160,
    proof: Proof<Signature, AccountId>,
    call: &<TestRuntime as frame_system::Config>::RuntimeCall,
) -> DispatchResultWithPostInfo {
    let info = info_from_weight(Weight::from_parts(1 as u64, 0));
    let len = TX_LEN;
    let ext = ChargeTransactionPayment::<TestRuntime>::from(0);
    let implicit = ext.implicit().map_err(|e| <&'static str>::from(e))?;
    let implication = TxBaseImplication(call.clone());
    let origin = RuntimeOrigin::signed(*sender);
    let (_valid, val, origin) = ext
        .validate(origin, call, &info, len, implicit, &implication, TransactionSource::Local)
        .map_err(|e| <&'static str>::from(e))?;
    let pre = ext
        .prepare(val, &origin, call, &info, len)
        .map_err(|e| <&'static str>::from(e))?;

    let result = TokenManager::schedule_signed_lower(
        RuntimeOrigin::signed(*sender),
        proof,
        *sender,
        token_id,
        amount,
        t1_recipient,
    );
    ChargeTransactionPayment::<TestRuntime>::post_dispatch(
        pre,
        &info,
        &mut Default::default(),
        len,
        &result.as_ref().map(|_| ()).map_err(|e| e.error.clone()),
    )
    .map_err(|e| <&'static str>::from(e))?;
    return result
}

fn build_proof(
    signer: &AccountId,
    relayer: &AccountId,
    signature: Signature,
) -> Proof<Signature, AccountId> {
    return Proof { signer: *signer, relayer: *relayer, signature }
}

fn setup(sender: &AccountId, nonce: u64) {
    TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID, sender), 2 * DEFAULT_AMOUNT);
    Nonces::<TestRuntime>::insert(sender, nonce);
}

fn default_setup() {
    setup(&default_sender(), DEFAULT_NONCE);
}

fn create_proof_for_signed_lower_with_relayer(relayer: &AccountId) -> Proof<Signature, AccountId> {
    return create_proof_for_signed_lower(
        relayer,
        &default_sender(),
        NON_AVT_TOKEN_ID,
        DEFAULT_AMOUNT,
        DEFAULT_NONCE,
        &default_key_pair(),
        default_t1_recipient(),
    )
}

fn create_proof_for_signed_lower_with_nonce(nonce: u64) -> Proof<Signature, AccountId> {
    return create_proof_for_signed_lower(
        &default_relayer(),
        &default_sender(),
        NON_AVT_TOKEN_ID,
        DEFAULT_AMOUNT,
        nonce,
        &default_key_pair(),
        default_t1_recipient(),
    )
}

fn create_default_proof_for_signed_lower() -> Proof<Signature, AccountId> {
    return create_proof_for_signed_lower(
        &default_relayer(),
        &default_sender(),
        NON_AVT_TOKEN_ID,
        DEFAULT_AMOUNT,
        DEFAULT_NONCE,
        &default_key_pair(),
        default_t1_recipient(),
    )
}

fn create_proof_for_signed_lower(
    relayer: &AccountId,
    from: &AccountId,
    token_id: H160,
    amount: u128,
    nonce: u64,
    keys: &sr25519::Pair,
    t1_recipient: H160,
) -> Proof<Signature, AccountId> {
    let context = SIGNED_LOWER_CONTEXT;
    let data_to_sign = (context, relayer, from, token_id, amount, t1_recipient, nonce);
    let signature = sign(&keys, &data_to_sign.encode());

    return build_proof(from, relayer, signature)
}

fn check_proxy_lower_default_call_succeed(call: Box<<TestRuntime as Config>::RuntimeCall>) {
    let call_hash = Hashing::hash_of(&call);

    assert_ok!(TokenManager::proxy(RuntimeOrigin::signed(default_relayer()), call));
    assert_eq!(System::events().len(), 3);
    assert!(System::events().iter().any(|a| a.event ==
        RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::CallDispatched {
            relayer: default_relayer(),
            call_hash
        })));

    // move a few blocks to trigger the execution
    fast_forward_to_block(get_expected_execution_block());

    assert!(System::events().iter().any(|a| a.event ==
        RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
            token_id: NON_AVT_TOKEN_ID,
            sender: default_sender(),
            recipient: default_receiver_account_id(),
            amount: DEFAULT_AMOUNT,
            t1_recipient: default_t1_recipient(),
            lower_id: 0
        })));
}

struct Context;

impl Context {
    pub fn default() -> (sr25519::Pair, AccountId, AccountId, AccountId, H160) {
        (
            default_key_pair(),
            default_sender(),
            default_relayer(),
            default_receiver_account_id(),
            default_t1_recipient(),
        )
    }
}

mod proxy_signed_lower {
    use super::*;

    mod succeeds_implies_that {
        use super::*;

        #[test]
        fn lower_amount_is_deducted_from_sender_balance() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (_, sender, relayer, _, t1_recipient) = Context::default();

                setup(&sender, NON_ZERO_NONCE);
                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    2 * DEFAULT_AMOUNT
                );

                let proof = create_proof_for_signed_lower_with_nonce(NON_ZERO_NONCE);

                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));

                assert_ok!(TokenManager::proxy(RuntimeOrigin::signed(relayer), call.clone()));

                // move a few blocks to trigger the execution
                fast_forward_to_block(get_expected_execution_block());

                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    DEFAULT_AMOUNT
                );
            });
        }

        #[test]
        fn events_are_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (_, sender, relayer, recipient_account_id, t1_recipient) = Context::default();

                setup(&sender, NON_ZERO_NONCE);
                let proof = create_proof_for_signed_lower_with_nonce(NON_ZERO_NONCE);

                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));

                assert_eq!(System::events().len(), 0);
                assert_ok!(TokenManager::proxy(RuntimeOrigin::signed(relayer), call.clone()));

                let call_hash = Hashing::hash_of(&call);
                assert!(System::events().iter().any(|a| a.event ==
                    RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::CallDispatched {
                        relayer,
                        call_hash
                    })));

                // move a few blocks to trigger the execution
                fast_forward_to_block(get_expected_execution_block());

                // In order to validate that the proxied call has been dispatched we need any proof
                // that lower was called. In this case we will check that the
                // Lowered signal was emitted.
                assert!(System::events().iter().any(|a| a.event ==
                    RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                        token_id: NON_AVT_TOKEN_ID,
                        sender,
                        recipient: recipient_account_id,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                        lower_id: 0
                    })));
            });
        }
    }

    #[test]
    fn succeeds_with_nonce_zero() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let (_, sender, relayer, recipient_account_id, t1_recipient) = Context::default();

            default_setup();
            let proof = create_default_proof_for_signed_lower();

            let call = Box::new(MockCall::TokenManager(
                super::Call::<TestRuntime>::schedule_signed_lower {
                    proof,
                    from: sender,
                    token_id: NON_AVT_TOKEN_ID,
                    amount: DEFAULT_AMOUNT,
                    t1_recipient,
                },
            ));
            let call_hash = Hashing::hash_of(&call);

            assert_eq!(System::events().len(), 0);
            assert_ok!(TokenManager::proxy(RuntimeOrigin::signed(relayer), call));

            // move a few blocks to trigger the execution
            fast_forward_to_block(get_expected_execution_block());

            assert_eq!(
                TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                DEFAULT_AMOUNT
            );

            assert!(System::events().iter().any(|a| a.event ==
                RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::CallDispatched {
                    relayer,
                    call_hash
                })));
            assert!(System::events().iter().any(|a| a.event ==
                RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                    token_id: NON_AVT_TOKEN_ID,
                    sender,
                    recipient: recipient_account_id,
                    amount: DEFAULT_AMOUNT,
                    t1_recipient,
                    lower_id: 0
                })));
        });
    }

    mod fails_with {
        use super::*;

        #[test]
        fn mismatching_proof_nonce() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (_, sender, relayer, _, t1_recipient) = Context::default();
                let bad_nonces = [0, 99, 101];
                setup(&sender, NON_ZERO_NONCE);

                for bad_nonce in bad_nonces.iter() {
                    let proof = create_proof_for_signed_lower_with_nonce(*bad_nonce);
                    let call = Box::new(MockCall::TokenManager(
                        super::Call::<TestRuntime>::schedule_signed_lower {
                            proof,
                            from: sender,
                            token_id: NON_AVT_TOKEN_ID,
                            amount: DEFAULT_AMOUNT,
                            t1_recipient,
                        },
                    ));

                    assert_err!(
                        TokenManager::proxy(RuntimeOrigin::signed(relayer), call),
                        Error::<TestRuntime>::UnauthorizedSignedLowerTransaction
                    );

                    assert_eq!(System::events().len(), 0);
                }
            });
        }

        #[test]
        fn mismatched_proof_other_amount() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (_, sender, relayer, _, t1_recipient) = Context::default();

                let mismatching_amount: u128 = 100;

                default_setup();
                let proof = create_default_proof_for_signed_lower();

                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof: proof.clone(),
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: mismatching_amount,
                        t1_recipient,
                    },
                ));

                assert_eq!(System::events().len(), 0);
                assert_err!(
                    TokenManager::proxy(RuntimeOrigin::signed(relayer), call),
                    Error::<TestRuntime>::UnauthorizedSignedLowerTransaction
                );

                // Show that it works with the correct input
                let proof = create_default_proof_for_signed_lower();
                check_proxy_lower_default_call_succeed(Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                )));
            });
        }

        #[test]
        fn mismatched_proof_other_keys() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (_, sender, relayer, _, t1_recipient) = Context::default();

                let other_sender_keys = sr25519::Pair::from_seed_slice(&[2u8; 32]).unwrap();

                default_setup();
                let mismatching_proof = create_proof_for_signed_lower(
                    &relayer,
                    &sender,
                    NON_AVT_TOKEN_ID,
                    DEFAULT_AMOUNT,
                    DEFAULT_NONCE,
                    &other_sender_keys,
                    t1_recipient,
                );

                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof: mismatching_proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));

                assert_err!(
                    TokenManager::proxy(RuntimeOrigin::signed(relayer), call),
                    Error::<TestRuntime>::UnauthorizedSignedLowerTransaction
                );
                assert_eq!(System::events().len(), 0);

                // Show that it works with the correct input
                let proof = create_default_proof_for_signed_lower();
                check_proxy_lower_default_call_succeed(Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                )));
            });
        }

        #[test]
        fn mismatched_proof_other_sender() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (sender_keys, sender, relayer, _, t1_recipient) = Context::default();

                let other_sender_account_id = AccountId::from_raw([55; 32]);

                default_setup();
                let mismatching_proof = create_proof_for_signed_lower(
                    &relayer,
                    &other_sender_account_id,
                    NON_AVT_TOKEN_ID,
                    DEFAULT_AMOUNT,
                    DEFAULT_NONCE,
                    &sender_keys,
                    t1_recipient,
                );

                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof: mismatching_proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));

                assert_err!(
                    TokenManager::proxy(RuntimeOrigin::signed(relayer), call),
                    Error::<TestRuntime>::SenderNotValid
                );
                assert_eq!(System::events().len(), 0);

                // Show that it works with the correct input
                let proof = create_default_proof_for_signed_lower();
                check_proxy_lower_default_call_succeed(Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                )));
            });
        }

        #[test]
        fn mismatched_proof_other_relayer() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (_, sender, relayer, recipient_account_id, t1_recipient) = Context::default();

                let other_relayer_account_id = recipient_account_id.clone();

                default_setup();
                let mismatching_proof =
                    create_proof_for_signed_lower_with_relayer(&other_relayer_account_id);
                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof: mismatching_proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));

                assert_err!(
                    TokenManager::proxy(RuntimeOrigin::signed(relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedProxyTransaction
                );
                assert_eq!(System::events().len(), 0);

                // Show that it works with the correct input
                let proof = create_default_proof_for_signed_lower();
                check_proxy_lower_default_call_succeed(Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                )));
            });
        }

        #[test]
        fn mismatched_proof_other_token_id() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (sender_keys, sender, relayer, _, t1_recipient) = Context::default();
                let other_token_id = NON_AVT_TOKEN_ID_2;

                default_setup();
                let mismatching_proof = create_proof_for_signed_lower(
                    &relayer,
                    &sender,
                    other_token_id,
                    DEFAULT_AMOUNT,
                    DEFAULT_NONCE,
                    &sender_keys,
                    t1_recipient,
                );

                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof: mismatching_proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));

                assert_err!(
                    TokenManager::proxy(RuntimeOrigin::signed(relayer), call),
                    Error::<TestRuntime>::UnauthorizedSignedLowerTransaction
                );
                assert_eq!(System::events().len(), 0);

                // Show that it works with the correct input
                let proof = create_default_proof_for_signed_lower();
                check_proxy_lower_default_call_succeed(Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                )));
            });
        }

        #[test]
        fn mismatched_proof_other_context() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (sender_keys, sender, relayer, _, t1_recipient) = Context::default();

                let other_context: &'static [u8] = b"authorizati0n for tr4nsfer op3ration";

                default_setup();
                let data_to_sign = (
                    other_context,
                    relayer,
                    sender,
                    NON_AVT_TOKEN_ID,
                    DEFAULT_AMOUNT,
                    DEFAULT_NONCE,
                    t1_recipient,
                );
                let alternative_signature = sign(&sender_keys, &data_to_sign.encode());

                let mismatching_proof =
                    Proof { signer: sender, relayer, signature: alternative_signature };

                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof: mismatching_proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));
                assert_err!(
                    TokenManager::proxy(RuntimeOrigin::signed(relayer), call),
                    Error::<TestRuntime>::UnauthorizedSignedLowerTransaction
                );

                assert_eq!(System::events().len(), 0);

                // Show that it works with the correct input
                let proof = create_default_proof_for_signed_lower();
                check_proxy_lower_default_call_succeed(Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                )));
            });
        }
    }
}

mod signed_lower {
    use super::*;

    mod succeeds_implies_that {
        use super::*;

        #[test]
        fn lower_amount_is_deducted_from_sender_balance() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (_, sender, _, _, t1_recipient) = Context::default();
                setup(&sender, NON_ZERO_NONCE);
                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    2 * DEFAULT_AMOUNT
                );

                let proof = create_proof_for_signed_lower_with_nonce(NON_ZERO_NONCE);

                assert_ok!(TokenManager::schedule_signed_lower(
                    RuntimeOrigin::signed(sender),
                    proof,
                    sender,
                    NON_AVT_TOKEN_ID,
                    DEFAULT_AMOUNT,
                    t1_recipient
                ));

                // move a few blocks to trigger the execution
                fast_forward_to_block(get_expected_execution_block());

                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    DEFAULT_AMOUNT
                );
            });
        }

        #[test]
        fn event_is_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let (_, sender, _, recipient_account_id, t1_recipient) = Context::default();
                setup(&sender, NON_ZERO_NONCE);
                let proof = create_proof_for_signed_lower_with_nonce(NON_ZERO_NONCE);

                assert_eq!(System::events().len(), 0);

                assert_ok!(TokenManager::schedule_signed_lower(
                    RuntimeOrigin::signed(sender),
                    proof,
                    sender,
                    NON_AVT_TOKEN_ID,
                    DEFAULT_AMOUNT,
                    t1_recipient
                ));

                // move a few blocks to trigger the execution
                fast_forward_to_block(get_expected_execution_block());

                assert!(System::events().iter().any(|a| a.event ==
                    RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                        token_id: NON_AVT_TOKEN_ID,
                        sender,
                        recipient: recipient_account_id,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                        lower_id: 0
                    })));
            });
        }
    }
}

mod get_proof {
    use super::*;

    #[test]
    fn succeeds_for_valid_signed_lower_call() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let sender = default_sender();
            let t1_recipient = default_t1_recipient();

            let proof = create_default_proof_for_signed_lower();
            let call = Box::new(MockCall::TokenManager(
                super::Call::<TestRuntime>::schedule_signed_lower {
                    proof: proof.clone(),
                    from: sender,
                    token_id: NON_AVT_TOKEN_ID,
                    amount: DEFAULT_AMOUNT,
                    t1_recipient,
                },
            ));

            let result = TokenManager::get_proof(&call);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), proof);
        });
    }

    #[test]
    fn fails_for_invalid_calls() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let invalid_call = MockCall::System(frame_system::Call::remark { remark: vec![] });

            assert!(matches!(
                TokenManager::get_proof(&invalid_call),
                Err(Error::<TestRuntime>::TransactionNotSupported)
            ));
        });
    }
}

mod fees {
    use super::*;

    mod are_paid_correctly {
        use super::*;

        #[test]
        // Ensure that the AVT gas fees are paid by the relayer
        fn when_relayer_with_enough_avt_proxy_a_signed_lower_call() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();

            ext.execute_with(|| {
                let (_, sender, _, recipient_account_id, t1_recipient) = Context::default();
                let relayer = account_id_with_100_avt();

                default_setup();
                let proof = create_proof_for_signed_lower_with_relayer(&relayer);

                assert_eq!(Balances::free_balance(relayer), AMOUNT_100_TOKEN);
                assert_eq!(Balances::free_balance(sender), 0);
                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    2 * DEFAULT_AMOUNT
                );

                // Prepare the calls
                let inner_call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof: proof.clone(),
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));
                let outer_call = &MockCall::TokenManager(token_manager::Call::proxy {
                    call: inner_call.clone(),
                });

                // Pay fees and submit the transaction
                assert_ok!(pay_gas_and_proxy_call(&relayer, outer_call, inner_call.clone()));

                // Check the effects of the transaction
                let call_hash = Hashing::hash_of(&inner_call);
                assert!(System::events().iter().any(|a| a.event ==
                    RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::CallDispatched {
                        relayer,
                        call_hash
                    })));

                // move a few blocks to trigger the execution
                fast_forward_to_block(get_expected_execution_block());

                assert!(System::events().iter().any(|a| a.event ==
                    RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                        token_id: NON_AVT_TOKEN_ID,
                        sender,
                        recipient: recipient_account_id,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                        lower_id: 0
                    })));

                let fee: u128 = (BASE_FEE + TX_LEN as u64) as u128;
                assert_eq!(Balances::free_balance(relayer), AMOUNT_100_TOKEN - fee);
                assert_eq!(Balances::free_balance(sender), 0);
                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    DEFAULT_AMOUNT
                );
            });
        }

        #[test]
        // Ensure that regular call's gas fees are paid by the sender
        fn when_sender_with_enough_avt_submit_a_signed_lower_call() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();

            ext.execute_with(|| {
                let (_, _, _, recipient_account_id, t1_recipient) = Context::default();
                let sender_keys = key_pair_for_account_with_100_avt();
                let sender = get_account_id(&sender_keys);

                setup(&sender, DEFAULT_NONCE);
                let proof = create_proof_for_signed_lower(
                    &sender,
                    &sender,
                    NON_AVT_TOKEN_ID,
                    DEFAULT_AMOUNT,
                    DEFAULT_NONCE,
                    &sender_keys,
                    t1_recipient,
                );

                assert_eq!(Balances::free_balance(sender), AMOUNT_100_TOKEN);
                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    2 * DEFAULT_AMOUNT
                );
                assert_eq!(System::events().len(), 0);

                let call = &MockCall::TokenManager(token_manager::Call::schedule_signed_lower {
                    proof: proof.clone(),
                    from: sender,
                    token_id: NON_AVT_TOKEN_ID,
                    amount: DEFAULT_AMOUNT,
                    t1_recipient,
                });
                assert_ok!(pay_gas_and_call_lower_directly(
                    &sender,
                    NON_AVT_TOKEN_ID,
                    DEFAULT_AMOUNT,
                    t1_recipient,
                    proof,
                    call
                ));

                assert!(System::events().iter().any(|a| {
                    matches!(
                        &a.event,
                        RuntimeEvent::TokenManager(
                            crate::Event::<TestRuntime>::LowerRequested {
                                from,
                                amount: DEFAULT_AMOUNT,
                                t1_recipient: t1_recipient,
                                ..
                            }
                        ) if *from == sender
                    )
                }));

                System::reset_events();

                // move a few blocks to trigger the execution
                fast_forward_to_block(get_expected_execution_block());

                let fee: u128 = (BASE_FEE + TX_LEN as u64) as u128;

                assert_eq!(Balances::free_balance(sender), AMOUNT_100_TOKEN - fee);
                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    DEFAULT_AMOUNT
                );
                assert!(System::events().iter().any(|a| a.event ==
                    RuntimeEvent::TokenManager(crate::Event::<TestRuntime>::TokenLowered {
                        token_id: NON_AVT_TOKEN_ID,
                        sender,
                        recipient: recipient_account_id,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                        lower_id: 0
                    })));
            });
        }
    }

    mod payment_fails {
        use super::*;

        #[test]
        // Relayer has insufficient funds to send transaction
        fn when_relayer_with_insufficient_avt_proxy_a_signed_lower_call() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();

            ext.execute_with(|| {
                let (_, sender, relayer, _, t1_recipient) = Context::default();

                default_setup();
                let proof = create_default_proof_for_signed_lower();

                assert_eq!(Balances::free_balance(relayer), 0);
                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    2 * DEFAULT_AMOUNT
                );

                // Prepare the calls
                let inner_call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof: proof.clone(),
                        from: sender,
                        token_id: NON_AVT_TOKEN_ID,
                        amount: DEFAULT_AMOUNT,
                        t1_recipient,
                    },
                ));
                let outer_call = &MockCall::TokenManager(token_manager::Call::proxy {
                    call: inner_call.clone(),
                });

                // Pay fees and submit the transaction.
                // Gas fee for this tx is (BASE_FEE + TX_LEN): 10 + 1 = 11 AVT
                assert_noop!(
                    pay_gas_and_proxy_call(&relayer, outer_call, inner_call),
                    <&str>::from(InvalidTransaction::Payment)
                );
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        // Ensure that regular call's gas fees are paid by the sender
        fn when_sender_with_insufficient_avt_submit_a_signed_lower_call() {
            let mut ext = ExtBuilder::build_default().with_balances().as_externality();

            ext.execute_with(|| {
                let (_, sender, _, _, t1_recipient) = Context::default();

                default_setup();
                let proof = create_default_proof_for_signed_lower();

                assert_eq!(Balances::free_balance(sender), 0);
                assert_eq!(
                    TokenManagerBalances::<TestRuntime>::get((NON_AVT_TOKEN_ID, sender)),
                    2 * DEFAULT_AMOUNT
                );

                let call = &MockCall::TokenManager(token_manager::Call::schedule_signed_lower {
                    proof: proof.clone(),
                    from: sender,
                    token_id: NON_AVT_TOKEN_ID,
                    amount: DEFAULT_AMOUNT,
                    t1_recipient,
                });
                assert_noop!(
                    pay_gas_and_call_lower_directly(
                        &sender,
                        NON_AVT_TOKEN_ID,
                        DEFAULT_AMOUNT,
                        t1_recipient,
                        proof,
                        call
                    ),
                    <&str>::from(InvalidTransaction::Payment)
                );
                assert_eq!(System::events().len(), 0);
            });
        }
    }
}

mod wrapped_signature_verification {
    use super::*;

    mod succeeds {
        use super::*;

        #[test]
        fn when_data_is_wrapped() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let token: H160 = H160(hex!("97d9b397189e8b771ffac3cb04cf26c780a93431"));
                let amount: u128 = 1_000_000_000_000_000_000;
                let t1_recipient: H160 = H160(hex!("DE7E1091cDE63c05Aa4D82C62e4C54eDbC701B22"));
                let nonce = 0;

                // Mnemonic for sender: "noise latin security unlock apart network sheriff visa
                // detail robust chapter disorder"
                let signer_pk: H256 =
                    H256(hex!("026f91b25aef5965a7390c72e94834bb976c1c8f211f97d465b77068166bee63"));
                let sender = <TestRuntime as frame_system::Config>::AccountId::decode(
                    &mut signer_pk.as_bytes(),
                )
                .unwrap();

                let relayer_pk: H256 =
                    H256(hex!("9c2bfffc466eb9c1bad0d8393df93770468ee54b0a0f05232e4b5dde6960b004"));
                let relayer = <TestRuntime as frame_system::Config>::AccountId::decode(
                    &mut relayer_pk.as_bytes(),
                )
                .unwrap();

                // Make user has some amount to lower
                TokenManagerBalances::<TestRuntime>::insert((token, sender), 2 * amount);
                Nonces::<TestRuntime>::insert(sender, nonce);

                assert_eq!(TokenManagerBalances::<TestRuntime>::get((token, sender)), 2 * amount);

                let context = SIGNED_LOWER_CONTEXT;
                let data_to_sign = (context, relayer, sender, token, amount, t1_recipient, nonce);
                // This is a signature generated for the input specified above, by polkadotJS
                // wallet, that adds an envelope around the message before signing data
                let signature = Signature::from_slice(&hex!("28030730e54df409e7ed3a45ef5f775383f1e1d1563b86b0fc7784d3b6e55802d03c3f46d29f699a3500c5dcab80ea9e3b4f81ccbb3d35e7f31cba5a9a1e2682")).unwrap(); //sign(&keys, &data_to_sign.encode());
                let proof = build_proof(&sender, &relayer, signature);

                assert_ok!(verify_signature::<Signature, AccountId>(
                    &proof,
                    &data_to_sign.encode()
                ));

                let call = Box::new(MockCall::TokenManager(
                    super::Call::<TestRuntime>::schedule_signed_lower {
                        proof,
                        from: sender,
                        token_id: token,
                        amount,
                        t1_recipient,
                    },
                ));

                assert_ok!(TokenManager::proxy(RuntimeOrigin::signed(relayer), call.clone()));

                // move a few blocks to trigger the execution
                fast_forward_to_block(get_expected_execution_block());

                assert_eq!(TokenManagerBalances::<TestRuntime>::get((token, sender)), amount);
            });
        }
    }
}
