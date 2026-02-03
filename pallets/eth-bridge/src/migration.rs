use frame_support::{
    pallet_prelude::*,
    traits::{Get, GetStorageVersion, OnRuntimeUpgrade},
    weights::Weight,
};
use sp_avn_common::eth::{PACKED_LOWER_V1_PARAMS_SIZE, PACKED_LOWER_V2_PARAMS_SIZE};

use crate::*;

#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

pub const V3_STORAGE_VERSION: StorageVersion = StorageVersion::new(3);

mod v2 {
    use super::*;
    use frame_support::storage_alias;

    #[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, Default, TypeInfo, MaxEncodedLen)]
    pub struct ActiveEthRange {
        pub range: EthBlockRange,
        pub partition: u16,
        pub event_types_filter: EthBridgeEventsFilter,
    }

    // TODO remove me, not used.
    /// V2 type for [`crate::ActiveEthRange`].
    #[storage_alias]
    pub type ActiveEthereumRange<T: crate::Config<I>, I: 'static> =
        StorageValue<Pallet<T, I>, ActiveEthRange>;
}

mod v3 {
    use super::*;
    /// Version 3 of ActiveRequestData, used in migration from v3 to v4.
    #[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, Default, TypeInfo, MaxEncodedLen)]
    pub struct ActiveRequestDataV3<BlockNumber, AccountId> {
        pub request: Request,
        pub confirmation: ActiveConfirmation,
        pub tx_data: Option<ActiveEthTransactionV3<AccountId>>,
        pub last_updated: BlockNumber,
    }

    /// Version 3 of ActiveEthTransaction, used in migration from v3 to v4.
    #[derive(Encode, Decode, Debug, Clone, PartialEq, Eq, Default, TypeInfo, MaxEncodedLen)]
    pub struct ActiveEthTransactionV3<AccountId> {
        pub function_name: BoundedVec<u8, FunctionLimit>,
        pub eth_tx_params:
            BoundedVec<(BoundedVec<u8, TypeLimit>, BoundedVec<u8, ValueLimit>), ParamsLimit>,
        pub sender: AccountId,
        pub expiry: u64,
        pub eth_tx_hash: H256,
        pub success_corroborations: BoundedVec<AccountId, ConfirmationsLimit>,
        pub failure_corroborations: BoundedVec<AccountId, ConfirmationsLimit>,
        pub valid_tx_hash_corroborations: BoundedVec<AccountId, ConfirmationsLimit>,
        pub invalid_tx_hash_corroborations: BoundedVec<AccountId, ConfirmationsLimit>,
        pub tx_succeeded: bool,
    }
}

mod v5 {
    use super::*;
    pub type LegacyLowerParams = [u8; PACKED_LOWER_V1_PARAMS_SIZE];

    #[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, Default, TypeInfo, MaxEncodedLen)]
    pub struct LegacyActiveRequestData<BlockNumber, AccountId> {
        pub request: LegacyRequest,
        pub confirmation: ActiveConfirmation,
        pub tx_data: Option<ActiveEthTransaction<AccountId>>,
        pub last_updated: BlockNumber,
    }

    #[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
    pub struct LegacyLowerProofRequestData {
        pub lower_id: LowerId,
        pub params: LegacyLowerParams,
        pub caller_id: BoundedVec<u8, CallerIdLimit>,
    }

    #[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
    pub enum LegacyRequest {
        Send(SendRequestData),
        LowerProof(LegacyLowerProofRequestData),
    }

    impl Default for LegacyRequest {
        fn default() -> Self {
            LegacyRequest::Send(Default::default())
        }
    }
}

pub struct EthBridgeMigrations<T: Config<I>, I: 'static = ()>(PhantomData<T>, PhantomData<I>);
impl<T: Config<I>, I: 'static> OnRuntimeUpgrade for EthBridgeMigrations<T, I> {
    fn on_runtime_upgrade() -> Weight {
        let current = Pallet::<T, I>::current_storage_version();
        let onchain = Pallet::<T, I>::on_chain_storage_version();

        log::info!(
            "ℹ️  Eth bridge migration started with current storage version {:?} / onchain {:?}",
            current,
            onchain
        );
        let mut consumed_weight = Weight::zero();

        if EthBlockRangeSize::<T, I>::get() == 0 {
            consumed_weight += set_block_range_size::<T, I>();
        }

        if onchain < 3 {
            consumed_weight += migrate_to_v3::<T, I>();
        }

        if onchain < 4 && (current == 4 || current == 5) {
            consumed_weight += migrate_to_v4::<T, I>();
        }

        if onchain <= 4 && current == 5 {
            consumed_weight += migrate_to_v5::<T, I>();
        }

        consumed_weight
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        Ok([0; 32].to_vec())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(_input: Vec<u8>) -> Result<(), TryRuntimeError> {
        use frame_support::ensure;
        let onchain = Pallet::<T, I>::on_chain_storage_version();

        ensure!(EthBlockRangeSize::<T, I>::get() != 0, "Block range not set");

        if onchain == 5 {
            if let Some(queue) = RequestQueue::<T, I>::get() {
                queue.iter().try_for_each(|req| -> Result<(), TryRuntimeError> {
                    if let Request::LowerProof(data) = req {
                        for byte in
                            &data.params[PACKED_LOWER_V1_PARAMS_SIZE..PACKED_LOWER_V2_PARAMS_SIZE]
                        {
                            ensure!(*byte == 0, "LowerProof params not migrated");
                        }
                    }
                    Ok(())
                })?;
            }

            if let Some(active_req) = ActiveRequest::<T, I>::get() {
                if let Request::LowerProof(data) = active_req.request {
                    for byte in
                        &data.params[PACKED_LOWER_V1_PARAMS_SIZE..PACKED_LOWER_V2_PARAMS_SIZE]
                    {
                        ensure!(*byte == 0, "ActiveRequest LowerProof params not migrated");
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn set_block_range_size<T: Config<I>, I: 'static>() -> Weight {
    log::info!("ℹ️  Starting `BlockRangeSize` migration");
    let mut consumed_weight: Weight = Weight::from_parts(0 as u64, 0);
    let mut add_weight = |reads, writes, weight: Weight| {
        consumed_weight += T::DbWeight::get().reads_writes(reads, writes);
        consumed_weight += weight;
    };

    EthBlockRangeSize::<T, I>::put(DEFAULT_ETH_RANGE);

    // 2 Storage writes
    add_weight(0, 2, Weight::from_parts(0 as u64, 0));

    log::info!("✅ BlockRangeSize set successfully");
    return consumed_weight + Weight::from_parts(25_000 as u64, 0)
}

pub fn migrate_to_v3<T: Config<I>, I: 'static>() -> Weight {
    let mut consumed_weight: Weight = T::DbWeight::get().reads(1);

    if let Some(old_range) = v2::ActiveEthereumRange::<T, I>::take() {
        ActiveEthereumRange::<T, I>::put(ActiveEthRange {
            range: old_range.range,
            partition: old_range.partition,
            event_types_filter: old_range.event_types_filter,
            additional_transactions: Default::default(),
        });
        log::info!("✅ ActiveEthereumRange set successfully");
        consumed_weight += T::DbWeight::get().writes(1);
    }
    V3_STORAGE_VERSION.put::<Pallet<T, I>>();
    consumed_weight += T::DbWeight::get().writes(1);

    consumed_weight
}

pub fn migrate_to_v4<T: Config<I>, I: 'static>() -> Weight {
    let mut consumed_weight: Weight = T::DbWeight::get().reads(1);

    log::info!("🔄 Starting ActiveRequest ReplayAttempt migration");

    let translate = |old: v3::ActiveRequestDataV3<BlockNumberFor<T>, T::AccountId>| -> ActiveRequestData<BlockNumberFor<T>, T::AccountId> {
        let tx_data: Option<ActiveEthTransaction<T::AccountId>> = match old.tx_data {
            Some(data) => Some(ActiveEthTransaction {
                function_name: data.function_name,
                eth_tx_params: data.eth_tx_params,
                sender: data.sender,
                expiry: data.expiry,
                eth_tx_hash: data.eth_tx_hash,
                success_corroborations: data.success_corroborations,
                failure_corroborations: data.failure_corroborations,
                valid_tx_hash_corroborations: data.valid_tx_hash_corroborations,
                invalid_tx_hash_corroborations: data.invalid_tx_hash_corroborations,
                tx_succeeded: data.tx_succeeded,
                replay_attempt: 0, // New field, defaulting to 0
            }),
            None => None,
        };
        let new = ActiveRequestData {
            request: old.request,
            confirmation: old.confirmation,
            last_updated: old.last_updated,
            tx_data,
        };
        new
    };

    if ActiveRequest::<T, I>::translate(|pre| pre.map(translate)).is_err() {
        log::error!(
            "unexpected error when performing translation of the ActiveRequest type \
            during storage upgrade to v4"
        );
    }
    log::info!("✅ ActiveRequest ReplayAttempt migration has been successful");
    consumed_weight += T::DbWeight::get().writes(1);
    STORAGE_VERSION.put::<Pallet<T, I>>();
    consumed_weight += T::DbWeight::get().writes(1);

    consumed_weight
}

pub fn migrate_to_v5<T: Config<I>, I: 'static>() -> Weight {
    let mut consumed_weight: Weight = T::DbWeight::get().reads(1);

    consumed_weight += migrate_active_request_to_v5::<T, I>();
    consumed_weight += migrate_request_queue_to_v5::<T, I>();

    STORAGE_VERSION.put::<Pallet<T, I>>();
    consumed_weight += T::DbWeight::get().writes(1);

    consumed_weight
}

fn resize_lower_params_to_v5<T: Config<I>, I: 'static>(
    params: v5::LegacyLowerParams,
) -> [u8; PACKED_LOWER_V2_PARAMS_SIZE] {
    let mut new_params = [0u8; PACKED_LOWER_V2_PARAMS_SIZE];
    new_params[..PACKED_LOWER_V1_PARAMS_SIZE].copy_from_slice(&params);
    new_params[PACKED_LOWER_V1_PARAMS_SIZE..PACKED_LOWER_V2_PARAMS_SIZE].fill(0);
    new_params
}

pub fn migrate_active_request_to_v5<T: Config<I>, I: 'static>() -> Weight {
    let mut consumed_weight: Weight = T::DbWeight::get().reads(1);

    log::info!("🔄 Starting ActiveRequest LowerParams migrations");

    let translate = |req: v5::LegacyActiveRequestData<BlockNumberFor<T>, T::AccountId>| -> ActiveRequestData<BlockNumberFor<T>, T::AccountId> {

        let request = match req.request {
            v5::LegacyRequest::LowerProof(d) => {
                let lower_params = resize_lower_params_to_v5::<T, I>(d.params);
                let new_proof_data = LowerProofRequestData {
                    lower_id: d.lower_id,
                    params: lower_params,
                    caller_id: d.caller_id,
                };
                Request::LowerProof(new_proof_data)
            },
            v5::LegacyRequest::Send(d) =>
                Request::Send(SendRequestData {
                    tx_id: d.tx_id,
                    function_name: d.function_name,
                    params: d.params,
                    caller_id: d.caller_id,
                }),
        };

        let new = ActiveRequestData {
            request,
            confirmation: req.confirmation,
            last_updated: req.last_updated,
            tx_data: req.tx_data,
        };
        new
    };

    if ActiveRequest::<T, I>::translate(|pre| pre.map(translate)).is_err() {
        log::error!(
            " 💔 unexpected error when performing translation of the LowerParams type \
            during storage upgrade to v5"
        );
    }

    log::info!("✅ ActiveRequest LowerParams migration completed successful");

    consumed_weight += T::DbWeight::get().writes(1);
    consumed_weight
}

pub fn migrate_request_queue_to_v5<T: Config<I>, I: 'static>() -> Weight {
    let mut consumed_weight: Weight = Default::default();

    log::info!("🔄 Starting RequestQueue LowerParams migrations");

    let mut read = 0u64;
    let mut translated = 0u64;
    let translate = |queue: BoundedVec<v5::LegacyRequest, T::MaxQueuedTxRequests>| -> BoundedVec<Request, T::MaxQueuedTxRequests> {
        let mut translated_queue: BoundedVec<Request, T::MaxQueuedTxRequests> = BoundedVec::default();

        for req in queue.into_iter() {
            read += 1;
            let new_req = match req {
                v5::LegacyRequest::Send(d) =>
                    Request::Send(SendRequestData {
                        tx_id: d.tx_id,
                        function_name: d.function_name,
                        params: d.params,
                        caller_id: d.caller_id,
                    }),
                v5::LegacyRequest::LowerProof(d) => {
                    translated += 1;
                    let lower_params = resize_lower_params_to_v5::<T, I>(d.params);
                    Request::LowerProof(LowerProofRequestData {
                        lower_id: d.lower_id,
                        params: lower_params,
                        caller_id: d.caller_id,
                    })
                }
            };
            translated_queue
                .try_push(new_req)
                .map_err(|_| log::error!(" 💔 RequestQueue migration exceeded max length"))
                .ok();
        }
        translated_queue
    };

    if RequestQueue::<T, I>::translate(|pre| pre.map(translate)).is_err() {
        log::error!(
            " 💔 unexpected error when performing translation of the RequestQueue LowerParams type \
            during storage upgrade to v5"
        );
    }

    log::info!("✅ {} RequestQueue LowerParams entries migrated successfully", translated);

    consumed_weight += T::DbWeight::get().reads_writes(read + 1, translated + 1);
    consumed_weight
}
