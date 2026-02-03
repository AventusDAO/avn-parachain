use crate::{server_error, sr25519, LocalKeystore, SrPair};
use sp_avn_common::ETHEREUM_SIGNING_KEY;
use sp_core::{crypto::KeyTypeId, Pair};
use sp_keystore::Keystore;

use std::{
    fs::{self, File},
    path::PathBuf,
};
use tide::Error as TideError;

///For this function to work, the name of the keystore file must be a valid Ethereum address
pub fn get_eth_address_bytes_from_keystore(keystore_path: &PathBuf) -> Result<Vec<u8>, TideError> {
    let addresses = raw_public_keys(ETHEREUM_SIGNING_KEY, keystore_path).map_err(|_| {
        server_error(format!(
            "Error getting public key from keystore for {:?}",
            ETHEREUM_SIGNING_KEY
        ))
    })?;

    if addresses.is_empty() {
        Err(server_error(format!("No keys found in the keystore for {:?}", ETHEREUM_SIGNING_KEY)))?
    }

    if addresses.len() > 1 {
        Err(server_error(format!(
            "Multiple keys found in the keystore for {:?}. Only one should be present.",
            ETHEREUM_SIGNING_KEY
        )))?
    }

    return Ok(addresses[0].clone())
}

pub fn get_priv_key(keystore_path: &PathBuf, eth_address: &Vec<u8>) -> Result<[u8; 32], TideError> {
    let priv_key =
        key_phrase_by_type(eth_address, ETHEREUM_SIGNING_KEY, keystore_path).map_err(|_| {
            server_error(format!(
                "Error getting private key from keystore for {:?}",
                ETHEREUM_SIGNING_KEY
            ))
        })?;
    let priv_key_bytes = hex::decode(priv_key)
        .map_err(|_| server_error("Error decoding private key to bytes".to_string()))?;

    // convert a [u8] into [u8; 32]
    let mut key: [u8; 32] = Default::default();
    key.copy_from_slice(&priv_key_bytes[0..32]);
    return Ok(key)
}

/// Returns a list of raw public keys filtered by `KeyTypeId`
// See https://github.com/paritytech/substrate/blob/7db3c4fc5221d1f3fde36f1a5ef3042725a0f616/client/keystore/src/local.rs#L522
pub fn raw_public_keys(
    key_type: KeyTypeId,
    keystore_path: &PathBuf,
) -> Result<Vec<Vec<u8>>, TideError> {
    let mut public_keys: Vec<Vec<u8>> = vec![];

    for entry in fs::read_dir(keystore_path)? {
        let entry = entry
            .map_err(|e| server_error(format!("Error getting files from directory: {:?}", e)))?;
        let path = entry.path();

        // skip directories and non-unicode file names (hex is unicode)
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            match hex::decode(name) {
                Ok(ref hex) if hex.len() > 4 => {
                    if hex[0..4] != key_type.0 {
                        continue
                    }
                    let public = hex[4..].to_vec();
                    public_keys.push(public);
                },
                _ => continue,
            }
        }
    }

    Ok(public_keys)
}

/// Get the key phrase for a given public key and key type.
// See: https://github.com/paritytech/substrate/blob/7db3c4fc5221d1f3fde36f1a5ef3042725a0f616/client/keystore/src/local.rs#L469
fn key_phrase_by_type(
    eth_address: &[u8],
    key_type: KeyTypeId,
    keystore_path: &PathBuf,
) -> Result<String, TideError> {
    let mut path = keystore_path.clone();
    path.push(hex::encode(key_type.0) + hex::encode(eth_address).as_str());

    if path.exists() {
        let file = File::open(path)
            .map_err(|e| server_error(format!("Error opening EthKey file: {:?}", e)))?;
        serde_json::from_reader(&file).map_err(Into::into)
    } else {
        Err(server_error(format!(
            "Keystore file for EthKey: {:?} not found",
            ETHEREUM_SIGNING_KEY
        )))?
    }
}

pub fn authenticate_token(
    keystore: &LocalKeystore,
    message_data: &Vec<u8>,
    signature: sr25519::Signature,
) -> bool {
    return keystore.sr25519_public_keys(KeyTypeId(*b"avnk")).into_iter().any(|public| {
        log::warn!(
            "⛓️  avn-service: Authenticating msg: {:?}, sign_data: {:?}, public: {:?}",
            message_data,
            signature,
            public
        );
        SrPair::verify(&signature, message_data, &public)
    })
}
