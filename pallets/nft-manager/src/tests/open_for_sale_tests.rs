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
use crate::mock::{AccountId, RuntimeEvent as Event, RuntimeOrigin as Origin, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use hex_literal::hex;
use sp_runtime::traits::BadOrigin;

mod open_for_sale {
    use super::*;

    #[derive(Clone)]
    struct Context {
        origin: Origin,
        info_id: NftInfoId,
        nft_owner: AccountId,
        market: NftSaleType,
        unique_id: NftUniqueId,
        royalties: Vec<Royalty>,
        t1_authority: H160,
        unique_external_ref: Vec<u8>,
    }

    impl Default for Context {
        fn default() -> Self {
            let nft_owner = TestAccount::new([1u8; 32]);
            Context {
                origin: Origin::signed(nft_owner.account_id()),
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
            }
        }
    }

    impl Context {
        pub fn open_for_sale_event_emitted(&self) -> bool {
            return System::events().iter().any(|a| {
                a.event
                    == Event::NftManager(crate::Event::<TestRuntime>::NftOpenForSale {
                        nft_id: self.nft_id(),
                        sale_type: self.market.clone(),
                    })
            });
        }

        pub fn call_list_nft_open_for_sale(&self) -> DispatchResult {
            return NftManager::list_nft_open_for_sale(
                self.origin.clone().into(),
                self.nft_id(),
                self.market.clone(),
            );
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
        fn origin_is_signed() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.inject_nft_to_chain();

                assert_ok!(context.call_list_nft_open_for_sale());
            });
        }

        #[test]
        fn nft_is_not_open_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.inject_nft_to_chain();

                assert_eq!(
                    NftSaleType::Unknown,
                    NftManager::get_nft_open_for_sale_on(context.nft_id())
                );
                assert_ok!(context.call_list_nft_open_for_sale());
            });
        }
    }

    mod when_successful_then {
        use super::*;

        #[test]
        fn event_is_emitted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.inject_nft_to_chain();

                assert_ok!(context.call_list_nft_open_for_sale());
                assert_eq!(true, context.open_for_sale_event_emitted());
            });
        }

        #[test]
        fn nft_sale_type_is_updated() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.inject_nft_to_chain();

                assert_ok!(context.call_list_nft_open_for_sale());
                assert_eq!(
                    NftSaleType::Ethereum,
                    NftManager::get_nft_open_for_sale_on(context.nft_id())
                );
            });
        }

        #[test]
        fn nft_nonce_is_increased() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.inject_nft_to_chain();

                let original_nonce = NftManager::nfts(context.nft_id()).unwrap().nonce;
                assert_ok!(context.call_list_nft_open_for_sale());
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
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context =
                    Context { origin: RawOrigin::None.into(), ..Default::default() };
                context.inject_nft_to_chain();

                assert_noop!(context.call_list_nft_open_for_sale(), BadOrigin);
            });
        }

        #[test]
        fn nft_already_listed_for_sale() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.inject_nft_to_chain();

                <NftOpenForSale<TestRuntime>>::insert(context.nft_id(), NftSaleType::Ethereum);

                assert_noop!(
                    context.call_list_nft_open_for_sale(),
                    Error::<TestRuntime>::NftAlreadyListed
                );
            });
        }

        #[test]
        fn sender_is_not_the_owner() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let mut context: Context = Default::default();
                context.inject_nft_to_chain();

                let not_nft_owner_account = TestAccount::new([4u8; 32]);
                context.origin = Origin::signed(not_nft_owner_account.account_id());

                assert_noop!(
                    context.call_list_nft_open_for_sale(),
                    Error::<TestRuntime>::SenderIsNotOwner
                );
            });
        }

        #[test]
        fn nft_does_not_exists() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();

                assert_noop!(
                    context.call_list_nft_open_for_sale(),
                    Error::<TestRuntime>::NftIdDoesNotExist
                );
            });
        }

        #[test]
        fn nft_is_locked() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                context.inject_nft_to_chain();

                <Nfts<TestRuntime>>::mutate(context.nft_id(), |maybe_nft| {
                    maybe_nft.as_mut().map(|nft| nft.is_locked = true)
                });

                assert_noop!(
                    context.call_list_nft_open_for_sale(),
                    Error::<TestRuntime>::NftIsLocked
                );
            });
        }
    }
}
