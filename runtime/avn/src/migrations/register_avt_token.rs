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

//! Migration to register the native AVT token in `orml_asset_registry`.
//!
//! Reads the AVT contract address from `pallet_token_manager::AVTTokenContract` (which is set
//! per-environment at genesis) and registers `Asset::Avt` with an `AvnAssetLocation::Ethereum`
//! location. The migration is idempotent: if the asset is already registered it is a no-op.

use orml_traits::asset_registry::{AssetMetadata, AvnAssetLocation, AvnAssetMetadata};
use polkadot_sdk::{
    frame_support::{pallet_prelude::PhantomData, traits::OnRuntimeUpgrade, weights::Weight},
    frame_system, sp_core,
};
use sp_avn_common::Asset;

#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

use crate::{Balance, Runtime, EXISTENTIAL_DEPOSIT};

pub struct RegisterAvtToken<T>(PhantomData<T>);

impl OnRuntimeUpgrade for RegisterAvtToken<Runtime> {
    fn on_runtime_upgrade() -> Weight {
        let mut weight = <Runtime as frame_system::Config>::DbWeight::get().reads(1);

        // Check if AVT is already registered – if so, skip.
        if orml_asset_registry::Metadata::<Runtime>::get(&Asset::Avt).is_some() {
            log::info!("⏭️  RegisterAvtToken migration: Asset::Avt already registered, skipping.");
            return weight
        }

        // Read the contract address set at genesis for this environment.
        let avt_contract = pallet_token_manager::AVTTokenContract::<Runtime>::get();
        weight += <Runtime as frame_system::Config>::DbWeight::get().reads(1);

        let metadata = build_avt_metadata(avt_contract);

        match orml_asset_registry::Pallet::<Runtime>::do_register_asset(metadata, Some(Asset::Avt))
        {
            Ok(()) => {
                weight += <Runtime as frame_system::Config>::DbWeight::get().writes(2); // Metadata + LocationToAssetId
                log::info!(
                    "✅ RegisterAvtToken migration: Asset::Avt registered with contract {:?}",
                    avt_contract,
                );
            },
            Err(e) => {
                log::error!(
                    "❌ RegisterAvtToken migration: failed to register Asset::Avt. Error: {:?}",
                    e,
                );
            },
        }

        weight
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, TryRuntimeError> {
        use codec::Encode;

        let already_registered =
            orml_asset_registry::Metadata::<Runtime>::get(&Asset::Avt).is_some();
        let contract = pallet_token_manager::AVTTokenContract::<Runtime>::get();

        log::info!(
            "🔍 RegisterAvtToken pre_upgrade: already_registered={}, contract={:?}",
            already_registered,
            contract,
        );

        Ok(contract.encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), TryRuntimeError> {
        use codec::Decode;

        let contract: sp_core::H160 = Decode::decode(&mut state.as_slice()).map_err(|_| {
            TryRuntimeError::Other("RegisterAvtToken: failed to decode pre-upgrade state")
        })?;

        let metadata = orml_asset_registry::Metadata::<Runtime>::get(&Asset::Avt).ok_or(
            TryRuntimeError::Other("RegisterAvtToken: Asset::Avt not found after migration"),
        )?;

        let expected_location = AvnAssetLocation::Ethereum(contract);
        if metadata.location.as_ref() != Some(&expected_location) {
            return Err(TryRuntimeError::Other(
                "RegisterAvtToken: registered location does not match AVTTokenContract",
            ))
        }

        log::info!("✅ RegisterAvtToken post_upgrade: Asset::Avt correctly registered.");
        Ok(())
    }
}

fn build_avt_metadata(
    contract: sp_core::H160,
) -> AssetMetadata<
    Balance,
    AvnAssetMetadata,
    AvnAssetLocation,
    crate::configs::AssetRegistryStringLimit,
> {
    AssetMetadata {
        decimals: 18,
        name: b"Aventus".to_vec().try_into().expect("name fits StringLimit"),
        symbol: b"AVT".to_vec().try_into().expect("symbol fits StringLimit"),
        existential_deposit: EXISTENTIAL_DEPOSIT,
        location: Some(AvnAssetLocation::Ethereum(contract)),
        additional: AvnAssetMetadata { appchain_native: false },
    }
}
