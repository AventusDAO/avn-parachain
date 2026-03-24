use crate::{Config, Pallet, STORAGE_VERSION};
use frame_support::{
    storage::migration::clear_storage_prefix,
    traits::{Get, OnRuntimeUpgrade, StorageVersion},
    weights::Weight,
};

/// Migration v1 -> v2: remove the `ChainData` storage map.
///
/// `ChainData` stored a (chain_id, name) pair keyed by ChainId. The name field is no longer
/// needed at runtime; chain registration now goes through `register_appchain` which stores the
/// name in the asset registry. This migration clears all on-chain entries before the
/// storage type is removed from the pallet.
pub mod v2 {
    use super::*;

    pub struct Migration<T>(core::marker::PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            let current = StorageVersion::get::<Pallet<T>>();
            log::warn!("🚧 🚧 Running avn-anchor v2 migration. Current version: {:?}", current);

            if current != 1 {
                log::warn!(
                    "🚧 🚧 v2 migration skipped: expected storage version 1, found {:?}",
                    current,
                );
                return T::DbWeight::get().reads(1)
            }

            let result = clear_storage_prefix(b"AvnAnchor", b"ChainData", b"", None, None);

            log::info!("✅ v2 migration: removed {} ChainData entries", result.unique,);

            STORAGE_VERSION.put::<Pallet<T>>();

            T::DbWeight::get().reads_writes(1 + result.unique as u64, 1 + result.unique as u64)
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<sp_std::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
            frame_support::ensure!(
                StorageVersion::get::<Pallet<T>>() == 1,
                sp_runtime::TryRuntimeError::Other("expected storage version 1 before migration")
            );
            Ok(sp_std::vec![])
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(_state: sp_std::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
            frame_support::ensure!(
                StorageVersion::get::<Pallet<T>>() == 2,
                sp_runtime::TryRuntimeError::Other("storage version not bumped to 2")
            );
            // Verify ChainData is empty (non-destructive: check if any key still shares the prefix)
            let prefix = frame_support::storage::storage_prefix(b"AvnAnchor", b"ChainData");
            let is_empty =
                sp_io::storage::next_key(&prefix).map_or(true, |next| !next.starts_with(&prefix));
            frame_support::ensure!(
                is_empty,
                sp_runtime::TryRuntimeError::Other("ChainData storage was not fully cleared")
            );
            Ok(())
        }
    }
}
