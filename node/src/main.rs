//! AVN Parachain Node CLI

#![warn(missing_docs)]

use polkadot_sdk::*;

mod chain_spec;
#[macro_use]
mod service;
mod avn_config;
mod cli;
mod command;
mod rpc;

fn main() -> sc_cli::Result<()> {
    command::run()
}

pub use avn_parachain_runtime::{apis::RuntimeApi, Block};
