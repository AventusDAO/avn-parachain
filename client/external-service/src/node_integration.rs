use crate::{
    chain::ChainClient,
    ethereum_events_handler::EthEventHandlerConfig,
    evm::client::EvmClient,
    server::AppState,
    signing::{KeystoreSignerProvider, SignerProvider},
};
use anyhow::{anyhow, Context, Result};
use node_primitives::AccountId;
use pallet_eth_bridge_runtime_api::EthEventHandlerApi;
use sc_client_api::{BlockBackend, UsageProvider};
use sc_keystore::LocalKeystore;
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use sp_api::ApiExt;
use sp_block_builder::BlockBuilder;
use sp_blockchain::HeaderBackend;
use sp_runtime::traits::Block as BlockT;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use url::Url;

#[derive(Clone)]
pub struct NodeDeps<Block: BlockT, ClientT> {
    pub keystore: Arc<LocalKeystore>,
    pub keystore_path: PathBuf,
    pub avn_port: Option<String>,
    pub eth_node_urls: Vec<String>,
    pub client: Arc<ClientT>,
    pub offchain_transaction_pool_factory: OffchainTransactionPoolFactory<Block>,
}

pub fn build_app_state<Block, ClientT>(
    deps: &NodeDeps<Block, ClientT>,
) -> Result<AppState<Block, ClientT>>
where
    Block: BlockT,
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    let first_url = deps
        .eth_node_urls
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("no ethereum node urls configured"))?;

    let evm_rpc_url: Url = first_url
        .parse()
        .with_context(|| format!("invalid ethereum rpc url: {first_url}"))?;

    let chain: Arc<dyn ChainClient> = Arc::new(
        EvmClient::new_http(evm_rpc_url.as_str())
            .map_err(|e| anyhow!("evm client init failed: {e:?}"))?,
    );

    let signer_provider: Arc<dyn SignerProvider> =
        Arc::new(KeystoreSignerProvider::new(deps.keystore_path.clone(), evm_rpc_url));

    Ok(AppState::<Block, ClientT> {
        keystore: deps.keystore.clone(),
        keystore_path: deps.keystore_path.clone(),
        avn_port: deps.avn_port.clone(),
        chain,
        signer_provider,
        client: deps.client.clone(),
        _block: Default::default(),
    })
}

pub fn build_eth_event_handler_config<Block, ClientT>(
    deps: NodeDeps<Block, ClientT>,
) -> EthEventHandlerConfig<Block, ClientT>
where
    Block: BlockT,
    ClientT: BlockBackend<Block>
        + UsageProvider<Block>
        + HeaderBackend<Block>
        + sp_api::ProvideRuntimeApi<Block>,
    ClientT::Api: EthEventHandlerApi<Block, AccountId> + ApiExt<Block> + BlockBuilder<Block>,
{
    EthEventHandlerConfig::<Block, ClientT> {
        keystore: deps.keystore,
        keystore_path: deps.keystore_path,
        avn_port: deps.avn_port,
        eth_node_urls: deps.eth_node_urls,
        evm_clients: HashMap::new(),
        client: deps.client,
        offchain_transaction_pool_factory: deps.offchain_transaction_pool_factory,
    }
}
