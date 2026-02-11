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

use crate::{
    keccak_256, BatchInfoId, BatchOpenForSale, Config, Decode, DispatchResult, Encode, Error,
    EthEventId, Event, Nft, NftBatchId, NftBatches, NftEndBatchListingData, NftExternalRefBound,
    NftInfo, NftInfoId, NftInfos, NftRoyaltiesBound, NftSaleType, NftUniqueId, Nfts, Pallet,
    ProcessedEventsChecker, Proof, Royalty, Vec, BATCH_ID_CONTEXT, BATCH_NFT_ID_CONTEXT, H160,
    U256,
};
use frame_support::ensure;
use sp_avn_common::event_types::NftMintData;
use sp_core::bounded::BoundedVec;
use sp_runtime::DispatchError;

pub const SIGNED_CREATE_BATCH_CONTEXT: &'static [u8] = b"authorization for create batch operation";
pub const SIGNED_MINT_BATCH_NFT_CONTEXT: &'static [u8] =
    b"authorization for mint batch nft operation";
pub const SIGNED_LIST_BATCH_FOR_SALE_CONTEXT: &'static [u8] =
    b"authorization for list batch for sale operation";
pub const SIGNED_END_BATCH_SALE_CONTEXT: &'static [u8] =
    b"authorization for end batch sale operation";

pub fn generate_batch_id<T: Config>(unique_id: NftUniqueId) -> U256 {
    let mut data_to_hash = BATCH_ID_CONTEXT.to_vec();
    let unique_id_be = unique_id.to_big_endian();
    data_to_hash.append(&mut unique_id_be.to_vec());

    let hash = keccak_256(&data_to_hash);

    return U256::from_big_endian(&hash)
}

/// The NftId for a Batch Sale is calculated by this formula: uint256(keccak256(“B”, batchId,
/// sales_index))
pub fn generate_batch_nft_id<T: Config>(batch_id: &NftBatchId, sales_index: &u64) -> U256 {
    let mut data_to_hash = BATCH_NFT_ID_CONTEXT.to_vec();

    let mut batch_id_be = batch_id.to_big_endian();
    data_to_hash.append(&mut batch_id_be.to_vec());
    data_to_hash.append(&mut sales_index.to_be_bytes().to_vec());

    let hash = keccak_256(&data_to_hash);

    return U256::from_big_endian(&hash)
}

pub fn get_nft_info_for_batch<T: Config>(
    batch_id: &NftBatchId,
) -> Result<NftInfo<T::AccountId>, Error<T>> {
    let nft_info_id = BatchInfoId::<T>::get(&batch_id);
    ensure!(<NftInfos<T>>::contains_key(&nft_info_id), Error::<T>::NftInfoMissing);

    return Ok(<NftInfos<T>>::get(nft_info_id).expect("key existance checked"))
}

pub fn create_batch<T: Config>(
    info_id: NftInfoId,
    batch_id: NftBatchId,
    royalties: BoundedVec<Royalty, NftRoyaltiesBound>,
    total_supply: u64,
    t1_authority: H160,
    creator: T::AccountId,
) {
    let info =
        NftInfo::new_batch(info_id, batch_id, royalties, t1_authority, total_supply, creator);
    <BatchInfoId<T>>::insert(batch_id, &info.info_id);
    <NftInfos<T>>::insert(info.info_id, info);
}

pub fn encode_create_batch_params<T: Config>(
    proof: &Proof<T::Signature, T::AccountId>,
    royalties: &Vec<Royalty>,
    t1_authority: &H160,
    total_supply: &u64,
    nonce: &u64,
) -> Vec<u8> {
    return (
        SIGNED_CREATE_BATCH_CONTEXT,
        &proof.relayer,
        total_supply,
        royalties,
        t1_authority,
        nonce,
    )
        .encode()
}

pub fn encode_mint_batch_nft_params<T: Config>(
    proof: &Proof<T::Signature, T::AccountId>,
    batch_id: &NftBatchId,
    index: &u64,
    unique_external_ref: &Vec<u8>,
    owner: &T::AccountId,
) -> Vec<u8> {
    return (
        SIGNED_MINT_BATCH_NFT_CONTEXT,
        &proof.relayer,
        batch_id,
        index,
        unique_external_ref,
        owner,
    )
        .encode()
}

pub fn encode_list_batch_for_sale_params<T: Config>(
    proof: &Proof<T::Signature, T::AccountId>,
    batch_id: &NftBatchId,
    market: &NftSaleType,
    nonce: &u64,
) -> Vec<u8> {
    return (SIGNED_LIST_BATCH_FOR_SALE_CONTEXT, &proof.relayer, batch_id, market, nonce).encode()
}

pub fn encode_end_batch_sale_params<T: Config>(
    proof: &Proof<T::Signature, T::AccountId>,
    batch_id: &NftBatchId,
    nonce: &u64,
) -> Vec<u8> {
    return (SIGNED_END_BATCH_SALE_CONTEXT, &proof.relayer, batch_id, nonce).encode()
}

pub fn process_mint_batch_nft_event<T: Config>(
    event_id: &EthEventId,
    data: &NftMintData,
) -> DispatchResult {
    ensure!(
        T::ProcessedEventsChecker::processed_event_exists(event_id),
        Error::<T>::NoTier1EventForNftOperation
    );
    ensure!(
        <BatchOpenForSale<T>>::get(&data.batch_id) == NftSaleType::Ethereum,
        Error::<T>::BatchNotListedForEthereumSale
    );

    let owner = T::AccountId::decode(&mut data.t2_owner_public_key.as_bytes())
        .expect("32 bytes will always decode into an AccountId");

    Ok(mint_batch_nft::<T>(
        data.batch_id,
        owner,
        data.sale_index,
        data.unique_external_ref.clone(),
    )?)
}

pub fn validate_mint_batch_nft_request<T: Config>(
    batch_id: NftBatchId,
    unique_external_ref: &BoundedVec<u8, NftExternalRefBound>,
) -> Result<NftInfo<T::AccountId>, DispatchError> {
    ensure!(batch_id.is_zero() == false, Error::<T>::BatchIdIsMandatory);
    ensure!(<BatchInfoId<T>>::contains_key(&batch_id), Error::<T>::BatchDoesNotExist);
    ensure!(<BatchOpenForSale<T>>::contains_key(&batch_id) == true, Error::<T>::BatchNotListed);

    let nft_info = get_nft_info_for_batch::<T>(&batch_id)?;
    ensure!(
        (<NftBatches<T>>::get(&batch_id).len() as u64) < nft_info.total_supply,
        Error::<T>::TotalSupplyExceeded
    );

    Pallet::<T>::validate_external_ref(unique_external_ref)?;

    Ok(nft_info)
}

pub fn mint_batch_nft<T: Config>(
    batch_id: NftBatchId,
    owner: T::AccountId,
    sale_index: u64,
    unique_external_ref: BoundedVec<u8, NftExternalRefBound>,
) -> DispatchResult {
    let nft_info = validate_mint_batch_nft_request::<T>(batch_id, &unique_external_ref)?;
    let nft_id = generate_batch_nft_id::<T>(&batch_id, &sale_index);
    ensure!(<Nfts<T>>::contains_key(&nft_id) == false, Error::<T>::NftAlreadyExists);

    let nft = Nft::new(nft_id, nft_info.info_id, unique_external_ref, owner.clone());
    Pallet::<T>::add_nft(&nft);

    let mut nfts_for_batch = <NftBatches<T>>::get(batch_id);
    nfts_for_batch.try_push(nft_id).map_err(|_| Error::<T>::BatchOutOfBounds)?;
    <NftBatches<T>>::insert(batch_id, nfts_for_batch);

    <crate::Pallet<T>>::deposit_event(Event::<T>::BatchNftMinted {
        nft_id: nft.nft_id,
        batch_nft_id: batch_id,
        authority: nft_info.t1_authority,
        owner: nft.owner,
    });

    Ok(())
}

pub fn process_end_batch_listing_event<T: Config>(
    event_id: &EthEventId,
    data: &NftEndBatchListingData,
) -> DispatchResult {
    ensure!(
        T::ProcessedEventsChecker::processed_event_exists(event_id),
        Error::<T>::NoTier1EventForNftOperation
    );

    let market = <BatchOpenForSale<T>>::get(data.batch_id);
    ensure!(market == NftSaleType::Ethereum, Error::<T>::BatchNotListedForEthereumSale);

    end_batch_listing::<T>(&data.batch_id, market)?;

    Ok(())
}

pub fn end_batch_listing<T: Config>(batch_id: &NftBatchId, market: NftSaleType) -> DispatchResult {
    validate_end_batch_listing_request::<T>(&batch_id)?;

    <BatchOpenForSale<T>>::remove(batch_id);
    <crate::Pallet<T>>::deposit_event(Event::<T>::BatchSaleEnded {
        batch_nft_id: *batch_id,
        sale_type: market,
    });

    Ok(())
}

pub fn validate_end_batch_listing_request<T: Config>(batch_id: &NftBatchId) -> DispatchResult {
    ensure!(batch_id.is_zero() == false, Error::<T>::BatchIdIsMandatory);
    ensure!(<BatchInfoId<T>>::contains_key(batch_id), Error::<T>::BatchDoesNotExist);
    ensure!(<BatchOpenForSale<T>>::contains_key(batch_id) == true, Error::<T>::BatchNotListed);

    Ok(())
}
