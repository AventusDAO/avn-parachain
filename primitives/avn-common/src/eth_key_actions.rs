use sp_core::{ecdsa, H512};

pub fn decompress_eth_public_key(
    compressed_eth_public_key: ecdsa::Public,
) -> Result<H512, libsecp256k1::Error> {
    let decompressed = libsecp256k1::PublicKey::parse_slice(
        &compressed_eth_public_key.0,
        Some(libsecp256k1::PublicKeyFormat::Compressed),
    );
    match decompressed {
        Ok(public_key) => {
            let decompressed = public_key.serialize();
            let mut m = [0u8; 64];
            m.copy_from_slice(&decompressed[1..65]);
            Ok(H512::from_slice(&m))
        },
        Err(err) => Err(err),
    }
}

/// We assume the full public key doesn't have the `04` prefix
#[allow(dead_code)]
pub fn compress_eth_public_key(full_public_key: H512) -> ecdsa::Public {
    let mut compressed_public_key = [0u8; 33];

    // Take bytes 0..32 from the full plublic key ()
    compressed_public_key[1..=32].copy_from_slice(&full_public_key.0[0..32]);
    // If the last byte of the full public key is even, prefix compresssed public key with 2,
    // otherwise prefix with 3
    compressed_public_key[0] = if full_public_key.0[63] % 2 == 0 { 2u8 } else { 3u8 };

    return ecdsa::Public::from_raw(compressed_public_key);
}

#[cfg(test)]
#[path = "tests/test_eth_key_actions.rs"]
mod test_eth_key_actions;
