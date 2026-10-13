use crate::{
    chain::ChainClient,
    ethereum_events_handler::EthEventHandlerConfig,
    evm::client::EvmClient,
    server::AppState,
    signing::{KeystoreSignerProvider, SignerProvider},
};
use anyhow::{Context, Result};
use pallet_eth_bridge_runtime_api::EthEventHandlerApi;
use sc_client_api::{BlockBackend, UsageProvider};
use sc_keystore::LocalKeystore;
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use sp_api::ApiExt;
use sp_avn_common::primitives::AccountId;
use sp_block_builder::BlockBuilder;
use sp_blockchain::HeaderBackend;
use sp_runtime::traits::Block as BlockT;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use url::Url;

#[derive(Clone)]
pub struct NodeDeps<Block: BlockT, ClientT> {
    pub keystore: Arc<LocalKeystore>,
    pub keystore_path: PathBuf,
    pub avn_port: Option<String>,
    pub eth_node_urls: Vec<String>,
    /// Ethereum endpoints for the floor channel; empty means the built-in mainnet defaults.
    /// Watchtowers read floor observations from them; every node uses them for checkpoints.
    pub floor_rpc_urls: Vec<String>,
    /// Venue chain endpoints for the venue channel; empty means the built-in Base defaults.
    pub venue_rpc_urls: Vec<String>,
    /// Whether this node is a registered watchtower. Only those serve observation routes.
    pub registered_node: bool,
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
    let (chain, signer_provider) = if let Some(first_url) = deps.eth_node_urls.first().cloned() {
        log::info!("external-service using RPC url for HTTP send path: {}", first_url);

        let evm_rpc_url: Url = first_url
            .parse()
            .with_context(|| format!("invalid ethereum rpc url: {first_url}"))?;

        let chain: Arc<dyn ChainClient> = Arc::new(
            EvmClient::new_http(evm_rpc_url.as_str())
                .map_err(|e| anyhow::anyhow!("evm client init failed: {e:?}"))?,
        );

        let signer_provider: Arc<dyn SignerProvider> =
            Arc::new(KeystoreSignerProvider::new(deps.keystore_path.clone(), evm_rpc_url));

        (Some(chain), Some(signer_provider))
    } else {
        log::info!("external-service starting without Ethereum RPC configuration");
        (None, None)
    };

    let floor_urls = oracle_endpoints(
        "floor",
        &deps.floor_rpc_urls,
        &deps.eth_node_urls,
        &crate::floor_oracle::DEFAULT_RPC_URLS,
    );
    let venue_urls = oracle_endpoints(
        "venue",
        &deps.venue_rpc_urls,
        &deps.eth_node_urls,
        &crate::venue_oracle::DEFAULT_RPC_URLS,
    );
    // Both channel lists already carry the bridge endpoints.
    let checkpoint_endpoints =
        Arc::new(crate::checkpoint::Endpoints::new(vec![floor_urls.clone(), venue_urls.clone()]));
    let (floor_rpc_urls, venue_rpc_urls) = if deps.registered_node {
        (Some(floor_urls), Some(venue_urls))
    } else {
        log::info!("external-service observation routes disabled: not a registered node");
        (None, None)
    };

    Ok(AppState::<Block, ClientT> {
        keystore: deps.keystore.clone(),
        keystore_path: deps.keystore_path.clone(),
        avn_port: deps.avn_port.clone(),
        chain,
        signer_provider,
        floor_rpc_urls,
        venue_rpc_urls,
        checkpoint_endpoints,
        node_country: Arc::new(crate::node_country::NodeCountry::default()),
        client: deps.client.clone(),
        send_lock: Arc::new(Mutex::new(())),
        _block: Default::default(),
    })
}

/// Endpoints of one oracle channel, in failover order: the channel's own flag, then the
/// bridge endpoints (`--ethereum-node-url`), then the built-in public defaults. Every endpoint
/// is checked against the requested chain id before use, so one on another chain is only a
/// skipped probe. Duplicates are dropped, first occurrence wins.
fn oracle_endpoints(
    channel: &str,
    configured: &[String],
    bridge: &[String],
    defaults: &[&str],
) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    for url in configured
        .iter()
        .chain(bridge.iter())
        .map(String::as_str)
        .chain(defaults.iter().copied())
    {
        if !urls.iter().any(|known| known == url) {
            urls.push(url.to_string());
        }
    }
    log::info!("external-service {channel} endpoints: {urls:?}");
    urls
}

#[cfg(test)]
mod tests {
    use super::oracle_endpoints;

    #[test]
    fn channel_flag_then_bridge_then_defaults_without_duplicates() {
        let own = vec!["https://own".to_string(), "https://bridge".to_string()];
        let bridge = vec!["https://bridge".to_string()];
        let urls = oracle_endpoints("floor", &own, &bridge, &["https://public", "https://own"]);
        assert_eq!(urls, vec!["https://own", "https://bridge", "https://public"]);

        let urls = oracle_endpoints("floor", &[], &bridge, &["https://public"]);
        assert_eq!(urls, vec!["https://bridge", "https://public"]);

        let urls = oracle_endpoints("venue", &[], &[], &["https://public"]);
        assert_eq!(urls, vec!["https://public"]);
    }
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
