// No state mutation allowed in this file because it is used in offchain context.

use crate::*;
pub const OCW_LOCK_PREFIX: &[u8] = b"sum-wt-ocw::lock::";

impl<T: Config> Pallet<T> {
    pub fn validate_root(
        now: BlockNumberFor<T>,
        root_data: &RootData<BlockNumberFor<T>>,
        proposal_id: &ProposalId,
    ) -> Result<bool, String> {
        let lock_id = Self::compute_lock_id(now, proposal_id, &root_data);
        let mut lock = AVN::<T>::get_ocw_locker(&lock_id);

        let result = match lock.try_lock() {
            Ok(guard) => {
                let recalculated_hash = Self::calculate_root_hash(
                    root_data.root_id.range.from_block,
                    root_data.root_id.range.to_block,
                )?;
                guard.forget();
                Ok(recalculated_hash == root_data.root_hash)
            },
            Err(_lock_error) =>
                Err("Failed to acquire OCW lock for verification processing".to_string()),
        };
        result
    }

    fn calculate_root_hash(
        from_block: BlockNumberFor<T>,
        to_block: BlockNumberFor<T>,
    ) -> Result<H256, String> {
        let from_block_u32: u32 = from_block
            .try_into()
            .map_err(|_| format!("From_block {:?} too large for u32", from_block))?;

        let to_block_u32: u32 = to_block
            .try_into()
            .map_err(|_| format!("To_block {:?} too large for u32", to_block))?;

        let url_path = format!("roothash/{}/{}", from_block_u32, to_block_u32);

        log::debug!("Fetching recalculated root hash using AVN service, path: {}", url_path);

        let response = AVN::<T>::get_data_from_service(url_path).map_err(|dispatch_err| {
            let err_msg = format!("AVN service call failed: {:?}", dispatch_err);
            err_msg
        })?;

        Self::validate_response(response)
    }

    pub fn validate_response(response: Vec<u8>) -> Result<H256, String> {
        if response.len() != 64 {
            return Err("Invalid root hash length, expected 64 bytes".to_string())
        }

        let root_hash_str = core::str::from_utf8(&response)
            .map_err(|_| "Response contains invalid UTF8 bytes".to_string())?;

        let mut data: [u8; 32] = [0; 32];
        hex::decode_to_slice(root_hash_str.trim(), &mut data[..])
            .map_err(|_| "Response contains invalid hex string".to_string())?;

        Ok(H256::from_slice(&data))
    }

    fn compute_lock_id(
        now: BlockNumberFor<T>,
        proposal_id: &ProposalId,
        root_data: &RootData<BlockNumberFor<T>>,
    ) -> Vec<u8> {
        let mut lock_id = OCW_LOCK_PREFIX.to_vec();
        lock_id.extend_from_slice(&proposal_id.encode());
        lock_id.extend_from_slice(&now.encode());
        lock_id.extend_from_slice(&root_data.encode());
        lock_id
    }
}
