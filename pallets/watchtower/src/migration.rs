// Copyright 2026 Aventus DAO.

use crate::{Config, Pallet, Payload, Proposal, Proposals, STORAGE_VERSION};
use codec::{Decode, Encode};
use frame_support::{
    pallet_prelude::{BoundedVec, PhantomData},
    traits::{Get, GetStorageVersion, OnRuntimeUpgrade, StorageVersion},
    weights::Weight,
};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_core::H256;
use sp_runtime::Perbill;
use sp_watchtower::{DecisionRule, ProposalSource};

#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;
#[cfg(feature = "try-runtime")]
use sp_std::vec::Vec;

/// Migration v0 -> v1: `Proposal` gained `committee_size` and `DecisionRule` was widened. In-flight
/// proposals (queued, active or awaiting cleanup) are re-encoded with the legacy values so they
/// keep decoding.
pub mod v1 {
    use super::*;

    const V1_STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

    /// `DecisionRule` as stored by storage version 0. It was only consulted at expiry and meant
    /// "expire unresolved" for internal proposals and "simple majority of cast votes" for
    /// external ones.
    #[derive(Encode, Decode, Debug, Clone, PartialEq, Eq)]
    pub enum DecisionRuleV0 {
        SimpleMajority,
    }

    impl DecisionRuleV0 {
        pub fn into_v1(self, source: &ProposalSource) -> DecisionRule {
            match source {
                ProposalSource::Internal(_) => DecisionRule::ExpireUnresolved,
                ProposalSource::External => DecisionRule::SimpleMajorityOnExpiry,
            }
        }
    }

    /// `Proposal<T>` as stored by storage version 0.
    #[derive(Encode, Decode)]
    pub struct ProposalV0<T: Config> {
        pub title: BoundedVec<u8, T::MaxTitleLen>,
        pub payload: Payload<T>,
        pub threshold: Perbill,
        pub source: ProposalSource,
        pub decision_rule: DecisionRuleV0,
        pub external_ref: H256,
        pub proposer: Option<T::AccountId>,
        pub created_at: BlockNumberFor<T>,
        pub vote_duration: u32,
        pub end_at: Option<BlockNumberFor<T>>,
    }

    pub struct Migration<T>(core::marker::PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            let current = StorageVersion::get::<Pallet<T>>();
            log::warn!("🚧 🚧 Running watchtower v1 migration. Current version: {:?}", current);

            if current != 0 {
                log::warn!(
                    "🚧 🚧 v1 migration skipped: expected storage version 0, found {:?}",
                    current,
                );
                return T::DbWeight::get().reads(1)
            }

            let mut translated = 0u64;
            Proposals::<T>::translate::<ProposalV0<T>, _>(|_, old| {
                translated = translated.saturating_add(1);
                let decision_rule = old.decision_rule.into_v1(&old.source);
                Some(Proposal {
                    title: old.title,
                    payload: old.payload,
                    threshold: old.threshold,
                    source: old.source,
                    decision_rule,
                    external_ref: old.external_ref,
                    proposer: old.proposer,
                    created_at: old.created_at,
                    vote_duration: old.vote_duration,
                    end_at: old.end_at,
                    committee_size: None,
                })
            });

            V1_STORAGE_VERSION.put::<Pallet<T>>();
            log::info!("✅ v1 migration: translated {} proposals", translated);

            T::DbWeight::get().reads_writes(1 + translated, 1 + translated)
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            frame_support::ensure!(
                StorageVersion::get::<Pallet<T>>() == 0,
                TryRuntimeError::Other("expected storage version 0 before migration")
            );
            let count = Proposals::<T>::iter_keys().count() as u32;
            Ok(count.encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let before = u32::decode(&mut &state[..])
                .map_err(|_| TryRuntimeError::Other("invalid pre-upgrade state"))?;
            frame_support::ensure!(
                StorageVersion::get::<Pallet<T>>() == 1,
                TryRuntimeError::Other("expected storage version 1 after migration")
            );
            // Every entry must decode as the new type, and none may have been dropped.
            let after = Proposals::<T>::iter().count() as u32;
            frame_support::ensure!(
                before == after,
                TryRuntimeError::Other("proposal count changed during migration")
            );
            frame_support::ensure!(
                Proposals::<T>::iter_values().all(|p| p.committee_size.is_none() &&
                    p.decision_rule == DecisionRuleV0::SimpleMajority.into_v1(&p.source)),
                TryRuntimeError::Other("translated proposals must use legacy defaults")
            );
            Ok(())
        }
    }
}

/// All watchtower migrations. Wire this into the runtime `Migrations` tuple.
pub struct WatchtowerMigrations<T>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for WatchtowerMigrations<T> {
    fn on_runtime_upgrade() -> Weight {
        let onchain = Pallet::<T>::on_chain_storage_version();
        let mut weight = T::DbWeight::get().reads(1);

        if onchain < 1 {
            weight = weight.saturating_add(v1::Migration::<T>::on_runtime_upgrade());
        }

        debug_assert!(Pallet::<T>::on_chain_storage_version() == STORAGE_VERSION);
        weight
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        if Pallet::<T>::on_chain_storage_version() < 1 {
            return v1::Migration::<T>::pre_upgrade()
        }
        Ok(sp_std::vec![])
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        frame_support::ensure!(
            Pallet::<T>::on_chain_storage_version() == STORAGE_VERSION,
            TryRuntimeError::Other("storage version not updated")
        );
        if !state.is_empty() {
            v1::Migration::<T>::post_upgrade(state)?;
        }
        Ok(())
    }
}
