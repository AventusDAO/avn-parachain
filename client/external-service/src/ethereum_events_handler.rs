use crate::{
    chain::{ChainClient, ChainLog, LogFilter},
    evm::client::EvmClient,
    timer::Timer,
    ETH_FINALITY,
};
use futures::future::try_join_all;
use node_primitives::AccountId;
use pallet_eth_bridge_runtime_api::EthEventHandlerApi;
use sc_client_api::{BlockBackend, UsageProvider};
use sc_keystore::LocalKeystore;
use sp_api::ApiExt;
use sp_avn_common::{
    eth::EthBridgeInstance,
    event_discovery::{
        encode_eth_event_submission_data, events_helpers::EthereumEventsPartitionFactory,
        DiscoveredEvent, EthBlockRange, EthereumEventsPartition,
    },
    event_types::{
        AddedValidatorData, AvtGrowthLiftedData, AvtLowerClaimedData, Error, EthEvent, EthEventId,
        EthTransactionId, EventData, LiftedData, LowerRevertedData, NftCancelListingData,
        NftEndBatchListingData, NftMintData, NftTransferToData, ValidEvents,
    },
    AVN_KEY_ID,
};
use sp_block_builder::BlockBuilder;
use sp_blockchain::HeaderBackend;
use sp_core::{sr25519::Public, H160, H256};
use sp_keystore::Keystore;
use sp_runtime::traits::Block as BlockT;
use std::collections::HashMap;
pub use std::{path::PathBuf, sync::Arc};
use tokio::time::{sleep, Duration};

pub use sp_avn_common::context_constants::{
    SUBMIT_ETHEREUM_EVENTS_HASH_CONTEXT, SUBMIT_LATEST_ETH_BLOCK_CONTEXT,
};

use pallet_eth_bridge_runtime_api::InstanceId;
use sc_transaction_pool_api::OffchainTransactionPoolFactory;

pub struct EventInfo {
    parser: fn(Option<Vec<u8>>, Vec<Vec<u8>>) -> Result<EventData, AppError>,
}

#[derive(Clone, Debug)]
pub struct CurrentNodeAuthor {
    address: Public,
    signing_key: Public,
}

impl CurrentNodeAuthor {
    pub fn new(address: Public, signing_key: Public) -> Self {
        CurrentNodeAuthor { address, signing_key }
    }
}

pub struct EventRegistry {
    registry: HashMap<H256, EventInfo>,
}

impl EventRegistry {
    pub fn new() -> Self {
        let mut m = HashMap::new();

        m.insert(
            ValidEvents::AddedValidator.signature(),
            EventInfo {
                parser: |data, topics| {
                    AddedValidatorData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogAddedValidator)
                },
            },
        );

        m.insert(
            ValidEvents::Lifted.signature(),
            EventInfo {
                parser: |data, topics| {
                    LiftedData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogLifted)
                },
            },
        );

        m.insert(
            ValidEvents::AvtGrowthLifted.signature(),
            EventInfo {
                parser: |data, topics| {
                    AvtGrowthLiftedData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogAvtGrowthLifted)
                },
            },
        );

        m.insert(
            ValidEvents::NftCancelListing.signature(),
            EventInfo {
                parser: |data, topics| {
                    NftCancelListingData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogNftCancelListing)
                },
            },
        );

        m.insert(
            ValidEvents::NftEndBatchListing.signature(),
            EventInfo {
                parser: |data, topics| {
                    NftEndBatchListingData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogNftEndBatchListing)
                },
            },
        );

        m.insert(
            ValidEvents::NftMint.signature(),
            EventInfo {
                parser: |data, topics| {
                    NftMintData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogNftMinted)
                },
            },
        );

        m.insert(
            ValidEvents::NftTransferTo.signature(),
            EventInfo {
                parser: |data, topics| {
                    NftTransferToData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogNftTransferTo)
                },
            },
        );

        m.insert(
            ValidEvents::AvtLowerClaimed.signature(),
            EventInfo {
                parser: |data, topics| {
                    AvtLowerClaimedData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogLowerClaimed)
                },
            },
        );

        m.insert(
            ValidEvents::LowerReverted.signature(),
            EventInfo {
                parser: |data, topics| {
                    LowerRevertedData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogLowerReverted)
                },
            },
        );

        m.insert(
            ValidEvents::LiftedToPredictionMarket.signature(),
            EventInfo {
                parser: |data, topics| {
                    LiftedData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogLiftedToPredictionMarket)
                },
            },
        );

        m.insert(
            ValidEvents::Erc20DirectTransfer.signature(),
            EventInfo {
                parser: |data, topics| {
                    LiftedData::from_erc_20_contract_transfer_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(EventData::LogErc20Transfer)
                },
            },
        );

        EventRegistry { registry: m }
    }

    pub fn get_event_info(&self, signature: &H256) -> Option<&EventInfo> {
        self.registry.get(signature)
    }
}

#[derive(Debug)]
pub enum AppError {
    ErrorParsingEventLogs,
    ErrorGettingEventLogs,
    ErrorGettingBridgeContract,
    RetryLimitReached,
    SignatureGenerationFailed,
    MissingTransactionHash,
    MissingBlockNumber,
    MissingEventSignature,
    ParsingError(Error),
    GenericError(String),
}

/// Identifies secondary events associated with the bridge contract
pub async fn identify_secondary_bridge_events(
    chain: &dyn ChainClient,
    start_block: u32,
    end_block: u32,
    contract_addresses: &[H160],
    event_types: Vec<ValidEvents>,
) -> Result<Vec<ChainLog>, AppError> {
    let topic0: Vec<H256> = event_types.iter().map(|e| e.signature()).collect();

    let topic2: Vec<H256> = contract_addresses
        .iter()
        .map(|a| {
            let mut b = [0u8; 32];
            b[12..].copy_from_slice(a.as_bytes());
            H256::from(b)
        })
        .collect();

    let filter = LogFilter {
        from_block: start_block as u64,
        to_block: end_block as u64,
        addresses: vec![],
        topics: [Some(topic0), None, Some(topic2), None],
    };

    chain.get_logs(filter).await.map_err(|_| AppError::ErrorGettingEventLogs)
}

pub async fn identify_primary_bridge_events(
    chain: &dyn ChainClient,
    start_block: u32,
    end_block: u32,
    bridge_contract_addresses: &[H160],
    event_types: Vec<ValidEvents>,
) -> Result<Vec<ChainLog>, AppError> {
    let topic0: Vec<H256> = event_types.iter().map(|e| e.signature()).collect();

    let filter = LogFilter {
        from_block: start_block as u64,
        to_block: end_block as u64,
        addresses: bridge_contract_addresses.to_vec(),
        topics: [Some(topic0), None, None, None],
    };

    chain.get_logs(filter).await.map_err(|_| AppError::ErrorGettingEventLogs)
}

pub async fn identify_events(
    chain: &dyn ChainClient,
    start_block: u32,
    end_block: u32,
    contract_addresses: &[H160],
    event_signatures_to_find: Vec<H256>,
    events_registry: &EventRegistry,
) -> Result<Vec<DiscoveredEvent>, AppError> {
    let (all_primary_events, all_secondary_events): (Vec<_>, Vec<_>) =
        ValidEvents::values().into_iter().partition(|event| event.is_primary());

    // First identify all possible primary events from the bridge contract, to ensure that if the
    // primary event isn't a part of the signatures to find, a secondary event will not be
    // accidentally included to its place.
    let logs = identify_primary_bridge_events(
        chain,
        start_block,
        end_block,
        contract_addresses,
        all_primary_events,
    )
    .await?;

    // If the event signatures we are looking, contain secondary events, conduct a secondary event
    // discovery.
    let extend_discovery_to_secondary_events = event_signatures_to_find
        .iter()
        .filter_map(|sig| ValidEvents::try_from(sig).ok())
        .any(|x| all_secondary_events.contains(&x));

    let secondary_logs = if extend_discovery_to_secondary_events {
        identify_secondary_bridge_events(
            chain,
            start_block,
            end_block,
            contract_addresses,
            all_secondary_events,
        )
        .await?
    } else {
        Vec::new()
    };

    // Combine the discovered primary and secondary events, ensuring that each tx id has a single
    // entry, with the primary taking precedence over the secondary
    let mut unique_transactions = HashMap::<H256, DiscoveredEvent>::new();
    for log in logs.into_iter().chain(secondary_logs.into_iter()) {
        if let Some(tx_hash) = log.transaction_hash {
            if unique_transactions.contains_key(&tx_hash) {
                continue;
            }
            let discovered_event = parse_log(log, events_registry)?;
            unique_transactions.insert(tx_hash, discovered_event);
        }
    }
    // Finally use the signatures to find, to filter the combined list and report back to the
    // runtime.
    unique_transactions
        .retain(|_, value| event_signatures_to_find.contains(&value.event.event_id.signature));
    Ok(unique_transactions.into_values().collect())
}

pub async fn identify_additional_event_info(
    chain: &dyn ChainClient,
    additional_transactions_to_check: &[EthTransactionId],
) -> Result<Vec<u64>, AppError> {
    let futures = additional_transactions_to_check.iter().map(|tx| {
        let h = H256::from_slice(&tx.to_fixed_bytes());
        chain.get_receipt(h)
    });

    let results = try_join_all(futures).await.map_err(|_| AppError::ErrorGettingEventLogs)?;

    Ok(results.into_iter().flatten().filter_map(|r| r.block_number).collect())
}

pub async fn identify_additional_events(
    chain: &dyn ChainClient,
    contract_addresses: &[H160],
    event_signatures_to_find: &[H256],
    events_registry: &EventRegistry,
    additional_transactions_to_check: Vec<EthTransactionId>,
) -> Result<Vec<DiscoveredEvent>, AppError> {
    let additional_blocks =
        identify_additional_event_info(chain, &additional_transactions_to_check).await?;

    let futures = additional_blocks.iter().map(|b| {
        identify_events(
            chain,
            *b as u32,
            *b as u32,
            contract_addresses,
            event_signatures_to_find.to_vec(),
            events_registry,
        )
    });

    let additional_events: Vec<DiscoveredEvent> =
        try_join_all(futures).await?.into_iter().flatten().collect();

    Ok(additional_events)
}

fn parse_log(log: ChainLog, events_registry: &EventRegistry) -> Result<DiscoveredEvent, AppError> {
    if log.topics.is_empty() {
        return Err(AppError::MissingEventSignature);
    }

    let signature = log.topics[0];
    let tx_hash = log.transaction_hash.ok_or(AppError::MissingTransactionHash)?;
    let block_number = log.block_number.ok_or(AppError::MissingBlockNumber)?;

    let event_id = EthEventId { signature, transaction_hash: tx_hash };

    let topics: Vec<Vec<u8>> = log.topics.iter().map(|t| t.as_bytes().to_vec()).collect();
    let data: Option<Vec<u8>> = if log.data.is_empty() { None } else { Some(log.data) };

    let mut event_data = parse_event_data(signature, data, topics, events_registry)?;

    if let EventData::LogErc20Transfer(ref mut d) = event_data {
        if d.token_contract.is_zero() {
            d.token_contract = sp_core::H160::from_slice(log.address.as_bytes());
        }
    }

    Ok(DiscoveredEvent { event: EthEvent { event_id, event_data }, block: block_number })
}

fn parse_event_data(
    signature: H256,
    data: Option<Vec<u8>>,
    topics: Vec<Vec<u8>>,
    events_registry: &EventRegistry,
) -> Result<EventData, AppError> {
    (events_registry
        .get_event_info(&signature)
        .ok_or(AppError::ErrorParsingEventLogs)?
        .parser)(data, topics)
}

pub struct EthEventHandlerConfig<Block: BlockT, ClientT>
where
    Block: BlockT,
    ClientT: BlockBackend<Block>
        + UsageProvider<Block>
        + HeaderBackend<Block>
        + sp_api::ProvideRuntimeApi<Block>,
    ClientT::Api: pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId>
        + ApiExt<Block>
        + BlockBuilder<Block>,
{
    pub keystore: Arc<LocalKeystore>,
    pub keystore_path: PathBuf,
    pub avn_port: Option<String>,
    pub eth_node_urls: Vec<String>,
    pub evm_clients: HashMap<u64, Arc<EvmClient>>,
    pub client: Arc<ClientT>,
    pub offchain_transaction_pool_factory: OffchainTransactionPoolFactory<Block>,
}

impl<
        Block: BlockT,
        ClientT: BlockBackend<Block>
            + UsageProvider<Block>
            + HeaderBackend<Block>
            + sp_api::ProvideRuntimeApi<Block>,
    > EthEventHandlerConfig<Block, ClientT>
where
    ClientT::Api: pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId>
        + ApiExt<Block>
        + BlockBuilder<Block>,
{
    pub async fn initialise_evm(
        &mut self,
        wanted_chain_id: u64,
    ) -> Result<Arc<EvmClient>, AppError> {
        let _init_time = Timer::new("ethereum-event-handler EVM client initialization");
        log::info!("⛓️  avn-events-handler: evm client init start");

        for eth_node_url in self.eth_node_urls.iter() {
            log::debug!("⛓️  Attempting to connect to EVM node: {}", eth_node_url);

            let client = match EvmClient::new_http(eth_node_url) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("💔 Error creating EVM client for URL {}: {:?}", eth_node_url, e);
                    continue;
                },
            };

            let chain_id = match client.chain_id().await {
                Ok(id) => id,
                Err(e) => {
                    log::error!(
                        "💔 Connected but failed to get chain id for {}: {:?}",
                        eth_node_url,
                        e
                    );
                    continue;
                },
            };

            log::info!(
                "⛓️  Successfully connected to node: {} with chain ID: {}",
                eth_node_url,
                chain_id
            );

            if self.evm_clients.get(&chain_id).is_some() {
                log::debug!(
                    "⛓️  EVM client for chain ID {} already exists, skipping creation.",
                    chain_id
                );
            } else {
                let arc = Arc::new(client);
                self.evm_clients.insert(chain_id, Arc::clone(&arc));
            }

            if chain_id == wanted_chain_id {
                return Ok(Arc::clone(self.evm_clients.get(&chain_id).expect("inserted above")));
            }
        }

        Err(AppError::GenericError(
            "Failed to acquire a valid EVM client for the instance.".to_string(),
        ))
    }
}

pub const SLEEP_TIME: u64 = 60;
pub const RETRY_LIMIT: usize = 3;
pub const RETRY_DELAY: u64 = 5;

async fn get_evm_client_for_instance<Block, ClientT>(
    config: &mut EthEventHandlerConfig<Block, ClientT>,
    instance: &EthBridgeInstance,
) -> Result<Arc<EvmClient>, AppError>
where
    Block: BlockT,
    ClientT: BlockBackend<Block>
        + UsageProvider<Block>
        + HeaderBackend<Block>
        + sp_api::ProvideRuntimeApi<Block>,
    ClientT::Api: pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId>
        + ApiExt<Block>
        + BlockBuilder<Block>,
{
    let chain_id = instance.network.chain_id();

    if let Some(c) = config.evm_clients.get(&chain_id) {
        log::debug!("⛓️  Found existing EVM client for chain: {}", chain_id);
        return Ok(Arc::clone(c));
    }

    log::debug!("⛓️  No EVM client found for chain {}. Initialising...", chain_id);

    let mut attempts = 0;
    while attempts < RETRY_LIMIT {
        match config.initialise_evm(chain_id).await {
            Ok(c) => return Ok(c),
            Err(e) => {
                attempts += 1;
                log::error!("Failed to initialize EVM client (attempt {}): {:?}", attempts, e);
                if attempts >= RETRY_LIMIT {
                    return Err(AppError::RetryLimitReached);
                }
                sleep(Duration::from_secs(RETRY_DELAY)).await;
            },
        }
    }

    Err(AppError::GenericError(
        "Failed to initialize EVM client after multiple attempts.".to_string(),
    ))
}

fn find_current_node_author<T>(
    authors: Result<Vec<([u8; 32], [u8; 32])>, T>,
    mut node_signing_keys: Vec<Public>,
) -> Option<CurrentNodeAuthor> {
    if let Ok(authors) = authors {
        node_signing_keys.sort();

        // Return the current node's address (NOT signing key)
        return authors
            .into_iter()
            .enumerate()
            .filter_map(move |(_, author)| {
                node_signing_keys.binary_search(&Public::from_raw(author.1)).ok().map(|_| {
                    CurrentNodeAuthor::new(Public::from_raw(author.0), Public::from_raw(author.1))
                })
            })
            .nth(0)
    }

    None
}

pub async fn start_eth_event_handler<Block, ClientT>(config: EthEventHandlerConfig<Block, ClientT>)
where
    Block: BlockT,
    ClientT: BlockBackend<Block>
        + UsageProvider<Block>
        + HeaderBackend<Block>
        + sp_api::ProvideRuntimeApi<Block>,
    ClientT::Api: pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId>
        + ApiExt<Block>
        + BlockBuilder<Block>,
{
    let mut config = config;

    let events_registry = EventRegistry::new();

    log::info!("⛓️  Ethereum events handler service initialised.");

    let current_node_author;
    loop {
        let authors = config
            .client
            .runtime_api()
            .query_authors(config.client.info().best_hash)
            .map_err(|e| {
                log::error!("Error querying authors: {:?}", e);
            });

        let node_signing_keys = config.keystore.sr25519_public_keys(AVN_KEY_ID);
        if let Some(node_author) =
            find_current_node_author(authors.clone(), node_signing_keys.clone())
        {
            current_node_author = node_author;
            break
        }
        log::error!("Author not found. Will attempt again after a while. Chain signing keys: {:?}, keystore keys: {:?}.",
            authors,
            node_signing_keys,
        );

        sleep(Duration::from_secs(10 * SLEEP_TIME)).await;
        continue
    }

    log::info!("Current node author address set: {:?}", current_node_author);

    loop {
        match query_runtime_and_process(&mut config, &current_node_author, &events_registry).await {
            Ok(_) => (),
            Err(e) => log::error!("{:?}", e),
        }

        log::debug!("Sleeping");
        sleep(Duration::from_secs(SLEEP_TIME)).await;
    }
}

async fn query_runtime_and_process<Block, ClientT>(
    config: &mut EthEventHandlerConfig<Block, ClientT>,
    current_node_author: &CurrentNodeAuthor,
    events_registry: &EventRegistry,
) -> Result<(), String>
where
    Block: BlockT,
    ClientT: BlockBackend<Block>
        + UsageProvider<Block>
        + HeaderBackend<Block>
        + sp_api::ProvideRuntimeApi<Block>,
    ClientT::Api: pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId>
        + ApiExt<Block>
        + BlockBuilder<Block>,
{
    let instances = if config
        .client
        .runtime_api()
        .has_api_with::<dyn EthEventHandlerApi<Block, AccountId>, _>(
            config.client.info().best_hash,
            |v| v >= 3,
        )
        .unwrap_or(false)
    {
        log::debug!("Querying eth-bridge instances...");

        config
            .client
            .runtime_api()
            .instances(config.client.info().best_hash)
            .map_err(|err| format!("Failed to get instances: {:?}", err))?
    } else {
        Default::default()
    };
    log::debug!("Eth-bridge instances found: {:?}", &instances);
    for (instance_id, instance) in instances {
        let result = &config
            .client
            .runtime_api()
            .query_active_block_range(config.client.info().best_hash, instance_id)
            .map_err(|err| format!("Failed to query bridge contract: {:?}", err))?;

        let evm = match get_evm_client_for_instance(config, &instance).await {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to initialize EVM client for instance: {:?}", e);
                continue;
            },
        };

        match result {
            // A range is active, attempt processing
            Some((range, partition_id)) => {
                log::info!("Getting events for range starting at: {:?}", range.start_block);

                if is_evm_block_finalised(evm.as_ref(), range.end_block() as u64, ETH_FINALITY)
                    .await?
                {
                    process_events(
                        evm.as_ref(),
                        config,
                        instance_id,
                        &instance,
                        range.clone(),
                        *partition_id,
                        &current_node_author,
                        &events_registry,
                    )
                    .await?;
                }
            },
            // There is no active range, attempt initial range voting.
            None => {
                log::info!("Active range setup - Submitting latest block");
                submit_latest_ethereum_block(
                    evm.as_ref(),
                    config,
                    instance_id,
                    &instance,
                    &current_node_author,
                )
                .await?;
            },
        };
    }

    Ok(())
}

async fn submit_latest_ethereum_block<Block, ClientT>(
    evm: &EvmClient,
    config: &EthEventHandlerConfig<Block, ClientT>,
    instance_id: InstanceId,
    eth_bridge_instance: &EthBridgeInstance,
    current_node_author: &CurrentNodeAuthor,
) -> Result<(), String>
where
    Block: BlockT,
    ClientT: BlockBackend<Block>
        + UsageProvider<Block>
        + HeaderBackend<Block>
        + sp_api::ProvideRuntimeApi<Block>,
    ClientT::Api: pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId>
        + ApiExt<Block>
        + BlockBuilder<Block>,
{
    let has_casted_vote = config
        .client
        .runtime_api()
        .query_has_author_casted_vote(
            config.client.info().best_hash,
            instance_id,
            current_node_author.address.0.into(),
        )
        .map_err(|err| format!("Failed to check if author has cast latest vote: {:?}", err))?;

    log::debug!("Checking if vote has been cast already. Result: {:?}", has_casted_vote);

    if !has_casted_vote {
        log::debug!("Getting current block from Ethereum");
        let latest_seen_ethereum_block = evm
            .block_number()
            .await
            .map_err(|err| format!("Failed to retrieve latest evm block: {:?}", err))?
            as u32;

        log::debug!("Encoding proof for latest block: {:?}", latest_seen_ethereum_block);
        let proof = encode_eth_event_submission_data::<AccountId, u32>(
            Some(eth_bridge_instance),
            &SUBMIT_LATEST_ETH_BLOCK_CONTEXT,
            &((*current_node_author).address).into(),
            latest_seen_ethereum_block,
        );

        let signature = config
            .keystore
            .sr25519_sign(
                AVN_KEY_ID,
                &current_node_author.signing_key,
                &proof.into_boxed_slice().as_ref(),
            )
            .map_err(|err| format!("Failed to sign the proof: {:?}", err))?
            .ok_or_else(|| "Signature generation failed".to_string())?;

        log::debug!("Setting up runtime API");
        let mut runtime_api = config.client.runtime_api();
        runtime_api.register_extension(
            config
                .offchain_transaction_pool_factory
                .offchain_transaction_pool(config.client.info().best_hash),
        );

        log::debug!("Sending transaction to runtime");
        runtime_api
            .submit_latest_ethereum_block(
                config.client.info().best_hash,
                instance_id,
                (*current_node_author).address.into(),
                latest_seen_ethereum_block,
                signature,
            )
            .map_err(|err| format!("Failed to submit latest ethereum block vote: {:?}", err))?;

        log::debug!(
            "Latest ethereum block {:?} submitted to pool successfully by {:?}.",
            latest_seen_ethereum_block,
            current_node_author
        );
    }

    Ok(())
}

async fn process_events<Block, ClientT>(
    evm: &EvmClient,
    config: &EthEventHandlerConfig<Block, ClientT>,
    instance_id: InstanceId,
    eth_bridge_instance: &EthBridgeInstance,
    range: EthBlockRange,
    partition_id: u16,
    current_node_author: &CurrentNodeAuthor,
    events_registry: &EventRegistry,
) -> Result<(), String>
where
    Block: BlockT,
    ClientT: BlockBackend<Block>
        + UsageProvider<Block>
        + HeaderBackend<Block>
        + sp_api::ProvideRuntimeApi<Block>,
    ClientT::Api: pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId>
        + ApiExt<Block>
        + BlockBuilder<Block>,
{
    let contract_addresses =
        vec![H160::from_slice(&eth_bridge_instance.bridge_contract.to_fixed_bytes())];

    let event_signatures = config
        .client
        .runtime_api()
        .query_signatures(config.client.info().best_hash, instance_id)
        .map_err(|err| format!("Failed to query event signatures: {:?}", err))?;

    let has_casted_vote = config
        .client
        .runtime_api()
        .query_has_author_casted_vote(
            config.client.info().best_hash,
            instance_id,
            current_node_author.address.0.into(),
        )
        .map_err(|err| format!("Failed to check if author has casted event vote: {:?}", err))?;

    let additional_transactions: Vec<_> = config
        .client
        .runtime_api()
        .additional_transactions(config.client.info().best_hash, instance_id)
        .map_err(|err| format!("Failed to query additional transactions: {:?}", err))?
        .iter()
        .flat_map(|events_set| events_set.iter())
        .cloned()
        .collect();

    if !has_casted_vote {
        execute_event_processing(
            evm,
            config,
            event_signatures,
            instance_id,
            eth_bridge_instance,
            contract_addresses,
            partition_id,
            current_node_author,
            range,
            events_registry,
            additional_transactions,
        )
        .await
    } else {
        Ok(())
    }
}

async fn execute_event_processing<Block, ClientT>(
    evm: &EvmClient,
    config: &EthEventHandlerConfig<Block, ClientT>,
    event_signatures: Vec<H256>,
    instance_id: InstanceId,
    eth_bridge_instance: &EthBridgeInstance,
    contract_addresses: Vec<H160>,
    partition_id: u16,
    current_node_author: &CurrentNodeAuthor,
    range: EthBlockRange,
    events_registry: &EventRegistry,
    additional_transactions_to_check: Vec<EthTransactionId>,
) -> Result<(), String>
where
    Block: BlockT,
    ClientT: BlockBackend<Block>
        + UsageProvider<Block>
        + HeaderBackend<Block>
        + sp_api::ProvideRuntimeApi<Block>,
    ClientT::Api: pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId>
        + ApiExt<Block>
        + BlockBuilder<Block>,
{
    let additional_events = identify_additional_events(
        evm as &dyn ChainClient,
        &contract_addresses,
        &event_signatures,
        events_registry,
        additional_transactions_to_check,
    )
    .await
    .map_err(|err| format!("Error retrieving additional events: {:?}", err))?;

    let range_events = identify_events(
        evm as &dyn ChainClient,
        range.start_block,
        range.end_block(),
        &contract_addresses,
        event_signatures,
        events_registry,
    )
    .await
    .map_err(|err| format!("Error retrieving events: {:?}", err))?;

    let all_events = additional_events.into_iter().chain(range_events.into_iter()).collect();

    let ethereum_events_partitions =
        EthereumEventsPartitionFactory::create_partitions(range, all_events);
    let partition = ethereum_events_partitions
        .iter()
        .find(|p| p.partition() == partition_id)
        .ok_or_else(|| format!("Partition with ID {} not found", partition_id))?;

    let proof = encode_eth_event_submission_data::<AccountId, &EthereumEventsPartition>(
        Some(eth_bridge_instance),
        &SUBMIT_ETHEREUM_EVENTS_HASH_CONTEXT,
        &((*current_node_author).address).into(),
        &partition.clone(),
    );

    let signature = config
        .keystore
        .sr25519_sign(
            AVN_KEY_ID,
            &current_node_author.signing_key,
            &proof.into_boxed_slice().as_ref(),
        )
        .map_err(|err| format!("Failed to sign the proof: {:?}", err))?
        .ok_or_else(|| "Signature generation failed".to_string())?;

    let mut runtime_api = config.client.runtime_api();
    runtime_api.register_extension(
        config
            .offchain_transaction_pool_factory
            .offchain_transaction_pool(config.client.info().best_hash),
    );

    runtime_api
        .submit_vote(
            config.client.info().best_hash,
            instance_id,
            (*current_node_author).address.into(),
            partition.clone(),
            signature,
        )
        .map_err(|err| format!("Failed to submit vote: {:?}", err))?;

    log::info!(
        "Vote for partition [{:?}, {}] submitted to pool successfully",
        partition.range(),
        partition.id()
    );
    Ok(())
}

pub async fn is_evm_block_finalised(
    evm: &EvmClient,
    current_block_num: u64,
    num_blocks_to_wait: u64,
) -> Result<bool, String> {
    let latest_block = evm
        .block_number()
        .await
        .map_err(|e| format!("Failed to get latest block number: {:?}", e))?;
    Ok(latest_block >= current_block_num + num_blocks_to_wait)
}
