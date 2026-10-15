// Copyright 2026 Aventus DAO Ltd

//! Offchain worker helpers shared by the floor and venue channels. Both workers run in the
//! same `offchain_worker` hook, so each channel carries its own ids: the per-block run lock
//! and the submission records must never be shared, or one channel would silence the other.

use crate::*;
use sp_avn_common::ocw_lock::{self as OcwLock, OcwStorageError};
use sp_runtime::offchain::storage::{MutateStorageError, StorageRetrievalError, StorageValueRef};

/// Blocks an OCW submission is given to land before the node retries.
pub(crate) const BLOCK_INCLUSION_PERIOD: u32 = 5;

/// Identifiers of one channel's offchain worker.
pub(crate) struct OcwChannel {
    /// Name used in log lines.
    pub name: &'static str,
    /// Per-block run lock id.
    pub id: &'static [u8],
    /// Prefix of the offchain db keys holding submission records.
    pub db_prefix: &'static [u8],
}

impl<T: Config> Pallet<T> {
    pub(crate) fn ocw_already_run(channel: &OcwChannel, now: BlockNumberFor<T>) -> Result<(), ()> {
        OcwLock::record_block_run(now, channel.id.to_vec()).map_err(|e| match e {
            OcwStorageError::OffchainWorkerAlreadyRun => {
                log::debug!("{} OCW already ran for block {:?}", channel.name, now)
            },
            OcwStorageError::ErrorRecordingOffchainWorkerRun => {
                log::error!("💔 {} OCW cannot record its run for block {:?}", channel.name, now)
            },
        })
    }

    fn ocw_storage_key(channel: &OcwChannel, suffix: &[u8]) -> Vec<u8> {
        let mut key = channel.db_prefix.to_vec();
        key.extend_from_slice(suffix);
        key
    }

    pub(crate) fn submission_in_progress(
        channel: &OcwChannel,
        suffix: &[u8],
        now: BlockNumberFor<T>,
    ) -> bool {
        match StorageValueRef::persistent(&Self::ocw_storage_key(channel, suffix))
            .get::<BlockNumberFor<T>>()
            .ok()
            .flatten()
        {
            Some(submitted_at) => now <= submitted_at.saturating_add(BLOCK_INCLUSION_PERIOD.into()),
            None => false,
        }
    }

    pub(crate) fn record_submission(channel: &OcwChannel, suffix: &[u8], now: BlockNumberFor<T>) {
        let result = StorageValueRef::persistent(&Self::ocw_storage_key(channel, suffix))
            .mutate(|_: Result<Option<BlockNumberFor<T>>, StorageRetrievalError>| Ok::<_, ()>(now));
        if let Err(MutateStorageError::ConcurrentModification(_)) = result {
            log::warn!("{} OCW: concurrent modification of submission record", channel.name);
        }
    }
}
