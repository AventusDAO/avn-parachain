// Copyright 2026 Aventus DAO Ltd

use crate::keystore_utils::{get_eth_address_bytes_from_keystore, get_priv_key};
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use std::path::PathBuf;

pub(crate) fn eth_priv_key_from_keystore(keystore_path: &PathBuf) -> Result<[u8; 32]> {
    let eth_address: Vec<u8> = get_eth_address_bytes_from_keystore(keystore_path)?;
    if eth_address.len() != 20 {
        anyhow::bail!("eth address must be 20 bytes");
    }

    let priv_key: [u8; 32] = get_priv_key(keystore_path, &eth_address)?;
    Ok(priv_key)
}

pub fn signer_from_keystore(keystore_path: &PathBuf) -> Result<PrivateKeySigner> {
    let priv_key = eth_priv_key_from_keystore(keystore_path)?;
    Ok(PrivateKeySigner::from_bytes(&priv_key.into())?)
}
