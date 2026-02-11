// Copyright 2026 Aventus DAO Ltd
// This file is part of Aventus.

// AvN specific cli configuration
use clap::Parser;

#[derive(Debug, Parser)]
pub struct AvnCliConfiguration {
    pub avn_port: Option<String>,
    pub ethereum_node_urls: Vec<String>,
    /// Enable node-level transaction filter (reject extrinsics before they enter the pool).
    pub enable_transaction_filter: bool,
    /// When the transaction filter is enabled, log each rejected extrinsic.
    pub transaction_filter_log_rejections: bool,
}
