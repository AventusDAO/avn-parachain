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
use super::*;
use crate::{
    mock::{AccountId, RuntimeCall as MockCall, RuntimeEvent as Event, RuntimeOrigin as Origin, *},
    Call,
};
use codec::Encode;
use frame_support::{assert_err, assert_noop, assert_ok, error::BadOrigin};
use frame_system::RawOrigin;
use hex_literal::hex;
use sp_core::sr25519::Pair;

const DEFAULT_NONCE: u64 = 0;

fn build_proof(
    signer: &AccountId,
    relayer: &AccountId,
    signature: Signature,
) -> Proof<Signature, AccountId> {
    return Proof { signer: *signer, relayer: *relayer, signature };
}

struct Context {
    nft_owner_account: AccountId,
    nft_owner_key_pair: Pair,
    relayer: AccountId,
    nft_id: NftId,
    market: NftSaleType,
    nft_nonce: u64,
}

impl Default for Context {
    fn default() -> Self {
        let t1_authority = H160(hex!("11111AAAAA22222BBBBB11111AAAAA22222BBBBB"));
        let unique_id = U256::from(1u8);
        let nft_id = NftManager::generate_nft_id_single_mint(&t1_authority, unique_id);
        let nft_owner = TestAccount::new([1u8; 32]);
        let relayer = TestAccount::new([2u8; 32]);
        Context {
            nft_owner_account: nft_owner.account_id(),
            nft_owner_key_pair: nft_owner.key_pair(),
            relayer: relayer.account_id(),
            nft_id,
            market: NftSaleType::Ethereum,
            nft_nonce: DEFAULT_NONCE,
        }
    }
}

impl Context {
    fn setup(&self) {
        let nft = Nft::new(
            self.nft_id,
            NftManager::get_info_id_and_advance(),
            BoundedVec::try_from(String::from("Offchain location of NFT").into_bytes())
                .expect("Test string should not exceed bound"),
            self.nft_owner_account,
        );
        Nfts::<TestRuntime>::insert(self.nft_id, &nft);
        NftOpenForSale::<TestRuntime>::remove(&self.nft_id);
    }

    fn create_signed_list_nft_open_for_sale_call(
        &self,
    ) -> Box<<TestRuntime as Config>::RuntimeCall> {
        let proof = self.create_signed_list_nft_open_for_sale_proof();

        return Box::new(MockCall::NftManager(
            super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                proof,
                nft_id: self.nft_id,
                market: self.market,
            },
        ));
    }

    fn create_signed_list_nft_open_for_sale_proof(&self) -> Proof<Signature, AccountId> {
        return create_proof_for_signed_list_nft_open_for_sale(
            &self.relayer,
            &self.nft_owner_account,
            &self.nft_owner_key_pair,
            self.nft_id,
            self.market,
            self.nft_nonce,
        );
    }

    fn open_for_sale_event_emitted(&self) -> bool {
        return System::events().iter().any(|a| {
            a.event
                == Event::NftManager(crate::Event::<TestRuntime>::NftOpenForSale {
                    nft_id: self.nft_id,
                    sale_type: self.market,
                })
        });
    }

    fn call_dispatched_event_emitted(
        &self,
        call: &Box<<TestRuntime as Config>::RuntimeCall>,
    ) -> bool {
        let relayer = TestAccount::new([2u8; 32]);
        return System::events().iter().any(|a| {
            a.event
                == Event::NftManager(crate::Event::<TestRuntime>::CallDispatched {
                    relayer: relayer.account_id(),
                    hash: Hashing::hash_of(call),
                })
        });
    }
}

mod proxy_signed_list_nft_open_for_sale {
    use super::*;

    mod succeeds_implies_that {
        use super::*;

        #[test]
        fn nft_is_registered_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_list_nft_open_for_sale_call();

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call));

                assert_eq!(true, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
            });
        }

        #[test]
        fn nft_nonce_is_increased_by_one() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_list_nft_open_for_sale_call();

                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call.clone()));

                assert_eq!(original_nonce + 1u64, NftManager::nfts(context.nft_id).unwrap().nonce);
            });
        }

        #[test]
        fn open_for_sale_event_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_list_nft_open_for_sale_call();

                assert_eq!(false, context.open_for_sale_event_emitted());

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call.clone()));

                assert_eq!(true, context.open_for_sale_event_emitted());
            });
        }

        #[test]
        fn call_dispatched_event_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_list_nft_open_for_sale_call();

                assert_eq!(false, context.call_dispatched_event_emitted(&call));

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call.clone()));

                assert_eq!(true, context.call_dispatched_event_emitted(&call));
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_list_nft_open_for_sale_call();

                assert_noop!(NftManager::proxy(RawOrigin::None.into(), call.clone()), BadOrigin);
            });
        }

        #[test]
        fn nft_already_listed_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                <NftOpenForSale<TestRuntime>>::insert(context.nft_id, NftSaleType::Ethereum);
                let call = context.create_signed_list_nft_open_for_sale_call();

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::NftAlreadyListed
                );
            });
        }

        #[test]
        fn sender_is_not_the_owner() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let not_nft_owner = TestAccount::new([4u8; 32]);
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &not_nft_owner.account_id(),
                    &not_nft_owner.key_pair(),
                    context.nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                        proof,
                        nft_id: context.nft_id,
                        market: context.market,
                    },
                ));

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::SenderIsNotOwner
                );
            });
        }

        #[test]
        fn nft_does_not_exists() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                Nfts::<TestRuntime>::remove(&context.nft_id);
                let call = context.create_signed_list_nft_open_for_sale_call();

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::NftIdDoesNotExist
                );
            });
        }

        #[test]
        fn nft_is_locked() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                <Nfts<TestRuntime>>::mutate(context.nft_id, |maybe_nft| {
                    maybe_nft.as_mut().map(|nft| nft.is_locked = true)
                });
                let call = context.create_signed_list_nft_open_for_sale_call();

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::NftIsLocked
                );
            });
        }

        #[test]
        fn call_is_invalid() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let invalid_call =
                    Box::new(MockCall::System(frame_system::Call::remark { remark: vec![] }));

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), invalid_call.clone()),
                    Error::<TestRuntime>::TransactionNotSupported
                );
            });
        }

        #[test]
        fn mismatched_proof_nft_nonce() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let bad_nonces: [u64; 3] = [2, 99, 101];

                for bad_nonce in bad_nonces.iter() {
                    let proof = create_proof_for_signed_list_nft_open_for_sale(
                        &context.relayer,
                        &context.nft_owner_account,
                        &context.nft_owner_key_pair,
                        context.nft_id,
                        context.market,
                        *bad_nonce,
                    );
                    let call = Box::new(MockCall::NftManager(
                        super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                            proof: proof.clone(),
                            nft_id: context.nft_id,
                            market: context.market,
                        },
                    ));
                    let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                    assert_err!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                    );

                    assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                    assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                    assert_eq!(System::events().len(), 0);
                }
            });
        }

        #[test]
        fn mismatched_proof_other_signer() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_signer = TestAccount::new([5u8; 32]);
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &other_signer.account_id(),
                    &context.nft_owner_key_pair,
                    context.nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                        proof: proof.clone(),
                        nft_id: context.nft_id,
                        market: context.market,
                    },
                ));
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::SenderIsNotOwner
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        fn mismatched_proof_other_nft_owner_key_pair() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();

                let other_nft_owner_account = TestAccount::new([5u8; 32]);
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &context.nft_owner_account,
                    &other_nft_owner_account.key_pair(),
                    context.nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                        proof: proof.clone(),
                        nft_id: context.nft_id,
                        market: context.market,
                    },
                ));
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        fn mismatched_proof_other_nft_id() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_nft_id = U256::one();
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &context.nft_owner_account,
                    &context.nft_owner_key_pair,
                    other_nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                        proof: proof.clone(),
                        nft_id: context.nft_id,
                        market: context.market,
                    },
                ));
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        fn mismatched_proof_other_market() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_market = NftSaleType::Unknown;
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &context.nft_owner_account,
                    &context.nft_owner_key_pair,
                    context.nft_id,
                    other_market,
                    context.nft_nonce,
                );
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                        proof: proof.clone(),
                        nft_id: context.nft_id,
                        market: context.market,
                    },
                ));
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        fn mismatched_proof_context() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_context: &'static [u8] = b"authorization for something else";
                let data_to_sign = (
                    other_context,
                    context.relayer,
                    context.nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                        proof: proof.clone(),
                        nft_id: context.nft_id,
                        market: context.market,
                    },
                ));
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }
    }
}

mod signed_lift_nft_open_for_sale {
    use super::*;

    mod succeeds_implies_that {
        use super::*;

        #[test]
        fn nft_is_registered_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_list_nft_open_for_sale_proof();

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));

                assert_ok!(NftManager::signed_list_nft_open_for_sale(
                    Origin::signed(context.nft_owner_account),
                    proof,
                    context.nft_id,
                    context.market
                ));

                assert_eq!(true, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
            });
        }

        #[test]
        fn nft_nonce_is_increased_by_one() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_list_nft_open_for_sale_proof();

                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_ok!(NftManager::signed_list_nft_open_for_sale(
                    Origin::signed(context.nft_owner_account),
                    proof,
                    context.nft_id,
                    context.market
                ));

                assert_eq!(original_nonce + 1u64, NftManager::nfts(context.nft_id).unwrap().nonce);
            });
        }

        #[test]
        fn open_for_sale_event_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_list_nft_open_for_sale_proof();

                assert_eq!(false, context.open_for_sale_event_emitted());

                assert_ok!(NftManager::signed_list_nft_open_for_sale(
                    Origin::signed(context.nft_owner_account),
                    proof,
                    context.nft_id,
                    context.market
                ));

                assert_eq!(true, context.open_for_sale_event_emitted());
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_list_nft_open_for_sale_proof();

                assert_noop!(
                    NftManager::signed_list_nft_open_for_sale(
                        RawOrigin::None.into(),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    BadOrigin
                );
            });
        }

        #[test]
        fn nft_already_listed_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                <NftOpenForSale<TestRuntime>>::insert(context.nft_id, NftSaleType::Ethereum);
                let proof = context.create_signed_list_nft_open_for_sale_proof();

                assert_noop!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::NftAlreadyListed
                );
            });
        }

        #[test]
        fn sender_is_not_the_owner() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let non_nft_owner = TestAccount::new([4u8; 32]);
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &non_nft_owner.account_id(),
                    &non_nft_owner.key_pair(),
                    context.nft_id,
                    context.market.clone(),
                    context.nft_nonce,
                );

                assert_noop!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(non_nft_owner.account_id()),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::SenderIsNotOwner
                );
            });
        }

        #[test]
        fn nft_does_not_exists() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                Nfts::<TestRuntime>::remove(&context.nft_id);
                let proof = context.create_signed_list_nft_open_for_sale_proof();

                assert_noop!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::NftIdDoesNotExist
                );
            });
        }

        #[test]
        fn nft_is_locked() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                <Nfts<TestRuntime>>::mutate(context.nft_id, |maybe_nft| {
                    maybe_nft.as_mut().map(|nft| nft.is_locked = true)
                });
                let proof = context.create_signed_list_nft_open_for_sale_proof();

                assert_noop!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::NftIsLocked
                );
            });
        }

        #[test]
        fn mismatched_proof_nft_nonce() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let bad_nonces: [u64; 3] = [2, 99, 101];

                for bad_nonce in bad_nonces.iter() {
                    let proof = create_proof_for_signed_list_nft_open_for_sale(
                        &context.relayer,
                        &context.nft_owner_account,
                        &context.nft_owner_key_pair,
                        context.nft_id,
                        context.market,
                        *bad_nonce,
                    );

                    let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                    assert_err!(
                        NftManager::signed_list_nft_open_for_sale(
                            Origin::signed(context.nft_owner_account),
                            proof,
                            context.nft_id,
                            context.market
                        ),
                        Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                    );

                    assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                    assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                    assert_eq!(System::events().len(), 0);
                }
            });
        }

        #[test]
        fn mismatched_proof_other_signer() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_signer = TestAccount::new([5u8; 32]);
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &other_signer.account_id(),
                    &context.nft_owner_key_pair,
                    context.nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::SenderIsNotSigner
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        fn mismatched_proof_other_nft_owner_key_pair() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_nft_owner_account = TestAccount::new([5u8; 32]);
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &context.nft_owner_account,
                    &other_nft_owner_account.key_pair(),
                    context.nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        fn mismatched_proof_other_nft_id() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_nft_id = U256::one();
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &context.nft_owner_account,
                    &context.nft_owner_key_pair,
                    other_nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        fn mismatched_proof_other_market() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_market = NftSaleType::Unknown;
                let proof = create_proof_for_signed_list_nft_open_for_sale(
                    &context.relayer,
                    &context.nft_owner_account,
                    &context.nft_owner_key_pair,
                    context.nft_id,
                    other_market,
                    context.nft_nonce,
                );
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }

        #[test]
        fn mismatched_proof_context() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_context: &'static [u8] = b"authorization for something else";
                let data_to_sign = (
                    other_context,
                    context.relayer,
                    context.nft_id,
                    context.market,
                    context.nft_nonce,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);
                let original_nonce = NftManager::nfts(context.nft_id).unwrap().nonce;

                assert_err!(
                    NftManager::signed_list_nft_open_for_sale(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.nft_id,
                        context.market
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedLiftNftOpenForSaleTransaction
                );

                assert_eq!(false, NftOpenForSale::<TestRuntime>::contains_key(&context.nft_id));
                assert_eq!(original_nonce, NftManager::nfts(context.nft_id).unwrap().nonce);
                assert_eq!(System::events().len(), 0);
            });
        }
    }
}

mod get_proof {
    use super::*;

    #[test]
    fn succeeds_for_valid_signed_list_nft_open_for_sale_call() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            context.setup();
            let proof = context.create_signed_list_nft_open_for_sale_proof();
            let call = Box::new(MockCall::NftManager(
                super::Call::<TestRuntime>::signed_list_nft_open_for_sale {
                    proof: proof.clone(),
                    nft_id: context.nft_id,
                    market: context.market,
                },
            ));

            let result = NftManager::get_proof(&call);

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
                NftManager::get_proof(&invalid_call),
                Err(Error::<TestRuntime>::TransactionNotSupported)
            ));
        });
    }
}

fn create_proof_for_signed_list_nft_open_for_sale(
    relayer: &AccountId,
    nft_owner_account: &AccountId,
    nft_owner_key_pair: &Pair,
    nft_id: NftId,
    market: NftSaleType,
    nft_nonce: u64,
) -> Proof<Signature, AccountId> {
    let context = SIGNED_LIST_NFT_OPEN_FOR_SALE_CONTEXT;
    let data_to_sign = (context, relayer, nft_id, market, nft_nonce);
    let signature = sign(nft_owner_key_pair, &data_to_sign.encode());

    return build_proof(nft_owner_account, relayer, signature);
}
