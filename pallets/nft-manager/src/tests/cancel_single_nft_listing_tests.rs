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
use crate::mock::{AccountId, RuntimeEvent as Event, *};
use frame_support::{assert_noop, assert_ok};
use hex_literal::hex;
use sp_avn_common::event_types::{EthEventId, ValidEvents};
use sp_core::H256;

mod cancel_single_nft_listing {
    use super::*;

    #[derive(Clone)]
    struct Context {
        info_id: NftInfoId,
        nft_owner: AccountId,
        market: NftSaleType,
        unique_id: NftUniqueId,
        royalties: Vec<Royalty>,
        t1_authority: H160,
        unique_external_ref: Vec<u8>,
        op_id: u64,
        event_id: EthEventId,
    }

    impl Default for Context {
        fn default() -> Self {
            let nft_owner = TestAccount::new([1u8; 32]);
            Context {
                nft_owner: nft_owner.account_id(),
                royalties: vec![Royalty {
                    recipient_t1_address: H160(hex!("33333CCCCC44444DDDDD33333CCCCC44444DDDDD")),
                    rate: RoyaltyRate { parts_per_million: 100 },
                }],
                unique_id: NftManager::next_unique_id(),
                market: NftSaleType::Ethereum,
                info_id: NftManager::get_info_id_and_advance(),
                t1_authority: H160(hex!("11111AAAAA22222BBBBB11111AAAAA22222BBBBB")),
                unique_external_ref: String::from("Offchain location of NFT").into_bytes(),
                op_id: 1u64,
                event_id: EthEventId {
                    signature: ValidEvents::NftCancelListing.signature(),
                    transaction_hash: H256::from([1u8; 32]),
                },
            }
        }
    }

    impl Context {
        pub fn cancel_single_nft_listing_event_emitted(&self) -> bool {
            return System::events().iter().any(|a| {
                a.event
                    == Event::NftManager(crate::Event::<TestRuntime>::CancelSingleEthNftListing {
                        nft_id: self.nft_id(),
                        sale_type: NftSaleType::Ethereum,
                        op_id: self.op_id,
                        eth_event_id: self.event_id.clone(),
                    })
            });
        }

        pub fn list_nft_open_for_sale(&self) {
            NftManager::open_nft_for_sale(&self.nft_id(), &self.market);
        }

        pub fn nft_id(&self) -> NftId {
            return NftManager::generate_nft_id_single_mint(&self.t1_authority, self.unique_id);
        }

        pub fn inject_nft_to_chain(&self) -> (Nft<AccountId>, NftInfo<AccountId>) {
            return NftManager::insert_single_nft_into_chain(
                self.info_id,
                self.bounded_royalties(),
                self.t1_authority,
                self.nft_id(),
                self.bounded_external_ref(),
                self.nft_owner,
            );
        }

        pub fn cancel_nft_listing_data(&self) -> NftCancelListingData {
            return NftCancelListingData { nft_id: self.nft_id(), op_id: self.op_id };
        }

        pub fn bounded_external_ref(&self) -> BoundedVec<u8, NftExternalRefBound> {
            BoundedVec::try_from(self.unique_external_ref.clone())
                .expect("Unique external reference bound was exceeded.")
        }

        pub fn bounded_royalties(&self) -> BoundedVec<Royalty, NftRoyaltiesBound> {
            BoundedVec::try_from(self.royalties.clone()).expect("Royalty bound was exceeded.")
        }
    }

    mod success_preconditions {
        use super::*;
        #[test]
        fn when_nft_is_open_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                insert_to_mock_processed_events(&context.event_id);
                context.inject_nft_to_chain();
                context.list_nft_open_for_sale();

                assert_ok!(NftManager::cancel_eth_nft_listing(
                    &context.event_id,
                    &context.cancel_nft_listing_data(),
                ));
            });
        }
    }

    mod when_successful {
        use super::*;

        #[test]
        fn event_is_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                insert_to_mock_processed_events(&context.event_id);
                context.inject_nft_to_chain();
                context.list_nft_open_for_sale();

                assert_ok!(NftManager::cancel_eth_nft_listing(
                    &context.event_id,
                    &context.cancel_nft_listing_data(),
                ));
                assert_eq!(true, context.cancel_single_nft_listing_event_emitted());
            });
        }

        #[test]
        fn nft_sale_type_is_updated() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                insert_to_mock_processed_events(&context.event_id);
                context.inject_nft_to_chain();
                context.list_nft_open_for_sale();

                assert_ok!(NftManager::cancel_eth_nft_listing(
                    &context.event_id,
                    &context.cancel_nft_listing_data(),
                ));
                assert_eq!(
                    NftSaleType::Unknown,
                    NftManager::get_nft_open_for_sale_on(context.nft_id())
                );
            });
        }

        #[test]
        fn nft_nonce_is_increased() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                insert_to_mock_processed_events(&context.event_id);
                context.inject_nft_to_chain();
                context.list_nft_open_for_sale();

                let original_nonce = NftManager::nfts(context.nft_id()).unwrap().nonce;
                assert_ok!(NftManager::cancel_eth_nft_listing(
                    &context.event_id,
                    &context.cancel_nft_listing_data(),
                ));
                assert_eq!(
                    original_nonce + 1u64,
                    NftManager::nfts(context.nft_id()).unwrap().nonce
                );
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn nft_is_not_listed_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                insert_to_mock_processed_events(&context.event_id);
                context.inject_nft_to_chain();

                assert_noop!(
                    NftManager::cancel_eth_nft_listing(
                        &context.event_id,
                        &context.cancel_nft_listing_data()
                    ),
                    Error::<TestRuntime>::NftNotListedForEthereumSale
                );
            });
        }

        #[test]
        fn nft_does_not_exists() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                insert_to_mock_processed_events(&context.event_id);

                assert_noop!(
                    NftManager::cancel_eth_nft_listing(
                        &context.event_id,
                        &context.cancel_nft_listing_data()
                    ),
                    Error::<TestRuntime>::NftIdDoesNotExist
                );
            });
        }

        #[test]
        fn event_id_is_not_in_processed_events() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.inject_nft_to_chain();
                context.list_nft_open_for_sale();

                assert_noop!(
                    NftManager::cancel_eth_nft_listing(
                        &context.event_id,
                        &context.cancel_nft_listing_data()
                    ),
                    Error::<TestRuntime>::NoTier1EventForNftOperation
                );
            });
        }
    }
}
