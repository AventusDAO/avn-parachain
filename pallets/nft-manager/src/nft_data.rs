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

use crate::*;
use sp_core::bounded::BoundedVec;
use sp_runtime::traits::Member;

pub(crate) use sp_avn_common::bounds::NftExternalRefBound;
pub const ROYALTY_RATE_DENOMINATOR: u32 = 1_000_000;

#[derive(
    Encode, Decode, Default, Debug, Clone, PartialEq, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub struct Royalty {
    pub recipient_t1_address: H160,
    pub rate: RoyaltyRate,
}

/// Royalty Rate Examples:
///     - 1%: { parts_per_million = 10000 }
///     - 0.03%: { parts_per_million = 300 }
#[derive(
    Encode, Decode, Default, Clone, Debug, PartialEq, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub struct RoyaltyRate {
    pub parts_per_million: u32,
}

impl RoyaltyRate {
    pub fn is_valid(&self) -> bool {
        return self.parts_per_million <= ROYALTY_RATE_DENOMINATOR;
    }
}

#[derive(Encode, Decode, Default, Clone, Debug, PartialEq, TypeInfo, MaxEncodedLen)]
pub struct Nft<AccountId: Member> {
    /// Unique identifier of a nft
    pub nft_id: NftId,
    /// Id of an info struct instance
    pub info_id: NftInfoId,
    /// Unique reference to the NFT asset stored off-chain
    pub unique_external_ref: BoundedVec<u8, NftExternalRefBound>,
    /// Transfer nonce of this NFT
    pub nonce: u64,
    /// Owner account address of this NFT
    pub owner: AccountId,
    /// Flag to indicate if the vendor has marked this NFT as transferrable or not
    ///  - false: able to be transfered (default)
    ///  - true: not able to be transfered
    pub is_locked: bool,
}

impl<AccountId: Member> Nft<AccountId> {
    pub fn new(
        nft_id: NftId,
        info_id: NftInfoId,
        unique_external_ref: BoundedVec<u8, NftExternalRefBound>,
        owner: AccountId,
    ) -> Self {
        return Nft::<AccountId> {
            nft_id,
            info_id,
            unique_external_ref,
            nonce: 0,
            owner,
            is_locked: false,
        };
    }
}

#[derive(Encode, Decode, Default, Clone, Debug, PartialEq, TypeInfo, MaxEncodedLen)]
pub struct NftInfo<AccountId: Member> {
    /// Unique identifier of this information
    pub info_id: NftInfoId,
    /// Batch Id defined by client
    pub batch_id: Option<NftBatchId>,
    /// Royalties payment rate for the nft.
    pub royalties: BoundedVec<Royalty, NftRoyaltiesBound>,
    /// Total supply of NFTs in this collection:
    ///  - 1: it is for a singleton
    ///  - >1: it is for a batch
    pub total_supply: u64,
    /// Minter's tier 1 address
    pub t1_authority: H160, /* TODO: rename and remove t1 reference. Call it something like
                             * "provenance" */
    /// The address of the initial creator
    pub creator: Option<AccountId>,
}

impl<AccountId: Member> NftInfo<AccountId> {
    pub fn new(
        info_id: NftInfoId,
        royalties: BoundedVec<Royalty, NftRoyaltiesBound>,
        t1_authority: H160,
    ) -> Self {
        return NftInfo::<AccountId> {
            info_id,
            batch_id: None,
            royalties,
            total_supply: 1u64,
            t1_authority,
            creator: None,
        };
    }

    pub fn new_batch(
        info_id: NftInfoId,
        batch_id: NftBatchId,
        royalties: BoundedVec<Royalty, NftRoyaltiesBound>,
        t1_authority: H160,
        total_supply: u64,
        creator: AccountId,
    ) -> Self {
        return NftInfo::<AccountId> {
            info_id,
            batch_id: Some(batch_id),
            royalties,
            total_supply,
            t1_authority,
            creator: Some(creator),
        };
    }
}

#[derive(
    Encode,
    Decode,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    MaxEncodedLen,
    TypeInfo,
    DecodeWithMemTracking,
)]
pub enum NftSaleType {
    Unknown, // value used by Default interface. Needed for Maps default value.
    Ethereum,
    Fiat,
}

impl Default for NftSaleType {
    fn default() -> Self {
        return NftSaleType::Unknown;
    }
}
