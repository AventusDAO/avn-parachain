// Copyright 2023 Aventus Network Services.
// This file is part of Aventus.

// AvN specific cli configuration
use clap::Parser;

#[derive(Debug, Parser)]
pub struct AvnCliConfiguration {
    pub avn_port: Option<String>,
    pub ethereum_node_urls: Vec<String>,
    pub finance_api_key: Option<String>,
    /// Enable extrinsic filtering
    pub enable_extrinsic_filter: bool,
    /// Log rejected extrinsics
    pub log_filtered_extrinsics: bool,
}
