// Copyright 2026 Aventus DAO Ltd

use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use codec::Encode;
use sp_core::{ecdsa, Pair};
use std::path::PathBuf;

pub fn sign_digest_from_keystore(keystore_path: &PathBuf, digest: &[u8]) -> Result<String> {
    use crate::keystore_utils::{get_eth_address_bytes_from_keystore, get_priv_key};

    if digest.len() != 32 {
        anyhow::bail!("digest must be 32 bytes");
    }

    let digest: &[u8; 32] =
        digest.try_into().map_err(|_| anyhow::anyhow!("digest must be 32 bytes"))?;

    let my_eth_address = get_eth_address_bytes_from_keystore(keystore_path)?;
    let my_priv_key = get_priv_key(keystore_path, &my_eth_address)?;

    if my_priv_key.len() != 32 {
        anyhow::bail!("private key must be 32 bytes");
    }

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&my_priv_key);
    let pair = ecdsa::Pair::from_seed(&seed);

    let signature: ecdsa::Signature = pair.sign_prehashed(digest);

    Ok(hex::encode(signature.encode()))
}

pub fn signer_from_keystore(keystore_path: &PathBuf) -> Result<PrivateKeySigner> {
    use crate::keystore_utils::{get_eth_address_bytes_from_keystore, get_priv_key};

    let my_eth_address = get_eth_address_bytes_from_keystore(keystore_path)?;
    let my_priv_key: [u8; 32] = get_priv_key(keystore_path, &my_eth_address)?;

    let signer = PrivateKeySigner::from_bytes(&my_priv_key.into())?;
    Ok(signer)
}
