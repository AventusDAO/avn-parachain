use anyhow::{anyhow, Result};
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use sp_core::{keccak_256, H160};

pub fn eth_address_from_private_key_hex(hex_sk: &str) -> Result<H160> {
    let s = hex_sk.strip_prefix("0x").unwrap_or(hex_sk);
    let bytes = hex::decode(s).map_err(|e| anyhow!("invalid hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(anyhow!("private key must be 32 bytes, got {}", bytes.len()))
    }

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);

    let sk = SecretKey::from_byte_array(seed).map_err(|e| anyhow!("invalid secp256k1 key: {e}"))?;
    Ok(eth_address_from_secret_key(&sk))
}

pub fn eth_address_from_secret_key(sk: &SecretKey) -> H160 {
    let secp = Secp256k1::new();
    let pk = PublicKey::from_secret_key(&secp, sk);
    let uncompressed = pk.serialize_uncompressed();
    let hash = keccak_256(&uncompressed[1..]);
    H160::from_slice(&hash[12..])
}
