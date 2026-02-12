// Copyright 2026 Aventus DAO Ltd

use secp256k1::{PublicKey, Secp256k1, SecretKey};
use sp_core::{keccak_256, H160};

#[derive(Debug)]
pub enum EthUtilsError {
    InvalidHex,
    InvalidLength,
    InvalidSecretKey,
}

pub fn eth_address_from_secret_key(sk: &SecretKey) -> H160 {
    let secp = Secp256k1::new();
    let pk = PublicKey::from_secret_key(&secp, sk);

    let uncompressed = pk.serialize_uncompressed();
    debug_assert_eq!(uncompressed[0], 0x04);

    let hash = keccak_256(&uncompressed[1..]);
    H160::from_slice(&hash[12..])
}

/// Parse a 32-byte secp256k1 private key hex (with or without "0x") and return the ETH address
pub fn eth_address_from_private_key_hex(hex_sk: &str) -> Result<H160, EthUtilsError> {
    let s = hex_sk.strip_prefix("0x").unwrap_or(hex_sk);
    let bytes = hex::decode(s).map_err(|_| EthUtilsError::InvalidHex)?;
    if bytes.len() != 32 {
        return Err(EthUtilsError::InvalidLength)
    }

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);

    let sk = SecretKey::from_byte_array(seed).map_err(|_| EthUtilsError::InvalidSecretKey)?;
    Ok(eth_address_from_secret_key(&sk))
}
