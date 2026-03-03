use clap::Parser;
use std::path::PathBuf;

#[path = "key/mod.rs"]
mod key;

/// Sub-commands supported by the collator.
#[derive(Debug, clap::Subcommand)]
pub enum Subcommand {
    /// Build a chain specification.
    BuildSpec(sc_cli::BuildSpecCmd),

    /// Validate blocks.
    CheckBlock(sc_cli::CheckBlockCmd),

    /// Export blocks.
    ExportBlocks(sc_cli::ExportBlocksCmd),

    /// Export the state of a given block into a chain spec.
    ExportState(sc_cli::ExportStateCmd),

    /// Import blocks.
    ImportBlocks(sc_cli::ImportBlocksCmd),

    /// Revert the chain to a previous state.
    Revert(sc_cli::RevertCmd),

    /// Remove the whole chain.
    PurgeChain(cumulus_client_cli::PurgeChainCmd),

    /// Export the genesis head data of the parachain.
    ///
    /// Head data is the encoded block header.
    #[command(alias = "export-genesis-state")]
    ExportGenesisHead(cumulus_client_cli::ExportGenesisHeadCommand),

    /// Export the genesis wasm of the parachain.
    ExportGenesisWasm(cumulus_client_cli::ExportGenesisWasmCommand),

    /// Sub-commands concerned with benchmarking.
    /// The pallet benchmarking moved to the `pallet` sub-command.
    #[command(subcommand)]
    Benchmark(frame_benchmarking_cli::BenchmarkCmd),

    /// Key management cli utilities
    #[clap(subcommand)]
    Key(key::AvnKeySubcommand),
}

#[derive(Parser, Debug)]
pub struct AvnRunCmd {
    #[command(flatten)]
    pub base: cumulus_client_cli::RunCmd,

    /// AvN server port number
    #[arg(long = "avn-port", value_name = "AvN PORT")]
    pub avn_port: Option<String>,

    /// URL for connecting with an ethereum node
    #[arg(long = "ethereum-node-url", value_name = "ETH URL", num_args = 0..=32)]
    pub eth_node_urls: Vec<String>,

    /// Enable the transaction filter: reject extrinsics at the node (e.g. for public RPC)
    /// before they enter the pool, using the runtime's call filter policy.
    #[arg(long = "enable-transaction-filter")]
    pub enable_transaction_filter: bool,

    /// When the transaction filter is enabled, log each rejected extrinsic (disallowed or
    /// malformed). No effect if --enable-transaction-filter is not set.
    #[arg(long = "transaction-filter-log-rejections")]
    pub transaction_filter_log_rejections: bool,
}

impl std::ops::Deref for AvnRunCmd {
    type Target = cumulus_client_cli::RunCmd;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

#[derive(Debug, Parser)]
#[command(
    propagate_version = true,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub subcommand: Option<Subcommand>,

    #[command(flatten)]
    pub run: AvnRunCmd,

    /// Disable automatic hardware benchmarks.
    ///
    /// By default these benchmarks are automatically ran at startup and measure
    /// the CPU speed, the memory bandwidth and the disk speed.
    ///
    /// The results are then printed out in the logs, and also sent as part of
    /// telemetry, if telemetry is enabled.
    #[arg(long)]
    pub no_hardware_benchmarks: bool,

    /// Relay chain arguments
    #[arg(raw = true)]
    pub relay_chain_args: Vec<String>,
}

#[derive(Debug)]
pub struct RelayChainCli {
    /// The actual relay chain cli object.
    pub base: polkadot_cli::RunCmd,

    /// Optional chain id that should be passed to the relay chain.
    pub chain_id: Option<String>,

    /// The base path that should be used by the relay chain.
    pub base_path: Option<PathBuf>,
}

impl RelayChainCli {
    /// Parse the relay chain CLI parameters using the para chain `Configuration`.
    pub fn new<'a>(
        para_config: &sc_service::Configuration,
        relay_chain_args: impl Iterator<Item = &'a String>,
    ) -> Self {
        let extension = crate::chain_spec::Extensions::try_get(&*para_config.chain_spec);
        let chain_id = extension.map(|e| e.relay_chain.clone());
        let base_path = Some(para_config.base_path.path().join("polkadot"));
        Self { base_path, chain_id, base: clap::Parser::parse_from(relay_chain_args) }
    }
}
