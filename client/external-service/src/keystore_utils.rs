use anyhow::{bail, Context, Result};
use sc_keystore::LocalKeystore;
use sp_avn_common::ETHEREUM_SIGNING_KEY;
use sp_core::{crypto::KeyTypeId, sr25519, sr25519::Pair as SrPair, Pair};
use sp_keystore::Keystore;
use std::{
    fs::{self, File},
    path::PathBuf,
};

/// For this function to work, the name of the keystore file must be a valid Ethereum address
pub fn get_eth_address_bytes_from_keystore(keystore_path: &PathBuf) -> Result<Vec<u8>> {
    let addresses = raw_public_keys(ETHEREUM_SIGNING_KEY, keystore_path)
        .context("Error getting public key(s) from keystore")?;

    if addresses.is_empty() {
        bail!("No keys found in the keystore for {:?}", ETHEREUM_SIGNING_KEY);
    }

    if addresses.len() > 1 {
        bail!(
            "Multiple keys found in the keystore for {:?}. Only one should be present.",
            ETHEREUM_SIGNING_KEY
        );
    }

    Ok(addresses[0].clone())
}

pub fn get_priv_key(keystore_path: &PathBuf, eth_address: &Vec<u8>) -> Result<[u8; 32]> {
    let priv_key_hex = key_phrase_by_type(eth_address, ETHEREUM_SIGNING_KEY, keystore_path)
        .with_context(|| {
            format!("Error reading private key from keystore for {:?}", ETHEREUM_SIGNING_KEY)
        })?;

    let priv_key_bytes =
        hex::decode(priv_key_hex.trim()).context("Error decoding private key hex")?;

    if priv_key_bytes.len() < 32 {
        bail!("Private key in keystore is too short: {} bytes", priv_key_bytes.len());
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&priv_key_bytes[..32]);
    Ok(key)
}

/// Returns a list of raw public keys filtered by `KeyTypeId`
///
/// See https://github.com/paritytech/substrate/blob/7db3c4fc5221d1f3fde36f1a5ef3042725a0f616/client/keystore/src/local.rs#L522
pub fn raw_public_keys(key_type: KeyTypeId, keystore_path: &PathBuf) -> Result<Vec<Vec<u8>>> {
    let mut public_keys: Vec<Vec<u8>> = vec![];

    for entry in fs::read_dir(keystore_path)
        .with_context(|| format!("Failed reading keystore directory: {:?}", keystore_path))?
    {
        let entry = entry.context("Error iterating keystore directory entries")?;
        let path = entry.path();

        // Skip directories and non-unicode file names (hex is unicode)
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            match hex::decode(name) {
                Ok(decoded) if decoded.len() > 4 => {
                    if decoded[0..4] != key_type.0 {
                        continue
                    }
                    public_keys.push(decoded[4..].to_vec());
                },
                _ => continue,
            }
        }
    }

    Ok(public_keys)
}

/// Get the key phrase for a given public key and key type.
///
/// See: https://github.com/paritytech/substrate/blob/7db3c4fc5221d1f3fde36f1a5ef3042725a0f616/client/keystore/src/local.rs#L469
fn key_phrase_by_type(
    eth_address: &[u8],
    key_type: KeyTypeId,
    keystore_path: &PathBuf,
) -> Result<String> {
    let mut path = keystore_path.clone();
    path.push(hex::encode(key_type.0) + hex::encode(eth_address).as_str());

    if !path.exists() {
        bail!("Keystore file for EthKey {:?} not found: {:?}", ETHEREUM_SIGNING_KEY, path);
    }

    let file =
        File::open(&path).with_context(|| format!("Error opening keystore file: {:?}", path))?;
    let phrase: String = serde_json::from_reader(&file)
        .with_context(|| format!("Error decoding JSON in keystore file: {:?}", path))?;
    Ok(phrase)
}

pub fn authenticate_token(
    keystore: &LocalKeystore,
    message_data: &[u8],
    signature: sr25519::Signature,
) -> bool {
    keystore.sr25519_public_keys(KeyTypeId(*b"avnk")).into_iter().any(|public| {
        log::warn!(
            "⛓️  external-service: Authenticating msg: {:?}, sign_data: {:?}, public: {:?}",
            message_data,
            signature,
            public
        );
        SrPair::verify(&signature, message_data, &public)
    })
}
