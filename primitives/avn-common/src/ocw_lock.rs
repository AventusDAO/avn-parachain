use codec::Codec;
use sp_runtime::{
    offchain::{
        storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
        storage_lock::{BlockAndTime, StorageLock},
    },
    traits::{AtLeast32Bit, BlockNumberProvider, Member},
};
use sp_std::vec::Vec;

pub enum OcwStorageError {
    OffchainWorkerAlreadyRun,
    ErrorRecordingOffchainWorkerRun,
}

pub fn record_block_run<BlockNumber: Member + Codec + AtLeast32Bit>(
    block_number: BlockNumber,
    caller_id: Vec<u8>,
) -> Result<(), OcwStorageError> {
    const ALREADY_RUN: () = ();
    let key = [caller_id.as_slice(), b"::last_run"].concat();
    let storage = StorageValueRef::persistent(&key);
    // Using `mutate` means that only one worker will be able to "acquire a lock" to update this
    // value.
    let result = storage.mutate(|last_run: Result<Option<BlockNumber>, StorageRetrievalError>| {
        match last_run {
            // If we already have a value in storage and the value is the same or greater than the
            // current block_number we abort the update as a worker from a newer block
            // has beaten us here.
            Ok(Some(block)) if block >= block_number => Err(ALREADY_RUN),
            // In every other case we attempt to acquire the lock and update the block_number.
            _ => Ok(block_number),
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(MutateStorageError::ValueFunctionFailed(_)) => {
            Err(OcwStorageError::OffchainWorkerAlreadyRun)
        },
        Err(MutateStorageError::ConcurrentModification(_)) => {
            Err(OcwStorageError::ErrorRecordingOffchainWorkerRun)
        },
    }
}

pub fn get_offchain_worker_locker<Provider: BlockNumberProvider>(
    lock_name: &[u8],
    expiry_in_blocks: u32,
) -> StorageLock<'_, BlockAndTime<Provider>> {
    StorageLock::<BlockAndTime<Provider>>::with_block_deadline(lock_name, expiry_in_blocks)
}

pub fn is_locked<Provider: BlockNumberProvider>(lock_name: &[u8]) -> bool {
    let mut lock = get_offchain_worker_locker::<Provider>(lock_name, 1u32);
    match lock.try_lock() {
        Ok(guard) => {
            drop(guard);
            return false;
        },
        Err(_) => return true,
    };
}
