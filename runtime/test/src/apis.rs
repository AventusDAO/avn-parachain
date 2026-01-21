// This file is part of Aventus.
// Copyright (C) 2026 Aventus Network Services (UK) Ltd.

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

// External crates imports
use alloc::vec::Vec;
use frame_support::{
    genesis_builder_helper::{build_state, get_preset},
    weights::Weight,
};
use pallet_aura::Authorities;
use sp_api::impl_runtime_apis;
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::{crypto::KeyTypeId, ByteArray, OpaqueMetadata, H160};
use sp_runtime::{
    traits::Block as BlockT,
    transaction_validity::{TransactionSource, TransactionValidity},
    ApplyExtrinsicResult,
};
use sp_version::RuntimeVersion;

// Local module imports
use super::{
    AccountId, Balance, Block, ConsensusHook, Executive, InherentDataExt, Nonce, ParachainSystem,
    Runtime, RuntimeCall, RuntimeGenesisConfig, SessionKeys, System, TransactionPayment,
    SLOT_DURATION, VERSION,
};

use crate::{
    AdditionalEvents, AuthorityDiscovery, AuthorityDiscoveryId, Avn, CrossChainVoting,
    EthBlockRange, EthBridge, EthBridgeInstance, EthereumEventsPartition, InstanceId,
    MAIN_ETH_BRIDGE_ID,
};

use crate::{EthSecondBridge, SECONDARY_ETH_BRIDGE_ID};

use codec::Encode;
use sp_std::collections::btree_map::BTreeMap;

// we move some impls outside so we can easily use them with `docify`.
impl Runtime {
    #[docify::export]
    fn impl_slot_duration() -> sp_consensus_aura::SlotDuration {
        sp_consensus_aura::SlotDuration::from_millis(SLOT_DURATION)
    }

    #[docify::export]
    fn impl_can_build_upon(
        included_hash: <Block as BlockT>::Hash,
        slot: cumulus_primitives_aura::Slot,
    ) -> bool {
        ConsensusHook::can_build_upon(included_hash, slot)
    }
}

impl_runtime_apis! {
    impl sp_consensus_aura::AuraApi<Block, AuraId> for Runtime {
        fn slot_duration() -> sp_consensus_aura::SlotDuration {
            Runtime::impl_slot_duration()
        }

        fn authorities() -> Vec<AuraId> {
            Authorities::<Runtime>::get().into_inner()
        }
    }

    impl cumulus_primitives_aura::AuraUnincludedSegmentApi<Block> for Runtime {
            fn can_build_upon(
                    included_hash: <Block as BlockT>::Hash,
                    slot: cumulus_primitives_aura::Slot,
            ) -> bool {
                Runtime::impl_can_build_upon(included_hash, slot)
            }
    }

    impl sp_api::Core<Block> for Runtime {
        fn version() -> RuntimeVersion {
            VERSION
        }

        fn execute_block(block: Block) {
            Executive::execute_block(block)
        }

        fn initialize_block(header: &<Block as BlockT>::Header) -> sp_runtime::ExtrinsicInclusionMode {
            Executive::initialize_block(header)
        }
    }

    impl sp_api::Metadata<Block> for Runtime {
        fn metadata() -> OpaqueMetadata {
            OpaqueMetadata::new(Runtime::metadata().into())
        }

        fn metadata_at_version(version: u32) -> Option<OpaqueMetadata> {
            Runtime::metadata_at_version(version)
        }

        fn metadata_versions() -> Vec<u32> {
            Runtime::metadata_versions()
        }
    }

    impl sp_block_builder::BlockBuilder<Block> for Runtime {
        fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
            Executive::apply_extrinsic(extrinsic)
        }

        fn finalize_block() -> <Block as BlockT>::Header {
            Executive::finalize_block()
        }

        fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
            data.create_extrinsics()
        }

        fn check_inherents(
            block: Block,
            data: sp_inherents::InherentData,
        ) -> sp_inherents::CheckInherentsResult {
            data.check_extrinsics(&block)
        }
    }

    impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
        fn validate_transaction(
            source: TransactionSource,
            tx: <Block as BlockT>::Extrinsic,
            block_hash: <Block as BlockT>::Hash,
        ) -> TransactionValidity {
            Executive::validate_transaction(source, tx, block_hash)
        }
    }

    impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
        fn offchain_worker(header: &<Block as BlockT>::Header) {
            Executive::offchain_worker(header)
        }
    }

    impl sp_session::SessionKeys<Block> for Runtime {
        fn generate_session_keys(seed: Option<Vec<u8>>) -> Vec<u8> {
            SessionKeys::generate(seed)
        }

        fn decode_session_keys(
            encoded: Vec<u8>,
        ) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
            SessionKeys::decode_into_raw_public_keys(&encoded)
        }
    }

    impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Nonce> for Runtime {
        fn account_nonce(account: AccountId) -> Nonce {
            System::account_nonce(account)
        }
    }

    impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
        fn query_info(
            uxt: <Block as BlockT>::Extrinsic,
            len: u32,
        ) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
            TransactionPayment::query_info(uxt, len)
        }
        fn query_fee_details(
            uxt: <Block as BlockT>::Extrinsic,
            len: u32,
        ) -> pallet_transaction_payment::FeeDetails<Balance> {
            TransactionPayment::query_fee_details(uxt, len)
        }
        fn query_weight_to_fee(weight: Weight) -> Balance {
            TransactionPayment::weight_to_fee(weight)
        }
        fn query_length_to_fee(length: u32) -> Balance {
            TransactionPayment::length_to_fee(length)
        }
    }

    impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentCallApi<Block, Balance, RuntimeCall>
        for Runtime
    {
        fn query_call_info(
            call: RuntimeCall,
            len: u32,
        ) -> pallet_transaction_payment::RuntimeDispatchInfo<Balance> {
            TransactionPayment::query_call_info(call, len)
        }
        fn query_call_fee_details(
            call: RuntimeCall,
            len: u32,
        ) -> pallet_transaction_payment::FeeDetails<Balance> {
            TransactionPayment::query_call_fee_details(call, len)
        }
        fn query_weight_to_fee(weight: Weight) -> Balance {
            TransactionPayment::weight_to_fee(weight)
        }
        fn query_length_to_fee(length: u32) -> Balance {
            TransactionPayment::length_to_fee(length)
        }
    }

    impl pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId> for Runtime {
        fn query_authors() -> Vec<([u8; 32], [u8; 32])> {
            let validators = Avn::validators().to_vec();
            let res = validators.iter().map(|validator| {
                let mut address: [u8; 32] = Default::default();
                address.copy_from_slice(&validator.account_id.encode()[0..32]);

                let mut key: [u8; 32] = Default::default();
                key.copy_from_slice(&validator.key.to_raw_vec()[0..32]);

                return (address, key)
            }).collect();
            return res
        }

        fn query_active_block_range(instance_id: InstanceId)-> Option<(EthBlockRange, u16)> {
            match instance_id {
                MAIN_ETH_BRIDGE_ID => {
                    EthBridge::active_ethereum_range().map(|active_eth_range| {
                        (active_eth_range.range, active_eth_range.partition)
                    })
                },
                SECONDARY_ETH_BRIDGE_ID => {
                    EthSecondBridge::active_ethereum_range().map(|active_eth_range| {
                        (active_eth_range.range, active_eth_range.partition)
                    })
                }
                _ => {
                    None
                }
            }
        }

        fn query_has_author_casted_vote(instance_id: InstanceId, account_id: AccountId) -> bool{
            match instance_id {
                MAIN_ETH_BRIDGE_ID => {
                    EthBridge::author_has_cast_event_vote(&account_id) ||
                    EthBridge::author_has_submitted_latest_block(&account_id)
                         },
                SECONDARY_ETH_BRIDGE_ID => {
                    EthSecondBridge::author_has_cast_event_vote(&account_id) ||
                    EthSecondBridge::author_has_submitted_latest_block(&account_id)
                         }
                _ => false
            }
        }

        fn query_signatures(instance_id: InstanceId) -> Vec<sp_core::H256> {
            match instance_id {
                MAIN_ETH_BRIDGE_ID => {
                    EthBridge::signatures()
                },
                SECONDARY_ETH_BRIDGE_ID => {
                    EthSecondBridge::signatures()
                }
                _ => Default::default()
            }
        }

        fn submit_vote(
            instance_id: InstanceId,
            author: AccountId,
            events_partition: EthereumEventsPartition,
            signature: sp_core::sr25519::Signature,
        ) -> Option<()>{
            match instance_id {
                MAIN_ETH_BRIDGE_ID => {
                    EthBridge::submit_vote(author, events_partition, signature.into()).ok()
                },
                SECONDARY_ETH_BRIDGE_ID => {
                    EthSecondBridge::submit_vote(author, events_partition, signature.into()).ok()
                }
                _ => None
            }
        }

        fn submit_latest_ethereum_block(
            instance_id: InstanceId,
            author: AccountId,
            latest_seen_block: u32,
            signature: sp_core::sr25519::Signature
        ) -> Option<()>{
            match instance_id {
                MAIN_ETH_BRIDGE_ID => {
                    EthBridge::submit_latest_ethereum_block_vote(author, latest_seen_block, signature.into()).ok()
                },
                SECONDARY_ETH_BRIDGE_ID => {
                    EthSecondBridge::submit_latest_ethereum_block_vote(author, latest_seen_block, signature.into()).ok()
                }
                _ => None
            }
        }

        fn additional_transactions(instance_id: InstanceId) -> Option<AdditionalEvents> {
            match instance_id {
                MAIN_ETH_BRIDGE_ID => {
                    EthBridge::active_ethereum_range().map(|active_eth_range| {
                        active_eth_range.additional_transactions
                    })
                },
                SECONDARY_ETH_BRIDGE_ID => {
                    EthSecondBridge::active_ethereum_range().map(|active_eth_range| {
                        active_eth_range.additional_transactions
                    })
                }
                _ => {
                    None
                }
            }
        }

        fn instances() -> BTreeMap<InstanceId, EthBridgeInstance> {
            let main_instance = EthBridge::instance();
            let secondary_instance = EthSecondBridge::instance();

            if main_instance == secondary_instance {
                return BTreeMap::from([(MAIN_ETH_BRIDGE_ID, main_instance)]);
            } else {
                return BTreeMap::from([
                    (MAIN_ETH_BRIDGE_ID, main_instance),
                    (SECONDARY_ETH_BRIDGE_ID, secondary_instance),
                ]);
            }
        }
    }

    impl pallet_cross_chain_voting_runtime_api::CrossChainVotingApi<Block> for Runtime {
        fn get_total_linked_balance(t1_identity_account: H160) -> Balance {
            CrossChainVoting::get_total_linked_balance(t1_identity_account)
        }

        fn get_linked_accounts(t1_identity_account: H160) -> Vec<AccountId> {
            CrossChainVoting::get_linked_accounts(t1_identity_account).to_vec()
        }

        fn get_identity_account(t2_linked_account: AccountId) -> Option<H160> {
            CrossChainVoting::get_identity_account(t2_linked_account)
        }
    }

    impl cumulus_primitives_core::CollectCollationInfo<Block> for Runtime {
        fn collect_collation_info(header: &<Block as BlockT>::Header) -> cumulus_primitives_core::CollationInfo {
            ParachainSystem::collect_collation_info(header)
        }
    }

    #[cfg(feature = "try-runtime")]
    impl frame_try_runtime::TryRuntime<Block> for Runtime {
        fn on_runtime_upgrade(checks: frame_try_runtime::UpgradeCheckSelect) -> (Weight, Weight) {
            use super::configs::RuntimeBlockWeights;

            log::info!("try-runtime::on_runtime_upgrade avn-parachain.");
            let weight = Executive::try_runtime_upgrade(checks).unwrap();
            (weight, RuntimeBlockWeights::get().max_block)
        }

        fn execute_block(
            block: Block,
            state_root_check: bool,
            signature_check: bool,
            select: frame_try_runtime::TryStateSelect,
        ) -> Weight {
            // NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
            // have a backtrace here.
            Executive::try_execute_block(block, state_root_check, signature_check, select).unwrap()
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    impl frame_benchmarking::Benchmark<Block> for Runtime {
        fn benchmark_metadata(extra: bool) -> (
            Vec<frame_benchmarking::BenchmarkList>,
            Vec<frame_support::traits::StorageInfo>,
        ) {
            use frame_benchmarking::{Benchmarking, BenchmarkList};
            use frame_support::traits::StorageInfoTrait;
            use frame_system_benchmarking::Pallet as SystemBench;
            use cumulus_pallet_session_benchmarking::Pallet as SessionBench;
            use super::*;

            let mut list = Vec::<BenchmarkList>::new();
            list_benchmarks!(list, extra);

            let storage_info = AllPalletsWithSystem::storage_info();
            (list, storage_info)
        }

        fn dispatch_benchmark(
            config: frame_benchmarking::BenchmarkConfig
        ) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, sp_runtime::RuntimeString> {
            use frame_benchmarking::{BenchmarkError, Benchmarking, BenchmarkBatch};
            use super::*;

            use frame_system_benchmarking::Pallet as SystemBench;
            impl frame_system_benchmarking::Config for Runtime {
                fn setup_set_code_requirements(code: &Vec<u8>) -> Result<(), BenchmarkError> {
                    ParachainSystem::initialize_for_set_code_benchmark(code.len() as u32);
                    Ok(())
                }

                fn verify_set_code() {
                    System::assert_last_event(cumulus_pallet_parachain_system::Event::<Runtime>::ValidationFunctionStored.into());
                }
            }

            use cumulus_pallet_session_benchmarking::Pallet as SessionBench;
            impl cumulus_pallet_session_benchmarking::Config for Runtime {}

            use frame_support::traits::WhitelistedStorageKeys;
            let whitelist = AllPalletsWithSystem::whitelisted_storage_keys();

            let mut batches = Vec::<BenchmarkBatch>::new();
            let params = (&config, &whitelist);
            add_benchmarks!(params, batches);

            if batches.is_empty() { return Err("Benchmark not found for this pallet.".into()) }
            Ok(batches)
        }
    }

    impl sp_genesis_builder::GenesisBuilder<Block> for Runtime {
        fn build_state(config: Vec<u8>) -> sp_genesis_builder::Result {
            build_state::<RuntimeGenesisConfig>(config)
        }

        fn get_preset(id: &Option<sp_genesis_builder::PresetId>) -> Option<Vec<u8>> {
            get_preset::<RuntimeGenesisConfig>(id, |_| None)
        }

        fn preset_names() -> Vec<sp_genesis_builder::PresetId> {
            Default::default()
        }
    }

    impl sp_authority_discovery::AuthorityDiscoveryApi<Block> for Runtime {
        fn authorities() -> Vec<AuthorityDiscoveryId> {
            AuthorityDiscovery::authorities()
        }
    }
}
