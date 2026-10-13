// Copyright 2026 Aventus DAO Ltd
// This file is part of Aventus.

// AvN specific cli configuration
use clap::Parser;

#[derive(Debug, Parser)]
pub struct AvnCliConfiguration {
    pub avn_port: Option<String>,
    pub ethereum_node_urls: Vec<String>,
    /// Ethereum endpoints for the floor channel; empty means the built-in defaults.
    pub floor_rpc_urls: Vec<String>,
    /// Venue chain endpoints for the venue channel; empty means the built-in defaults.
    pub venue_rpc_urls: Vec<String>,
    pub registered_node_id: Option<String>,
    /// Enable node-level transaction filter (reject extrinsics before they enter the pool).
    pub enable_transaction_filter: bool,
    /// When the transaction filter is enabled, log each rejected extrinsic.
    pub transaction_filter_log_rejections: bool,
}
