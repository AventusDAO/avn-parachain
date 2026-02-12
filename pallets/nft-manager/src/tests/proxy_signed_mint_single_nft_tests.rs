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
    info_id: NftInfoId,
    unique_external_ref: Vec<u8>,
    royalties: Vec<Royalty>,
    t1_authority: H160,
}

impl Default for Context {
    fn default() -> Self {
        let t1_authority = H160(hex!("0000000000000000000000000000000000000001"));
        let nft_id = U256::from_big_endian(&[
            144, 32, 76, 127, 69, 26, 191, 42, 121, 72, 235, 94, 179, 147, 69, 29, 167, 189, 8, 44,
            104, 83, 241, 253, 146, 114, 166, 195, 200, 254, 120, 78,
        ]);
        let nft_owner = TestAccount::new([1u8; 32]);
        let relayer = TestAccount::new([2u8; 32]);
        let royalties = vec![
            Royalty {
                recipient_t1_address: H160(hex!("0000000000000000000000000000000000000002")),
                rate: RoyaltyRate { parts_per_million: 1_000u32 },
            },
            Royalty {
                recipient_t1_address: H160(hex!("0000000000000000000000000000000000000003")),
                rate: RoyaltyRate { parts_per_million: 500u32 },
            },
        ];
        let unique_external_ref = String::from("Offchain location of NFT").into_bytes();

        Context {
            nft_owner_account: nft_owner.account_id(),
            nft_owner_key_pair: nft_owner.key_pair(),
            relayer: relayer.account_id(),
            nft_id,
            info_id: U256::zero(),
            unique_external_ref,
            royalties,
            t1_authority,
        }
    }
}

impl Context {
    fn setup(&self) {
        <Nfts<TestRuntime>>::remove(&self.nft_id);
        <NftInfos<TestRuntime>>::remove(&self.nft_id);
        <UsedExternalReferences<TestRuntime>>::remove(&self.bounded_external_ref());
    }

    fn create_signed_mint_single_nft_call(&self) -> Box<<TestRuntime as Config>::RuntimeCall> {
        let proof = self.create_signed_mint_single_nft_proof();

        return Box::new(MockCall::NftManager(super::Call::<TestRuntime>::signed_mint_single_nft {
            proof,
            unique_external_ref: self.unique_external_ref.clone(),
            royalties: self.royalties.clone(),
            t1_authority: self.t1_authority,
        }));
    }

    fn create_signed_mint_single_nft_proof(&self) -> Proof<Signature, AccountId> {
        return create_proof_for_signed_mint_single_nft(
            &self.relayer,
            &self.nft_owner_account,
            &self.nft_owner_key_pair,
            &self.unique_external_ref,
            &self.royalties,
            self.t1_authority,
        );
    }

    fn mint_single_nft_event_emitted(&self) -> bool {
        return System::events().iter().any(|a| {
            a.event
                == Event::NftManager(crate::Event::<TestRuntime>::SingleNftMinted {
                    nft_id: self.nft_id,
                    owner: self.nft_owner_account,
                    authority: self.t1_authority,
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

    pub fn bounded_external_ref(&self) -> BoundedVec<u8, NftExternalRefBound> {
        BoundedVec::try_from(self.unique_external_ref.clone())
            .expect("Unique external reference bound was exceeded.")
    }

    pub fn bounded_royalties(&self) -> BoundedVec<Royalty, NftRoyaltiesBound> {
        BoundedVec::try_from(self.royalties.clone()).expect("Royalty bound was exceeded.")
    }
}

mod proxy_signed_mint_single_nft {
    use super::*;

    mod succeeds_implies_that {
        use super::*;

        #[test]
        fn nft_id_is_created() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call));

                assert_eq!(true, <Nfts<TestRuntime>>::contains_key(&context.nft_id));

                assert_eq!(
                    Nft::new(
                        context.nft_id,
                        context.info_id,
                        context.bounded_external_ref(),
                        context.nft_owner_account
                    ),
                    <Nfts<TestRuntime>>::get(&context.nft_id).unwrap()
                );
            });
        }

        #[test]
        fn nft_info_is_created() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call.clone()));

                assert_eq!(true, <NftInfos<TestRuntime>>::contains_key(&context.info_id));

                assert_eq!(
                    NftInfo::new(
                        context.info_id,
                        context.bounded_royalties(),
                        context.t1_authority
                    ),
                    <NftInfos<TestRuntime>>::get(&context.info_id).unwrap()
                );
            });
        }

        #[test]
        fn external_reference_is_used() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call.clone()));

                assert_eq!(
                    true,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(
                    true,
                    <UsedExternalReferences<TestRuntime>>::get(context.bounded_external_ref())
                );
            });
        }

        #[test]
        fn mint_single_nft_event_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_eq!(false, context.mint_single_nft_event_emitted());

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call.clone()));

                assert_eq!(true, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn owned_nft_list_is_updated_test() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_eq!(false, context.call_dispatched_event_emitted(&call));

                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call.clone()));
            });
        }

        #[test]
        fn call_dispatched_event_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

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
                let call = context.create_signed_mint_single_nft_call();

                assert_noop!(NftManager::proxy(RawOrigin::None.into(), call.clone()), BadOrigin);
            });
        }

        #[test]
        fn unique_external_ref_is_empty() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = Context::default();
                context.unique_external_ref = vec![];
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::ExternalRefIsMandatory
                );
            });
        }

        #[test]
        fn unique_external_ref_is_already_in_use() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                <UsedExternalReferences<TestRuntime>>::insert(
                    &context.bounded_external_ref(),
                    true,
                );
                let call = context.create_signed_mint_single_nft_call();

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::ExternalRefIsAlreadyInUse
                );
            });
        }

        #[test]
        fn t1_authority_is_empty() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = Context::default();
                context.t1_authority = H160::zero();
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::T1AuthorityIsMandatory
                );
            });
        }

        #[test]
        fn one_royalty_rate_is_greater_than_denominator() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = Context::default();
                context.royalties = vec![
                    Royalty {
                        recipient_t1_address: H160(hex!(
                            "0000000000000000000000000000000000000002"
                        )),
                        rate: RoyaltyRate { parts_per_million: 1 },
                    },
                    Royalty {
                        recipient_t1_address: H160(hex!(
                            "0000000000000000000000000000000000000003"
                        )),
                        rate: RoyaltyRate { parts_per_million: ROYALTY_RATE_DENOMINATOR + 1 },
                    },
                ];
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::RoyaltyRateIsNotValid
                );
            });
        }

        #[test]
        fn royalty_rates_total_is_greater_than_denominator() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = Context::default();
                context.royalties = vec![
                    Royalty {
                        recipient_t1_address: H160(hex!(
                            "0000000000000000000000000000000000000002"
                        )),
                        rate: RoyaltyRate { parts_per_million: 1 },
                    },
                    Royalty {
                        recipient_t1_address: H160(hex!(
                            "0000000000000000000000000000000000000003"
                        )),
                        rate: RoyaltyRate { parts_per_million: ROYALTY_RATE_DENOMINATOR },
                    },
                ];
                context.setup();
                let call = context.create_signed_mint_single_nft_call();

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::TotalRoyaltyRateIsNotValid
                );
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
                    &context.relayer,
                    &context.unique_external_ref,
                    &context.royalties,
                    context.t1_authority,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_mint_single_nft {
                        proof,
                        unique_external_ref: context.unique_external_ref.clone(),
                        royalties: context.royalties.clone(),
                        t1_authority: context.t1_authority,
                    },
                ));

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_nft_owner() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_nft_owner = TestAccount::new([5u8; 32]);
                let proof = create_proof_for_signed_mint_single_nft(
                    &context.relayer,
                    &other_nft_owner.account_id(),
                    &context.nft_owner_key_pair,
                    &context.unique_external_ref,
                    &context.royalties,
                    context.t1_authority,
                );
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_mint_single_nft {
                        proof,
                        unique_external_ref: context.unique_external_ref.clone(),
                        royalties: context.royalties.clone(),
                        t1_authority: context.t1_authority,
                    },
                ));

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_nft_owner_key_pair() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_nft_owner_account = TestAccount::new([5u8; 32]);
                let proof = create_proof_for_signed_mint_single_nft(
                    &context.relayer,
                    &context.nft_owner_account,
                    &other_nft_owner_account.key_pair(),
                    &context.unique_external_ref,
                    &context.royalties,
                    context.t1_authority,
                );
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_mint_single_nft {
                        proof,
                        unique_external_ref: context.unique_external_ref.clone(),
                        royalties: context.royalties.clone(),
                        t1_authority: context.t1_authority,
                    },
                ));

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_unique_external_ref() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();

                let other_unique_external_ref =
                    String::from("Other offchain location of NFT").into_bytes();
                let data_to_sign = (
                    SIGNED_MINT_SINGLE_NFT_CONTEXT,
                    &context.relayer,
                    other_unique_external_ref,
                    &context.royalties,
                    context.t1_authority,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_mint_single_nft {
                        proof,
                        unique_external_ref: context.unique_external_ref.clone(),
                        royalties: context.royalties.clone(),
                        t1_authority: context.t1_authority,
                    },
                ));

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_royalties() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();

                let other_royalties = vec![Royalty {
                    recipient_t1_address: H160(hex!("0000000000000000000000000000000000000001")),
                    rate: RoyaltyRate { parts_per_million: 1 },
                }];
                let data_to_sign = (
                    SIGNED_MINT_SINGLE_NFT_CONTEXT,
                    &context.relayer,
                    &context.bounded_external_ref(),
                    other_royalties,
                    context.t1_authority,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_mint_single_nft {
                        proof,
                        unique_external_ref: context.unique_external_ref.clone(),
                        royalties: context.royalties.clone(),
                        t1_authority: context.t1_authority,
                    },
                ));

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_t1_authority() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();

                let other_t1_authority = H160(hex!("1111111111111111111111111111111111111111"));
                let data_to_sign = (
                    SIGNED_MINT_SINGLE_NFT_CONTEXT,
                    &context.relayer,
                    &context.unique_external_ref,
                    &context.royalties,
                    other_t1_authority,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);
                let call = Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_mint_single_nft {
                        proof,
                        unique_external_ref: context.unique_external_ref.clone(),
                        royalties: context.royalties.clone(),
                        t1_authority: context.t1_authority,
                    },
                ));

                assert_err!(
                    NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }
    }
}

mod signed_mint_single_nft {
    use super::*;

    mod succeeds_implies_that {
        use super::*;

        #[test]
        fn nft_id_is_created() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_mint_single_nft_proof();

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));

                assert_ok!(NftManager::signed_mint_single_nft(
                    Origin::signed(context.nft_owner_account),
                    proof,
                    context.unique_external_ref.clone(),
                    context.royalties.clone(),
                    context.t1_authority
                ));

                assert_eq!(true, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(
                    Nft::new(
                        context.nft_id,
                        context.info_id,
                        context.bounded_external_ref(),
                        context.nft_owner_account
                    ),
                    <Nfts<TestRuntime>>::get(&context.nft_id).unwrap()
                );
            });
        }

        #[test]
        fn nft_info_is_created() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_mint_single_nft_proof();

                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));

                assert_ok!(NftManager::signed_mint_single_nft(
                    Origin::signed(context.nft_owner_account),
                    proof,
                    context.unique_external_ref.clone(),
                    context.royalties.clone(),
                    context.t1_authority
                ));

                assert_eq!(true, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    NftInfo::new(
                        context.info_id,
                        context.bounded_royalties(),
                        context.t1_authority
                    ),
                    <NftInfos<TestRuntime>>::get(&context.info_id).unwrap()
                );
            });
        }

        #[test]
        fn external_reference_is_used() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_mint_single_nft_proof();

                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );

                assert_ok!(NftManager::signed_mint_single_nft(
                    Origin::signed(context.nft_owner_account),
                    proof,
                    context.unique_external_ref.clone(),
                    context.royalties.clone(),
                    context.t1_authority
                ));

                assert_eq!(
                    true,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(
                    true,
                    <UsedExternalReferences<TestRuntime>>::get(context.bounded_external_ref())
                );
            });
        }

        #[test]
        fn single_nft_minted_event_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_mint_single_nft_proof();

                assert_eq!(false, context.mint_single_nft_event_emitted());

                assert_ok!(NftManager::signed_mint_single_nft(
                    Origin::signed(context.nft_owner_account),
                    proof,
                    context.unique_external_ref.clone(),
                    context.royalties.clone(),
                    context.t1_authority
                ));

                assert_eq!(true, context.mint_single_nft_event_emitted());
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
                let proof = context.create_signed_mint_single_nft_proof();

                assert_noop!(
                    NftManager::signed_mint_single_nft(
                        RawOrigin::None.into(),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties,
                        context.t1_authority
                    ),
                    BadOrigin
                );
            });
        }

        #[test]
        fn unique_external_ref_is_empty() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_mint_single_nft_proof();

                assert_noop!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        vec![],
                        context.royalties,
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::ExternalRefIsMandatory
                );
            });
        }

        #[test]
        fn unique_external_ref_is_already_in_use() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                <UsedExternalReferences<TestRuntime>>::insert(
                    &context.bounded_external_ref(),
                    true,
                );
                let proof = context.create_signed_mint_single_nft_proof();

                assert_noop!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties,
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::ExternalRefIsAlreadyInUse
                );
            });
        }

        #[test]
        fn t1_authority_is_empty() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_mint_single_nft_proof();

                assert_noop!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties,
                        H160::zero()
                    ),
                    Error::<TestRuntime>::T1AuthorityIsMandatory
                );
            });
        }

        #[test]
        fn one_royalty_rate_is_greater_than_denominator() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_mint_single_nft_proof();

                assert_noop!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        vec![
                            Royalty {
                                recipient_t1_address: H160(hex!(
                                    "0000000000000000000000000000000000000002"
                                )),
                                rate: RoyaltyRate { parts_per_million: 1 },
                            },
                            Royalty {
                                recipient_t1_address: H160(hex!(
                                    "0000000000000000000000000000000000000003"
                                )),
                                rate: RoyaltyRate {
                                    parts_per_million: ROYALTY_RATE_DENOMINATOR + 1
                                },
                            }
                        ],
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::RoyaltyRateIsNotValid
                );
            });
        }

        #[test]
        fn royalty_rates_total_is_greater_than_denominator() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let proof = context.create_signed_mint_single_nft_proof();

                assert_noop!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        vec![
                            Royalty {
                                recipient_t1_address: H160(hex!(
                                    "0000000000000000000000000000000000000002"
                                )),
                                rate: RoyaltyRate { parts_per_million: 1 },
                            },
                            Royalty {
                                recipient_t1_address: H160(hex!(
                                    "0000000000000000000000000000000000000003"
                                )),
                                rate: RoyaltyRate { parts_per_million: ROYALTY_RATE_DENOMINATOR },
                            }
                        ],
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::TotalRoyaltyRateIsNotValid
                );
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
                    &context.relayer,
                    &context.unique_external_ref,
                    &context.royalties,
                    context.t1_authority,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);

                assert_err!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties.clone(),
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_nft_owner() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_nft_owner = TestAccount::new([5u8; 32]);
                let proof = create_proof_for_signed_mint_single_nft(
                    &context.relayer,
                    &other_nft_owner.account_id(),
                    &context.nft_owner_key_pair,
                    &context.unique_external_ref,
                    &context.royalties,
                    context.t1_authority,
                );

                assert_err!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties.clone(),
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::SenderIsNotSigner
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_nft_owner_key_pair() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();
                let other_nft_owner_account = TestAccount::new([5u8; 32]);
                let proof = create_proof_for_signed_mint_single_nft(
                    &context.relayer,
                    &context.nft_owner_account,
                    &other_nft_owner_account.key_pair(),
                    &context.unique_external_ref,
                    &context.royalties,
                    context.t1_authority,
                );

                assert_err!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties.clone(),
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_unique_external_ref() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();

                let other_unique_external_ref =
                    String::from("Other offchain location of NFT").into_bytes();
                let data_to_sign = (
                    SIGNED_MINT_SINGLE_NFT_CONTEXT,
                    &context.relayer,
                    other_unique_external_ref,
                    &context.royalties,
                    context.t1_authority,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);

                assert_err!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties.clone(),
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_royalties() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();

                let other_royalties = vec![Royalty {
                    recipient_t1_address: H160(hex!("0000000000000000000000000000000000000001")),
                    rate: RoyaltyRate { parts_per_million: 1 },
                }];
                let data_to_sign = (
                    SIGNED_MINT_SINGLE_NFT_CONTEXT,
                    &context.relayer,
                    &context.unique_external_ref,
                    other_royalties,
                    context.t1_authority,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);

                assert_err!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties.clone(),
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }

        #[test]
        fn mismatched_proof_other_t1_authority() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                context.setup();

                let other_t1_authority = H160(hex!("1111111111111111111111111111111111111111"));
                let data_to_sign = (
                    SIGNED_MINT_SINGLE_NFT_CONTEXT,
                    &context.relayer,
                    &context.unique_external_ref,
                    &context.royalties,
                    other_t1_authority,
                );
                let signature = sign(&context.nft_owner_key_pair, &data_to_sign.encode());
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);

                assert_err!(
                    NftManager::signed_mint_single_nft(
                        Origin::signed(context.nft_owner_account),
                        proof,
                        context.unique_external_ref.clone(),
                        context.royalties.clone(),
                        context.t1_authority
                    ),
                    Error::<TestRuntime>::UnauthorizedSignedMintSingleNftTransaction
                );

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&context.info_id));
                assert_eq!(
                    false,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        &context.bounded_external_ref()
                    )
                );
                assert_eq!(false, context.mint_single_nft_event_emitted());
            });
        }
    }
}

mod get_proof {
    use super::*;

    #[test]
    fn succeeds_for_valid_signed_mint_single_nft_call() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            context.setup();
            let proof = context.create_signed_mint_single_nft_proof();
            let call = Box::new(MockCall::NftManager(
                super::Call::<TestRuntime>::signed_mint_single_nft {
                    proof: proof.clone(),
                    unique_external_ref: context.unique_external_ref.clone(),
                    royalties: context.royalties.clone(),
                    t1_authority: context.t1_authority,
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

fn create_proof_for_signed_mint_single_nft(
    relayer: &AccountId,
    nft_owner_account: &AccountId,
    nft_owner_key_pair: &Pair,
    unique_external_ref: &Vec<u8>,
    royalties: &Vec<Royalty>,
    t1_authority: H160,
) -> Proof<Signature, AccountId> {
    let context = SIGNED_MINT_SINGLE_NFT_CONTEXT;
    let data_to_sign = (context, relayer, unique_external_ref, royalties, t1_authority);
    let signature = sign(nft_owner_key_pair, &data_to_sign.encode());

    return build_proof(nft_owner_account, relayer, signature);
}
