extern crate alloc;

use super::PALLET_MIGRATIONS_ID;
use crate::{
    pallet::{Config, VotesRepository},
    RootId,
};
use frame_support::{
    migrations::{MigrationId, SteppedMigration, SteppedMigrationError},
    pallet_prelude::PhantomData,
    weights::WeightMeter,
};
use frame_system::pallet_prelude::BlockNumberFor;
use pallet_avn::vote::VotingSessionData;

mod tests;
pub mod weights;
/// Before running this migration, the storage alias defined here represents the
/// `on_chain` storage.
// This module is public only for the purposes of linking it in the documentation. It is not
// intended to be used by any other code.
pub mod v0 {
    use super::{BlockNumberFor, Config, RootId};
    use crate::{pallet::Pallet, Decode, Encode, MaxEncodedLen, TypeInfo};
    use frame_support::{
        pallet_prelude::{ValueQuery, Zero},
        storage_alias, Blake2_128Concat, BoundedVec,
    };
    use sp_avn_common::bounds::{MaximumValidatorsBound, VotingSessionIdBound};
    use sp_runtime::WeakBoundedVec;

    #[derive(PartialEq, Eq, Clone, Encode, Decode, Debug, TypeInfo, MaxEncodedLen)]
    pub struct VotingSessionDataV0<AccountId, BlockNumber> {
        /// The unique identifier for this voting session
        pub voting_session_id: WeakBoundedVec<u8, VotingSessionIdBound>,
        /// The number of approval votes that are needed to reach an outcome.
        pub threshold: u32,
        /// The current set of voters that approved it.
        pub ayes: BoundedVec<AccountId, MaximumValidatorsBound>,
        /// The current set of voters that rejected it.
        pub nays: BoundedVec<AccountId, MaximumValidatorsBound>,
        /// The hard end time of this vote.
        pub end_of_voting_period: BlockNumber,
        /// The block number this session was created on
        pub created_at_block: BlockNumber,
    }

    impl<AccountId, BlockNumber: Zero> Default for VotingSessionDataV0<AccountId, BlockNumber> {
        fn default() -> Self {
            Self {
                voting_session_id: WeakBoundedVec::default(),
                threshold: 0u32,
                ayes: BoundedVec::default(),
                nays: BoundedVec::default(),
                end_of_voting_period: Zero::zero(),
                created_at_block: Zero::zero(),
            }
        }
    }

    impl<AccountId, BlockNumber> Into<super::VotingSessionData<AccountId, BlockNumber>>
        for VotingSessionDataV0<AccountId, BlockNumber>
    {
        fn into(self) -> super::VotingSessionData<AccountId, BlockNumber> {
            super::VotingSessionData {
                voting_session_id: BoundedVec::truncate_from(self.voting_session_id.into_inner()),
                threshold: self.threshold,
                ayes: self.ayes,
                nays: self.nays,
                end_of_voting_period: self.end_of_voting_period,
                created_at_block: self.created_at_block,
            }
        }
    }

    #[storage_alias]
    pub type VotesRepository<T: Config<I>, I: 'static> = StorageMap<
        Pallet<T, I>,
        Blake2_128Concat,
        RootId<BlockNumberFor<T>>,
        VotingSessionDataV0<<T as frame_system::Config>::AccountId, BlockNumberFor<T>>,
        ValueQuery,
    >;
}
pub struct LazyVotingDataMigrationV1<T: Config<I>, I: 'static, W: weights::WeightInfo>(
    PhantomData<(T, I, W)>,
);
impl<T: Config<I>, I: 'static, W: weights::WeightInfo> SteppedMigration
    for LazyVotingDataMigrationV1<T, I, W>
{
    type Cursor = RootId<BlockNumberFor<T>>;
    // Without the explicit length here the construction of the ID would not be infallible.
    type Identifier = MigrationId<18>;

    /// The identifier of this migration. Which should be globally unique.
    fn id() -> Self::Identifier {
        MigrationId { pallet_id: *PALLET_MIGRATIONS_ID, version_from: 0, version_to: 1 }
    }

    /// The actual logic of the migration.
    ///
    /// This function is called repeatedly until it returns `Ok(None)`, indicating that the
    /// migration is complete. Ideally, the migration should be designed in such a way that each
    /// step consumes as much weight as possible. However, this is simplified to perform one stored
    /// value mutation per block.
    fn step(
        mut cursor: Option<Self::Cursor>,
        meter: &mut WeightMeter,
    ) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
        let required = W::step();
        // If there is not enough weight for a single step, return an error. This case can be
        // problematic if it is the first migration that ran in this block. But there is nothing
        // that we can do about it here.
        if meter.remaining().any_lt(required) {
            return Err(SteppedMigrationError::InsufficientWeight { required })
        }

        // We loop here to do as much progress as possible per step.
        loop {
            if meter.try_consume(required).is_err() {
                break
            }

            let mut iter = if let Some(last_key) = cursor {
                // If a cursor is provided, start iterating from the stored value
                // corresponding to the last key processed in the previous step.
                // Note that this only works if the old and the new map use the same way to hash
                // storage keys.
                v0::VotesRepository::<T, I>::iter_from(v0::VotesRepository::<T, I>::hashed_key_for(
                    last_key,
                ))
            } else {
                // If no cursor is provided, start iterating from the beginning.
                v0::VotesRepository::<T, I>::iter()
            };

            // If there's a next item in the iterator, perform the migration.
            if let Some((last_key, old_data)) = iter.next() {
                let new_data: VotingSessionData<_, _> = old_data.into();

                // We can just insert here since the old and the new map share the same key-space.
                // Otherwise it would have to invert the concat hash function and re-hash it.
                VotesRepository::<T, I>::insert(last_key, new_data);
                cursor = Some(last_key) // Return the processed key as the new cursor.
            } else {
                cursor = None; // Signal that the migration is complete (no more items to process).
                break
            }
        }
        Ok(cursor)
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::DispatchError> {
        use crate::Pallet;
        use codec::Encode;
        use frame_support::{ensure, traits::GetStorageVersion};

        let on_chain_version = Pallet::<T, I>::in_code_storage_version();
        ensure!(
            on_chain_version < 1,
            "VotesRepository::LazyVotingDataMigrationV1 migration can be deleted"
        );

        let number_of_voting_sessions = v0::VotesRepository::<T, I>::iter_keys().count();
        log::info!("Number of voting sessions before migration: {number_of_voting_sessions}");
        Ok((number_of_voting_sessions as u32).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(prev: Vec<u8>) -> Result<(), frame_support::sp_runtime::TryRuntimeError> {
        use codec::Decode;

        // Check the state of the storage after the migration.
        let prev_size =
            u32::decode(&mut &prev[..]).expect("Failed to decode the previous storage count");

        // Check the len of prev and post are the same.
        assert_eq!(
			VotesRepository::<T, I>::iter().count() as u32,
			prev_size,
			"Migration failed: the number of items in the storage after the migration is not the same as before"
		);

        // Just ensure the value is decoding with the truncated session_id
        for (_key, _value) in VotesRepository::<T, I>::iter() {}

        Ok(())
    }
}
