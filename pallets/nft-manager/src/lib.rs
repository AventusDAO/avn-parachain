// This file is part of Aventus.
// Copyright (C) 2022 Aventus Network Services (UK) Ltd.

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

//! # nft-manager pallet

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode, MaxEncodedLen};
use core::convert::TryInto;
use frame_support::{
    dispatch::{
        DispatchErrorWithPostInfo, DispatchResult, DispatchResultWithPostInfo, PostDispatchInfo,
    },
    ensure,
    pallet_prelude::StorageVersion,
    traits::{Get, IsSubType},
    weights::Weight,
    Parameter,
};
use frame_system::ensure_signed;
use pallet_avn::{self as avn, BridgeInterfaceNotification, ProcessedEventsChecker};
use sp_avn_common::{
    event_types::{
        EthEvent, EthEventId, EventData, NftCancelListingData, NftEndBatchListingData,
        NftTransferToData, ProcessedEventHandler,
    },
    verify_signature, CallDecoder, InnerCallValidator, Proof,
};
use sp_core::{ConstU32, H160, H256, U256};
use sp_io::hashing::keccak_256;
use sp_runtime::{
    scale_info::TypeInfo,
    traits::{Dispatchable, Hash, IdentifyAccount, Member, Verify},
    BoundedVec,
};
use sp_std::prelude::*;

pub use pallet::*;

pub mod nft_data;
use crate::nft_data::*;

pub mod batch_nft;
use crate::batch_nft::*;

pub mod default_weights;
pub use default_weights::WeightInfo;

const SINGLE_NFT_ID_CONTEXT: &'static [u8; 1] = b"A";
const BATCH_NFT_ID_CONTEXT: &'static [u8; 1] = b"B";
const BATCH_ID_CONTEXT: &'static [u8; 1] = b"G";
pub const SIGNED_MINT_SINGLE_NFT_CONTEXT: &'static [u8] =
    b"authorization for mint single nft operation";
pub const SIGNED_LIST_NFT_OPEN_FOR_SALE_CONTEXT: &'static [u8] =
    b"authorization for list nft open for sale operation";
pub const SIGNED_TRANSFER_FIAT_NFT_CONTEXT: &'static [u8] =
    b"authorization for transfer fiat nft operation";
pub const SIGNED_CANCEL_LIST_FIAT_NFT_CONTEXT: &'static [u8] =
    b"authorization for cancel list fiat nft for sale operation";
pub const SIGNED_MINT_BATCH_NFT_CONTEXT: &'static [u8] =
    b"authorization for mint batch nft operation";

const MAX_NUMBER_OF_ROYALTIES: u32 = 16;
/// Bound used for number of Royalties an NFTs that can have
pub(crate) type NftRoyaltiesBound = ConstU32<MAX_NUMBER_OF_ROYALTIES>;

pub type NftId = U256;
pub type NftInfoId = U256;
pub type NftBatchId = U256;
pub type NftUniqueId = U256;

/// Suggested bound to use in runtime for number of NFTs that can exist in a single Batch
pub type BatchNftBound = ConstU32<16384>;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{pallet_prelude::*, Blake2_128Concat};
    use frame_system::pallet_prelude::*;

    // Public interface of this pallet
    #[pallet::config]
    pub trait Config: frame_system::Config + avn::Config {
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + IsSubType<Call<Self>>
            + From<Call<Self>>;

        type ProcessedEventsChecker: ProcessedEventsChecker;

        /// A type that can be used to verify signatures
        type Public: IdentifyAccount<AccountId = Self::AccountId>;

        /// The signature type used by accounts/transactions.
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;

        type WeightInfo: WeightInfo;

        #[pallet::constant]
        type BatchBound: Get<u32>;
    }

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub _phantom: sp_std::marker::PhantomData<T>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self { _phantom: Default::default() }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            crate::STORAGE_VERSION.put::<Pallet<T>>();
        }
    }

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::event]
    /// This attribute generate the function `deposit_event` to deposit one of this pallet event,
    /// it is optional, it is also possible to provide a custom implementation.
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        SingleNftMinted {
            nft_id: NftId,
            owner: T::AccountId,
            authority: H160,
        },
        ///nft_id, batch_id, provenance, owner
        BatchNftMinted {
            nft_id: NftId,
            batch_nft_id: NftBatchId,
            authority: H160,
            owner: T::AccountId,
        },
        /// nft_id, sale_type
        NftOpenForSale {
            nft_id: NftId,
            sale_type: NftSaleType,
        },
        /// batch_id, sale_type
        BatchOpenForSale {
            batch_nft_id: NftBatchId,
            sale_type: NftSaleType,
        },
        /// EthNftTransfer(NftId, NewOwnerAccountId, NftSaleType, u64, EthEventId),
        EthNftTransfer {
            nft_id: NftId,
            new_owner: T::AccountId,
            sale_type: NftSaleType,
            op_id: u64,
            eth_event_id: EthEventId,
        },
        /// FiatNftTransfer(NftId, SenderAccountId, NewOwnerAccountId, NftSaleType, NftNonce)
        FiatNftTransfer {
            nft_id: NftId,
            sender: T::AccountId,
            new_owner: T::AccountId,
            sale_type: NftSaleType,
            op_id: u64,
        },
        /// CancelSingleEthNftListing(NftId, NftSaleType, u64, EthEventId),
        CancelSingleEthNftListing {
            nft_id: NftId,
            sale_type: NftSaleType,
            op_id: u64,
            eth_event_id: EthEventId,
        },
        /// CancelSingleFiatNftListing(NftId, NftSaleType, NftNonce)
        CancelSingleFiatNftListing {
            nft_id: NftId,
            sale_type: NftSaleType,
            op_id: u64,
        },
        ///Call dispatched by `relayer` with `hash`
        CallDispatched {
            relayer: T::AccountId,
            hash: T::Hash,
        },
        ///batch_id, total_supply, batch_creator, provenance
        BatchCreated {
            batch_nft_id: NftBatchId,
            total_supply: u64,
            batch_creator: T::AccountId,
            authority: H160,
        },
        /// batch_id, market
        BatchSaleEnded {
            batch_nft_id: NftBatchId,
            sale_type: NftSaleType,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        NftAlreadyExists,
        /// When specifying rates, parts_per_million must not be greater than 1 million
        RoyaltyRateIsNotValid,
        /// When specifying rates, sum of parts_per_millions must not be greater than 1 million
        TotalRoyaltyRateIsNotValid,
        T1AuthorityIsMandatory,
        ExternalRefIsMandatory,
        /// The external reference is already used
        ExternalRefIsAlreadyInUse,
        /// There is not data associated with an nftInfoId
        NftInfoMissing,
        NftIdDoesNotExist,
        UnsupportedMarket,
        /// Signed extrinsic with a proof must be called by the signer of the proof
        SenderIsNotSigner,
        SenderIsNotOwner,
        NftAlreadyListed,
        NftIsLocked,
        NftNotListedForSale,
        NftNotListedForEthereumSale,
        NftNotListedForFiatSale,
        NoTier1EventForNftOperation,
        /// The op_id did not match the nft token nonce for the operation
        NftNonceMismatch,
        UnauthorizedTransaction,
        UnauthorizedProxyTransaction,
        UnauthorizedSignedLiftNftOpenForSaleTransaction,
        UnauthorizedSignedMintSingleNftTransaction,
        UnauthorizedSignedTransferFiatNftTransaction,
        UnauthorizedSignedCancelListFiatNftTransaction,
        TransactionNotSupported,
        TransferToIsMandatory,
        UnauthorizedSignedCreateBatchTransaction,
        BatchAlreadyExists,
        TotalSupplyZero,
        UnauthorizedSignedMintBatchNftTransaction,
        BatchIdIsMandatory,
        BatchDoesNotExist,
        SenderIsNotBatchCreator,
        TotalSupplyExceeded,
        UnauthorizedSignedListBatchForSaleTransaction,
        BatchAlreadyListed,
        NoNftsToSell,
        BatchNotListed,
        UnauthorizedSignedEndBatchSaleTransaction,
        BatchNotListedForFiatSale,
        BatchNotListedForEthereumSale,
        /// External reference size is out of bounds
        ExternalRefOutOfBounds,
        /// Nft Royalties size is out of bounds
        RoyaltiesOutOfBounds,
        /// Batch size is out of bounds
        BatchOutOfBounds,
    }

    /// A mapping between NFT Id and data
    #[pallet::storage]
    #[pallet::getter(fn nfts)]
    pub type Nfts<T: Config> =
        StorageMap<_, Blake2_128Concat, NftId, Nft<T::AccountId>, OptionQuery>;

    /// A mapping between NFT info Id and info data
    #[pallet::storage]
    #[pallet::getter(fn nft_infos)]
    pub type NftInfos<T: Config> =
        StorageMap<_, Blake2_128Concat, NftInfoId, NftInfo<T::AccountId>, OptionQuery>;

    /// A mapping between the external batch id and its nft Ids
    #[pallet::storage]
    #[pallet::getter(fn nft_batches)]
    pub type NftBatches<T: Config> =
        StorageMap<_, Blake2_128Concat, NftBatchId, BoundedVec<NftId, T::BatchBound>, ValueQuery>;

    /// A mapping between the external batch id and its corresponding NtfInfoId
    #[pallet::storage]
    #[pallet::getter(fn batch_info_id)]
    pub type BatchInfoId<T: Config> =
        StorageMap<_, Blake2_128Concat, NftBatchId, NftInfoId, ValueQuery>;

    /// A mapping between an ExternalRef and a flag to show that an NFT has used it
    #[pallet::storage]
    #[pallet::getter(fn is_external_ref_used)]
    pub type UsedExternalReferences<T: Config> =
        StorageMap<_, Blake2_128Concat, BoundedVec<u8, NftExternalRefBound>, bool, ValueQuery>;

    /// The Id that will be used when creating the new NftInfo record
    #[pallet::storage]
    #[pallet::getter(fn next_info_id)]
    pub type NextInfoId<T: Config> = StorageValue<_, NftInfoId, ValueQuery>;

    /// The Id that will be used when creating the new single Nft
    //TODO: Rename this item because its not just used for single NFTs
    #[pallet::storage]
    #[pallet::getter(fn next_unique_id)]
    pub type NextSingleNftUniqueId<T: Config> = StorageValue<_, U256, ValueQuery>;

    /// A mapping that keeps all the nfts that are open to sale in a specific market
    #[pallet::storage]
    #[pallet::getter(fn get_nft_open_for_sale_on)]
    pub type NftOpenForSale<T: Config> =
        StorageMap<_, Blake2_128Concat, NftId, NftSaleType, ValueQuery>;

    /// An account nonce that represents the number of proxy transactions from this account
    #[pallet::storage]
    #[pallet::getter(fn batch_nonce)]
    pub type BatchNonces<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u64, ValueQuery>;

    /// A mapping that keeps all the batches that are open to sale in a specific market
    #[pallet::storage]
    #[pallet::getter(fn get_batch_sale_market)]
    pub type BatchOpenForSale<T: Config> =
        StorageMap<_, Blake2_128Concat, NftBatchId, NftSaleType, ValueQuery>;

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Mint a single NFT
        #[pallet::call_index(0)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::mint_single_nft(MAX_NUMBER_OF_ROYALTIES))]
        pub fn mint_single_nft(
            origin: OriginFor<T>,
            unique_external_ref: Vec<u8>,
            royalties: Vec<Royalty>,
            t1_authority: H160,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;

            let bounded_unique_external_ref =
                BoundedVec::<u8, NftExternalRefBound>::try_from(unique_external_ref)
                    .map_err(|_| Error::<T>::ExternalRefOutOfBounds)?;
            Self::validate_mint_single_nft_request(
                &bounded_unique_external_ref,
                &royalties,
                t1_authority,
            )?;

            // We trust the input for the value of t1_authority
            let nft_id =
                Self::generate_nft_id_single_mint(&t1_authority, Self::get_unique_id_and_advance());
            ensure!(Nfts::<T>::contains_key(&nft_id) == false, Error::<T>::NftAlreadyExists);

            // No errors allowed after this point because `get_info_id_and_advance` mutates storage
            let info_id = Self::get_info_id_and_advance();
            let (nft, info) = Self::insert_single_nft_into_chain(
                info_id,
                BoundedVec::try_from(royalties).map_err(|_| Error::<T>::RoyaltiesOutOfBounds)?,
                t1_authority,
                nft_id,
                bounded_unique_external_ref,
                sender,
            );

            Self::deposit_event(Event::<T>::SingleNftMinted {
                nft_id: nft.nft_id,
                owner: nft.owner,
                authority: info.t1_authority,
            });

            Ok(())
        }

        /// Mint a single NFT signed by nft owner
        #[pallet::call_index(1)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_mint_single_nft(MAX_NUMBER_OF_ROYALTIES))]
        pub fn signed_mint_single_nft(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            unique_external_ref: Vec<u8>,
            royalties: Vec<Royalty>,
            t1_authority: H160,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            let bounded_unique_external_ref =
                BoundedVec::<u8, NftExternalRefBound>::try_from(unique_external_ref.clone())
                    .map_err(|_| Error::<T>::ExternalRefOutOfBounds)?;
            Self::validate_mint_single_nft_request(
                &bounded_unique_external_ref,
                &royalties,
                t1_authority,
            )?;

            let signed_payload = Self::encode_mint_single_nft_params(
                &proof,
                &unique_external_ref,
                &royalties,
                &t1_authority,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedMintSingleNftTransaction
            );

            // We trust the input for the value of t1_authority
            let nft_id =
                Self::generate_nft_id_single_mint(&t1_authority, Self::get_unique_id_and_advance());
            ensure!(Nfts::<T>::contains_key(&nft_id) == false, Error::<T>::NftAlreadyExists);

            // No errors allowed after this point because `get_info_id_and_advance` mutates storage
            let info_id = Self::get_info_id_and_advance();
            let (nft, info) = Self::insert_single_nft_into_chain(
                info_id,
                BoundedVec::try_from(royalties).map_err(|_| Error::<T>::RoyaltiesOutOfBounds)?,
                t1_authority,
                nft_id,
                bounded_unique_external_ref,
                proof.signer,
            );

            Self::deposit_event(Event::<T>::SingleNftMinted {
                nft_id: nft.nft_id,
                owner: nft.owner,
                authority: info.t1_authority,
            });

            Ok(())
        }

        /// List an nft open for sale
        #[pallet::call_index(2)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::list_nft_open_for_sale())]
        pub fn list_nft_open_for_sale(
            origin: OriginFor<T>,
            nft_id: NftId,
            market: NftSaleType,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            Self::validate_open_for_sale_request(sender, nft_id, market.clone())?;
            Self::open_nft_for_sale(&nft_id, &market);
            Self::deposit_event(Event::<T>::NftOpenForSale { nft_id, sale_type: market });
            Ok(())
        }

        /// List an nft open for sale by a relayer
        #[pallet::call_index(3)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_list_nft_open_for_sale())]
        pub fn signed_list_nft_open_for_sale(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            nft_id: NftId,
            market: NftSaleType,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            Self::validate_open_for_sale_request(sender, nft_id, market.clone())?;

            let signed_payload = Self::encode_list_nft_for_sale_params(&proof, &nft_id, &market)?;
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedLiftNftOpenForSaleTransaction
            );

            Self::open_nft_for_sale(&nft_id, &market);
            Self::deposit_event(Event::<T>::NftOpenForSale { nft_id, sale_type: market });

            Ok(())
        }

        /// Transfer a nft open for sale on fiat market to a new owner by a relayer
        #[pallet::call_index(4)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_transfer_fiat_nft())]
        pub fn signed_transfer_fiat_nft(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            nft_id: U256,
            t2_transfer_to_public_key: H256,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            ensure!(
                t2_transfer_to_public_key.is_zero() == false,
                Error::<T>::TransferToIsMandatory
            );
            Self::validate_nft_open_for_fiat_sale(sender.clone(), nft_id)?;

            let nft = Self::try_get_nft(&nft_id)?;
            let signed_payload =
                Self::encode_transfer_fiat_nft_params(&proof, &nft_id, &t2_transfer_to_public_key)?;
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedTransferFiatNftTransaction
            );

            let new_nft_owner = T::AccountId::decode(&mut t2_transfer_to_public_key.as_bytes())
                .expect("32 bytes will always decode into an AccountId");
            let market = Self::get_nft_open_for_sale_on(nft_id);

            Self::transfer_nft(&nft_id, &new_nft_owner.clone())?;
            Self::deposit_event(Event::<T>::FiatNftTransfer {
                nft_id,
                sender,
                new_owner: new_nft_owner,
                sale_type: market,
                op_id: nft.nonce,
            });

            Ok(())
        }

        /// Cancel a nft open for sale on fiat market by a relayer
        #[pallet::call_index(5)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_cancel_list_fiat_nft())]
        pub fn signed_cancel_list_fiat_nft(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            nft_id: U256,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            Self::validate_nft_open_for_fiat_sale(sender.clone(), nft_id)?;

            let nft = Self::try_get_nft(&nft_id)?;
            let signed_payload = Self::encode_cancel_list_fiat_nft_params(&proof, &nft_id)?;
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedCancelListFiatNftTransaction
            );

            let market = Self::get_nft_open_for_sale_on(nft_id);

            Self::unlist_nft_for_sale(nft_id)?;
            Self::deposit_event(Event::<T>::CancelSingleFiatNftListing {
                nft_id,
                sale_type: market,
                op_id: nft.nonce,
            });

            Ok(())
        }

        /// This extrinsic allows a relayer to dispatch a call from this pallet for a sender.
        /// Currently only `signed_list_nft_open_for_sale` is allowed
        ///
        /// As a general rule, every function that can be proxied should follow this convention:
        /// - its first argument (after origin) should be a public verification key and a signature
        #[pallet::call_index(6)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::proxy_signed_list_nft_open_for_sale()
            .max(<T as pallet::Config>::WeightInfo::proxy_signed_mint_single_nft(MAX_NUMBER_OF_ROYALTIES))
            .max(<T as pallet::Config>::WeightInfo::proxy_signed_transfer_fiat_nft())
            .max(<T as pallet::Config>::WeightInfo::proxy_signed_cancel_list_fiat_nft()))]
        pub fn proxy(
            origin: OriginFor<T>,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResultWithPostInfo {
            let relayer = ensure_signed(origin)?;

            let proof = Self::get_proof(&*call)?;
            ensure!(relayer == proof.relayer, Error::<T>::UnauthorizedProxyTransaction);

            let call_hash: T::Hash = T::Hashing::hash_of(&call);
            call.clone()
                .dispatch(frame_system::RawOrigin::Signed(proof.signer).into())
                .map(|_| ())
                .map_err(|e| e.error)?;
            Self::deposit_event(Event::<T>::CallDispatched { relayer, hash: call_hash });

            return Self::get_dispatch_result_with_post_info(call)
        }

        /// Creates a new batch
        #[pallet::call_index(7)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::proxy_signed_create_batch(MAX_NUMBER_OF_ROYALTIES))]
        pub fn signed_create_batch(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            total_supply: u64,
            royalties: Vec<Royalty>,
            t1_authority: H160,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            ensure!(t1_authority.is_zero() == false, Error::<T>::T1AuthorityIsMandatory);
            ensure!(total_supply > 0u64, Error::<T>::TotalSupplyZero);
            ensure!(total_supply <= T::BatchBound::get().into(), Error::<T>::BatchOutOfBounds);

            Self::validate_royalties(&royalties)?;

            let sender_nonce = Self::batch_nonce(&sender);
            let signed_payload = encode_create_batch_params::<T>(
                &proof,
                &royalties,
                &t1_authority,
                &total_supply,
                &sender_nonce,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedCreateBatchTransaction
            );

            let batch_id = generate_batch_id::<T>(Self::get_unique_id_and_advance());
            ensure!(
                BatchInfoId::<T>::contains_key(&batch_id) == false,
                Error::<T>::BatchAlreadyExists
            );

            // No errors allowed after this point because `get_info_id_and_advance` mutates storage
            let info_id = Self::get_info_id_and_advance();
            create_batch::<T>(
                info_id,
                batch_id,
                BoundedVec::try_from(royalties).map_err(|_| Error::<T>::RoyaltiesOutOfBounds)?,
                total_supply,
                t1_authority,
                sender.clone(),
            );

            <BatchNonces<T>>::mutate(&sender, |n| *n += 1);

            Self::deposit_event(Event::<T>::BatchCreated {
                batch_nft_id: batch_id,
                total_supply,
                batch_creator: sender,
                authority: t1_authority,
            });

            Ok(())
        }

        /// Mints an nft that belongs to a batch
        #[pallet::call_index(8)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::proxy_signed_mint_batch_nft())]
        pub fn signed_mint_batch_nft(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            batch_id: NftBatchId,
            index: u64,
            owner: T::AccountId,
            unique_external_ref: Vec<u8>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);

            let bounded_unique_external_ref =
                BoundedVec::<u8, NftExternalRefBound>::try_from(unique_external_ref)
                    .map_err(|_| Error::<T>::ExternalRefOutOfBounds)?;
            let nft_info =
                validate_mint_batch_nft_request::<T>(batch_id, &bounded_unique_external_ref)?;
            ensure!(
                <BatchOpenForSale<T>>::get(&batch_id) == NftSaleType::Fiat,
                Error::<T>::BatchNotListedForFiatSale
            );
            ensure!(nft_info.creator == Some(sender), Error::<T>::SenderIsNotBatchCreator);

            let signed_payload = encode_mint_batch_nft_params::<T>(
                &proof,
                &batch_id,
                &index,
                &bounded_unique_external_ref,
                &owner,
            );
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedMintBatchNftTransaction
            );

            mint_batch_nft::<T>(batch_id, owner, index, bounded_unique_external_ref)?;

            Ok(())
        }

        #[pallet::call_index(9)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::proxy_signed_list_batch_for_sale())]
        pub fn signed_list_batch_for_sale(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            batch_id: NftBatchId,
            market: NftSaleType,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            ensure!(batch_id.is_zero() == false, Error::<T>::BatchIdIsMandatory);
            ensure!(<BatchInfoId<T>>::contains_key(&batch_id), Error::<T>::BatchDoesNotExist);
            ensure!(market != NftSaleType::Unknown, Error::<T>::UnsupportedMarket);

            let sender_nonce = Self::batch_nonce(&sender);
            let nft_info = get_nft_info_for_batch::<T>(&batch_id)?;
            //Only the batch creator can allow mint operations.
            ensure!(nft_info.creator == Some(sender.clone()), Error::<T>::SenderIsNotBatchCreator);
            ensure!(
                (<NftBatches<T>>::get(&batch_id).len() as u64) < nft_info.total_supply,
                Error::<T>::NoNftsToSell
            );
            ensure!(
                <BatchOpenForSale<T>>::contains_key(&batch_id) == false,
                Error::<T>::BatchAlreadyListed
            );

            let signed_payload =
                encode_list_batch_for_sale_params::<T>(&proof, &batch_id, &market, &sender_nonce);
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedListBatchForSaleTransaction
            );

            <BatchOpenForSale<T>>::insert(batch_id, market);
            <BatchNonces<T>>::mutate(&sender, |n| *n += 1);

            Self::deposit_event(Event::<T>::BatchOpenForSale {
                batch_nft_id: batch_id,
                sale_type: market,
            });

            Ok(())
        }

        #[pallet::call_index(10)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::proxy_signed_end_batch_sale())]
        pub fn signed_end_batch_sale(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            batch_id: NftBatchId,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);
            validate_end_batch_listing_request::<T>(&batch_id)?;
            ensure!(
                <BatchOpenForSale<T>>::get(&batch_id) == NftSaleType::Fiat,
                Error::<T>::BatchNotListedForFiatSale
            );

            let sender_nonce = Self::batch_nonce(&sender);
            let nft_info = get_nft_info_for_batch::<T>(&batch_id)?;
            //Only the batch creator can end the listing
            ensure!(nft_info.creator == Some(sender.clone()), Error::<T>::SenderIsNotBatchCreator);

            let signed_payload =
                encode_end_batch_sale_params::<T>(&proof, &batch_id, &sender_nonce);
            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedEndBatchSaleTransaction
            );

            let market = <BatchOpenForSale<T>>::get(batch_id);
            end_batch_listing::<T>(&batch_id, market)?;
            <BatchNonces<T>>::mutate(&sender, |n| *n += 1);

            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        // Note: this "special" function will run during every runtime upgrade. Any complicated
        // migration logic should be done in a separate function so it can be tested
        // properly.
        fn on_runtime_upgrade() -> Weight {
            let onchain_version = Pallet::<T>::on_chain_storage_version();
            log::debug!(
                "Nft manager storage chain/current storage version: {:?} / {:?}",
                onchain_version,
                Pallet::<T>::in_code_storage_version(),
            );
            return Weight::from_parts(0 as u64, 0)
        }
    }
}

impl<T: Config> Pallet<T> {
    fn validate_mint_single_nft_request(
        unique_external_ref: &BoundedVec<u8, NftExternalRefBound>,
        royalties: &Vec<Royalty>,
        t1_authority: H160,
    ) -> DispatchResult {
        ensure!(t1_authority.is_zero() == false, Error::<T>::T1AuthorityIsMandatory);

        Self::validate_external_ref(unique_external_ref)?;
        Self::validate_royalties(royalties)?;

        Ok(())
    }

    fn validate_external_ref(
        unique_external_ref: &BoundedVec<u8, NftExternalRefBound>,
    ) -> DispatchResult {
        ensure!(!unique_external_ref.is_empty(), Error::<T>::ExternalRefIsMandatory);
        ensure!(
            Self::is_external_ref_used(&unique_external_ref) == false,
            Error::<T>::ExternalRefIsAlreadyInUse
        );

        Ok(())
    }

    fn validate_royalties(royalties: &Vec<Royalty>) -> DispatchResult {
        // TODO: Review this comment https://github.com/Aventus-Network-Services/avn-tier2/pull/763#discussion_r617360380
        let invalid_rates_found = royalties.iter().any(|r| !r.rate.is_valid());
        ensure!(invalid_rates_found == false, Error::<T>::RoyaltyRateIsNotValid);

        let rate_total = royalties.iter().map(|r| r.rate.parts_per_million).sum::<u32>();

        ensure!(rate_total <= 1_000_000, Error::<T>::TotalRoyaltyRateIsNotValid);

        Ok(())
    }

    fn validate_open_for_sale_request(
        sender: T::AccountId,
        nft_id: NftId,
        market: NftSaleType,
    ) -> DispatchResult {
        ensure!(market != NftSaleType::Unknown, Error::<T>::UnsupportedMarket);
        ensure!(<Nfts<T>>::contains_key(&nft_id) == true, Error::<T>::NftIdDoesNotExist);
        ensure!(<NftOpenForSale<T>>::contains_key(&nft_id) == false, Error::<T>::NftAlreadyListed);

        let nft = Self::try_get_nft(&nft_id)?;
        ensure!(nft.owner == sender, Error::<T>::SenderIsNotOwner);
        ensure!(nft.is_locked == false, Error::<T>::NftIsLocked);

        Ok(())
    }

    fn validate_nft_open_for_fiat_sale(sender: T::AccountId, nft_id: NftId) -> DispatchResult {
        ensure!(<NftOpenForSale<T>>::contains_key(nft_id) == true, Error::<T>::NftNotListedForSale);
        ensure!(
            Self::get_nft_open_for_sale_on(nft_id) == NftSaleType::Fiat,
            Error::<T>::NftNotListedForFiatSale
        );

        let nft = Self::try_get_nft(&nft_id)?;

        ensure!(nft.owner == sender, Error::<T>::SenderIsNotOwner);
        ensure!(nft.is_locked == false, Error::<T>::NftIsLocked);

        Ok(())
    }

    /// Returns the next available info id and increases the storage item by 1
    fn get_info_id_and_advance() -> NftInfoId {
        let id = Self::next_info_id();
        <NextInfoId<T>>::mutate(|n| *n += U256::from(1));

        return id
    }

    fn get_unique_id_and_advance() -> NftUniqueId {
        let id = Self::next_unique_id();
        <NextSingleNftUniqueId<T>>::mutate(|n| *n += U256::from(1));

        return id
    }

    fn insert_single_nft_into_chain(
        info_id: NftInfoId,
        royalties: BoundedVec<Royalty, NftRoyaltiesBound>,
        t1_authority: H160,
        nft_id: NftId,
        unique_external_ref: BoundedVec<u8, NftExternalRefBound>,
        owner: T::AccountId,
    ) -> (Nft<T::AccountId>, NftInfo<T::AccountId>) {
        let info = NftInfo::new(info_id, royalties, t1_authority);
        let nft = Nft::new(nft_id, info_id, unique_external_ref, owner.clone());

        <NftInfos<T>>::insert(info.info_id, &info);

        Self::add_nft(&nft);
        return (nft, info)
    }

    fn open_nft_for_sale(nft_id: &NftId, market: &NftSaleType) {
        <NftOpenForSale<T>>::insert(nft_id, market);
        <Nfts<T>>::mutate(nft_id, |maybe_nft| maybe_nft.as_mut().map(|nft| nft.nonce += 1u64));
    }

    /// The NftId for a single mint is calculated by this formula: uint256(keccak256(“A”,
    /// contract_address, unique_id))
    fn generate_nft_id_single_mint(contract: &H160, unique_id: NftUniqueId) -> U256 {
        let mut data_to_hash = SINGLE_NFT_ID_CONTEXT.to_vec();

        data_to_hash.append(&mut contract[..].to_vec());

        let unique_id_be = unique_id.to_big_endian();
        data_to_hash.append(&mut unique_id_be.to_vec());

        let hash = keccak_256(&data_to_hash);

        return U256::from_big_endian(&hash)
    }

    fn remove_listing_from_open_for_sale(nft_id: &NftId) -> DispatchResult {
        ensure!(<NftOpenForSale<T>>::contains_key(nft_id) == true, Error::<T>::NftNotListedForSale);
        <NftOpenForSale<T>>::remove(nft_id);
        Ok(())
    }

    fn transfer_eth_nft(event_id: &EthEventId, data: &NftTransferToData) -> DispatchResult {
        let market = Self::get_nft_open_for_sale_on(data.nft_id);
        ensure!(market == NftSaleType::Ethereum, Error::<T>::NftNotListedForEthereumSale);

        let nft = Self::try_get_nft(&data.nft_id)?;

        ensure!(data.op_id == nft.nonce, Error::<T>::NftNonceMismatch);
        ensure!(
            T::ProcessedEventsChecker::processed_event_exists(event_id),
            Error::<T>::NoTier1EventForNftOperation
        );

        let new_nft_owner = T::AccountId::decode(&mut data.t2_transfer_to_public_key.as_bytes())
            .expect("32 bytes will always decode into an AccountId");
        Self::transfer_nft(&data.nft_id, &new_nft_owner)?;
        Self::deposit_event(Event::<T>::EthNftTransfer {
            nft_id: data.nft_id,
            new_owner: new_nft_owner,
            sale_type: market,
            op_id: data.op_id,
            eth_event_id: event_id.clone(),
        });

        Ok(())
    }

    fn transfer_nft(nft_id: &NftId, new_nft_owner: &T::AccountId) -> DispatchResult {
        Self::remove_listing_from_open_for_sale(nft_id)?;
        Self::update_owner_for_transfer(nft_id, new_nft_owner);
        Ok(())
    }

    // See https://github.com/Aventus-Network-Services/avn-tier2/pull/991#discussion_r832470480 for details of why we have this
    // as a separate function
    fn update_owner_for_transfer(nft_id: &NftId, new_nft_owner: &T::AccountId) {
        <Nfts<T>>::mutate(nft_id, |maybe_nft| {
            maybe_nft.as_mut().map(|nft| {
                nft.owner = new_nft_owner.clone();
                nft.nonce += 1u64;
            })
        });
    }

    // See https://github.com/Aventus-Network-Services/avn-tier2/pull/991#discussion_r832470480 for details of why we have this
    // as a separate function
    fn add_nft(nft: &Nft<T::AccountId>) {
        <Nfts<T>>::insert(nft.nft_id, &nft);
        <UsedExternalReferences<T>>::insert(&nft.unique_external_ref, true);
    }

    fn cancel_eth_nft_listing(
        event_id: &EthEventId,
        data: &NftCancelListingData,
    ) -> DispatchResult {
        let market = Self::get_nft_open_for_sale_on(data.nft_id);
        let nft = Self::try_get_nft(&data.nft_id)?;

        ensure!(market == NftSaleType::Ethereum, Error::<T>::NftNotListedForEthereumSale);
        ensure!(data.op_id == nft.nonce, Error::<T>::NftNonceMismatch);
        ensure!(
            T::ProcessedEventsChecker::processed_event_exists(event_id),
            Error::<T>::NoTier1EventForNftOperation
        );

        Self::unlist_nft_for_sale(data.nft_id)?;
        Self::deposit_event(Event::<T>::CancelSingleEthNftListing {
            nft_id: data.nft_id,
            sale_type: market,
            op_id: data.op_id,
            eth_event_id: event_id.clone(),
        });

        Ok(())
    }

    fn unlist_nft_for_sale(nft_id: NftId) -> DispatchResult {
        Self::remove_listing_from_open_for_sale(&nft_id)?;
        <Nfts<T>>::mutate(nft_id, |maybe_nft| maybe_nft.as_mut().map(|nft| nft.nonce += 1u64));

        Ok(())
    }

    fn get_dispatch_result_with_post_info(
        call: Box<<T as Config>::RuntimeCall>,
    ) -> DispatchResultWithPostInfo {
        match call.is_sub_type() {
            Some(call) => {
                let final_weight = match call {
                    Call::signed_mint_single_nft { royalties, .. } =>
                        <T as pallet::Config>::WeightInfo::proxy_signed_mint_single_nft(
                            royalties.len().try_into().unwrap(),
                        ),
                    Call::signed_list_nft_open_for_sale { .. } =>
                        <T as pallet::Config>::WeightInfo::proxy_signed_list_nft_open_for_sale(),
                    Call::signed_transfer_fiat_nft { .. } =>
                        <T as pallet::Config>::WeightInfo::proxy_signed_transfer_fiat_nft(),
                    Call::signed_cancel_list_fiat_nft { .. } =>
                        <T as pallet::Config>::WeightInfo::proxy_signed_cancel_list_fiat_nft(),
                    _ => <T as pallet::Config>::WeightInfo::proxy_signed_list_nft_open_for_sale()
                        .max(<T as pallet::Config>::WeightInfo::proxy_signed_mint_single_nft(
                            MAX_NUMBER_OF_ROYALTIES,
                        )),
                };
                Ok(Some(final_weight).into())
            },
            None => Err(DispatchErrorWithPostInfo {
                error: Error::<T>::TransactionNotSupported.into(),
                post_info: PostDispatchInfo {
                    actual_weight: None, // None which stands for the worst case static weight
                    pays_fee: Default::default(),
                },
            }),
        }
    }

    fn encode_mint_single_nft_params(
        proof: &Proof<T::Signature, T::AccountId>,
        unique_external_ref: &Vec<u8>,
        royalties: &Vec<Royalty>,
        t1_authority: &H160,
    ) -> Vec<u8> {
        return (
            SIGNED_MINT_SINGLE_NFT_CONTEXT,
            &proof.relayer,
            unique_external_ref,
            royalties,
            t1_authority,
        )
            .encode()
    }

    fn encode_list_nft_for_sale_params(
        proof: &Proof<T::Signature, T::AccountId>,
        nft_id: &NftId,
        market: &NftSaleType,
    ) -> Result<Vec<u8>, Error<T>> {
        let nft = Self::try_get_nft(nft_id)?;
        return Ok((
            SIGNED_LIST_NFT_OPEN_FOR_SALE_CONTEXT,
            &proof.relayer,
            nft_id,
            market,
            nft.nonce,
        )
            .encode())
    }

    fn encode_transfer_fiat_nft_params(
        proof: &Proof<T::Signature, T::AccountId>,
        nft_id: &NftId,
        recipient: &H256,
    ) -> Result<Vec<u8>, Error<T>> {
        let nft = Self::try_get_nft(nft_id)?;
        return Ok((SIGNED_TRANSFER_FIAT_NFT_CONTEXT, &proof.relayer, nft_id, recipient, nft.nonce)
            .encode())
    }

    fn encode_cancel_list_fiat_nft_params(
        proof: &Proof<T::Signature, T::AccountId>,
        nft_id: &NftId,
    ) -> Result<Vec<u8>, Error<T>> {
        let nft = Self::try_get_nft(nft_id)?;
        return Ok((SIGNED_CANCEL_LIST_FIAT_NFT_CONTEXT, &proof.relayer, nft_id, nft.nonce).encode())
    }

    fn get_encoded_call_param(
        call: &<T as Config>::RuntimeCall,
    ) -> Option<(&Proof<T::Signature, T::AccountId>, Vec<u8>)> {
        let call = match call.is_sub_type() {
            Some(call) => call,
            None => return None,
        };

        match call {
            Call::signed_mint_single_nft {
                proof,
                unique_external_ref,
                royalties,
                t1_authority,
            } =>
                return Some((
                    proof,
                    Self::encode_mint_single_nft_params(
                        proof,
                        unique_external_ref,
                        royalties,
                        t1_authority,
                    ),
                )),
            Call::signed_list_nft_open_for_sale { proof, nft_id, market } => {
                let encoded_params = Self::encode_list_nft_for_sale_params(proof, nft_id, market);
                if encoded_params.is_err() {
                    return None
                }

                return Some((proof, encoded_params.expect("checked for none")))
            },
            Call::signed_transfer_fiat_nft { proof, nft_id, t2_transfer_to_public_key } => {
                let encoded_data =
                    Self::encode_transfer_fiat_nft_params(proof, nft_id, t2_transfer_to_public_key);

                if encoded_data.is_err() {
                    return None
                }

                return Some((proof, encoded_data.expect("checked for none")))
            },
            Call::signed_cancel_list_fiat_nft { proof, nft_id } => {
                let encoded_data = Self::encode_cancel_list_fiat_nft_params(proof, nft_id);
                if encoded_data.is_err() {
                    return None
                }

                return Some((proof, encoded_data.expect("checked for none")))
            },
            Call::signed_create_batch { proof, total_supply, royalties, t1_authority } => {
                let sender_nonce = Self::batch_nonce(&proof.signer);
                return Some((
                    proof,
                    encode_create_batch_params::<T>(
                        proof,
                        royalties,
                        t1_authority,
                        total_supply,
                        &sender_nonce,
                    ),
                ))
            },
            Call::signed_mint_batch_nft { proof, batch_id, index, owner, unique_external_ref } =>
                return Some((
                    proof,
                    encode_mint_batch_nft_params::<T>(
                        proof,
                        batch_id,
                        index,
                        unique_external_ref,
                        owner,
                    ),
                )),
            Call::signed_list_batch_for_sale { proof, batch_id, market } => {
                let sender_nonce = Self::batch_nonce(&proof.signer);
                return Some((
                    proof,
                    encode_list_batch_for_sale_params::<T>(proof, batch_id, market, &sender_nonce),
                ))
            },
            Call::signed_end_batch_sale { proof, batch_id } => {
                let sender_nonce = Self::batch_nonce(&proof.signer);
                return Some((
                    proof,
                    encode_end_batch_sale_params::<T>(proof, batch_id, &sender_nonce),
                ))
            },
            _ => return None,
        }
    }

    fn try_get_nft(nft_id: &NftId) -> Result<Nft<T::AccountId>, Error<T>> {
        let maybe_nft = Self::nfts(nft_id);

        if maybe_nft.is_none() {
            return Err(Error::<T>::NftIdDoesNotExist)?
        }

        Ok(maybe_nft.expect("checked for none"))
    }

    fn processed_event_handler(event: &EthEvent) -> DispatchResult {
        return match &event.event_data {
            EventData::LogNftTransferTo(data) => Self::transfer_eth_nft(&event.event_id, data),
            EventData::LogNftCancelListing(data) =>
                Self::cancel_eth_nft_listing(&event.event_id, data),
            EventData::LogNftMinted(data) =>
                process_mint_batch_nft_event::<T>(&event.event_id, data),
            EventData::LogNftEndBatchListing(data) =>
                process_end_batch_listing_event::<T>(&event.event_id, data),
            _ => Ok(()),
        }
    }
}

impl<T: Config> ProcessedEventHandler for Pallet<T> {
    fn on_event_processed(event: &EthEvent) -> DispatchResult {
        Self::processed_event_handler(event)
    }
}

impl<T: Config> BridgeInterfaceNotification for Pallet<T> {
    fn process_result(_: u32, _: Vec<u8>, _: bool) -> DispatchResult {
        Ok(())
    }
    #[cfg(feature = "ethbridge-all-events")]
    fn on_incoming_event_processed(event: &EthEvent) -> DispatchResult {
        Self::processed_event_handler(event)
    }
}

impl<T: Config> CallDecoder for Pallet<T> {
    type AccountId = T::AccountId;
    type Signature = <T as Config>::Signature;
    type Error = Error<T>;
    type Call = <T as Config>::RuntimeCall;

    fn get_proof(
        call: &Self::Call,
    ) -> Result<Proof<Self::Signature, Self::AccountId>, Self::Error> {
        let call = match call.is_sub_type() {
            Some(call) => call,
            None => return Err(Error::TransactionNotSupported),
        };

        match call {
            Call::signed_mint_single_nft { proof, .. } => return Ok(proof.clone()),
            Call::signed_list_nft_open_for_sale { proof, .. } => return Ok(proof.clone()),
            Call::signed_transfer_fiat_nft { proof, .. } => return Ok(proof.clone()),
            Call::signed_cancel_list_fiat_nft { proof, .. } => return Ok(proof.clone()),
            Call::signed_create_batch { proof, .. } => return Ok(proof.clone()),
            Call::signed_mint_batch_nft { proof, .. } => return Ok(proof.clone()),
            Call::signed_list_batch_for_sale { proof, .. } => return Ok(proof.clone()),
            Call::signed_end_batch_sale { proof, .. } => return Ok(proof.clone()),
            _ => return Err(Error::TransactionNotSupported),
        }
    }
}

impl<T: Config> InnerCallValidator for Pallet<T> {
    type Call = <T as Config>::RuntimeCall;

    fn signature_is_valid(call: &Box<Self::Call>) -> bool {
        if let Some((proof, signed_payload)) = Self::get_encoded_call_param(call) {
            return verify_signature::<T::Signature, T::AccountId>(
                &proof,
                &signed_payload.as_slice(),
            )
            .is_ok()
        }

        return false
    }
}

const STORAGE_VERSION: StorageVersion = StorageVersion::new(4);

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;

#[cfg(test)]
#[path = "tests/single_mint_nft_tests.rs"]
pub mod single_mint_nft_tests;

#[cfg(test)]
#[path = "tests/open_for_sale_tests.rs"]
pub mod open_for_sale_tests;

#[cfg(test)]
#[path = "tests/proxy_signed_mint_single_nft_tests.rs"]
pub mod proxy_signed_mint_single_nft_tests;

#[cfg(test)]
#[path = "tests/proxy_signed_list_nft_open_for_sale_tests.rs"]
pub mod proxy_signed_list_nft_open_for_sale_tests;

#[cfg(test)]
#[path = "tests/proxy_signed_transfer_fiat_nft_tests.rs"]
pub mod proxy_signed_transfer_fiat_nft_tests;

#[cfg(test)]
#[path = "tests/proxy_signed_cancel_list_fiat_nft_tests.rs"]
pub mod proxy_signed_cancel_list_fiat_nft_tests;

#[cfg(test)]
#[path = "tests/transfer_to_tests.rs"]
pub mod transfer_to_tests;

#[cfg(test)]
#[path = "tests/cancel_single_nft_listing_tests.rs"]
pub mod cancel_single_nft_listing_tests;

#[cfg(test)]
#[path = "tests/batch_nft_tests.rs"]
pub mod batch_nft_tests;

mod benchmarking;
