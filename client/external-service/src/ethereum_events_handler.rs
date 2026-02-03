use crate::{timer::Web3Timer, web3_utils, BlockT, ETH_FINALITY};
use futures::{future::try_join_all, lock::Mutex};
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
use sp_core::{sr25519::Public, H256 as SpH256};
use sp_keystore::Keystore;
use sp_runtime::SaturatedConversion;
use std::collections::HashMap;
pub use std::{path::PathBuf, sync::Arc};
use tide::Error as TideError;
use tokio::time::{sleep, Duration};
use web3::{
    transports::Http,
    types::{FilterBuilder, Log, TransactionReceipt, H160, H256 as Web3H256, U64},
    Web3,
};

use pallet_eth_bridge::{SUBMIT_ETHEREUM_EVENTS_HASH_CONTEXT, SUBMIT_LATEST_ETH_BLOCK_CONTEXT};
use pallet_eth_bridge_runtime_api::InstanceId;

use crate::{get_chain_id, server_error, setup_web3_connection, Web3Data};
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
    registry: HashMap<SpH256, EventInfo>,
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
                        .map(|data| EventData::LogLifted(data))
                },
            },
        );
        m.insert(
            ValidEvents::AvtGrowthLifted.signature(),
            EventInfo {
                parser: |data, topics| {
                    AvtGrowthLiftedData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(|data| EventData::LogAvtGrowthLifted(data))
                },
            },
        );
        m.insert(
            ValidEvents::NftCancelListing.signature(),
            EventInfo {
                parser: |data, topics| {
                    NftCancelListingData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(|data| EventData::LogNftCancelListing(data))
                },
            },
        );
        m.insert(
            ValidEvents::NftEndBatchListing.signature(),
            EventInfo {
                parser: |data, topics| {
                    NftEndBatchListingData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(|data| EventData::LogNftEndBatchListing(data))
                },
            },
        );
        m.insert(
            ValidEvents::NftMint.signature(),
            EventInfo {
                parser: |data, topics| {
                    NftMintData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(|data| EventData::LogNftMinted(data))
                },
            },
        );
        m.insert(
            ValidEvents::NftTransferTo.signature(),
            EventInfo {
                parser: |data, topics| {
                    NftTransferToData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(|data| EventData::LogNftTransferTo(data))
                },
            },
        );
        m.insert(
            ValidEvents::AvtLowerClaimed.signature(),
            EventInfo {
                parser: |data, topics| {
                    AvtLowerClaimedData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(|data| EventData::LogLowerClaimed(data))
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
                        .map(|data| EventData::LogLiftedToPredictionMarket(data))
                },
            },
        );
        m.insert(
            ValidEvents::Erc20DirectTransfer.signature(),
            EventInfo {
                parser: |data, topics| {
                    LiftedData::from_erc_20_contract_transfer_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(|data| EventData::LogErc20Transfer(data))
                },
            },
        );
        m.insert(
            ValidEvents::LiftedToPredictionMarket.signature(),
            EventInfo {
                parser: |data: Option<Vec<u8>>, topics| {
                    LiftedData::parse_bytes(data, topics)
                        .map_err(|err| AppError::ParsingError(err.into()))
                        .map(|data| EventData::LogLiftedToPredictionMarket(data))
                },
            },
        );

        EventRegistry { registry: m }
    }

    pub fn get_event_info(&self, signature: &SpH256) -> Option<&EventInfo> {
        self.registry.get(signature)
    }
}

#[derive(Debug)]
pub enum AppError {
    ErrorParsingEventLogs,
    ErrorGettingEventLogs,
    ErrorGettingBridgeContract,
    Web3RetryLimitReached,
    SignatureGenerationFailed,
    MissingTransactionHash,
    MissingBlockNumber,
    MissingEventSignature,
    ParsingError(Error),
    GenericError(String),
}

/// Identifies secondary events associated with the bridge contract
pub async fn identify_secondary_bridge_events(
    web3: &Web3<web3::transports::Http>,
    start_block: u32,
    end_block: u32,
    contract_addresses: &Vec<H160>,
    event_types: Vec<ValidEvents>,
) -> Result<Vec<Log>, AppError> {
    let secondary_events_signatures = event_types
        .iter()
        .map(|event| Web3H256::from_slice(&event.signature().to_fixed_bytes()))
        .collect();

    // Currently only ERC-20 transfer are supported, which is the 3rd topic of a Transfer event.
    let contracts_topics =
        contract_addresses.iter().map(|contract| Web3H256::from(*contract)).collect();
    let filter = FilterBuilder::default()
        .topics(Some(secondary_events_signatures), None, Some(contracts_topics), None)
        .from_block(web3::types::BlockNumber::Number(U64::from(start_block)))
        .to_block(web3::types::BlockNumber::Number(U64::from(end_block)))
        .build();

    let logs_result = web3.eth().logs(filter).await;
    log::trace!("Result of secondary bridge events discovery: {:?}", logs_result);
    match logs_result {
        Ok(logs) => Ok(logs),
        Err(_) => return Err(AppError::ErrorGettingEventLogs),
    }
}

pub async fn identify_primary_bridge_events(
    web3: &Web3<web3::transports::Http>,
    start_block: u32,
    end_block: u32,
    bridge_contract_addresses: &Vec<H160>,
    event_types: Vec<ValidEvents>,
) -> Result<Vec<Log>, AppError> {
    let primary_events_signatures: Vec<Web3H256> = event_types
        .iter()
        .map(|event| Web3H256::from_slice(&event.signature().to_fixed_bytes()))
        .collect();

    let filter = FilterBuilder::default()
        .address(bridge_contract_addresses.to_owned())
        .topics(Some(primary_events_signatures), None, None, None)
        .from_block(web3::types::BlockNumber::Number(U64::from(start_block)))
        .to_block(web3::types::BlockNumber::Number(U64::from(end_block)))
        .build();

    let logs_result = web3.eth().logs(filter).await;
    log::trace!("Result of primary bridge events discovery: {:?}", logs_result);
    match logs_result {
        Ok(logs) => Ok(logs),
        Err(_) => return Err(AppError::ErrorGettingEventLogs),
    }
}

pub async fn identify_events(
    web3: &Web3<web3::transports::Http>,
    start_block: u32,
    end_block: u32,
    contract_addresses: &Vec<H160>,
    event_signatures_to_find: Vec<SpH256>,
    events_registry: &EventRegistry,
) -> Result<Vec<DiscoveredEvent>, AppError> {
    let (all_primary_events, all_secondary_events): (Vec<_>, Vec<_>) =
        ValidEvents::values().into_iter().partition(|event| event.is_primary());

    // First identify all possible primary events from the bridge contract, to ensure that if the
    // primary event isn't a part of the signatures to find, a secondary event will not be
    // accidentally included to its place.
    let logs = identify_primary_bridge_events(
        web3,
        start_block,
        end_block,
        &contract_addresses,
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
            web3,
            start_block,
            end_block,
            &contract_addresses,
            all_secondary_events,
        )
        .await?
    } else {
        Default::default()
    };

    log::debug!(
        "🔭 Events found on [{},{}]: primary: {:#?} secondary: {:#?}",
        start_block,
        end_block,
        logs,
        secondary_logs
    );

    // Combine the discovered primary and secondary events, ensuring that each tx id has a single
    // entry, with the primary taking precedence over the secondary
    let mut unique_transactions = HashMap::<Web3H256, DiscoveredEvent>::new();
    for log in logs.into_iter().chain(secondary_logs.into_iter()) {
        if let Some(tx_hash) = log.transaction_hash {
            if unique_transactions.contains_key(&tx_hash) {
                continue
            }
            match parse_log(log, events_registry) {
                Ok(discovered_event) => {
                    unique_transactions.insert(tx_hash, discovered_event);
                },
                Err(err) => return Err(err),
            }
        }
    }
    // Finally use the signatures to find, to filter the combined list and report back to the
    // runtime.
    unique_transactions
        .retain(|_, value| event_signatures_to_find.contains(&value.event.event_id.signature));
    Ok(unique_transactions.into_values().collect())
}

pub async fn identify_additional_event_info(
    web3: &Web3<web3::transports::Http>,
    additional_transactions_to_check: &Vec<EthTransactionId>,
) -> Result<Vec<TransactionReceipt>, AppError> {
    log::debug!("🔭 Additional events to find: {:#?}", additional_transactions_to_check);
    // Create a future for each event
    let futures = additional_transactions_to_check.iter().map(|transaction_hash| async move {
        Ok(web3
            .eth()
            .transaction_receipt(Web3H256::from_slice(&transaction_hash.to_fixed_bytes()))
            .await)
    });

    let results = try_join_all(futures).await?;

    // check results, return early if any error occurred
    let mut additional_transactions_receipts = Vec::new();
    for result in results {
        match result {
            Ok(Some(event)) => additional_transactions_receipts.push(event),
            Ok(None) => {},
            Err(_) => return Err(AppError::ErrorGettingEventLogs),
        }
    }

    log::debug!(
        "🔭 Additional events found to report back: {:#?}",
        &additional_transactions_receipts
    );
    Ok(additional_transactions_receipts)
}

pub async fn identify_additional_events(
    web3: &Web3<web3::transports::Http>,
    contract_addresses: &Vec<H160>,
    event_signatures_to_find: &Vec<SpH256>,
    events_registry: &EventRegistry,
    additional_transactions_to_check: Vec<EthTransactionId>,
) -> Result<Vec<DiscoveredEvent>, AppError> {
    let additional_events_info =
        identify_additional_event_info(web3, &additional_transactions_to_check).await?;

    log::debug!("🔭 Additional transactions to find: {:#?}", &additional_transactions_to_check);
    // Create a future for each event discovery
    let futures = additional_events_info.iter().map(|event_receipt| {
        let contract = contract_addresses.clone();
        async move {
            let identified_events = identify_events(
                web3,
                event_receipt.block_number.unwrap_or_default().saturated_into(),
                event_receipt.block_number.unwrap_or_default().saturated_into(),
                &contract,
                event_signatures_to_find.clone(),
                events_registry,
            )
            .await?;
            Ok(identified_events)
        }
    });

    let additional_events: Vec<DiscoveredEvent> =
        try_join_all(futures).await?.into_iter().flatten().collect();

    log::debug!("🔭 Additional events found to report back: {:#?}", &additional_events);
    Ok(additional_events)
}

fn parse_log(log: Log, events_registry: &EventRegistry) -> Result<DiscoveredEvent, AppError> {
    if log.topics.is_empty() {
        return Err(AppError::MissingEventSignature)
    }
    log::debug!("⛓️ Parsing discovered log: {:?}", &log);

    let web3_signature = log.topics[0];
    let signature = SpH256::from(web3_signature.0);

    let transaction_hash = log.transaction_hash.ok_or(AppError::MissingTransactionHash)?;

    let event_id = EthEventId { signature, transaction_hash: SpH256::from(transaction_hash.0) };

    let topics: Vec<Vec<u8>> = log.topics.iter().map(|t| t.0.to_vec()).collect();
    let data: Option<Vec<u8>> = if log.data.0.is_empty() { None } else { Some(log.data.0) };

    log::debug!(
        "⛓️ Parsing discovered event: signature {:?}, data: {:?}, topics: {:?}",
        signature,
        data,
        topics,
    );
    let mut event_data = parse_event_data(signature, data, topics, events_registry)?;

    let block_number = log.block_number.ok_or(AppError::MissingBlockNumber)?;
    if let EventData::LogErc20Transfer(ref mut data) = event_data {
        if data.token_contract.is_zero() {
            data.token_contract = sp_core::H160::from(log.address.as_fixed_bytes());
        }
    }

    Ok(DiscoveredEvent { event: EthEvent { event_id, event_data }, block: block_number.as_u64() })
}

fn parse_event_data(
    signature: SpH256,
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
    pub web3_data_mutexes: HashMap<u64, Arc<Mutex<Web3Data>>>,
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
    pub async fn initialise_web3(
        &mut self,
        chain_id: u64,
    ) -> Result<Arc<Mutex<Web3Data>>, TideError> {
        let _web3_init_time = Web3Timer::new("ethereum-event-handler Web3 Initialization");
        log::info!("⛓️  avn-events-handler: web3 initialisation start");

        // No web3 connection found for network_id, try the rest of the URLs
        let web3_connection_locks = Mutex::new(());
        for eth_node_url in self.eth_node_urls.iter() {
            log::debug!("⛓️  Attempting to connect to Ethereum node: {}", eth_node_url);
            let web3 = setup_web3_connection(eth_node_url);
            if let Some(web3) = web3 {
                let web3_chain_id = get_chain_id(&web3)
                    .await
                    .map_err(|_e| server_error("Error getting chain ID from web3".to_string()))?;

                log::info!(
                    "⛓️  Successfully connected to node: {} with chain ID: {}",
                    eth_node_url,
                    web3_chain_id
                );

                {
                    // Lock the mutex to ensure only one thread can alter the web3 connection
                    let _lock = web3_connection_locks.lock();

                    if self.web3_data_mutexes.get(&web3_chain_id).is_some() {
                        log::debug!(
                            "⛓️  Web3 connection for chain ID {} already exists, skipping creation.",
                            web3_chain_id
                        );
                        continue
                    }

                    // Create a new mutex for the web3 data and store it in the map
                    let mut web3_data = Web3Data::new();
                    web3_data.web3 = Some(web3);
                    let web3_data_mutex = Arc::new(Mutex::new(web3_data));
                    self.web3_data_mutexes.insert(web3_chain_id, Arc::clone(&web3_data_mutex));

                    if web3_chain_id == chain_id {
                        log::info!(
                            "⛓️  Web3 connection for chain ID {} successfully created.",
                            web3_chain_id
                        );
                        return Ok(web3_data_mutex)
                    }
                }
            } else {
                log::error!("💔 Error creating a web3 connection for URL: {}", eth_node_url);
            }
        }

        Err(server_error("Failed to acquire a valid web3 connection for the instance.".to_string()))
    }
}

pub const SLEEP_TIME: u64 = 60;
pub const RETRY_LIMIT: usize = 3;
pub const RETRY_DELAY: u64 = 5;

async fn get_web3_connection_for_instance<Block, ClientT>(
    config: &mut EthEventHandlerConfig<Block, ClientT>,
    instance: &EthBridgeInstance,
) -> Result<Arc<Mutex<Web3Data>>, AppError>
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
    // See if we have an existing web3 data mutex for the chain_id
    match config.web3_data_mutexes.get(&chain_id) {
        Some(web3_data_pointer) => {
            log::debug!("⛓️  Found existing web3 connection for network: {}", chain_id);
            return Ok(Arc::clone(&web3_data_pointer))
        },
        None => log::debug!(
            "⛓️  No existing web3 connection found for network: {}. Initialising new...",
            chain_id
        ),
    };

    let mut attempts = 0;

    while attempts < RETRY_LIMIT {
        match config.initialise_web3(chain_id).await {
            Ok(web3_lock) => {
                log::info!("Successfully initialized web3 connection.");
                return Ok(web3_lock)
            },
            Err(e) => {
                attempts += 1;
                log::error!("Failed to initialize web3 (attempt {}): {:?}", attempts, e);
                if attempts >= RETRY_LIMIT {
                    log::error!("Reached maximum retry limit for initializing web3.");
                    return Err(AppError::Web3RetryLimitReached)
                }
                sleep(Duration::from_secs(RETRY_DELAY)).await;
            },
        }
    }

    Err(AppError::GenericError("Failed to initialize web3 after multiple attempts.".to_string()))
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
            Err(e) => log::error!("{}", e),
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

        let web3_data_lock = match get_web3_connection_for_instance(config, &instance).await {
            Ok(web3_data) => web3_data,
            Err(e) => {
                log::error!("Failed to initialize web3 connection for instance: {:?}", e);
                continue
            },
        };

        let web3_data_mutex = web3_data_lock.lock().await;
        let web3_ref = match web3_data_mutex.web3.as_ref() {
            Some(web3) => web3,
            None => return Err("Web3 connection not set up".into()),
        };

        match result {
            // A range is active, attempt processing
            Some((range, partition_id)) => {
                log::info!("Getting events for range starting at: {:?}", range.start_block);

                if web3_utils::is_eth_block_finalised(
                    &web3_ref,
                    range.end_block() as u64,
                    ETH_FINALITY,
                )
                .await?
                {
                    process_events(
                        &web3_ref,
                        &config,
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
                    &web3_ref,
                    &config,
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
    web3: &Web3<Http>,
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
        let latest_seen_ethereum_block = web3_utils::get_current_block_number(web3)
            .await
            .map_err(|err| format!("Failed to retrieve latest ethereum block: {:?}", err))?
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
    web3: &Web3<Http>,
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
    let contract_address_web3 =
        web3::types::H160::from_slice(&eth_bridge_instance.bridge_contract.to_fixed_bytes());
    let contract_addresses = vec![contract_address_web3];

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
            web3,
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
    web3: &Web3<Http>,
    config: &EthEventHandlerConfig<Block, ClientT>,
    event_signatures: Vec<SpH256>,
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
        web3,
        &contract_addresses,
        &event_signatures,
        events_registry,
        additional_transactions_to_check,
    )
    .await
    .map_err(|err| format!("Error retrieving additional events: {:?}", err))?;

    let range_events = identify_events(
        web3,
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
