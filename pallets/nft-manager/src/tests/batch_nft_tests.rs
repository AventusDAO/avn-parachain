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
    mock::{AccountId, RuntimeCall as MockCall, *},
    Call,
};
use codec::Encode;
use frame_support::{assert_noop, assert_ok, error::BadOrigin};
use frame_system::RawOrigin;
use hex_literal::hex;
use mock::{RuntimeEvent as Event, RuntimeOrigin as Origin};
use sp_core::sr25519::Pair;

fn build_proof(
    signer: &AccountId,
    relayer: &AccountId,
    signature: Signature,
) -> Proof<Signature, AccountId> {
    return Proof { signer: *signer, relayer: *relayer, signature }
}

fn create_batch_and_list() -> U256 {
    let batch_id = create_batch();
    list_batch(batch_id, NftSaleType::Fiat);

    return batch_id
}

fn create_batch() -> U256 {
    let context = CreateBatchContext::default();
    let batch_id = generate_batch_id::<TestRuntime>(<NextSingleNftUniqueId<TestRuntime>>::get());

    assert_eq!(false, <BatchInfoId<TestRuntime>>::contains_key(batch_id));
    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
    let inner_call = context.create_signed_create_batch_call(nonce);
    assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

    return batch_id
}

fn list_batch(batch_id: U256, market: NftSaleType) {
    let mut sale_context = OpenForSaleContext::default();
    sale_context.market = market;

    let nonce = <BatchNonces<TestRuntime>>::get(&sale_context.creator_account);
    let inner_call = sale_context.create_signed_list_batch_for_sale_call(batch_id, nonce);
    assert_ok!(NftManager::proxy(Origin::signed(sale_context.relayer), inner_call));
}

struct CreateBatchContext {
    relayer: AccountId,
    royalties: Vec<Royalty>,
    t1_authority: H160,
    total_supply: u64,
    creator_account: AccountId,
    creator_key_pair: Pair,
}

impl Default for CreateBatchContext {
    fn default() -> Self {
        let t1_authority = H160(hex!("0000000000000000000000000000000000000001"));
        let creator = TestAccount::new([1u8; 32]);
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

        CreateBatchContext {
            relayer: relayer.account_id(),
            royalties,
            t1_authority,
            total_supply: 5u64,
            creator_account: creator.account_id(),
            creator_key_pair: creator.key_pair(),
        }
    }
}

impl CreateBatchContext {
    fn create_signed_create_batch_call(
        &self,
        nonce: u64,
    ) -> Box<<TestRuntime as Config>::RuntimeCall> {
        let proof = self.create_signed_create_batch_proof(nonce);

        return Box::new(MockCall::NftManager(super::Call::<TestRuntime>::signed_create_batch {
            proof,
            total_supply: self.total_supply,
            royalties: self.royalties.clone(),
            t1_authority: self.t1_authority,
        }))
    }

    fn create_signed_create_batch_proof(&self, nonce: u64) -> Proof<Signature, AccountId> {
        let data_to_sign = (
            SIGNED_CREATE_BATCH_CONTEXT,
            self.relayer,
            self.total_supply,
            self.royalties.clone(),
            self.t1_authority,
            nonce,
        );
        let signature = sign(&self.creator_key_pair, &data_to_sign.encode());

        return build_proof(&self.creator_account, &self.relayer, signature)
    }

    fn batch_created_event_emitted(&self, batch_id: U256) -> bool {
        return System::events().iter().any(|a| {
            a.event ==
                Event::NftManager(crate::Event::<TestRuntime>::BatchCreated {
                    batch_nft_id: batch_id,
                    total_supply: self.total_supply,
                    batch_creator: self.creator_account,
                    authority: self.t1_authority,
                })
        })
    }
}

mod signed_create_batch {
    use super::*;

    #[test]
    fn succeeds_with_good_parameters() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = CreateBatchContext::default();
            let expected_batch_id =
                generate_batch_id::<TestRuntime>(<NextSingleNftUniqueId<TestRuntime>>::get());

            assert_eq!(false, <BatchInfoId<TestRuntime>>::contains_key(expected_batch_id));

            let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
            let inner_call = context.create_signed_create_batch_call(nonce);

            assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

            //Batch has been created
            assert_eq!(true, <BatchInfoId<TestRuntime>>::contains_key(expected_batch_id));

            //Correct event is emited
            assert_eq!(true, context.batch_created_event_emitted(expected_batch_id));

            //Info is correctly populated
            let info =
                <NftInfos<TestRuntime>>::get(<BatchInfoId<TestRuntime>>::get(expected_batch_id))
                    .unwrap();
            assert_eq!(Some(expected_batch_id), info.batch_id);
            assert_eq!(context.total_supply, info.total_supply);
            assert_eq!(Some(context.creator_account), info.creator);

            //Nonce has incremented
            assert_eq!(nonce + 1, <BatchNonces<TestRuntime>>::get(context.creator_account));

            //Unique id has incremented
            assert_eq!(U256::zero() + 1, <NextSingleNftUniqueId<TestRuntime>>::get());
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = CreateBatchContext::default();

                let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
                let inner_call = context.create_signed_create_batch_call(nonce);

                assert_noop!(NftManager::proxy(RawOrigin::None.into(), inner_call), BadOrigin);
            });
        }

        #[test]
        fn total_supply_is_zero() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = CreateBatchContext::default();
                context.total_supply = 0u64;

                let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
                let inner_call = context.create_signed_create_batch_call(nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::TotalSupplyZero
                );
            });
        }

        #[test]
        fn total_supply_exceeds_bounds() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = CreateBatchContext::default();
                let out_of_bounds_supply: u32 = <MockNftBatchBound as sp_core::Get<u32>>::get() + 1;

                context.total_supply = out_of_bounds_supply as u64;

                let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
                let inner_call = context.create_signed_create_batch_call(nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchOutOfBounds
                );
            });
        }

        #[test]
        fn royalties_exceeds_bounds() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = CreateBatchContext::default();
                let out_of_bounds_royalties: u32 =
                    <NftRoyaltiesBound as sp_core::Get<u32>>::get() + 1;

                context.royalties = vec![
                    Royalty {
                        recipient_t1_address: H160(hex!(
                            "0000000000000000000000000000000000000002"
                        )),
                        rate: RoyaltyRate { parts_per_million: 1_000u32 },
                    };
                    out_of_bounds_royalties as usize
                ];

                let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
                let inner_call = context.create_signed_create_batch_call(nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::RoyaltiesOutOfBounds
                );
            });
        }

        #[test]
        fn t1_authority_is_empty() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = CreateBatchContext::default();
                context.t1_authority = H160::zero();

                let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
                let inner_call = context.create_signed_create_batch_call(nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::T1AuthorityIsMandatory
                );
            });
        }

        #[test]
        fn one_royalty_rate_is_greater_than_denominator() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = CreateBatchContext::default();
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

                let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
                let inner_call = context.create_signed_create_batch_call(nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::RoyaltyRateIsNotValid
                );
            });
        }

        #[test]
        fn royalty_rates_total_is_greater_than_denominator() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = CreateBatchContext::default();
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

                let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);
                let inner_call = context.create_signed_create_batch_call(nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::TotalRoyaltyRateIsNotValid
                );
            });
        }

        mod the_proof_is_invalid_because {
            use super::*;

            fn get_call(context: &CreateBatchContext, data_to_sign: &[u8]) -> Box<MockCall> {
                let signature = sign(&context.creator_key_pair, data_to_sign);
                let proof = build_proof(&context.creator_account, &context.relayer, signature);

                return Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_create_batch {
                        proof,
                        total_supply: context.total_supply.clone(),
                        royalties: context.royalties.clone(),
                        t1_authority: context.t1_authority,
                    },
                ))
            }

            fn create_batch_and_assert_success(context: &CreateBatchContext, nonce: u64) {
                let batch_id =
                    generate_batch_id::<TestRuntime>(<NextSingleNftUniqueId<TestRuntime>>::get());
                assert_eq!(false, context.batch_created_event_emitted(batch_id));

                //now show that it will work if we fix the bad data.
                let data_to_sign = (
                    SIGNED_CREATE_BATCH_CONTEXT,
                    &context.relayer,
                    &context.total_supply,
                    &context.royalties,
                    context.t1_authority,
                    nonce,
                );

                let call = get_call(&context, &data_to_sign.encode());
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call));
                assert_eq!(true, context.batch_created_event_emitted(batch_id));
            }

            #[test]
            fn context_is_bad() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = CreateBatchContext::default();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let other_context: &'static [u8] = b"authorization for something else";
                    let data_to_sign = (
                        other_context,
                        &context.relayer,
                        &context.total_supply,
                        &context.royalties,
                        context.t1_authority,
                        nonce,
                    );

                    let call = get_call(&context, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedCreateBatchTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, nonce);
                });
            }

            #[test]
            fn mismatched_proof_total_supply() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = CreateBatchContext::default();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let other_total_supply = 17u64;
                    let data_to_sign = (
                        SIGNED_CREATE_BATCH_CONTEXT,
                        &context.relayer,
                        other_total_supply,
                        &context.royalties,
                        context.t1_authority,
                        nonce,
                    );

                    let call = get_call(&context, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedCreateBatchTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, nonce);
                });
            }

            #[test]
            fn mismatched_proof_other_royalties() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = CreateBatchContext::default();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let other_royalties = vec![Royalty {
                        recipient_t1_address: H160(hex!(
                            "0000000000000000000000000000000000000001"
                        )),
                        rate: RoyaltyRate { parts_per_million: 1 },
                    }];
                    let data_to_sign = (
                        SIGNED_CREATE_BATCH_CONTEXT,
                        &context.relayer,
                        &context.total_supply,
                        other_royalties,
                        context.t1_authority,
                    );

                    let call = get_call(&context, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedCreateBatchTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, nonce);
                });
            }

            #[test]
            fn mismatched_proof_other_t1_authority() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = CreateBatchContext::default();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let other_t1_authority = H160(hex!("1111111111111111111111111111111111111111"));
                    let data_to_sign = (
                        SIGNED_CREATE_BATCH_CONTEXT,
                        &context.relayer,
                        &context.total_supply,
                        &context.royalties,
                        other_t1_authority,
                    );

                    let call = get_call(&context, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedCreateBatchTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, nonce);
                });
            }

            #[test]
            fn mismatched_proof_relayer() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = CreateBatchContext::default();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let bad_relayer = TestAccount::new([71u8; 32]).account_id();
                    let data_to_sign = (
                        SIGNED_CREATE_BATCH_CONTEXT,
                        &bad_relayer,
                        &context.total_supply,
                        &context.royalties,
                        &context.t1_authority,
                    );

                    let call = get_call(&context, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedCreateBatchTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, nonce);
                });
            }
        }
    }
}

struct MintBatchNftContext {
    nft_owner_account: AccountId,
    nft_owner_key_pair: Pair,
    relayer: AccountId,
    nft_id: NftId,
    unique_external_ref: Vec<u8>,
    t1_authority: H160,
}

impl Default for MintBatchNftContext {
    fn default() -> Self {
        // This is generated based on unique_id = 1 and index = 0
        let nft_id = U256::from_big_endian(&[
            101, 94, 240, 118, 189, 202, 200, 247, 116, 145, 110, 133, 216, 128, 100, 172, 36, 189,
            18, 53, 164, 178, 200, 65, 155, 27, 180, 246, 23, 91, 12, 175,
        ]);

        let nft_owner = TestAccount::new([1u8; 32]);
        let relayer = TestAccount::new([2u8; 32]);
        let unique_external_ref = String::from("Offchain location of NFT").into_bytes();
        let t1_authority = H160(hex!("0000000000000000000000000000000000000001"));

        MintBatchNftContext {
            nft_owner_account: nft_owner.account_id(),
            nft_owner_key_pair: nft_owner.key_pair(),
            relayer: relayer.account_id(),
            nft_id,
            unique_external_ref,
            t1_authority,
        }
    }
}

impl MintBatchNftContext {
    fn setup(&self) {
        <Nfts<TestRuntime>>::remove(&self.nft_id);
        <NftInfos<TestRuntime>>::remove(&self.nft_id);
        <UsedExternalReferences<TestRuntime>>::remove(
            &BoundedVec::try_from(self.unique_external_ref.clone())
                .expect("Unique external reference bound was exceeded."),
        );
    }

    fn create_signed_mint_batch_nft_call(
        &self,
        batch_id: U256,
        index: u64,
    ) -> Box<<TestRuntime as Config>::RuntimeCall> {
        let proof = self.create_signed_mint_batch_nft_proof(batch_id, index);

        return Box::new(MockCall::NftManager(super::Call::<TestRuntime>::signed_mint_batch_nft {
            proof,
            batch_id,
            index,
            owner: self.nft_owner_account,
            unique_external_ref: self.unique_external_ref.clone(),
        }))
    }

    fn create_signed_mint_batch_nft_proof(
        &self,
        batch_id: U256,
        index: u64,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = (
            SIGNED_MINT_BATCH_NFT_CONTEXT,
            self.relayer,
            batch_id,
            index,
            self.unique_external_ref.clone(),
            self.nft_owner_account,
        );
        let signature = sign(&self.nft_owner_key_pair, &data_to_sign.encode());

        return build_proof(&self.nft_owner_account, &self.relayer, signature)
    }

    fn mint_batch_nft_event_emitted(&self, batch_id: U256) -> bool {
        return System::events().iter().any(|a| {
            a.event ==
                Event::NftManager(crate::Event::<TestRuntime>::BatchNftMinted {
                    nft_id: self.nft_id,
                    batch_nft_id: batch_id,
                    authority: self.t1_authority,
                    owner: self.nft_owner_account,
                })
        })
    }
}

mod signed_mint_batch_nft {
    use super::*;

    mod succeeds {
        use super::*;

        #[test]
        fn with_good_parameters() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = MintBatchNftContext::default();
                context.setup();

                let index = 0u64;
                let batch_id = create_batch_and_list();

                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                // Nft has been minted
                assert_eq!(true, <Nfts<TestRuntime>>::contains_key(&context.nft_id));

                // Batch_id map has been created
                assert!(<NftBatches<TestRuntime>>::contains_key(&batch_id));

                // Newly minted nft is linked to the correct batch_id
                assert_eq!(<NftBatches<TestRuntime>>::get(batch_id)[0], context.nft_id);

                // Correct event is emited
                assert_eq!(true, context.mint_batch_nft_event_emitted(batch_id));

                // External ref is used
                assert_eq!(
                    true,
                    <UsedExternalReferences<TestRuntime>>::contains_key(
                        BoundedVec::try_from(context.unique_external_ref)
                            .expect("Unique external reference bound was exceeded.")
                    )
                );

                // Total supply has not been exceeded
                let info = <NftInfos<TestRuntime>>::get(<BatchInfoId<TestRuntime>>::get(batch_id))
                    .unwrap();
                assert!(
                    <NftBatches<TestRuntime>>::get(&batch_id).len() <=
                        info.total_supply.try_into().unwrap()
                );
            });
        }

        #[test]
        fn with_multiple_batch_nfts_minted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = MintBatchNftContext::default();
                context.setup();

                let mut index = 0u64;
                let batch_id = create_batch_and_list();

                // mint first nft
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                context.unique_external_ref =
                    String::from("Offchain location of NFT 1").into_bytes();
                index = 1u64;

                // mint second nft
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                // 2 Nfts minted and assigned to the same batch
                assert_eq!(<NftBatches<TestRuntime>>::get(batch_id).len(), 2);
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = MintBatchNftContext::default();
                context.setup();

                let index = 0u64;
                let batch_id = create_batch_and_list();
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);

                assert_noop!(NftManager::proxy(RawOrigin::None.into(), inner_call), BadOrigin);
            });
        }

        #[test]
        fn batch_id_does_not_exist() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = MintBatchNftContext::default();
                context.setup();

                let index = 0u64;
                let batch_id = U256::zero() + 1;
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchDoesNotExist
                );
            });
        }

        #[test]
        fn batch_not_listed_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = MintBatchNftContext::default();
                context.setup();

                let index = 0u64;
                // Create the batch without listing it
                let batch_id = create_batch();

                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchNotListed
                );
            });
        }

        #[test]
        fn batch_listed_for_sale_on_wrong_market() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = MintBatchNftContext::default();
                context.setup();

                let index = 0u64;
                let batch_id = create_batch();

                // List on Etherum. Only fiat listings can be minted via an extrinsic
                list_batch(batch_id, NftSaleType::Ethereum);

                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchNotListedForFiatSale
                );
            });
        }

        #[test]
        fn external_ref_is_empty() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = MintBatchNftContext::default();
                context.setup();

                // bad external ref
                context.unique_external_ref = vec![];

                let index = 0u64;
                let batch_id = create_batch_and_list();

                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::ExternalRefIsMandatory
                );
            });
        }

        #[test]
        fn external_ref_is_out_of_bounds() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = MintBatchNftContext::default();
                context.setup();

                let out_of_bounds_size: u32 = <NftExternalRefBound as sp_core::Get<u32>>::get() + 1;
                context.unique_external_ref = vec![b'A'; out_of_bounds_size as usize];

                let index = 0u64;
                let batch_id = create_batch_and_list();

                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::ExternalRefOutOfBounds
                );
            });
        }

        #[test]
        fn external_ref_is_reused() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = MintBatchNftContext::default();
                context.setup();

                let mut index = 0u64;
                let batch_id = create_batch_and_list();

                // mint first nft
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                // Keep external ref the same but increment index
                index = 1u64;

                // mint second nft
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);
                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::ExternalRefIsAlreadyInUse
                );
            });
        }

        #[test]
        fn total_supply_is_exceeded() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = MintBatchNftContext::default();
                context.setup();

                let mut index = 0u64;
                let batch_id = create_batch_and_list();
                let info = <NftInfos<TestRuntime>>::get(<BatchInfoId<TestRuntime>>::get(batch_id))
                    .unwrap();

                // Can mint until total supply is exceeded
                for i in 0..info.total_supply + 1 {
                    let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);

                    if i == info.total_supply {
                        assert_noop!(
                            NftManager::proxy(Origin::signed(context.relayer), inner_call),
                            Error::<TestRuntime>::TotalSupplyExceeded
                        );
                    } else {
                        assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));
                    }

                    context.unique_external_ref = index.encode();
                    index += 1;
                }

                assert!(
                    <NftBatches<TestRuntime>>::get(&batch_id).len() <=
                        info.total_supply.try_into().unwrap()
                );
            });
        }

        #[test]
        fn batch_id_is_not_set() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = MintBatchNftContext::default();
                context.setup();

                let index = 0u64;
                let batch_id = U256::zero();
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchIdIsMandatory
                );
            });
        }

        #[test]
        fn index_is_reused() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = MintBatchNftContext::default();
                context.setup();

                let index = 0u64;
                let batch_id = create_batch_and_list();

                // mint first nft
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                // update external ref but keep at 0 to cause error
                context.unique_external_ref =
                    String::from("Offchain location of NFT 1").into_bytes();

                // mint second nft
                let inner_call = context.create_signed_mint_batch_nft_call(batch_id, index);
                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::NftAlreadyExists
                );
            });
        }

        mod the_proof_is_invalid_because {
            use super::*;

            fn get_call(
                context: &MintBatchNftContext,
                batch_id: &U256,
                index: &u64,
                data_to_sign: &[u8],
            ) -> Box<MockCall> {
                let signature = sign(&context.nft_owner_key_pair, data_to_sign);
                let proof = build_proof(&context.nft_owner_account, &context.relayer, signature);

                return Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_mint_batch_nft {
                        proof,
                        batch_id: *batch_id,
                        index: *index,
                        owner: context.nft_owner_account,
                        unique_external_ref: context.unique_external_ref.clone(),
                    },
                ))
            }

            fn create_batch_and_assert_success(
                context: &MintBatchNftContext,
                batch_id: U256,
                index: u64,
            ) {
                //now show that it will work if we fix the bad data.
                let data_to_sign = (
                    SIGNED_MINT_BATCH_NFT_CONTEXT,
                    &context.relayer,
                    &batch_id,
                    index,
                    &context.unique_external_ref,
                    &context.nft_owner_account,
                );

                let call = get_call(&context, &batch_id, &index, &data_to_sign.encode());
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call));
                assert_eq!(true, context.mint_batch_nft_event_emitted(batch_id));
            }

            #[test]
            fn context_is_bad() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = MintBatchNftContext::default();
                    context.setup();

                    let index = 0u64;
                    let batch_id = create_batch_and_list();

                    let other_context: &'static [u8] = b"authorization for something else";
                    let data_to_sign = (
                        other_context,
                        &context.relayer,
                        &batch_id,
                        index,
                        &context.unique_external_ref,
                        &context.nft_owner_account,
                    );

                    let call = get_call(&context, &batch_id, &index, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedMintBatchNftTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, index);
                });
            }

            #[test]
            fn mismatched_proof_batch_id() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = MintBatchNftContext::default();
                    context.setup();

                    let index = 0u64;
                    let batch_id = create_batch_and_list();

                    let bad_batch_id = batch_id + 5;
                    let data_to_sign = (
                        SIGNED_MINT_BATCH_NFT_CONTEXT,
                        &context.relayer,
                        &bad_batch_id,
                        index,
                        &context.unique_external_ref,
                        &context.nft_owner_account,
                    );

                    let call = get_call(&context, &batch_id, &index, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedMintBatchNftTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, index);
                });
            }

            #[test]
            fn mismatched_proof_index() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = MintBatchNftContext::default();
                    context.setup();

                    let index = 0u64;
                    let batch_id = create_batch_and_list();

                    let bad_index = index + 5;
                    let data_to_sign = (
                        SIGNED_MINT_BATCH_NFT_CONTEXT,
                        &context.relayer,
                        &batch_id,
                        bad_index,
                        &context.unique_external_ref,
                        &context.nft_owner_account,
                    );

                    let call = get_call(&context, &batch_id, &index, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedMintBatchNftTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, index);
                });
            }

            #[test]
            fn mismatched_proof_external_ref() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = MintBatchNftContext::default();
                    context.setup();

                    let index = 0u64;
                    let batch_id = create_batch_and_list();

                    let bad_external_ref =
                        (context.unique_external_ref.clone(), String::from("bad_ref")).encode();
                    let data_to_sign = (
                        SIGNED_MINT_BATCH_NFT_CONTEXT,
                        &context.relayer,
                        &batch_id,
                        index,
                        &bad_external_ref,
                        &context.nft_owner_account,
                    );

                    let call = get_call(&context, &batch_id, &index, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedMintBatchNftTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, index);
                });
            }

            #[test]
            fn mismatched_proof_external_owner() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = MintBatchNftContext::default();
                    context.setup();

                    let index = 0u64;
                    let batch_id = create_batch_and_list();

                    let bad_owner_account = TestAccount::new([71u8; 32]).account_id();
                    let data_to_sign = (
                        SIGNED_MINT_BATCH_NFT_CONTEXT,
                        &context.relayer,
                        &batch_id,
                        index,
                        &context.unique_external_ref,
                        &bad_owner_account,
                    );

                    let call = get_call(&context, &batch_id, &index, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedMintBatchNftTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, index);
                });
            }

            #[test]
            fn mismatched_proof_relayer() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = MintBatchNftContext::default();
                    context.setup();

                    let index = 0u64;
                    let batch_id = create_batch_and_list();

                    let bad_relayer = TestAccount::new([71u8; 32]).account_id();
                    let data_to_sign = (
                        SIGNED_MINT_BATCH_NFT_CONTEXT,
                        &bad_relayer,
                        &batch_id,
                        index,
                        &context.unique_external_ref,
                        &context.nft_owner_account,
                    );

                    let call = get_call(&context, &batch_id, &index, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedMintBatchNftTransaction
                    );

                    // now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, index);
                });
            }
        }
    }
}

struct OpenForSaleContext {
    relayer: AccountId,
    market: NftSaleType,
    creator_account: AccountId,
    creator_key_pair: Pair,
}

impl Default for OpenForSaleContext {
    fn default() -> Self {
        let creator_account = TestAccount::new([1u8; 32]);
        let relayer = TestAccount::new([2u8; 32]);
        OpenForSaleContext {
            creator_account: creator_account.account_id(),
            creator_key_pair: creator_account.key_pair(),
            relayer: relayer.account_id(),
            market: NftSaleType::Fiat,
        }
    }
}

impl OpenForSaleContext {
    fn create_signed_list_batch_for_sale_call(
        &self,
        batch_id: U256,
        nonce: u64,
    ) -> Box<<TestRuntime as Config>::RuntimeCall> {
        let proof = self.create_signed_list_batch_for_sale_proof(batch_id, nonce);

        return Box::new(MockCall::NftManager(
            super::Call::<TestRuntime>::signed_list_batch_for_sale {
                proof,
                batch_id,
                market: self.market,
            },
        ))
    }

    fn create_signed_list_batch_for_sale_proof(
        &self,
        batch_id: U256,
        nonce: u64,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign =
            (SIGNED_LIST_BATCH_FOR_SALE_CONTEXT, &self.relayer, batch_id, self.market, nonce);

        let signature = sign(&self.creator_key_pair, &data_to_sign.encode());
        return build_proof(&self.creator_account, &self.relayer, signature)
    }

    fn batch_listed_for_sale_event_emitted(&self, batch_id: U256) -> bool {
        return System::events().iter().any(|a| {
            a.event ==
                Event::NftManager(crate::Event::<TestRuntime>::BatchOpenForSale {
                    batch_nft_id: batch_id,
                    sale_type: self.market,
                })
        })
    }
}

mod signed_list_batch_for_sale {
    use super::*;

    mod succeeds {
        use super::*;

        #[test]
        fn with_good_parameters_on_ethereum() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = OpenForSaleContext::default();

                let batch_id = create_batch();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                // State changed to listed
                assert_eq!(true, <BatchOpenForSale<TestRuntime>>::contains_key(&batch_id));

                // The market is correctly set
                assert_eq!(<BatchOpenForSale<TestRuntime>>::get(&batch_id), context.market);

                // Correct event is emited
                assert_eq!(true, context.batch_listed_for_sale_event_emitted(batch_id));
            });
        }

        #[test]
        fn with_good_parameters_on_fiat() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = OpenForSaleContext::default();
                context.market = NftSaleType::Fiat;

                let batch_id = create_batch();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                // State changed to listed
                assert_eq!(true, <BatchOpenForSale<TestRuntime>>::contains_key(&batch_id));

                // The market is correctly set
                assert_eq!(<BatchOpenForSale<TestRuntime>>::get(&batch_id), context.market);

                // Correct event is emited
                assert_eq!(true, context.batch_listed_for_sale_event_emitted(batch_id));
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = OpenForSaleContext::default();

                let batch_id = create_batch();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);

                let inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);

                assert_noop!(NftManager::proxy(RawOrigin::None.into(), inner_call), BadOrigin);
            });
        }

        #[test]
        fn batch_id_does_not_exist() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = OpenForSaleContext::default();

                let batch_id = U256::zero() + 1;
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchDoesNotExist
                );
            });
        }

        #[test]
        fn batch_id_is_not_set() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = OpenForSaleContext::default();

                let batch_id = U256::zero();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchIdIsMandatory
                );
            });
        }

        #[test]
        fn total_supply_is_exceeded() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = OpenForSaleContext::default();

                let batch_id = create_batch();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);

                // Manually "mint" nfts and exhaust the total supply
                let info = <NftInfos<TestRuntime>>::get(<BatchInfoId<TestRuntime>>::get(batch_id))
                    .unwrap();
                let mut nft_ids: Vec<U256> = vec![U256::zero()];
                for i in 0..info.total_supply {
                    nft_ids.push(U256::zero() + i);
                }
                <NftBatches<TestRuntime>>::insert(
                    batch_id,
                    BoundedVec::try_from(nft_ids).expect("Batch boundaries were exceeded"),
                );

                // Now try to list the batch for sale
                let inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::NoNftsToSell
                );
            });
        }

        #[test]
        fn market_is_invalid() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = OpenForSaleContext::default();
                context.market = NftSaleType::Unknown;

                let batch_id = create_batch();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::UnsupportedMarket
                );
            });
        }

        #[test]
        fn sender_is_not_batch_creator() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = OpenForSaleContext::default();
                let batch_id = create_batch();

                context.creator_account = TestAccount::new([41u8; 32]).account_id();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::SenderIsNotBatchCreator
                );
            });
        }

        #[test]
        fn batch_already_listed() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = OpenForSaleContext::default();

                let batch_id = create_batch();
                let mut nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let mut inner_call =
                    context.create_signed_list_batch_for_sale_call(batch_id, nonce);

                // Initial listing works
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                //List the same batch again, with the correct nonce
                nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                inner_call = context.create_signed_list_batch_for_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchAlreadyListed
                );
            });
        }

        mod the_proof_is_invalid_because {
            use super::*;

            fn get_call(
                context: &OpenForSaleContext,
                batch_id: &U256,
                data_to_sign: &[u8],
            ) -> Box<MockCall> {
                let signature = sign(&context.creator_key_pair, data_to_sign);
                let proof = build_proof(&context.creator_account, &context.relayer, signature);

                return Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_list_batch_for_sale {
                        proof,
                        batch_id: *batch_id,
                        market: context.market,
                    },
                ))
            }

            fn create_batch_and_assert_success(
                context: &OpenForSaleContext,
                batch_id: U256,
                nonce: u64,
            ) {
                //now show that it will work if we fix the bad data.
                let data_to_sign = (
                    SIGNED_LIST_BATCH_FOR_SALE_CONTEXT,
                    &context.relayer,
                    &batch_id,
                    &context.market,
                    nonce,
                );

                let call = get_call(&context, &batch_id, &data_to_sign.encode());
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call));
                assert_eq!(true, context.batch_listed_for_sale_event_emitted(batch_id));
            }

            #[test]
            fn context_is_bad() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = OpenForSaleContext::default();
                    let batch_id = create_batch();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let other_context: &'static [u8] = b"authorization for something else";
                    let data_to_sign =
                        (other_context, &context.relayer, &batch_id, &context.market, nonce);

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedListBatchForSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, nonce);
                });
            }

            #[test]
            fn mismatched_proof_batch_id() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = OpenForSaleContext::default();
                    let batch_id = create_batch();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let bad_batch_id = batch_id + 5;
                    let data_to_sign = (
                        SIGNED_LIST_BATCH_FOR_SALE_CONTEXT,
                        &context.relayer,
                        &bad_batch_id,
                        &context.market,
                        nonce,
                    );

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedListBatchForSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, nonce);
                });
            }

            #[test]
            fn mismatched_proof_market() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = OpenForSaleContext::default();
                    let batch_id = create_batch();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let bad_market = NftSaleType::Unknown;
                    let data_to_sign = (
                        SIGNED_LIST_BATCH_FOR_SALE_CONTEXT,
                        &context.relayer,
                        &batch_id,
                        &bad_market,
                        nonce,
                    );

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedListBatchForSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, nonce);
                });
            }

            #[test]
            fn mismatched_proof_nonce() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = OpenForSaleContext::default();
                    let batch_id = create_batch();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let bad_nonce = nonce + 99;
                    let data_to_sign = (
                        SIGNED_LIST_BATCH_FOR_SALE_CONTEXT,
                        &context.relayer,
                        &batch_id,
                        &context.market,
                        bad_nonce,
                    );

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedListBatchForSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, nonce);
                });
            }

            #[test]
            fn mismatched_proof_relayer() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = OpenForSaleContext::default();
                    let batch_id = create_batch();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let bad_relayer = TestAccount::new([71u8; 32]).account_id();
                    let data_to_sign = (
                        SIGNED_LIST_BATCH_FOR_SALE_CONTEXT,
                        &bad_relayer,
                        &batch_id,
                        &context.market,
                        nonce,
                    );

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedListBatchForSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    create_batch_and_assert_success(&context, batch_id, nonce);
                });
            }
        }
    }
}

struct EndBatchSaleContext {
    relayer: AccountId,
    market: NftSaleType,
    creator_account: AccountId,
    creator_key_pair: Pair,
}

impl Default for EndBatchSaleContext {
    fn default() -> Self {
        let creator_account = TestAccount::new([1u8; 32]);
        let relayer = TestAccount::new([2u8; 32]);
        EndBatchSaleContext {
            //origin: Origin::signed(creator_account.account_id()),
            creator_account: creator_account.account_id(),
            creator_key_pair: creator_account.key_pair(),
            relayer: relayer.account_id(),
            market: NftSaleType::Fiat,
        }
    }
}

impl EndBatchSaleContext {
    fn create_signed_end_batch_sale_call(
        &self,
        batch_id: U256,
        nonce: u64,
    ) -> Box<<TestRuntime as Config>::RuntimeCall> {
        let proof = self.create_signed_end_batch_sale_proof(batch_id, nonce);

        return Box::new(MockCall::NftManager(super::Call::<TestRuntime>::signed_end_batch_sale {
            proof,
            batch_id,
        }))
    }

    fn create_signed_end_batch_sale_proof(
        &self,
        batch_id: U256,
        nonce: u64,
    ) -> Proof<Signature, AccountId> {
        let data_to_sign = (SIGNED_END_BATCH_SALE_CONTEXT, &self.relayer, batch_id, nonce);

        let signature = sign(&self.creator_key_pair, &data_to_sign.encode());
        return build_proof(&self.creator_account, &self.relayer, signature)
    }

    fn batch_sale_ended_event_emitted(&self, batch_id: U256) -> bool {
        return System::events().iter().any(|a| {
            a.event ==
                Event::NftManager(crate::Event::<TestRuntime>::BatchSaleEnded {
                    batch_nft_id: batch_id,
                    sale_type: self.market,
                })
        })
    }
}

mod signed_end_batch_sale {
    use super::*;

    mod succeeds {
        use super::*;

        #[test]
        fn with_good_parameters() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = EndBatchSaleContext::default();

                let batch_id = create_batch_and_list();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);

                assert_eq!(true, <BatchOpenForSale<TestRuntime>>::contains_key(&batch_id));

                let inner_call = context.create_signed_end_batch_sale_call(batch_id, nonce);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                // State changed to not listed
                assert_eq!(false, <BatchOpenForSale<TestRuntime>>::contains_key(&batch_id));

                // Correct event is emited
                assert_eq!(true, context.batch_sale_ended_event_emitted(batch_id));
            });
        }

        #[test]
        fn when_a_batch_is_relisted_with_good_parameters() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = EndBatchSaleContext::default();

                let batch_id = create_batch_and_list();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);

                // The batch is listed
                assert_eq!(true, <BatchOpenForSale<TestRuntime>>::contains_key(&batch_id));

                let inner_call = context.create_signed_end_batch_sale_call(batch_id, nonce);
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), inner_call));

                // State changed to not listed
                assert_eq!(false, <BatchOpenForSale<TestRuntime>>::contains_key(&batch_id));
                // Correct event is emited
                assert_eq!(true, context.batch_sale_ended_event_emitted(batch_id));

                list_batch(batch_id, NftSaleType::Fiat);

                // The batch is listed again
                assert_eq!(true, <BatchOpenForSale<TestRuntime>>::contains_key(&batch_id));
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = EndBatchSaleContext::default();

                let batch_id = create_batch_and_list();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_end_batch_sale_call(batch_id, nonce);

                assert_noop!(NftManager::proxy(RawOrigin::None.into(), inner_call), BadOrigin);
            });
        }

        #[test]
        fn batch_id_does_not_exist() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = EndBatchSaleContext::default();

                let batch_id = U256::zero() + 1;
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_end_batch_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchDoesNotExist
                );
            });
        }

        #[test]
        fn batch_id_is_not_set() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = EndBatchSaleContext::default();

                let batch_id = U256::zero();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_end_batch_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchIdIsMandatory
                );
            });
        }

        #[test]
        fn sender_is_not_batch_creator() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context = EndBatchSaleContext::default();
                let batch_id = create_batch_and_list();

                context.creator_account = TestAccount::new([41u8; 32]).account_id();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_end_batch_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::SenderIsNotBatchCreator
                );
            });
        }

        #[test]
        fn batch_not_listed() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = EndBatchSaleContext::default();

                // create the batch, but not list it
                let batch_id = create_batch();
                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_end_batch_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchNotListed
                );
            });
        }

        #[test]
        fn batch_listed_for_sale_on_wrong_market() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context = EndBatchSaleContext::default();

                let batch_id = create_batch();

                // List on Etherum. Only fiat listings can be minted via an extrinsic
                list_batch(batch_id, NftSaleType::Ethereum);

                let nonce = <BatchNonces<TestRuntime>>::get(&context.creator_account);
                let inner_call = context.create_signed_end_batch_sale_call(batch_id, nonce);

                assert_noop!(
                    NftManager::proxy(Origin::signed(context.relayer), inner_call),
                    Error::<TestRuntime>::BatchNotListedForFiatSale
                );
            });
        }

        mod the_proof_is_invalid_because {
            use super::*;

            fn get_call(
                context: &EndBatchSaleContext,
                batch_id: &U256,
                data_to_sign: &[u8],
            ) -> Box<MockCall> {
                let signature = sign(&context.creator_key_pair, data_to_sign);
                let proof = build_proof(&context.creator_account, &context.relayer, signature);

                return Box::new(MockCall::NftManager(
                    super::Call::<TestRuntime>::signed_end_batch_sale {
                        proof,
                        batch_id: *batch_id,
                    },
                ))
            }

            fn end_sale_and_assert_success(
                context: &EndBatchSaleContext,
                batch_id: U256,
                nonce: u64,
            ) {
                //now show that it will work if we fix the bad data.
                let data_to_sign =
                    (SIGNED_END_BATCH_SALE_CONTEXT, &context.relayer, &batch_id, nonce);

                let call = get_call(&context, &batch_id, &data_to_sign.encode());
                assert_ok!(NftManager::proxy(Origin::signed(context.relayer), call));
                assert_eq!(true, context.batch_sale_ended_event_emitted(batch_id));
            }

            #[test]
            fn context_is_bad() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = EndBatchSaleContext::default();
                    let batch_id = create_batch_and_list();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let other_context: &'static [u8] = b"authorization for something else";
                    let data_to_sign = (other_context, &context.relayer, &batch_id, nonce);

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedEndBatchSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    end_sale_and_assert_success(&context, batch_id, nonce);
                });
            }

            #[test]
            fn mismatched_proof_batch_id() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = EndBatchSaleContext::default();
                    let batch_id = create_batch_and_list();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let bad_batch_id = batch_id + 5;
                    let data_to_sign =
                        (SIGNED_END_BATCH_SALE_CONTEXT, &context.relayer, &bad_batch_id, nonce);

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedEndBatchSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    end_sale_and_assert_success(&context, batch_id, nonce);
                });
            }

            #[test]
            fn mismatched_proof_nonce() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = EndBatchSaleContext::default();
                    let batch_id = create_batch_and_list();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let bad_nonce = nonce + 99;
                    let data_to_sign =
                        (SIGNED_END_BATCH_SALE_CONTEXT, &context.relayer, &batch_id, bad_nonce);

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedEndBatchSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    end_sale_and_assert_success(&context, batch_id, nonce);
                });
            }

            #[test]
            fn mismatched_proof_relayer() {
                let mut ext = ExtBuilder::build_default().as_externality();
                ext.execute_with(|| {
                    let context = EndBatchSaleContext::default();
                    let batch_id = create_batch_and_list();
                    let nonce = <BatchNonces<TestRuntime>>::get(context.creator_account);

                    let bad_relayer = TestAccount::new([71u8; 32]).account_id();
                    let data_to_sign =
                        (SIGNED_END_BATCH_SALE_CONTEXT, &bad_relayer, &batch_id, nonce);

                    let call = get_call(&context, &batch_id, &data_to_sign.encode());

                    assert_noop!(
                        NftManager::proxy(Origin::signed(context.relayer), call.clone()),
                        Error::<TestRuntime>::UnauthorizedSignedEndBatchSaleTransaction
                    );

                    //now show that it will work if we fix the bad data.
                    end_sale_and_assert_success(&context, batch_id, nonce);
                });
            }
        }
    }
}
