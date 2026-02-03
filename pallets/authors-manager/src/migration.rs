use crate::*;
use frame_support::{
    pallet_prelude::*,
    traits::{Get, GetStorageVersion, OnRuntimeUpgrade},
    weights::Weight,
};

#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

pub struct AuthorsManagerMigrations<T>(PhantomData<T>);
impl<T: Config> OnRuntimeUpgrade for AuthorsManagerMigrations<T> {
    fn on_runtime_upgrade() -> Weight {
        let current = Pallet::<T>::current_storage_version();
        let onchain = Pallet::<T>::on_chain_storage_version();

        log::info!(
            "ℹ️  AuthorsManager migration invoked with current storage version {:?} / onchain {:?}",
            current,
            onchain
        );

        let mut consumed_weight = Weight::zero();

        if onchain < 1 {
            consumed_weight.saturating_accrue(populate_reverse_map::<T>());
        }

        consumed_weight
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        Ok([0u8; 0].to_vec())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(_input: Vec<u8>) -> Result<(), TryRuntimeError> {
        frame_support::ensure!(
            Pallet::<T>::on_chain_storage_version() >= STORAGE_VERSION,
            "AuthorsManager storage version not bumped"
        );

        // Verify reverse map correctness
        for (eth_key, account_id) in <EthereumPublicKeys<T>>::iter() {
            let reverse = <AccountIdToEthereumKeys<T>>::get(&account_id)
                .ok_or(TryRuntimeError::Other("Missing reverse mapping".into()))?;
            frame_support::ensure!(reverse == eth_key, "Mismatched reverse mapping");
        }

        Ok(())
    }
}

fn populate_reverse_map<T: Config>() -> Weight {
    let mut reads: u64 = 0;
    let mut writes: u64 = 0;

    for (eth_key, account_id) in <EthereumPublicKeys<T>>::iter() {
        reads += 1;
        <AccountIdToEthereumKeys<T>>::insert(&account_id, eth_key);
        writes += 1;
    }

    // Bump storage version
    STORAGE_VERSION.put::<Pallet<T>>();
    writes += 1;

    T::DbWeight::get().reads_writes(reads, writes)
}
