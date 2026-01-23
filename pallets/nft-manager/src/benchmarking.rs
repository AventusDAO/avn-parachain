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

//! nft-manager pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite};
use frame_system::{EventRecord, RawOrigin};
use hex_literal::hex;
use pallet_avn::{self as avn};
use sp_core::{crypto::UncheckedInto, ByteArray, H256};
use sp_runtime::RuntimeAppPublic;

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};

const MNEMONIC: &str = "kiss mule sheriff twice make bike twice improve rate quote draw enough";

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    assert_last_nth_event::<T>(generic_event, 1);
}

fn assert_last_nth_event<T: Config>(generic_event: <T as Config>::RuntimeEvent, n: u32) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(n as usize)];
    assert_eq!(event, &system_event);
}

fn into_bytes<T: Config>(account: &<T as avn::Config>::AuthorityId) -> [u8; 32] {
    let bytes = account.encode();
    let mut vector: [u8; 32] = Default::default();
    vector.copy_from_slice(&bytes[0..32]);
    return vector
}

fn get_proof<T: Config>(
    signer: T::AccountId,
    relayer: T::AccountId,
    signature: &[u8],
) -> Proof<T::Signature, T::AccountId> {
    return Proof {
        signer: signer.clone(),
        relayer: relayer.clone(),
        signature: T::Signature::decode(&mut &signature[..]).unwrap(),
    }
}

fn bounded_unique_external_ref() -> BoundedVec<u8, NftExternalRefBound> {
    BoundedVec::try_from(String::from("Offchain location of NFT").into_bytes())
        .expect("Unique external reference bound was exceeded.")
}

pub fn bounded_royalties(royalties: Vec<Royalty>) -> BoundedVec<Royalty, NftRoyaltiesBound> {
    BoundedVec::try_from(royalties.clone()).expect("Royalty bound was exceeded.")
}

fn get_relayer<T: Config>() -> T::AccountId {
    let relayer_account: H256 =
        H256(hex!("0000000000000000000000000000000000000000000000000000000000000001"));
    return T::AccountId::decode(&mut relayer_account.as_bytes()).expect("valid relayer account id")
}

fn get_user_account<T: Config>() -> (<T as avn::Config>::AuthorityId, T::AccountId) {
    let key_pair =
        <T as avn::Config>::AuthorityId::generate_pair(Some(MNEMONIC.as_bytes().to_vec()));
    let account_bytes = into_bytes::<T>(&key_pair);
    let account_id = T::AccountId::decode(&mut &account_bytes.encode()[..]).unwrap();
    return (key_pair, account_id)
}

struct MintSingleNft<T: Config> {
    relayer: T::AccountId,
    nft_owner: T::AccountId,
    nft_id: U256,
    info_id: U256,
    unique_external_ref: BoundedVec<u8, NftExternalRefBound>,
    royalties: Vec<Royalty>,
    t1_authority: H160,
    signature: Vec<u8>,
}

impl<T: Config> MintSingleNft<T> {
    fn new(number_of_royalties: u32) -> Self {
        let relayer_account_id = get_relayer::<T>();
        let (nft_owner_key_pair, nft_owner_account_id) = get_user_account::<T>();

        let nft_id = U256::from_big_endian(&[
            144, 32, 76, 127, 69, 26, 191, 42, 121, 72, 235, 94, 179, 147, 69, 29, 167, 189, 8, 44,
            104, 83, 241, 253, 146, 114, 166, 195, 200, 254, 120, 78,
        ]);

        let unique_external_ref = bounded_unique_external_ref();
        let royalties = Self::setup_royalties(number_of_royalties);
        let t1_authority = H160(hex!("0000000000000000000000000000000000000001"));

        let signed_payload = (
            SIGNED_MINT_SINGLE_NFT_CONTEXT,
            &relayer_account_id,
            &unique_external_ref,
            &royalties,
            t1_authority,
        );
        let signature =
            nft_owner_key_pair.sign(&signed_payload.encode().as_slice()).unwrap().encode();

        return MintSingleNft {
            relayer: relayer_account_id,
            nft_owner: nft_owner_account_id,
            nft_id,
            info_id: U256::zero(),
            unique_external_ref,
            royalties,
            t1_authority,
            signature,
        }
    }

    fn setup_royalties(number_of_royalties: u32) -> Vec<Royalty> {
        let mut royalties: Vec<Royalty> = Vec::new();
        for _r in 0..number_of_royalties {
            royalties.push(Royalty {
                recipient_t1_address: H160(hex!("afdf36201bf70F1232111b5c6a9a424558755134")),
                rate: RoyaltyRate { parts_per_million: 1u32 },
            });
        }
        royalties
    }

    fn setup(self) -> Self {
        <Nfts<T>>::remove(&self.nft_id);
        <NftInfos<T>>::remove(&self.nft_id);
        <UsedExternalReferences<T>>::remove(&self.unique_external_ref);
        return self
    }

    fn generate_signed_mint_single_nft(&self) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> =
            get_proof::<T>(self.nft_owner.clone(), self.relayer.clone(), &self.signature);
        return Call::signed_mint_single_nft {
            proof,
            unique_external_ref: self.unique_external_ref.to_vec(),
            royalties: self.royalties.clone(),
            t1_authority: self.t1_authority,
        }
        .into()
    }
}

struct ListNftOpenForSale<T: Config> {
    relayer: T::AccountId,
    nft_owner: T::AccountId,
    nft_id: NftId,
    nft: Nft<T::AccountId>,
    market: NftSaleType,
    signature: [u8; 64],
}

impl<T: Config> ListNftOpenForSale<T> {
    fn new() -> Self {
        let relayer_account_id = get_relayer::<T>();
        let (_, nft_owner_account_id) = get_user_account::<T>();

        let nft_id = U256::from(1u8);
        let nft = Nft::new(
            nft_id,
            U256::one(),
            bounded_unique_external_ref(),
            nft_owner_account_id.clone(),
        );

        // Signature is generated using the script in `scripts/benchmarking`.
        let signature = hex!("6a767c9fb339b8ba6438f146f133ffd72b4d4b6745483f630a2dfdfecc57f240153ada88864251da658b837c661d82078e9c8eba8d09d47e487a3ab2b8d71a87");

        return ListNftOpenForSale {
            relayer: relayer_account_id,
            nft_owner: nft_owner_account_id,
            nft_id,
            nft,
            market: NftSaleType::Ethereum,
            signature,
        }
    }

    fn setup(self) -> Self {
        <Nfts<T>>::insert(self.nft_id, self.nft.clone());
        <NftOpenForSale<T>>::remove(&self.nft_id);
        return self
    }

    fn generate_signed_list_nft_open_for_sale_call(&self) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> =
            get_proof::<T>(self.nft_owner.clone(), self.relayer.clone(), &self.signature);
        return Call::signed_list_nft_open_for_sale {
            proof,
            nft_id: self.nft_id,
            market: self.market,
        }
        .into()
    }
}

struct TransferFiatNft<T: Config> {
    relayer: T::AccountId,
    nft_owner: T::AccountId,
    nft_id: NftId,
    nft: Nft<T::AccountId>,
    t2_transfer_to_public_key: H256,
    new_nft_owner_account: T::AccountId,
    op_id: u64,
    signature: Vec<u8>,
}

impl<T: Config> TransferFiatNft<T> {
    fn new() -> Self {
        let relayer_account_id = get_relayer::<T>();
        let (nft_owner_key_pair, nft_owner_account_id) = get_user_account::<T>();

        let nft_id = U256::from(1u8);
        let nft = Nft::new(
            nft_id,
            U256::one(),
            bounded_unique_external_ref(),
            nft_owner_account_id.clone(),
        );

        let t2_transfer_to_public_key = H256::from([1; 32]);
        let new_nft_owner_account = T::AccountId::decode(&mut t2_transfer_to_public_key.as_bytes())
            .expect("32 bytes will always decode into an AccountId");

        let op_id = 0;

        let signed_payload = (
            SIGNED_TRANSFER_FIAT_NFT_CONTEXT,
            &relayer_account_id,
            nft_id,
            t2_transfer_to_public_key,
            op_id,
        );
        let signature =
            nft_owner_key_pair.sign(&signed_payload.encode().as_slice()).unwrap().encode();

        return TransferFiatNft {
            relayer: relayer_account_id,
            nft_owner: nft_owner_account_id,
            nft_id,
            nft,
            t2_transfer_to_public_key,
            new_nft_owner_account,
            op_id,
            signature,
        }
    }

    fn setup(self) -> Self {
        <Nfts<T>>::insert(self.nft_id, self.nft.clone());
        <NftOpenForSale<T>>::insert(&self.nft_id, NftSaleType::Fiat);
        return self
    }

    fn generate_signed_transfer_fiat_nft_call(&self) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> =
            get_proof::<T>(self.nft_owner.clone(), self.relayer.clone(), &self.signature);
        return Call::signed_transfer_fiat_nft {
            proof,
            nft_id: self.nft_id,
            t2_transfer_to_public_key: self.t2_transfer_to_public_key,
        }
        .into()
    }
}

struct CancelListFiatNft<T: Config> {
    relayer: T::AccountId,
    nft_owner: T::AccountId,
    nft_id: NftId,
    nft: Nft<T::AccountId>,
    op_id: u64,
    signature: Vec<u8>,
}

impl<T: Config> CancelListFiatNft<T> {
    fn new() -> Self {
        let relayer_account_id = get_relayer::<T>();
        let (nft_owner_key_pair, nft_owner_account_id) = get_user_account::<T>();

        let nft_id = U256::from(1u8);
        let nft = Nft::new(
            nft_id,
            U256::one(),
            bounded_unique_external_ref(),
            nft_owner_account_id.clone(),
        );

        let op_id = 0;

        let signed_payload =
            (SIGNED_CANCEL_LIST_FIAT_NFT_CONTEXT, &relayer_account_id, nft_id, op_id);
        let signature =
            nft_owner_key_pair.sign(&signed_payload.encode().as_slice()).unwrap().encode();

        return CancelListFiatNft {
            relayer: relayer_account_id,
            nft_owner: nft_owner_account_id,
            nft_id,
            nft,
            op_id,
            signature,
        }
    }

    fn setup(self) -> Self {
        <Nfts<T>>::insert(self.nft_id, self.nft.clone());
        <NftOpenForSale<T>>::insert(&self.nft_id, NftSaleType::Fiat);
        return self
    }

    fn generate_signed_cancel_list_fiat_nft_call(&self) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> =
            get_proof::<T>(self.nft_owner.clone(), self.relayer.clone(), &self.signature);
        return Call::signed_cancel_list_fiat_nft { proof, nft_id: self.nft_id }.into()
    }
}

struct CreateBatch<T: Config> {
    relayer: T::AccountId,
    creator_account_id: T::AccountId,
    total_supply: u64,
    royalties: Vec<Royalty>,
    t1_authority: H160,
    signature: Vec<u8>,
}

impl<T: Config> CreateBatch<T> {
    fn new(batch_size: u32) -> Self {
        let relayer_account_id = get_relayer::<T>();
        let (creator_key_pair, creator_account_id) = get_user_account::<T>();

        let total_supply = batch_size as u64;
        let royalties = Self::setup_royalties(NftRoyaltiesBound::get());
        let t1_authority = H160(hex!("0000000000000000000000000000000000000001"));
        let nonce = 0u64;

        let signed_payload = (
            SIGNED_CREATE_BATCH_CONTEXT,
            &relayer_account_id,
            &total_supply,
            &royalties,
            t1_authority,
            nonce,
        );
        let signature =
            creator_key_pair.sign(&signed_payload.encode().as_slice()).unwrap().encode();

        return CreateBatch {
            relayer: relayer_account_id,
            creator_account_id,
            total_supply,
            royalties,
            t1_authority,
            signature,
        }
    }

    fn setup_royalties(number_of_royalties: u32) -> Vec<Royalty> {
        let mut royalties: Vec<Royalty> = Vec::new();
        for _r in 0..number_of_royalties {
            royalties.push(Royalty {
                recipient_t1_address: H160(hex!("afdf36201bf70F1232111b5c6a9a424558755134")),
                rate: RoyaltyRate { parts_per_million: 1u32 },
            });
        }
        royalties
    }

    fn generate_signed_create_batch(&self) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> =
            get_proof::<T>(self.creator_account_id.clone(), self.relayer.clone(), &self.signature);
        return Call::signed_create_batch {
            proof,
            total_supply: self.total_supply,
            royalties: self.royalties.clone(),
            t1_authority: self.t1_authority,
        }
        .into()
    }

    fn create_batch_for_setup(&self) -> U256 {
        let batch_id = generate_batch_id::<T>(<NextSingleNftUniqueId<T>>::get());
        create_batch::<T>(
            U256::zero(),
            batch_id,
            self.bounded_royalties(),
            self.total_supply,
            self.t1_authority,
            self.creator_account_id.clone(),
        );

        <BatchNonces<T>>::mutate(&self.creator_account_id, |n| *n += 1);

        return batch_id
    }

    pub fn bounded_royalties(&self) -> BoundedVec<Royalty, NftRoyaltiesBound> {
        bounded_royalties(self.royalties.clone())
    }
}

struct MintBatchNft<T: Config> {
    relayer: T::AccountId,
    nft_owner: T::AccountId,
    batch_id: NftBatchId,
    nft_id: U256,
    unique_external_ref: BoundedVec<u8, NftExternalRefBound>,
    t1_authority: H160,
    signature: Vec<u8>,
}

impl<T: Config> MintBatchNft<T> {
    fn new() -> Self {
        let index = 0u64;
        let relayer_account_id = get_relayer::<T>();
        let (nft_owner_key_pair, nft_owner_account_id) = get_user_account::<T>();

        // create batch and list
        let batch: CreateBatch<T> = CreateBatch::new(T::BatchBound::get());
        let batch_id = batch.create_batch_for_setup();
        <BatchOpenForSale<T>>::insert(batch_id, NftSaleType::Fiat);

        // Copied from batch_nft_tests.rs
        // This value is generated from batch_id (generated itself with unique_id = 0) and index = 1
        // nft_id = toBigEndian(generate_batch_nft_id(generate_batch_id(0), 1))
        let nft_id = U256::from_big_endian(&[
            101, 94, 240, 118, 189, 202, 200, 247, 116, 145, 110, 133, 216, 128, 100, 172, 36, 189,
            18, 53, 164, 178, 200, 65, 155, 27, 180, 246, 23, 91, 12, 175,
        ]);

        let unique_external_ref = bounded_unique_external_ref();
        let t1_authority = H160(hex!("0000000000000000000000000000000000000001"));

        let signed_payload = (
            SIGNED_MINT_BATCH_NFT_CONTEXT,
            &relayer_account_id,
            batch_id,
            index,
            &unique_external_ref,
            &nft_owner_account_id,
        );

        let signature =
            nft_owner_key_pair.sign(&signed_payload.encode().as_slice()).unwrap().encode();

        return MintBatchNft {
            relayer: relayer_account_id,
            nft_owner: nft_owner_account_id,
            batch_id,
            nft_id,
            unique_external_ref,
            t1_authority,
            signature,
        }
    }

    fn generate_signed_mint_batch_nft(
        &self,
        batch_id: U256,
        index: u64,
    ) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> =
            get_proof::<T>(self.nft_owner.clone(), self.relayer.clone(), &self.signature);
        return Call::signed_mint_batch_nft {
            proof,
            batch_id,
            index,
            owner: self.nft_owner.clone(),
            unique_external_ref: self.unique_external_ref.to_vec(),
        }
        .into()
    }
}

struct ListBatch<T: Config> {
    relayer: T::AccountId,
    creator_account_id: T::AccountId,
    batch_id: NftBatchId,
    market: NftSaleType,
    signature: Vec<u8>,
}

impl<T: Config> ListBatch<T> {
    fn new() -> Self {
        let relayer_account_id = get_relayer::<T>();
        let (creator_key_pair, creator_account_id) = get_user_account::<T>();

        let market = NftSaleType::Fiat;

        let batch: CreateBatch<T> = CreateBatch::new(T::BatchBound::get());
        let batch_id = batch.create_batch_for_setup();
        let nonce = <BatchNonces<T>>::get(&creator_account_id);

        let signed_payload =
            (SIGNED_LIST_BATCH_FOR_SALE_CONTEXT, &relayer_account_id, &batch_id, &market, nonce);
        let signature =
            creator_key_pair.sign(&signed_payload.encode().as_slice()).unwrap().encode();

        return ListBatch {
            relayer: relayer_account_id,
            creator_account_id,
            batch_id,
            market,
            signature,
        }
    }

    fn generate_signed_list_batch(&self) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> =
            get_proof::<T>(self.creator_account_id.clone(), self.relayer.clone(), &self.signature);
        return Call::signed_list_batch_for_sale {
            proof,
            batch_id: self.batch_id,
            market: self.market,
        }
        .into()
    }
}

struct EndBatchSale<T: Config> {
    relayer: T::AccountId,
    creator_account_id: T::AccountId,
    batch_id: NftBatchId,
    market: NftSaleType,
    signature: Vec<u8>,
}

impl<T: Config> EndBatchSale<T> {
    fn new() -> Self {
        let relayer_account_id = get_relayer::<T>();
        let (creator_key_pair, creator_account_id) = get_user_account::<T>();

        let market = NftSaleType::Fiat;

        let batch: CreateBatch<T> = CreateBatch::new(T::BatchBound::get());
        let batch_id = batch.create_batch_for_setup();

        <BatchOpenForSale<T>>::insert(batch_id, market);

        let nonce = <BatchNonces<T>>::get(&creator_account_id);

        let signed_payload = (SIGNED_END_BATCH_SALE_CONTEXT, &relayer_account_id, &batch_id, nonce);
        let signature =
            creator_key_pair.sign(&signed_payload.encode().as_slice()).unwrap().encode();

        return EndBatchSale {
            relayer: relayer_account_id,
            creator_account_id,
            batch_id,
            market,
            signature,
        }
    }

    fn generate_signed_end_batch_sale(&self) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> =
            get_proof::<T>(self.creator_account_id.clone(), self.relayer.clone(), &self.signature);
        return Call::signed_end_batch_sale { proof, batch_id: self.batch_id }.into()
    }
}

benchmarks! {
    mint_single_nft {
        let r in 1 .. MAX_NUMBER_OF_ROYALTIES;
        let mint_nft: MintSingleNft<T> = MintSingleNft::new(r).setup();
    }: _(
        RawOrigin::<T::AccountId>::Signed(mint_nft.nft_owner.clone()),
        mint_nft.unique_external_ref.to_vec(),
        mint_nft.royalties.clone(),
        mint_nft.t1_authority
    )
    verify {
        assert_eq!(true, Nfts::<T>::contains_key(&mint_nft.nft_id));
        assert_eq!(
            Nft::new(mint_nft.nft_id, mint_nft.info_id, mint_nft.unique_external_ref.clone(), mint_nft.nft_owner.clone()),
            Nfts::<T>::get(&mint_nft.nft_id).unwrap()
        );
        assert_eq!(true, NftInfos::<T>::contains_key(&mint_nft.info_id));
        assert_eq!(
            NftInfo::new(mint_nft.info_id, bounded_royalties(mint_nft.royalties.clone()), mint_nft.t1_authority),
            <NftInfos<T>>::get(&mint_nft.info_id).unwrap()
        );
        assert_eq!(true, <UsedExternalReferences<T>>::contains_key(&mint_nft.unique_external_ref));
        assert_eq!(true, <UsedExternalReferences<T>>::get(mint_nft.unique_external_ref));
        assert_last_event::<T>(Event::<T>::SingleNftMinted {
            nft_id: mint_nft.nft_id,
            owner: mint_nft.nft_owner,
            authority: mint_nft.t1_authority
        }.into());
    }

    signed_mint_single_nft {
        let r in 1 .. MAX_NUMBER_OF_ROYALTIES;
        let mint_nft: MintSingleNft<T> = MintSingleNft::new(r).setup();
        let proof: Proof<T::Signature, T::AccountId> = get_proof::<T>(
            mint_nft.nft_owner.clone(),
            mint_nft.relayer.clone(),
            &mint_nft.signature
        );
    }: _(
        RawOrigin::<T::AccountId>::Signed(mint_nft.nft_owner.clone()),
        proof,
        mint_nft.unique_external_ref.to_vec(),
        mint_nft.royalties.clone(),
        mint_nft.t1_authority
    )
    verify {
        assert_eq!(true, Nfts::<T>::contains_key(&mint_nft.nft_id));
        assert_eq!(
            Nft::new(mint_nft.nft_id, mint_nft.info_id, mint_nft.unique_external_ref.clone(), mint_nft.nft_owner.clone()),
            Nfts::<T>::get(&mint_nft.nft_id).unwrap()
        );
        assert_eq!(true, NftInfos::<T>::contains_key(&mint_nft.info_id));
        assert_eq!(
            NftInfo::new(mint_nft.info_id, bounded_royalties(mint_nft.royalties.clone()), mint_nft.t1_authority),
            <NftInfos<T>>::get(&mint_nft.info_id).unwrap()
        );
        assert_eq!(true, <UsedExternalReferences<T>>::contains_key(&mint_nft.unique_external_ref));
        assert_eq!(true, <UsedExternalReferences<T>>::get(mint_nft.unique_external_ref));
        assert_last_event::<T>(Event::<T>::SingleNftMinted {
            nft_id: mint_nft.nft_id,
            owner: mint_nft.nft_owner,
            authority: mint_nft.t1_authority
        }.into());
    }

    list_nft_open_for_sale {
        let owner_account_bytes = [1u8;32];
        let nft_owner_account_id = T::AccountId::decode(&mut &owner_account_bytes[..]).unwrap();
        let nft_id = U256::from(1u8);
        let nft = Nft::new(
            nft_id,
            U256::one(),
            bounded_unique_external_ref(),
            nft_owner_account_id.clone(),
        );
        let market = NftSaleType::Ethereum;

        <Nfts<T>>::insert(nft_id, nft.clone());
        let original_nonce = Nfts::<T>::get(nft_id).unwrap().nonce;
    }: _(
        RawOrigin::<T::AccountId>::Signed(nft_owner_account_id),
        nft_id,
        market.clone()
    )
    verify {
        assert_eq!(original_nonce + 1u64, Nfts::<T>::get(&nft_id).unwrap().nonce);
        assert_eq!(true, <NftOpenForSale<T>>::contains_key(&nft_id));
        assert_last_event::<T>(Event::<T>::NftOpenForSale{ nft_id: nft_id, sale_type: market }.into());
    }

    signed_list_nft_open_for_sale {
        let open_for_sale: ListNftOpenForSale<T> = ListNftOpenForSale::new().setup();
        let original_nonce = Nfts::<T>::get(open_for_sale.nft_id).unwrap().nonce;
        let proof: Proof<T::Signature, T::AccountId> = get_proof::<T>(
            open_for_sale.nft_owner.clone(),
            open_for_sale.relayer.clone(),
            &open_for_sale.signature
        );
    }: _(
        RawOrigin::<T::AccountId>::Signed(open_for_sale.nft_owner),
        proof,
        open_for_sale.nft_id,
        open_for_sale.market
    )
    verify {
        assert_eq!(original_nonce + 1u64, Nfts::<T>::get(&open_for_sale.nft_id).unwrap().nonce);
        assert_eq!(true, <NftOpenForSale<T>>::contains_key(&open_for_sale.nft_id));
        assert_last_event::<T>(Event::<T>::NftOpenForSale{ nft_id: open_for_sale.nft_id, sale_type: open_for_sale.market }.into());
    }

    signed_transfer_fiat_nft {
        let transfer_fiat_nft: TransferFiatNft<T> = TransferFiatNft::new().setup();
        let original_nonce = Nfts::<T>::get(transfer_fiat_nft.nft_id).unwrap().nonce;
        let proof: Proof<T::Signature, T::AccountId> = get_proof::<T>(
            transfer_fiat_nft.nft_owner.clone(),
            transfer_fiat_nft.relayer.clone(),
            &transfer_fiat_nft.signature
        );
    }: _(
        RawOrigin::<T::AccountId>::Signed(transfer_fiat_nft.nft_owner.clone()),
        proof,
        transfer_fiat_nft.nft_id,
        transfer_fiat_nft.t2_transfer_to_public_key
    )
    verify {
        assert_eq!(original_nonce + 1u64, Nfts::<T>::get(&transfer_fiat_nft.nft_id).unwrap().nonce);
        assert_eq!(false, <NftOpenForSale<T>>::contains_key(&transfer_fiat_nft.nft_id));
        assert_eq!(transfer_fiat_nft.new_nft_owner_account, Nfts::<T>::get(&transfer_fiat_nft.nft_id).unwrap().owner);
        assert_last_event::<T>(Event::<T>::FiatNftTransfer {
            nft_id: transfer_fiat_nft.nft_id,
            sender: transfer_fiat_nft.nft_owner,
            new_owner: transfer_fiat_nft.new_nft_owner_account,
            sale_type: NftSaleType::Fiat,
            op_id: transfer_fiat_nft.op_id
        }.into());
    }

    signed_cancel_list_fiat_nft {
        let cancel_list_fiat_nft: CancelListFiatNft<T> = CancelListFiatNft::new().setup();
        let original_nonce = Nfts::<T>::get(cancel_list_fiat_nft.nft_id).unwrap().nonce;
        let proof: Proof<T::Signature, T::AccountId> = get_proof::<T>(
            cancel_list_fiat_nft.nft_owner.clone(),
            cancel_list_fiat_nft.relayer.clone(),
            &cancel_list_fiat_nft.signature
        );
    }: _(
        RawOrigin::<T::AccountId>::Signed(cancel_list_fiat_nft.nft_owner.clone()),
        proof,
        cancel_list_fiat_nft.nft_id
    )
    verify {
        assert_eq!(original_nonce + 1u64, Nfts::<T>::get(&cancel_list_fiat_nft.nft_id).unwrap().nonce);
        assert_eq!(false, <NftOpenForSale<T>>::contains_key(&cancel_list_fiat_nft.nft_id));
        assert_eq!(cancel_list_fiat_nft.nft_owner, Nfts::<T>::get(&cancel_list_fiat_nft.nft_id).unwrap().owner);
        assert_last_event::<T>(Event::<T>::CancelSingleFiatNftListing {
            nft_id: cancel_list_fiat_nft.nft_id,
            sale_type: NftSaleType::Fiat,
            op_id: cancel_list_fiat_nft.op_id
        }.into());
    }

    proxy_signed_mint_single_nft {
        let r in 1 .. MAX_NUMBER_OF_ROYALTIES;
        let mint_nft: MintSingleNft<T> = MintSingleNft::new(r).setup();
        let call: <T as Config>::RuntimeCall = mint_nft.generate_signed_mint_single_nft();
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
    }: proxy(RawOrigin::<T::AccountId>::Signed(mint_nft.relayer.clone()), boxed_call)
    verify {
        assert_eq!(true, Nfts::<T>::contains_key(&mint_nft.nft_id));
        assert_eq!(
            Nft::new(mint_nft.nft_id, mint_nft.info_id, mint_nft.unique_external_ref.clone(), mint_nft.nft_owner.clone()),
            Nfts::<T>::get(&mint_nft.nft_id).unwrap()
        );
        assert_eq!(true, NftInfos::<T>::contains_key(&mint_nft.info_id));
        assert_eq!(
            NftInfo::new(mint_nft.info_id, bounded_royalties(mint_nft.royalties.clone()), mint_nft.t1_authority),
            <NftInfos<T>>::get(&mint_nft.info_id).unwrap()
        );
        assert_eq!(true, <UsedExternalReferences<T>>::contains_key(&mint_nft.unique_external_ref));
        assert_eq!(true, <UsedExternalReferences<T>>::get(mint_nft.unique_external_ref));
        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: mint_nft.relayer.clone(), hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::SingleNftMinted {
            nft_id: mint_nft.nft_id,
            owner: mint_nft.nft_owner,
            authority: mint_nft.t1_authority
        }.into(), 2);
    }

    proxy_signed_list_nft_open_for_sale {
        let open_for_sale: ListNftOpenForSale<T> = ListNftOpenForSale::new().setup();
        let original_nonce = Nfts::<T>::get(open_for_sale.nft_id).unwrap().nonce;
        let call: <T as Config>::RuntimeCall = open_for_sale.generate_signed_list_nft_open_for_sale_call();
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
    }: proxy(RawOrigin::<T::AccountId>::Signed(open_for_sale.relayer.clone()), boxed_call)
    verify {
        assert_eq!(original_nonce + 1u64, Nfts::<T>::get(&open_for_sale.nft_id).unwrap().nonce);
        assert_eq!(true, <NftOpenForSale<T>>::contains_key(&open_for_sale.nft_id));
        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: open_for_sale.relayer.clone(), hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::NftOpenForSale{ nft_id: open_for_sale.nft_id, sale_type: open_for_sale.market }.into(), 2);
    }

    proxy_signed_transfer_fiat_nft {
        let transfer_fiat_nft: TransferFiatNft<T> = TransferFiatNft::new().setup();
        let original_nonce = Nfts::<T>::get(transfer_fiat_nft.nft_id).unwrap().nonce;
        let call: <T as Config>::RuntimeCall = transfer_fiat_nft.generate_signed_transfer_fiat_nft_call();
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
    }: proxy(RawOrigin::<T::AccountId>::Signed(transfer_fiat_nft.relayer.clone()), boxed_call)
    verify {
        assert_eq!(original_nonce + 1u64, Nfts::<T>::get(&transfer_fiat_nft.nft_id).unwrap().nonce);
        assert_eq!(false, <NftOpenForSale<T>>::contains_key(&transfer_fiat_nft.nft_id));
        assert_eq!(transfer_fiat_nft.new_nft_owner_account, Nfts::<T>::get(&transfer_fiat_nft.nft_id).unwrap().owner);
        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: transfer_fiat_nft.relayer.clone(), hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::FiatNftTransfer {
            nft_id: transfer_fiat_nft.nft_id,
            sender: transfer_fiat_nft.nft_owner,
            new_owner: transfer_fiat_nft.new_nft_owner_account,
            sale_type: NftSaleType::Fiat,
            op_id: transfer_fiat_nft.op_id
        }.into(), 2);
    }

    proxy_signed_cancel_list_fiat_nft {
        let cancel_list_fiat_nft: CancelListFiatNft<T> = CancelListFiatNft::new().setup();
        let original_nonce = Nfts::<T>::get(cancel_list_fiat_nft.nft_id).unwrap().nonce;
        let call: <T as Config>::RuntimeCall = cancel_list_fiat_nft.generate_signed_cancel_list_fiat_nft_call();
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
    }: proxy(RawOrigin::<T::AccountId>::Signed(cancel_list_fiat_nft.relayer.clone()), boxed_call)
    verify {
        assert_eq!(original_nonce + 1u64, Nfts::<T>::get(&cancel_list_fiat_nft.nft_id).unwrap().nonce);
        assert_eq!(false, <NftOpenForSale<T>>::contains_key(&cancel_list_fiat_nft.nft_id));
        assert_eq!(cancel_list_fiat_nft.nft_owner, Nfts::<T>::get(&cancel_list_fiat_nft.nft_id).unwrap().owner);
        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: cancel_list_fiat_nft.relayer.clone(), hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::CancelSingleFiatNftListing {
            nft_id: cancel_list_fiat_nft.nft_id,
            sale_type: NftSaleType::Fiat,
            op_id: cancel_list_fiat_nft.op_id
        }.into(), 2);
    }

    proxy_signed_create_batch {
        let r in 1 .. T::BatchBound::get();
        let batch: CreateBatch<T> = CreateBatch::new(r);
        let call: <T as Config>::RuntimeCall = batch.generate_signed_create_batch();
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
        let expected_batch_id = generate_batch_id::<T>(<NextSingleNftUniqueId<T>>::get());
    }: proxy(RawOrigin::<T::AccountId>::Signed(batch.relayer.clone()), boxed_call)
    verify {
        assert_eq!(true, <BatchInfoId<T>>::contains_key(expected_batch_id));

        let info = <NftInfos<T>>::get(<BatchInfoId<T>>::get(expected_batch_id)).unwrap();
        assert_eq!(Some(expected_batch_id), info.batch_id);
        assert_eq!(batch.total_supply, info.total_supply);
        assert_eq!(Some(batch.creator_account_id.clone()), info.creator);

        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: batch.relayer.clone(), hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::BatchCreated {
            batch_nft_id: expected_batch_id,
            total_supply: batch.total_supply,
            batch_creator: batch.creator_account_id,
            authority: batch.t1_authority
        }.into(), 2);
    }

    proxy_signed_mint_batch_nft {
        let index = 0u64;
        let context: MintBatchNft<T> = MintBatchNft::new();
        let call: <T as Config>::RuntimeCall = context.generate_signed_mint_batch_nft(context.batch_id, index);
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
    }: proxy(RawOrigin::<T::AccountId>::Signed(context.relayer.clone()), boxed_call)
    verify {
        assert_eq!(<NftBatches<T>>::get(context.batch_id)[0], context.nft_id);
        assert_eq!(true, Nfts::<T>::contains_key(&context.nft_id));
        assert_eq!(
            Nft::new(context.nft_id, U256::zero(), context.unique_external_ref.clone(), context.nft_owner.clone()),
            Nfts::<T>::get(&context.nft_id).unwrap()
        );
        assert_eq!(true, <UsedExternalReferences<T>>::contains_key(&context.unique_external_ref));
        assert_eq!(true, <UsedExternalReferences<T>>::get(context.unique_external_ref));

        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: context.relayer.clone(), hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::BatchNftMinted {
            nft_id: context.nft_id,
            batch_nft_id: context.batch_id,
            authority: context.t1_authority,
            owner: context.nft_owner,
        }.into(), 2);
    }

    proxy_signed_list_batch_for_sale {
        let context: ListBatch<T> = ListBatch::new();
        let call: <T as Config>::RuntimeCall = context.generate_signed_list_batch();
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
    }: proxy(RawOrigin::<T::AccountId>::Signed(context.relayer.clone()), boxed_call)
    verify {

        assert_eq!(true, <BatchOpenForSale<T>>::contains_key(&context.batch_id));
        assert_eq!(<BatchOpenForSale<T>>::get(&context.batch_id), context.market);

        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: context.relayer.clone(), hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::BatchOpenForSale{ batch_nft_id: context.batch_id, sale_type: context.market }.into(), 2);
    }

    proxy_signed_end_batch_sale {
        let context: EndBatchSale<T> = EndBatchSale::new();
        let call: <T as Config>::RuntimeCall = context.generate_signed_end_batch_sale();
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
    }: proxy(RawOrigin::<T::AccountId>::Signed(context.relayer.clone()), boxed_call)
    verify {
        assert_eq!(false, <BatchOpenForSale<T>>::contains_key(&context.batch_id));
        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: context.relayer.clone(), hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::BatchSaleEnded{ batch_nft_id: context.batch_id, sale_type: context.market }.into(), 2);
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().as_externality(),
    crate::mock::TestRuntime,
);
