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
use crate::mock::{AccountId, *};
use frame_support::{assert_noop, assert_ok};
use hex_literal::hex;
use mock::RuntimeEvent as Event;
use sp_avn_common::event_types::{EthEventId, ValidEvents};
use sp_core::H256;

mod transfer_eth_nft {
    use super::*;

    #[derive(Clone)]
    struct Context {
        info_id: NftInfoId,
        nft_owner: AccountId,
        new_nft_owner: H256,
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
                new_nft_owner: H256::from([1u8; 32]),
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
                    signature: ValidEvents::NftTransferTo.signature(),
                    transaction_hash: H256::from([1u8; 32]),
                },
            }
        }
    }

    impl Context {
        pub fn nft_transfer_to_event_emitted(&self) -> bool {
            return System::events().iter().any(|a| {
                a.event ==
                    Event::NftManager(crate::Event::<TestRuntime>::EthNftTransfer {
                        nft_id: self.nft_id(),
                        new_owner: self.new_owner_to_account_id(),
                        sale_type: NftSaleType::Ethereum,
                        op_id: self.op_id,
                        eth_event_id: self.event_id.clone(),
                    })
            })
        }

        pub fn list_nft_open_for_sale(&self) {
            NftManager::open_nft_for_sale(&self.nft_id(), &self.market);
        }

        pub fn nft_id(&self) -> NftId {
            return NftManager::generate_nft_id_single_mint(&self.t1_authority, self.unique_id)
        }

        pub fn inject_nft_to_chain(&self) -> (Nft<AccountId>, NftInfo<AccountId>) {
            return NftManager::insert_single_nft_into_chain(
                self.info_id,
                self.bounded_royalties(),
                self.t1_authority,
                self.nft_id(),
                self.bounded_external_ref(),
                self.nft_owner,
            )
        }

        pub fn new_owner_to_account_id(&self) -> AccountId {
            return AccountId::decode(&mut self.new_nft_owner.as_bytes()).expect("should succeed")
        }

        pub fn transfer_to_nft_data(&self) -> NftTransferToData {
            return NftTransferToData {
                nft_id: self.nft_id(),
                t2_transfer_to_public_key: self.new_nft_owner.clone(),
                op_id: self.op_id,
            }
        }

        pub fn setup_valid_transfer(&self) {
            insert_to_mock_processed_events(&self.event_id);
            self.inject_nft_to_chain();
            self.list_nft_open_for_sale();
        }

        pub fn perform_transfer(&self) -> DispatchResult {
            return NftManager::transfer_eth_nft(&self.event_id, &self.transfer_to_nft_data())
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
        fn nft_is_open_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.setup_valid_transfer();
                assert_ok!(context.perform_transfer());
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
                context.setup_valid_transfer();
                assert_ok!(context.perform_transfer());

                assert_eq!(true, context.nft_transfer_to_event_emitted());
            });
        }

        #[test]
        fn nft_is_not_listed_for_sale_anymore() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.setup_valid_transfer();
                assert_ok!(context.perform_transfer());

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
                context.setup_valid_transfer();
                let original_nonce = NftManager::nfts(context.nft_id()).unwrap().nonce;
                assert_ok!(context.perform_transfer());

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
                    context.perform_transfer(),
                    Error::<TestRuntime>::NftNotListedForEthereumSale
                );
            });
        }

        #[test]
        fn nft_does_not_exist() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                insert_to_mock_processed_events(&context.event_id);

                assert_noop!(
                    context.perform_transfer(),
                    Error::<TestRuntime>::NftNotListedForEthereumSale
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
                    context.perform_transfer(),
                    Error::<TestRuntime>::NoTier1EventForNftOperation
                );
            });
        }
    }
}
