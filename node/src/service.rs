//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

// std
use codec::Encode;
use cumulus_client_cli::CollatorOptions;
use futures::lock::Mutex;
use runtime_common::opaque::{Block, Hash};
use sc_client_api::Backend;
use sc_network_sync::SyncingService;
use sp_api::ConstructRuntimeApi;
use sp_core::offchain::OffchainStorage;
use std::{sync::Arc, time::Duration};

// Cumulus Imports
use cumulus_client_collator::service::CollatorService;
use cumulus_client_consensus_common::ParachainBlockImport as TParachainBlockImport;
use cumulus_client_consensus_proposer::Proposer;
use cumulus_client_service::{
    build_relay_chain_interface, prepare_node_config, start_relay_chain_tasks,
    CollatorSybilResistance, DARecoveryProfile, StartRelayChainTasksParams,
};
use cumulus_primitives_core::{relay_chain::CollatorPair, ParaId};
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface};

// Substrate Imports
use sc_consensus::ImportQueue;
use sc_executor::{
    HeapAllocStrategy, NativeElseWasmExecutor, WasmExecutor, DEFAULT_HEAP_ALLOC_STRATEGY,
};
use sc_network::NetworkBlock;
use sc_service::{
    config::KeystoreConfig, Configuration, PartialComponents, TFullBackend, TFullClient,
    TaskManager,
};
use sc_telemetry::{Telemetry, TelemetryHandle, TelemetryWorker, TelemetryWorkerHandle};

use sp_avn_common::{
    transaction_filter::{ExtrinsicFilter, FilteredPool},
    DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER, EXTERNAL_SERVICE_PORT_NUMBER_KEY,
};
use sp_keystore::KeystorePtr;
use substrate_prometheus_endpoint::Registry;

use crate::{avn_config::*, common::AvnRuntimeApiCollection};
use avn_service::{self, web3_utils::Web3Data};
use sc_transaction_pool_api::OffchainTransactionPoolFactory;

use crate::common::AvnParachainExecutor;
type ParachainExecutor = NativeElseWasmExecutor<AvnParachainExecutor>;

type ParachainClient<RuntimeApi> = TFullClient<Block, RuntimeApi, ParachainExecutor>;

type ParachainBackend = TFullBackend<Block>;

type ParachainBlockImport<RuntimeApi> =
    TParachainBlockImport<Block, Arc<ParachainClient<RuntimeApi>>, ParachainBackend>;

/// Assembly of PartialComponents (enough to run chain ops subcommands)
pub type Service<RuntimeApi> = PartialComponents<
    ParachainClient<RuntimeApi>,
    ParachainBackend,
    (),
    sc_consensus::DefaultImportQueue<Block>,
    sc_transaction_pool::FullPool<Block, ParachainClient<RuntimeApi>>,
    (ParachainBlockImport<RuntimeApi>, Option<Telemetry>, Option<TelemetryWorkerHandle>),
>;

/// Creates the extrinsic filter using the runtime's filtering policy.
///
/// This filter uses `avn_parachain_runtime::is_extrinsic_allowed` to determine
/// which extrinsics are permitted through the transaction pool.
fn create_extrinsic_filter(enabled: bool, log_rejections: bool) -> Arc<dyn ExtrinsicFilter> {
    struct RuntimeExtrinsicFilter {
        enabled: bool,
        log_rejections: bool,
    }

    impl ExtrinsicFilter for RuntimeExtrinsicFilter {
        fn is_banned(&self, xt: &sp_core::Bytes) -> bool {
            if !self.enabled {
                return false
            }

            let allowed = avn_parachain_runtime::is_extrinsic_allowed(xt);
            if !allowed && self.log_rejections {
                log::warn!(target: "tx-filter", "Rejected disallowed transaction");
            }
            !allowed
        }
    }

    Arc::new(RuntimeExtrinsicFilter { enabled, log_rejections })
}

/// Starts a `ServiceBuilder` for a full service.
///
/// Use this macro if you don't actually need the full service, but just the builder in order to
/// be able to perform chain operations.
pub fn new_partial<RuntimeApi>(
    config: &Configuration,
) -> Result<Service<RuntimeApi>, sc_service::Error>
where
    RuntimeApi: ConstructRuntimeApi<Block, ParachainClient<RuntimeApi>> + Send + Sync + 'static,
    RuntimeApi::RuntimeApi: AvnRuntimeApiCollection,
{
    let telemetry = config
        .telemetry_endpoints
        .clone()
        .filter(|x| !x.is_empty())
        .map(|endpoints| -> Result<_, sc_telemetry::Error> {
            let worker = TelemetryWorker::new(16)?;
            let telemetry = worker.handle().new_telemetry(endpoints);
            Ok((worker, telemetry))
        })
        .transpose()?;

    let heap_pages = config
        .default_heap_pages
        .map_or(DEFAULT_HEAP_ALLOC_STRATEGY, |h| HeapAllocStrategy::Static { extra_pages: h as _ });

    let wasm = WasmExecutor::builder()
        .with_execution_method(config.wasm_method)
        .with_onchain_heap_alloc_strategy(heap_pages)
        .with_offchain_heap_alloc_strategy(heap_pages)
        .with_max_runtime_instances(config.max_runtime_instances)
        .with_runtime_cache_size(config.runtime_cache_size)
        .build();

    let executor = ParachainExecutor::new_with_wasm_executor(wasm);

    let (client, backend, keystore_container, task_manager) =
        sc_service::new_full_parts::<Block, RuntimeApi, _>(
            config,
            telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
            executor,
        )?;
    let client = Arc::new(client);

    let telemetry_worker_handle = telemetry.as_ref().map(|(worker, _)| worker.handle());

    let telemetry = telemetry.map(|(worker, telemetry)| {
        task_manager.spawn_handle().spawn("telemetry", None, worker.run());
        telemetry
    });

    let transaction_pool = sc_transaction_pool::BasicPool::new_full(
        config.transaction_pool.clone(),
        config.role.is_authority().into(),
        config.prometheus_registry(),
        task_manager.spawn_essential_handle(),
        client.clone(),
    );

    let block_import = ParachainBlockImport::new(client.clone(), backend.clone());

    let import_queue = build_import_queue(
        client.clone(),
        block_import.clone(),
        config,
        telemetry.as_ref().map(|telemetry| telemetry.handle()),
        &task_manager,
    )?;

    Ok(PartialComponents {
        backend,
        client,
        import_queue,
        keystore_container,
        task_manager,
        transaction_pool,
        select_chain: (),
        other: (block_import, telemetry, telemetry_worker_handle),
    })
}

/// Start a node with the given parachain `Configuration` and relay chain `Configuration`.
///
/// This is the actual implementation that is abstract over the executor and the runtime api.
#[sc_tracing::logging::prefix_logs_with("Parachain")]
async fn start_node_impl<RuntimeApi>(
    parachain_config: Configuration,
    polkadot_config: Configuration,
    avn_cli_config: AvnCliConfiguration,
    collator_options: CollatorOptions,
    para_id: ParaId,
    hwbench: Option<sc_sysinfo::HwBench>,
) -> sc_service::error::Result<(TaskManager, Arc<ParachainClient<RuntimeApi>>)>
where
    RuntimeApi: ConstructRuntimeApi<Block, ParachainClient<RuntimeApi>> + Send + Sync + 'static,
    RuntimeApi::RuntimeApi: AvnRuntimeApiCollection,
{
    let parachain_config = prepare_node_config(parachain_config);

    let params = new_partial::<RuntimeApi>(&parachain_config)?;
    let (block_import, mut telemetry, telemetry_worker_handle) = params.other;
    let net_config = sc_network::config::FullNetworkConfiguration::new(&parachain_config.network);

    let client = params.client.clone();
    let backend = params.backend.clone();
    let mut task_manager = params.task_manager;

    let (relay_chain_interface, collator_key) = build_relay_chain_interface(
        polkadot_config,
        &parachain_config,
        telemetry_worker_handle,
        &mut task_manager,
        collator_options.clone(),
        hwbench.clone(),
    )
    .await
    .map_err(|e| sc_service::Error::Application(Box::new(e) as Box<_>))?;

    let validator = parachain_config.role.is_authority();
    let prometheus_registry = parachain_config.prometheus_registry().cloned();

    let filter = create_extrinsic_filter(
        avn_cli_config.enable_extrinsic_filter,
        avn_cli_config.log_filtered_extrinsics,
    );
    let inner_pool = params.transaction_pool.clone();
    let transaction_pool = Arc::new(FilteredPool::new(params.transaction_pool, filter));
    let import_queue_service = params.import_queue.service();

    let avn_port = avn_cli_config.avn_port.clone();
    let finance_api_key: String = avn_cli_config.finance_api_key.clone().unwrap_or_default();

    let (network, system_rpc_tx, tx_handler_controller, start_network, sync_service) =
        cumulus_client_service::build_network(cumulus_client_service::BuildNetworkParams {
            parachain_config: &parachain_config,
            net_config,
            client: client.clone(),
            transaction_pool: inner_pool, // Cumulus API requires concrete BasicPool type
            para_id,
            spawn_handle: task_manager.spawn_handle(),
            relay_chain_interface: relay_chain_interface.clone(),
            import_queue: params.import_queue,
            sybil_resistance_level: CollatorSybilResistance::Resistant, // because of Aura
        })
        .await?;

    if parachain_config.offchain_worker.enabled {
        use futures::FutureExt;

        let port_number = avn_port
            .clone()
            .unwrap_or_else(|| DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER.to_string());

        if let Some(mut local_db) = backend.offchain_storage() {
            local_db.set(
                sp_core::offchain::STORAGE_PREFIX,
                EXTERNAL_SERVICE_PORT_NUMBER_KEY,
                &port_number.encode(),
            );
        }

        task_manager.spawn_handle().spawn(
            "offchain-workers-runner",
            "offchain-work",
            sc_offchain::OffchainWorkers::new(sc_offchain::OffchainWorkerOptions {
                runtime_api_provider: client.clone(),
                keystore: Some(params.keystore_container.keystore()),
                offchain_db: backend.offchain_storage(),
                transaction_pool: Some(OffchainTransactionPoolFactory::new(
                    transaction_pool.clone(),
                )),
                network_provider: network.clone(),
                is_validator: parachain_config.role.is_authority(),
                enable_http_requests: true,
                custom_extensions: move |_| vec![],
            })
            .run(client.clone(), task_manager.spawn_handle())
            .boxed(),
        );
    }

    let rpc_builder = {
        let client = client.clone();
        let pool = transaction_pool.clone();

        Box::new(move |deny_unsafe, _| {
            let deps =
                crate::rpc::FullDeps { client: client.clone(), pool: pool.clone(), deny_unsafe };

            crate::rpc::create_full(deps).map_err(Into::into)
        })
    };

    // Assigning here before `parachain_config` is borrowed
    let parachain_config_keystore = parachain_config.keystore.clone();

    sc_service::spawn_tasks(sc_service::SpawnTasksParams {
        rpc_builder,
        client: client.clone(),
        transaction_pool: transaction_pool.clone(),
        task_manager: &mut task_manager,
        config: parachain_config,
        keystore: params.keystore_container.keystore(),
        backend,
        network: network.clone(),
        sync_service: sync_service.clone(),
        system_rpc_tx,
        tx_handler_controller,
        telemetry: telemetry.as_mut(),
    })?;

    if let Some(hwbench) = hwbench {
        sc_sysinfo::print_hwbench(&hwbench);

        // TODO: check hw bench.

        if let Some(ref mut telemetry) = telemetry {
            let telemetry_handle = telemetry.handle();
            task_manager.spawn_handle().spawn(
                "telemetry_hwbench",
                None,
                sc_sysinfo::initialize_hwbench_telemetry(telemetry_handle, hwbench),
            );
        }
    }

    let announce_block = {
        let sync_service = sync_service.clone();
        Arc::new(move |hash, data| sync_service.announce_block(hash, data))
    };

    let relay_chain_slot_duration = Duration::from_secs(6);

    let overseer_handle = relay_chain_interface
        .overseer_handle()
        .map_err(|e| sc_service::Error::Application(Box::new(e)))?;

    start_relay_chain_tasks(StartRelayChainTasksParams {
        client: client.clone(),
        announce_block: announce_block.clone(),
        para_id,
        relay_chain_interface: relay_chain_interface.clone(),
        task_manager: &mut task_manager,
        da_recovery_profile: if validator {
            DARecoveryProfile::Collator
        } else {
            DARecoveryProfile::FullNode
        },
        import_queue: import_queue_service,
        relay_chain_slot_duration,
        recovery_handle: Box::new(overseer_handle.clone()),
        sync_service: sync_service.clone(),
    })?;

    if validator {
        let keystore_path = match parachain_config_keystore {
            KeystoreConfig::Path { path, password: _ } => Ok(path.clone()),
            _ => Err("Keystore must be local"),
        }?;

        let eth_web3_url = avn_cli_config
            .ethereum_node_urls
            .first()
            .cloned()
            .unwrap_or_else(|| "".to_string());
        let avn_config = avn_service::Config::<Block, _> {
            keystore: params.keystore_container.local_keystore(),
            keystore_path: keystore_path.clone(),
            avn_port: avn_port.clone(),
            finance_api_key: finance_api_key.clone(),
            eth_node_url: eth_web3_url,
            web3_data_mutex: Arc::new(Mutex::new(Web3Data::new())),
            client: client.clone(),
            _block: Default::default(),
        };

        let eth_event_handler_config =
            avn_service::ethereum_events_handler::EthEventHandlerConfig::<Block, _> {
                keystore: params.keystore_container.local_keystore(),
                keystore_path: keystore_path.clone(),
                avn_port: avn_port.clone(),
                eth_node_urls: avn_cli_config.ethereum_node_urls.clone(),
                web3_data_mutexes: Default::default(),
                client: client.clone(),
                offchain_transaction_pool_factory: OffchainTransactionPoolFactory::new(
                    transaction_pool.clone(),
                ),
            };

        task_manager.spawn_essential_handle().spawn(
            "avn-service",
            None,
            avn_service::start(avn_config),
        );
        task_manager.spawn_essential_handle().spawn(
            "eth-events-handler",
            None,
            avn_service::ethereum_events_handler::start_eth_event_handler(eth_event_handler_config),
        );

        start_consensus(
            client.clone(),
            block_import,
            prometheus_registry.as_ref(),
            telemetry.as_ref().map(|t| t.handle()),
            &task_manager,
            relay_chain_interface.clone(),
            transaction_pool.clone(),
            sync_service.clone(),
            params.keystore_container.keystore(),
            relay_chain_slot_duration,
            para_id,
            collator_key.expect("Command line arguments do not allow this. qed"),
            overseer_handle,
            announce_block,
        )?;
    }

    start_network.start_network();

    Ok((task_manager, client))
}

/// Build the import queue for the parachain runtime.
fn build_import_queue<RuntimeApi>(
    client: Arc<ParachainClient<RuntimeApi>>,
    block_import: ParachainBlockImport<RuntimeApi>,
    config: &Configuration,
    telemetry: Option<TelemetryHandle>,
    task_manager: &TaskManager,
) -> Result<sc_consensus::DefaultImportQueue<Block>, sc_service::Error>
where
    RuntimeApi: ConstructRuntimeApi<Block, ParachainClient<RuntimeApi>> + Send + Sync + 'static,
    RuntimeApi::RuntimeApi: AvnRuntimeApiCollection,
{
    let slot_duration = cumulus_client_consensus_aura::slot_duration(&*client)?;

    Ok(cumulus_client_consensus_aura::equivocation_import_queue::fully_verifying_import_queue::<
        sp_consensus_aura::sr25519::AuthorityPair,
        _,
        _,
        _,
        _,
    >(
        client,
        block_import,
        move |_, _| async move {
            let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
            Ok(timestamp)
        },
        slot_duration,
        &task_manager.spawn_essential_handle(),
        config.prometheus_registry(),
        telemetry,
    ))
}

fn start_consensus<RuntimeApi, Pool>(
    client: Arc<ParachainClient<RuntimeApi>>,
    block_import: ParachainBlockImport<RuntimeApi>,
    prometheus_registry: Option<&Registry>,
    telemetry: Option<TelemetryHandle>,
    task_manager: &TaskManager,
    relay_chain_interface: Arc<dyn RelayChainInterface>,
    transaction_pool: Arc<Pool>,
    sync_oracle: Arc<SyncingService<Block>>,
    keystore: KeystorePtr,
    relay_chain_slot_duration: Duration,
    para_id: ParaId,
    collator_key: CollatorPair,
    overseer_handle: OverseerHandle,
    announce_block: Arc<dyn Fn(Hash, Option<Vec<u8>>) + Send + Sync>,
) -> Result<(), sc_service::Error>
where
    RuntimeApi: ConstructRuntimeApi<Block, ParachainClient<RuntimeApi>> + Send + Sync + 'static,
    RuntimeApi::RuntimeApi: AvnRuntimeApiCollection,
    Pool: sc_transaction_pool_api::TransactionPool<Block = Block> + 'static,
{
    use cumulus_client_consensus_aura::collators::basic::{
        self as basic_aura, Params as BasicAuraParams,
    };

    // NOTE: because we use Aura here explicitly, we can use `CollatorSybilResistance::Resistant`
    // when starting the network.

    let slot_duration = cumulus_client_consensus_aura::slot_duration(&*client)?;

    let proposer_factory = sc_basic_authorship::ProposerFactory::with_proof_recording(
        task_manager.spawn_handle(),
        client.clone(),
        transaction_pool,
        prometheus_registry,
        telemetry.clone(),
    );

    let proposer = Proposer::new(proposer_factory);

    let collator_service = CollatorService::new(
        client.clone(),
        Arc::new(task_manager.spawn_handle()),
        announce_block,
        client.clone(),
    );

    let params = BasicAuraParams {
        create_inherent_data_providers: move |_, ()| async move { Ok(()) },
        block_import,
        para_client: client,
        relay_client: relay_chain_interface,
        sync_oracle,
        keystore,
        collator_key,
        para_id,
        overseer_handle,
        slot_duration,
        relay_chain_slot_duration,
        proposer,
        collator_service,
        // Very limited proposal time.
        authoring_duration: Duration::from_millis(500),
        collation_request_receiver: None,
    };

    let fut =
        basic_aura::run::<Block, sp_consensus_aura::sr25519::AuthorityPair, _, _, _, _, _, _, _>(
            params,
        );
    task_manager.spawn_essential_handle().spawn("aura", None, fut);

    Ok(())
}

/// Start a parachain node.
pub async fn start_parachain_node<RuntimeApi>(
    parachain_config: Configuration,
    polkadot_config: Configuration,
    avn_cli_config: AvnCliConfiguration,
    collator_options: CollatorOptions,
    para_id: ParaId,
    hwbench: Option<sc_sysinfo::HwBench>,
) -> sc_service::error::Result<(TaskManager, Arc<ParachainClient<RuntimeApi>>)>
where
    RuntimeApi: ConstructRuntimeApi<Block, ParachainClient<RuntimeApi>> + Send + Sync + 'static,
    RuntimeApi::RuntimeApi: AvnRuntimeApiCollection,
{
    start_node_impl(
        parachain_config,
        polkadot_config,
        avn_cli_config,
        collator_options,
        para_id,
        hwbench,
    )
    .await
}
