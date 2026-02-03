use codec::Codec;
use node_primitives::{AccountId, Balance, Block as BlockT, Nonce};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;

use sc_cli::ChainSpec;

cfg_if::cfg_if! {
if #[cfg(feature = "test-native-runtime")] {
    pub use avn_test_runtime::{Block, RuntimeApi};
    pub use crate::common::TestParachainExecutor as ParachainExecutor;
}
else {
    pub use avn_parachain_runtime::{Block, RuntimeApi};
    pub use crate::common::AvnParachainExecutor as ParachainExecutor;
}
}

/// A set of APIs that polkadot-like runtimes must implement.
pub trait AvnRuntimeApiCollection:
    sp_transaction_pool::runtime_api::TaggedTransactionQueue<BlockT>
    + sp_api::ApiExt<BlockT>
    + sp_block_builder::BlockBuilder<BlockT>
    + substrate_frame_rpc_system::AccountNonceApi<BlockT, AccountId, Nonce>
    + pallet_transaction_payment_rpc::TransactionPaymentRuntimeApi<BlockT, Balance>
    + sp_api::Metadata<BlockT>
    + sp_offchain::OffchainWorkerApi<BlockT>
    + sp_session::SessionKeys<BlockT>
    + cumulus_primitives_core::CollectCollationInfo<BlockT>
    + sp_consensus_aura::AuraApi<BlockT, AuraId>
    + pallet_eth_bridge_runtime_api::EthEventHandlerApi<BlockT, AccountId>
where
    AccountId: Codec,
{
}

impl<Api> AvnRuntimeApiCollection for Api where
    Api: sp_transaction_pool::runtime_api::TaggedTransactionQueue<BlockT>
        + sp_api::ApiExt<BlockT>
        + sp_block_builder::BlockBuilder<BlockT>
        + substrate_frame_rpc_system::AccountNonceApi<BlockT, AccountId, Nonce>
        + pallet_transaction_payment_rpc::TransactionPaymentRuntimeApi<BlockT, Balance>
        + sp_api::Metadata<BlockT>
        + sp_offchain::OffchainWorkerApi<BlockT>
        + sp_session::SessionKeys<BlockT>
        + cumulus_primitives_core::CollectCollationInfo<BlockT>
        + sp_consensus_aura::AuraApi<BlockT, AuraId>
        + pallet_eth_bridge_runtime_api::EthEventHandlerApi<BlockT, AccountId>
{
}

/// Native executor type.

#[cfg(feature = "avn-native-runtime")]
pub struct AvnParachainExecutor;

#[cfg(feature = "avn-native-runtime")]
impl sc_executor::NativeExecutionDispatch for AvnParachainExecutor {
    type ExtendHostFunctions = frame_benchmarking::benchmarking::HostFunctions;

    fn dispatch(method: &str, data: &[u8]) -> Option<Vec<u8>> {
        avn_parachain_runtime::api::dispatch(method, data)
    }

    fn native_version() -> sc_executor::NativeVersion {
        avn_parachain_runtime::native_version()
    }
}

#[cfg(feature = "test-native-runtime")]
pub struct TestParachainExecutor;

#[cfg(feature = "test-native-runtime")]
impl sc_executor::NativeExecutionDispatch for TestParachainExecutor {
    type ExtendHostFunctions = frame_benchmarking::benchmarking::HostFunctions;

    fn dispatch(method: &str, data: &[u8]) -> Option<Vec<u8>> {
        avn_test_runtime::api::dispatch(method, data)
    }

    fn native_version() -> sc_executor::NativeVersion {
        avn_test_runtime::native_version()
    }
}

pub trait AvnRuntimeIdentity {
    #[allow(dead_code)]
    fn is_test_runtime(&self) -> bool;
    #[allow(dead_code)]
    fn is_production(&self) -> bool;
}

impl AvnRuntimeIdentity for Box<dyn ChainSpec> {
    fn is_production(&self) -> bool {
        self.id().starts_with("avn_rococo") || self.id().starts_with("avn_polkadot")
    }
    fn is_test_runtime(&self) -> bool {
        self.id().starts_with("avn_garde")
    }
}
