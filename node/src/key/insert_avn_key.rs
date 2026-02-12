// Copyright 2026 Aventus DAO Ltd
// This file is part of Aventus and extends the original implementation
// from Substrate (Parity Technologies):
// client/cli/src/commands/insert.rs

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

//! Implementation of the `insert` subcommand

use avn_service::{secret_key_address, web3SecretKey};
use clap::{Parser, ValueEnum};
use hex::ToHex;
use sc_cli::{
    utils, with_crypto_scheme, CryptoScheme, Error, KeystoreParams, SharedParams, SubstrateCli,
};
use sc_keystore::LocalKeystore;
use sc_service::config::{BasePath, KeystoreConfig};
use sp_core::crypto::{KeyTypeId, SecretString};
use sp_keystore::{Keystore, KeystorePtr};
use std::sync::Arc;
use web3::types::H160;

/// The crypto scheme to use.
#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum AvNCryptoScheme {
    /// Use ed25519.
    Ed25519,
    /// Use sr25519.
    Sr25519,
    /// Use Ecdsa
    Ecdsa,
    /// Use a Raw Ecdsa seed
    EcdsaSeed,
}
impl AvNCryptoScheme {
    fn to_substrate_crypto_scheme(&self) -> Result<CryptoScheme, Error> {
        match self {
            Self::Ed25519 => Ok(CryptoScheme::Ed25519),
            Self::Sr25519 => Ok(CryptoScheme::Sr25519),
            Self::Ecdsa => Ok(CryptoScheme::Ecdsa),
            _ => Err(Error::KeyTypeInvalid),
        }
    }
}

/// The `insert` command
#[derive(Debug, Clone, Parser)]
#[clap(name = "insert", about = "Insert a key to the keystore of a node.")]
pub struct InsertAvNKeyCmd {
    /// The secret key URI.
    /// If the value is a file, the file content is used as URI.
    /// If not given, you will be prompted for the URI.
    /// When injecting an ethk, the suri must be the private seed of the ethereum key
    #[clap(long)]
    suri: Option<String>,

    /// Key type, examples: "gran", or "imon"
    #[clap(long)]
    key_type: String,

    #[allow(missing_docs)]
    #[clap(flatten)]
    pub shared_params: SharedParams,

    #[allow(missing_docs)]
    #[clap(flatten)]
    pub keystore_params: KeystoreParams,

    /// The cryptography scheme that should be used to generate the key out of the given URI.
    #[clap(long, value_name = "SCHEME", value_enum, ignore_case = true)]
    pub scheme: AvNCryptoScheme,
}

impl InsertAvNKeyCmd {
    /// Run the command
    pub fn run<C: SubstrateCli>(&self, cli: &C) -> Result<(), Error> {
        let suri = utils::read_uri(self.suri.as_ref())?;
        let base_path = self
            .shared_params
            .base_path()?
            .unwrap_or_else(|| BasePath::from_project("", "", &C::executable_name()));
        let chain_id = self.shared_params.chain_id(self.shared_params.is_dev());
        let chain_spec = cli.load_spec(&chain_id)?;
        let config_dir = base_path.config_dir(chain_spec.id());

        let keystore_config = self.keystore_params.keystore_config(&config_dir)?;
        let (keystore, public) = match keystore_config {
            KeystoreConfig::Path { path, password } => {
                let public: Vec<u8> = match self.scheme {
                    AvNCryptoScheme::EcdsaSeed => {
                        get_public_key_string_bytes_from_private_key(suri.as_str())?
                    },
                    scheme => with_crypto_scheme!(
                        scheme.to_substrate_crypto_scheme().expect("Already checked"),
                        to_vec(&suri, password.clone())
                    )?,
                };
                let keystore: KeystorePtr = Arc::new(LocalKeystore::open(path, password)?);
                (keystore, public)
            },
            _ => unreachable!("keystore_config always returns path and password; qed"),
        };

        let key_type =
            KeyTypeId::try_from(self.key_type.as_str()).map_err(|_| Error::KeyTypeInvalid)?;

        Keystore::insert(&*keystore, key_type, &suri, &public[..])
            .map_err(|_| Error::KeystoreOperation)?;

        Ok(())
    }
}

fn get_public_key_string_bytes_from_private_key(suri: &str) -> Result<Vec<u8>, Error> {
    let seed_encoded = hex::decode(suri).map_err(|_| Error::KeyFormatInvalid)?;
    let secret_key =
        web3SecretKey::from_slice(&seed_encoded).map_err(|_| Error::KeyFormatInvalid)?;
    let public_eth_address: H160 = secret_key_address(&secret_key);

    return get_ethereum_public_address_lowercase_string_bytes(public_eth_address);
}

fn get_ethereum_public_address_lowercase_string_bytes(
    public_eth_address: H160,
) -> Result<Vec<u8>, Error> {
    //  encode hex formats the output to lowercase.
    Ok(hex::decode(public_eth_address.encode_hex::<String>())
        .map_err(|_| Error::KeyFormatInvalid)?)
}

fn to_vec<P: sp_core::Pair>(uri: &str, pass: Option<SecretString>) -> Result<Vec<u8>, Error> {
    let p = utils::pair_from_suri::<P>(uri, pass)?;
    Ok(p.public().as_ref().to_vec())
}

#[cfg(test)]
mod tests {
    use std::{any::Any, process::id};

    use super::*;
    use futures::io::Chain;
    use polkadot_service::chain_spec::Extensions;
    use sc_service::{ChainSpec, ChainType, GenericChainSpec, NoExtension};
    use sp_core::{sr25519::Pair, ByteArray, Pair as _};
    use tempfile::TempDir;

    struct Cli;

    impl SubstrateCli for Cli {
        fn impl_name() -> String {
            "test".into()
        }

        fn impl_version() -> String {
            "2.0".into()
        }

        fn description() -> String {
            "test".into()
        }

        fn support_url() -> String {
            "test.test".into()
        }

        fn copyright_start_year() -> i32 {
            2021
        }

        fn author() -> String {
            "test".into()
        }

        fn load_spec(&self, _: &str) -> std::result::Result<Box<dyn ChainSpec>, String> {
            let builder =
                GenericChainSpec::<NoExtension, ()>::builder(Default::default(), NoExtension::None);
            Ok(Box::new(
                builder
                    .with_name("test")
                    .with_id("test_id")
                    .with_chain_type(ChainType::Development)
                    .with_genesis_config_patch(Default::default())
                    .build(),
            ))
        }
    }

    #[test]
    fn insert_with_custom_base_path() {
        let path = TempDir::new().unwrap();
        let path_str = format!("{}", path.path().display());
        let (key, uri, _) = Pair::generate_with_phrase(None);

        let inspect = InsertAvNKeyCmd::parse_from(&[
            "insert-key",
            "-d",
            &path_str,
            "--key-type",
            "test",
            "--suri",
            &uri,
            "--scheme=sr25519",
        ]);
        assert!(inspect.run(&Cli).is_ok());

        let keystore =
            LocalKeystore::open(path.path().join("chains").join("test_id").join("keystore"), None)
                .unwrap();
        assert!(keystore.has_keys(&[(key.public().to_raw_vec(), KeyTypeId(*b"test"))]));
    }
}
