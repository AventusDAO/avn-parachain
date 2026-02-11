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

use super::*;
use crate::{
    mock::{AccountId, RuntimeEvent as Event, RuntimeOrigin as Origin, *},
    nft_data::ROYALTY_RATE_DENOMINATOR,
};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use hex_literal::hex;
use sp_runtime::traits::BadOrigin;

mod mint_single_nft {
    use super::*;

    #[derive(Clone)]
    struct Context {
        origin: Origin,
        unique_external_ref: Vec<u8>,
        owner: AccountId,
        royalties: Vec<Royalty>,
        t1_authority: H160,
        unique_id: NftUniqueId,
    }

    impl Default for Context {
        fn default() -> Self {
            let default_minter = TestAccount::new([0u8; 32]);
            Context {
                origin: Origin::signed(default_minter.account_id()),
                unique_external_ref: String::from("Offchain location of NFT").into_bytes(),
                owner: default_minter.account_id(),
                royalties: vec![Royalty {
                    recipient_t1_address: H160(hex!("33333CCCCC44444DDDDD33333CCCCC44444DDDDD")),
                    rate: RoyaltyRate { parts_per_million: 100 },
                }],
                t1_authority: H160(hex!("11111AAAAA22222BBBBB11111AAAAA22222BBBBB")),
                unique_id: NftManager::next_unique_id(),
            }
        }
    }

    impl Context {
        pub fn event_emitted_with_single_nft_minted(&self) -> bool {
            return System::events().iter().any(|a| {
                a.event ==
                    Event::NftManager(crate::Event::<TestRuntime>::SingleNftMinted {
                        nft_id: self.generate_nft_id(),
                        owner: self.owner,
                        authority: self.t1_authority,
                    })
            })
        }

        pub fn call_mint_single_nft(&self) -> DispatchResult {
            return NftManager::mint_single_nft(
                self.origin.clone().into(),
                self.unique_external_ref.clone(),
                self.royalties.clone(),
                self.t1_authority,
            )
        }

        pub fn generate_nft_id(&self) -> NftId {
            return NftManager::generate_nft_id_single_mint(&self.t1_authority, self.unique_id)
        }
    }

    mod succeeds_when {
        use super::*;

        #[test]
        fn input_is_correct() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();

                let expected_info_id: NftInfoId = NftManager::next_info_id();
                let nft_id = context.generate_nft_id();

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&nft_id));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&expected_info_id));
                assert_eq!(false, context.event_emitted_with_single_nft_minted());

                assert_ok!(context.call_mint_single_nft());

                assert_eq!(true, <Nfts<TestRuntime>>::contains_key(&nft_id));
                assert_eq!(true, <NftInfos<TestRuntime>>::contains_key(&expected_info_id));
                assert_eq!(true, context.event_emitted_with_single_nft_minted());
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn sender_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context =
                    Context { origin: RawOrigin::None.into(), ..Default::default() };
                let expected_info_id: NftInfoId = NftManager::next_info_id();

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.generate_nft_id()));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&expected_info_id));
                assert_eq!(false, context.event_emitted_with_single_nft_minted());

                assert_noop!(context.call_mint_single_nft(), BadOrigin);
                assert_eq!(false, context.event_emitted_with_single_nft_minted());
            });
        }

        // TODO This test might not be relevant anymore if we don't want to use t1 contracts in the
        // generation.
        #[ignore]
        #[test]
        fn minter_t1_address_is_missing() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                let expected_info_id: NftInfoId = NftManager::next_info_id();

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.generate_nft_id()));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&expected_info_id));
                assert_eq!(false, context.event_emitted_with_single_nft_minted());

                assert_noop!(
                    context.call_mint_single_nft(),
                    Error::<TestRuntime>::T1AuthorityIsMandatory
                );
                assert_eq!(false, context.event_emitted_with_single_nft_minted());
            });
        }

        #[test]
        fn external_ref_is_missing() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context: Context = Default::default();
                let expected_info_id: NftInfoId = NftManager::next_info_id();

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.generate_nft_id()));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&expected_info_id));
                assert_eq!(false, context.event_emitted_with_single_nft_minted());

                context.unique_external_ref = Vec::<u8>::new();

                assert_noop!(
                    context.call_mint_single_nft(),
                    Error::<TestRuntime>::ExternalRefIsMandatory
                );
                assert_eq!(false, context.event_emitted_with_single_nft_minted());
            });
        }

        #[test]
        fn external_ref_is_out_of_bounds() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context: Context = Default::default();
                let expected_info_id: NftInfoId = NftManager::next_info_id();

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.generate_nft_id()));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&expected_info_id));
                assert_eq!(false, context.event_emitted_with_single_nft_minted());

                let out_of_bounds_size: u32 = <NftExternalRefBound as sp_core::Get<u32>>::get() + 1;
                context.unique_external_ref = vec![b'A'; out_of_bounds_size as usize];

                assert_noop!(
                    context.call_mint_single_nft(),
                    Error::<TestRuntime>::ExternalRefOutOfBounds
                );
                assert_eq!(false, context.event_emitted_with_single_nft_minted());
            });
        }

        #[test]
        fn external_ref_is_taken() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();

                let new_token_context = Context {
                    unique_external_ref: context.unique_external_ref.clone(),
                    ..Default::default()
                };

                assert_ok!(context.call_mint_single_nft());
                assert_noop!(
                    new_token_context.call_mint_single_nft(),
                    Error::<TestRuntime>::ExternalRefIsAlreadyInUse
                );
            });
        }

        #[test]
        fn royalty_rate_is_invalid() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context: Context = Default::default();
                let expected_info_id: NftInfoId = NftManager::next_info_id();

                assert_eq!(false, <Nfts<TestRuntime>>::contains_key(&context.generate_nft_id()));
                assert_eq!(false, <NftInfos<TestRuntime>>::contains_key(&expected_info_id));
                assert_eq!(false, context.event_emitted_with_single_nft_minted());

                context.royalties = vec![Royalty {
                    recipient_t1_address: H160(hex!("33333CCCCC44444DDDDD33333CCCCC44444DDDDD")),
                    rate: RoyaltyRate { parts_per_million: ROYALTY_RATE_DENOMINATOR + 1 },
                }];

                assert_noop!(
                    context.call_mint_single_nft(),
                    Error::<TestRuntime>::RoyaltyRateIsNotValid
                );
                assert_eq!(false, context.event_emitted_with_single_nft_minted());
            });
        }
    }
}
