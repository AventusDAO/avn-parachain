use crate::{
    Config, FailedLowerProofs, LowerProofData, LowerSchedulePeriod, LowersPendingProof,
    LowersReadyToClaim, Pallet,
};
use frame_support::{
    pallet_prelude::{PhantomData, StorageVersion},
    traits::{Get, GetStorageVersion, OnRuntimeUpgrade},
    weights::Weight,
};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_avn_common::eth::{LowerParams, PACKED_LOWER_V1_PARAMS_SIZE, PACKED_LOWER_V2_PARAMS_SIZE};

#[cfg(feature = "try-runtime")]
use crate::Vec;

#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(2);

mod legacy {
    use super::PACKED_LOWER_V1_PARAMS_SIZE;
    use crate::LowerDataLimit;
    use frame_support::BoundedVec;

    pub type LowerParamsV1 = [u8; PACKED_LOWER_V1_PARAMS_SIZE];

    #[derive(codec::Encode, codec::Decode, Clone, PartialEq, Eq, Debug)]
    pub struct LowerProofDataV1 {
        pub params: LowerParamsV1,
        pub encoded_lower_data: BoundedVec<u8, LowerDataLimit>,
    }
}

fn expand_lower_v1_to_v2(v1: &legacy::LowerParamsV1) -> LowerParams {
    let mut v2: LowerParams = [0u8; PACKED_LOWER_V2_PARAMS_SIZE];
    v2[..PACKED_LOWER_V1_PARAMS_SIZE].copy_from_slice(v1);
    v2
}

pub fn set_lower_schedule_period<T: Config>() -> Weight {
    let default_lower_schedule_period: BlockNumberFor<T> = 3275u32.into(); // ~ 12 hrs
    let mut consumed_weight: Weight = Weight::from_parts(0 as u64, 0);
    let mut add_weight = |reads, writes, weight: Weight| {
        consumed_weight += T::DbWeight::get().reads_writes(reads, writes);
        consumed_weight += weight;
    };

    log::info!("🚧 🚧 Running migration to set lower schedule period");

    <LowerSchedulePeriod<T>>::put(default_lower_schedule_period);

    //Write: [LowerSchedulePeriod, STORAGE_VERSION]
    add_weight(0, 2, Weight::zero());
    STORAGE_VERSION.put::<Pallet<T>>();

    log::info!("✅ Lower schedule period successfully");

    // add a bit extra as safety margin for computation
    return consumed_weight + Weight::from_parts(25_000_000 as u64, 0)
}

pub fn translate_lower_data<T: Config>() -> Weight {
    use legacy::{LowerParamsV1, LowerProofDataV1};

    let mut consumed_weight: Weight = Weight::zero();
    let mut add_weight = |reads, writes, weight: Weight| {
        consumed_weight += T::DbWeight::get().reads_writes(reads, writes);
        consumed_weight += weight;
    };

    log::info!("🚧 🚧 Running migration to translate FailedLowerProofs, LowersPendingProof, and LowersReadyToClaim lower data from V1 to V2");

    FailedLowerProofs::<T>::translate::<LowerParamsV1, _>(|_lower_id, v1_lower_params| {
        add_weight(1, 1, Weight::zero());
        let v2_lower_params = expand_lower_v1_to_v2(&v1_lower_params);
        Some(v2_lower_params)
    });

    LowersPendingProof::<T>::translate::<LowerParamsV1, _>(|_lower_id, v1_lower_params| {
        add_weight(1, 1, Weight::zero());
        let v2_lower_params = expand_lower_v1_to_v2(&v1_lower_params);
        Some(v2_lower_params)
    });

    LowersReadyToClaim::<T>::translate::<LowerProofDataV1, _>(|_lower_id, v1_proof| {
        add_weight(1, 1, Weight::zero());
        let v2_lower_params = expand_lower_v1_to_v2(&v1_proof.params);
        let v2_proof = LowerProofData {
            params: v2_lower_params,
            encoded_lower_data: v1_proof.encoded_lower_data,
        };
        Some(v2_proof)
    });

    add_weight(0, 1, Weight::zero());
    STORAGE_VERSION.put::<Pallet<T>>();

    log::info!("✅ FailedLowerProofs, LowersPendingProof, and LowersReadyToClaim lower data translated from V1 to V2");

    // add a bit extra as safety margin for computation
    consumed_weight + Weight::from_parts(50_000_000 as u64, 0)
}

/// Migration to enable staking pallet and translate lower data from V1 into V2
pub struct SetLowerSchedulePeriod<T>(PhantomData<T>);
impl<T: Config> OnRuntimeUpgrade for SetLowerSchedulePeriod<T> {
    fn on_runtime_upgrade() -> Weight {
        let current = Pallet::<T>::current_storage_version();
        let onchain = Pallet::<T>::on_chain_storage_version();
        let mut total_weight = Weight::zero();

        if onchain < 1 {
            log::info!(
                "💽 Running Token manager migration with current storage version {:?} / onchain {:?}",
                current,
                onchain
            );
            total_weight += set_lower_schedule_period::<T>();
        }

        if onchain < 2 {
            log::info!(
                "💽 Running lower data V1 to V2 migration with current storage version {:?} / onchain {:?}",
                current,
                onchain
            );
            total_weight += translate_lower_data::<T>();
        }

        total_weight
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        use codec::Encode;

        Ok(<LowerSchedulePeriod<T>>::get().encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(input: Vec<u8>) -> Result<(), TryRuntimeError> {
        use codec::Decode;
        use sp_runtime::traits::Zero;

        let initial_lower_schedule_period: BlockNumberFor<T> =
            Decode::decode(&mut input.as_slice()).expect("Initial lower schedule is invalid");
        if initial_lower_schedule_period == BlockNumberFor::<T>::zero() {
            assert_eq!(initial_lower_schedule_period, 3275u32.into());
            log::info!(
                "💽 lower_schedule_period updated successfully to {:?}",
                <LowerSchedulePeriod<T>>::get()
            );
        } else {
            log::info!(
                "💽 lower_schedule_period was not updated because it had a non zero value: {:?}",
                initial_lower_schedule_period
            );
        }

        Ok(())
    }
}
