// Copyright 2026 Aventus DAO Ltd
// This file is part of Aventus and extends the original implementation
// from Substrate (Parity Technologies):
// client/cli/src/commands/key.rs

// Copyright (C) 2020-2022 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Key related CLI utilities

use sc_cli::{
    Error, GenerateCmd, GenerateKeyCmdCommon, InspectKeyCmd, InspectNodeKeyCmd, SubstrateCli,
};

use super::insert_avn_key::InsertAvNKeyCmd;

/// Key utilities for the cli.
#[derive(Debug, clap::Subcommand)]
pub enum AvnKeySubcommand {
    /// Generate a random node key, write it to a file or stdout and write the
    /// corresponding peer-id to stderr
    GenerateNodeKey(GenerateKeyCmdCommon),

    /// Generate a random account
    Generate(GenerateCmd),

    /// Gets a public key and a SS58 address from the provided Secret URI
    Inspect(InspectKeyCmd),

    /// Load a node key from a file or stdin and print the corresponding peer-id
    InspectNodeKey(InspectNodeKeyCmd),

    /// Insert a key to the keystore of a node.
    Insert(InsertAvNKeyCmd),
}

impl AvnKeySubcommand {
    /// run the key subcommands
    pub fn run<C: SubstrateCli>(&self, cli: &C) -> Result<(), Error> {
        match self {
            AvnKeySubcommand::GenerateNodeKey(cmd) => cmd.run(),
            AvnKeySubcommand::Generate(cmd) => cmd.run(),
            AvnKeySubcommand::Inspect(cmd) => cmd.run(),
            AvnKeySubcommand::Insert(cmd) => cmd.run(cli),
            AvnKeySubcommand::InspectNodeKey(cmd) => cmd.run(),
        }
    }
}
