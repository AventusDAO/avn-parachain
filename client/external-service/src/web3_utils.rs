use anyhow::{ensure, Context};
use ethereum_types;
use sp_avn_common::EthTransaction;
pub use std::{
    error::Error as StdError,
    path::PathBuf,
    sync::{Arc, MutexGuard},
};
use web3::{
    signing::keccak256,
    transports::Http,
    types::{
        Address, Bytes, CallRequest, Transaction, TransactionParameters, TransactionReceipt, H160,
        U256,
    },
    Web3,
};
use web3Secp256k1::{All, PublicKey, Secp256k1, SecretKey};

pub struct Web3Data {
    pub web3: Option<Web3<Http>>,
    nonce: Option<u64>,
}

impl Web3Data {
    pub fn new() -> Self {
        Web3Data { web3: None, nonce: None }
    }

    /// Updates the web3 nonce value if needed. If the force_update flag is set to true then it
    /// always does.
    pub async fn get_nonce(
        &mut self,
        sender_eth_address: &Vec<u8>,
        force_update: bool,
    ) -> anyhow::Result<u64> {
        ensure!(self.web3.is_some(), "No web3 instance available.");
        if force_update || self.nonce == None {
            self.nonce = Some(
                get_nonce_from_ethereum(
                    &self.web3.as_ref().expect("already checked."),
                    sender_eth_address,
                )
                .await
                .with_context(|| format!("Error while getting nonce from Ethereum"))?
                .low_u64(),
            );
        }
        ensure!(self.nonce.is_some(), "Invalid nonce (None)");

        let nonce = self.nonce.expect("already checked");
        log::info!("⛓️  avn-service: web3 nonce value: {}", nonce);
        Ok(nonce)
    }

    pub fn increment_nonce(&mut self) -> anyhow::Result<()> {
        ensure!(self.nonce.is_some(), "Invalid nonce (None)");
        self.nonce = Some(self.nonce.expect("already checked") + 1);
        Ok(())
    }

    pub fn get_web3_instance(&self) -> anyhow::Result<&Web3<Http>> {
        ensure!(self.web3.is_some(), "No web3 instance available.");
        Ok(self.web3.as_ref().expect("already checked"))
    }
}

pub fn setup_web3_connection(url: &String) -> Option<Web3<Http>> {
    let transport_init_result = web3::transports::Http::new(url);

    if transport_init_result.is_err() {
        return None
    }
    let transport = transport_init_result.expect("Already checked");
    return Some(web3::Web3::new(transport))
}

pub async fn get_nonce_from_ethereum(
    web3: &Web3<Http>,
    sender_eth_address: &Vec<u8>,
) -> anyhow::Result<U256> {
    ensure!(
        sender_eth_address.len() == 20,
        format!("sender address ({:?}) is not a valid Ethereum address", sender_eth_address)
    );

    return Ok(web3.eth().transaction_count(H160::from_slice(sender_eth_address), None).await?)
}

/// Note: this is called by the signer which has different ethereum types to web3
pub async fn build_raw_transaction(
    web3_data: &mut Web3Data,
    send_request: &EthTransaction,
    sender_eth_address: &Vec<u8>,
) -> anyhow::Result<TransactionParameters> {
    let recipient = send_request.to.as_bytes();

    let nonce = web3_data.get_nonce(sender_eth_address, false).await?;
    let web3 = web3_data.get_web3_instance()?;
    let gas_estimate =
        estimate_gas(web3, sender_eth_address, recipient, &send_request.data).await?;

    Ok(TransactionParameters {
        nonce: Some(nonce.into()),
        to: Some(H160::from_slice(recipient)),
        value: U256::zero(),
        gas: gas_estimate,
        gas_price: None,
        data: web3::types::Bytes(send_request.data.clone()),
        chain_id: Some(get_chain_id(web3).await?),
        ..Default::default()
    })
}

pub async fn build_call_request(view_request: &EthTransaction) -> anyhow::Result<CallRequest> {
    Ok(CallRequest {
        to: Some(H160::from_slice(view_request.to.as_bytes())),
        data: Some(Bytes(view_request.data.clone())),
        ..Default::default()
    })
}

pub async fn get_chain_id(web3: &Web3<Http>) -> anyhow::Result<u64> {
    Ok(web3
        .eth()
        .chain_id()
        .await
        .with_context(|| "Error getting chain Id".to_string())?
        .as_u64())
}

async fn estimate_gas(
    web3: &Web3<Http>,
    sender: &Vec<u8>,
    recipient: &[u8],
    data: &Vec<u8>,
) -> anyhow::Result<U256> {
    let call_request = CallRequest {
        from: Some(H160::from_slice(&sender)),
        to: Some(H160::from_slice(recipient)),
        gas: None,
        gas_price: None,
        value: Some(U256::zero()),
        data: Some(Bytes(data.to_vec())),
        access_list: None,
        max_fee_per_gas: None,
        max_priority_fee_per_gas: None,
        transaction_type: None,
    };

    Ok(web3.eth().estimate_gas(call_request.clone(), None).await.with_context(|| {
        format!(
            "Error estimating gas for data: {}",
            serde_json::to_string_pretty(&call_request).unwrap_or(format!("{:?}", call_request))
        )
    })?)
}

pub async fn get_current_block_number(web3: &Web3<Http>) -> anyhow::Result<u64> {
    Ok(web3.eth().block_number().await?.as_u64())
}

pub async fn get_tx_receipt(
    web3: &Web3<Http>,
    tx_hash: ethereum_types::H256,
) -> anyhow::Result<Option<TransactionReceipt>> {
    Ok(web3.eth().transaction_receipt(web3::types::H256(tx_hash.0)).await?)
}

pub async fn get_tx_call_data(
    web3: &Web3<Http>,
    tx_hash: ethereum_types::H256,
) -> anyhow::Result<Option<Transaction>> {
    Ok(web3
        .eth()
        .transaction(web3::types::TransactionId::Hash(web3::types::H256(tx_hash.0)))
        .await?)
}

pub async fn send_raw_transaction(
    web3: &Web3<Http>,
    tx: Bytes,
) -> anyhow::Result<web3::types::H256> {
    Ok(web3
        .eth()
        .send_raw_transaction(tx)
        .await
        .with_context(|| format!("Error while sending raw transaction to Ethereum"))?)
}

pub async fn is_eth_block_finalised(
    web3: &Web3<Http>,
    current_block_num: u64,
    num_blocks_to_wait: u64,
) -> anyhow::Result<bool, String> {
    let latest_block = get_current_block_number(web3)
        .await
        .map_err(|err| format!("Failed to get latest block number: {:?}", err))?;

    Ok(latest_block >= current_block_num + num_blocks_to_wait)
}

// Based and refactored from: https://github.com/tomusdrw/rust-web3/blob/v0.18.0/src/signing.rs#L151-L172

/// Gets the address of a public key.
///
/// The public address is defined as the low 20 bytes of the keccak hash of
/// the public key. Note that the public key returned from the `secp256k1`
/// crate is 65 bytes long, that is because it is prefixed by `0x04` to
/// indicate an uncompressed public key; this first byte is ignored when
/// computing the hash.
pub fn public_key_address(public_key: &PublicKey) -> Address {
    let uncompressed_key_flag = 0x04;
    let ethereum_address_start_index = 12;
    let public_key = public_key.serialize_uncompressed();

    debug_assert_eq!(public_key[0], uncompressed_key_flag);
    let hash = keccak256(&public_key[1..]);

    Address::from_slice(&hash[ethereum_address_start_index..])
}

/// Gets the public address of a private key.
pub fn secret_key_address(key: &SecretKey) -> Address {
    let secp: Secp256k1<All> = Secp256k1::new();
    let public_key = PublicKey::from_secret_key(&secp, key);
    public_key_address(&public_key)
}
