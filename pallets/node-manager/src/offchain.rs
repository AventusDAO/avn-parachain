// No storage mutation allowed in this file
#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::String;

use crate::*;
// We allow up to 5 blocks for ocw transactions
const BLOCK_INCLUSION_PERIOD: u32 = 5;
const PALLET_REGISTERED_NODE_KEY: &'static [u8; 26] = b"ocw_pallet_registered_node";
pub const OCW_ID: &'static [u8; 22] = b"node_manager::last_run";
const OC_HB_DB_PREFIX: &[u8] = b"tnf/node-manager-heartbeat/";

impl<T: Config> Pallet<T> {
    pub fn trigger_payment_if_required(reward_period_index: RewardPeriodIndex, author: Author<T>) {
        if Self::can_trigger_payment().unwrap_or(false) {
            log::info!("🌐 Triggering payment for period: {:?}", reward_period_index);

            let signature = author.key.sign(&(PAYOUT_REWARD_CONTEXT, reward_period_index).encode());

            match signature {
                Some(signature) => {
                    let call = T::create_inherent(
                        Call::<T>::offchain_pay_nodes {
                            reward_period_index,
                            author: author.clone(),
                            signature,
                        }
                        .into(),
                    );

                    if let Err(e) = SubmitTransaction::<T, Call<T>>::submit_transaction(call) {
                        log::error!("💔 Error submitting transaction to trigger payment. Period: {:?}, Error: {:?}", reward_period_index, e);
                    }
                },
                None => {
                    log::error!(
                        "💔 Error signing payment transaction. Period: {:?}",
                        reward_period_index
                    );
                },
            }
        }
    }

    pub fn send_heartbeat_if_required(block_number: BlockNumberFor<T>) {
        let maybe_node_key = Self::get_node_from_signing_key();
        if let Some((node, signing_key)) = maybe_node_key {
            let reward_period = RewardPeriod::<T>::get();
            let current_reward_period = reward_period.current;
            let uptime_info = <NodeUptime<T>>::get(current_reward_period, &node);
            let heartbeat_count = uptime_info.map(|info| info.count).unwrap_or(0);

            if Self::should_send_heartbeat(
                block_number,
                uptime_info,
                &reward_period,
                heartbeat_count,
            ) {
                log::info!(
                    "🌐 Sending heartbeat for reward period: {:?}, block number: {:?}",
                    block_number,
                    current_reward_period
                );

                let signature = signing_key
                    .sign(&(HEARTBEAT_CONTEXT, heartbeat_count, current_reward_period).encode());

                match signature {
                    Some(signature) => {
                        let call = T::create_inherent(
                            Call::<T>::offchain_submit_heartbeat {
                                node,
                                reward_period_index: current_reward_period,
                                heartbeat_count,
                                signature,
                            }
                            .into(),
                        );

                        match SubmitTransaction::<T, Call<T>>::submit_transaction(call) {
                            Ok(_) => {
                                // If this fails, the extrinsic will still reject duplicates
                                let _ = Self::record_heartbeat_ocw_submission(
                                    block_number,
                                    current_reward_period,
                                    heartbeat_count
                                );
                            },
                            Err(e) => log::error!(
                                "💔 Error submitting heartbeat transaction. Period: {:?}, Heartbeat count: {:?}, Error: {:?}",
                                current_reward_period, heartbeat_count, e),
                        }

                        log::info!(
                            "🌐 heartbeat transaction sent. Reward period: {:?}, Block number: {:?}",
                            block_number, current_reward_period);
                    },
                    None => {
                        log::error!(
                            "💔 Error signing heartbeat transaction. Reward period: {:?}, Block number: {:?}",
                            block_number, current_reward_period);
                    },
                }
            }
        }
    }

    pub fn try_get_node_author(block_number: BlockNumberFor<T>) -> Option<Author<T>> {
        let setup_result = AVN::<T>::pre_run_setup(block_number, OCW_ID.to_vec());
        if let Ok((this_author, _)) = setup_result {
            let is_primary = AVN::<T>::is_primary_for_block(block_number, &this_author.account_id);

            if is_primary.is_err() {
                log::error!("💔 Error checking if author is Primary");
                return None
            }

            return Some(this_author)
        }

        return None
    }

    pub fn can_trigger_payment() -> Result<bool, ()> {
        let oldest_period = OldestUnpaidRewardPeriodIndex::<T>::get();
        let current_period = RewardPeriod::<T>::get().current;

        if oldest_period >= current_period { return Ok(false) }

        let last_paid_pointer = LastPaidPointer::<T>::get();
        if last_paid_pointer.is_some() {
            log::info!("👷 Resuming payment for period: {:?}", oldest_period);
        } else {
            log::info!(
                "👷 Triggering payment for period: {:?}. Current period: {:?}",
                oldest_period,
                current_period
            );
        }

        return Ok(true)
    }

    pub fn get_node_from_signing_key() -> Option<(T::AccountId, T::SignerId)> {
        let mut local_keys: Vec<T::SignerId> = T::SignerId::all();
        local_keys.sort();

        // Attempt to read the CLI-provided node ID (only happens on startup).
        let maybe_node_id = Self::get_cli_node_id_from_local_storage();
        if let Some(cli_node_id) = maybe_node_id {
            log::info!(
                "🌐 Setting NodeId {:?} in pallet local storage",
                hex::encode(cli_node_id.encode())
            );
            if Self::record_formatted_node_id(cli_node_id).is_ok() {
                Self::clear_cli_node_id_from_local_storage();
            }
        }

        // There is no guarantee that the node_id is stored in the local storage
        let maybe_node_id = Self::get_formatted_node_id_from_local_storage();
        if let Some(node_id) = maybe_node_id {
            if let Some(key_pair) = Self::match_node_id_to_signing_key(node_id, &local_keys) {
                return Some(key_pair)
            }
        }

        // If we get here, we were not successful in finding the nodeId in the local storage
        // We will search all registered nodes using the local signing key
        if let Some((node_id, signing_key)) = Self::search_node_id_by_signing_key(&local_keys) {
            log::info!("🔐 NodeId found, storing in local db for next time.");
            let _ = Self::record_formatted_node_id(node_id.clone());
            return Some((node_id, signing_key))
        }

        log::error!("💔 Unable to find a valid nodeId.");
        None
    }

    pub fn should_send_heartbeat(
        block_number: BlockNumberFor<T>,
        uptime_info: Option<UptimeInfo<BlockNumberFor<T>>>,
        reward_period: &RewardPeriodInfo<BlockNumberFor<T>>,
        heartbeat_count: u64,
    ) -> bool {
        if Self::heartbeat_submission_in_progress(
            reward_period.current,
            heartbeat_count,
            block_number,
        ) {
            return false
        }

        let heartbeat_period = HeartbeatPeriod::<T>::get();
        if let Some(uptime_info) = uptime_info {
            let last_submission = uptime_info.last_reported;
            let below_threshold = uptime_info.count < reward_period.uptime_threshold as u64;
            // Send heartbeat if threshold is not reached and the current block is at or past the
            // next allowed block.
            return below_threshold &&
                block_number >= last_submission + BlockNumberFor::<T>::from(heartbeat_period)
        } else {
            // First heartbeat
            return true
        }
    }

    // TODO: cleanup old data
    fn record_heartbeat_ocw_submission(
        now: BlockNumberFor<T>,
        reward_period_index: RewardPeriodIndex,
        heartbeat_count: u64,
    ) -> Result<(), Error<T>> {
        let mut key = OC_HB_DB_PREFIX.to_vec();
        key.extend((reward_period_index, heartbeat_count).encode());

        let storage = StorageValueRef::persistent(&key);
        let result =
            storage.mutate(|_: Result<Option<BlockNumberFor<T>>, StorageRetrievalError>| Ok(now));
        match result {
            Err(MutateStorageError::ValueFunctionFailed(e)) => Err(e),
            Err(MutateStorageError::ConcurrentModification(_)) =>
                Err(Error::<T>::FailedToAcquireOcwDbLock),
            Ok(_) => return Ok(()),
        }
    }

    // Formatted - because the nodeId has the correct type (unlike the cli nodeId which is a string)
    fn record_formatted_node_id(node_id: NodeId<T>) -> Result<(), Error<T>> {
        let storage = StorageValueRef::persistent(PALLET_REGISTERED_NODE_KEY);
        match storage.mutate(|_: Result<Option<NodeId<T>>, StorageRetrievalError>| Ok(node_id)) {
            Err(MutateStorageError::ValueFunctionFailed(e)) => Err(e),
            Err(MutateStorageError::ConcurrentModification(_)) =>
                Err(Error::<T>::FailedToAcquireOcwDbLock),
            Ok(_) => return Ok(()),
        }
    }

    fn heartbeat_submission_in_progress(
        reward_period_index: RewardPeriodIndex,
        heartbeat_count: u64,
        current_block: BlockNumberFor<T>,
    ) -> bool {
        let mut key = OC_HB_DB_PREFIX.to_vec();
        key.extend((reward_period_index, heartbeat_count).encode());

        match StorageValueRef::persistent(&key).get::<BlockNumberFor<T>>().ok().flatten() {
            Some(last_submission) => {
                // Allow BLOCK_INCLUSION_PERIOD blocks for the transaction to be included
                return current_block <=
                    last_submission
                        .saturating_add(BlockNumberFor::<T>::from(BLOCK_INCLUSION_PERIOD))
            },
            _ => false,
        }
    }

    fn clear_cli_node_id_from_local_storage() {
        let mut storage = StorageValueRef::persistent(REGISTERED_NODE_KEY);
        storage.clear();
    }

    // Get the nodeId passed in the CLI arguments (Initial nodeId).
    // This will read the nodeId as a string, which is what the client does.
    fn get_cli_node_id_from_local_storage() -> Option<NodeId<T>> {
        StorageValueRef::persistent(REGISTERED_NODE_KEY)
            .get::<String>()
            .ok()
            .flatten()
            .and_then(|node_id_string| hex::decode(&node_id_string).ok())
            .and_then(|node_id_bytes| T::AccountId::decode(&mut &node_id_bytes[..]).ok())
    }

    // Get the correctly formatted nodeId from OCW database
    // This will expect the nodeId to be set as a NodeId
    fn get_formatted_node_id_from_local_storage() -> Option<NodeId<T>> {
        let node_id = StorageValueRef::persistent(PALLET_REGISTERED_NODE_KEY)
            .get::<NodeId<T>>()
            .ok()
            .flatten();

        if node_id.is_none() {
            log::warn!("🔐 Cannot find nodeId in local database");
        }

        node_id
    }

    fn match_node_id_to_signing_key(
        node_id: NodeId<T>,
        local_keys: &Vec<T::SignerId>,
    ) -> Option<(T::AccountId, T::SignerId)> {
        if let Some(node_info) = NodeRegistry::<T>::get(&node_id) {
            if local_keys.binary_search(&node_info.signing_key).is_ok() {
                return Some((node_id, node_info.signing_key))
            } else {
                log::warn!("🔐 NodeId and signing key do not match");
            }
        } else {
            log::warn!("🔐 Node {:?} not found in registry.", hex::encode(node_id.encode()));
        };

        None
    }

    pub fn search_node_id_by_signing_key(
        local_keys: &Vec<T::SignerId>,
    ) -> Option<(NodeId<T>, T::SignerId)> {
        log::warn!("🔐 Fallback - Looking up node from onchain state storage.");
        for key in local_keys.iter() {
            if let Some(node_id) = SigningKeyToNodeId::<T>::get(key) {
                // Optional safety: verify NodeRegistry still matches.
                if let Some(info) = NodeRegistry::<T>::get(&node_id) {
                    if info.signing_key == *key {
                        return Some((node_id, key.clone()))
                    }
                }
            }
        }

        None
    }
}
