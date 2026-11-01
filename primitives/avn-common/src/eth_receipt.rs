//! Reads the Ethereum transaction receipts returned by the external service's `/eth/query`.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use crate::EthQueryResponse;
use codec::Decode;
use serde_json::{Map, Value};
use sp_core::{H160, H256};
use sp_std::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptError {
    /// The response is not a hex encoded `EthQueryResponse` holding the receipt bytes.
    InvalidResponse,
    /// The receipt is not a JSON object.
    InvalidJson,
    /// A field is missing or does not hold what an Ethereum receipt would.
    InvalidField(&'static str),
}

/// A log emitted by a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EthLog {
    pub address: H160,
    pub topics: Vec<Vec<u8>>,
    pub data: Vec<u8>,
}

impl EthLog {
    pub fn has_signature(&self, signature: &H256) -> bool {
        self.topics
            .first()
            .is_some_and(|topic| topic.as_slice() == signature.as_bytes())
    }

    /// The data in the form event parsers take it: `None` when the log has no data.
    pub fn data(&self) -> Option<Vec<u8>> {
        (!self.data.is_empty()).then(|| self.data.clone())
    }
}

/// The parts of a transaction receipt T2 checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EthReceipt {
    /// Whether the transaction executed successfully. Receipts from before Byzantium have no
    /// status, so they never count as successful.
    pub succeeded: bool,
    pub logs: Vec<EthLog>,
}

impl EthReceipt {
    /// The first log with this event signature.
    pub fn find_log(&self, signature: &H256) -> Option<&EthLog> {
        self.logs.iter().find(|log| log.has_signature(signature))
    }

    /// The logs with this event signature emitted by `address`.
    pub fn logs_from<'a>(
        &'a self,
        address: &'a H160,
        signature: &'a H256,
    ) -> impl Iterator<Item = &'a EthLog> {
        self.logs
            .iter()
            .filter(move |log| &log.address == address && log.has_signature(signature))
    }
}

/// Decodes the response to a `TransactionReceipt` query, returning the receipt and the number of
/// confirmations of its block. Returns `None` if Ethereum has no receipt for the transaction.
pub fn decode_receipt_response(body: &[u8]) -> Result<Option<(EthReceipt, u64)>, ReceiptError> {
    if body.is_empty() {
        return Ok(None)
    }

    let bytes = hex::decode(body).map_err(|_| ReceiptError::InvalidResponse)?;
    let response =
        EthQueryResponse::decode(&mut &bytes[..]).map_err(|_| ReceiptError::InvalidResponse)?;
    let receipt_json =
        Vec::<u8>::decode(&mut &response.data[..]).map_err(|_| ReceiptError::InvalidResponse)?;

    Ok(Some((parse_receipt(&receipt_json)?, response.num_confirmations)))
}

/// Parses a receipt in the JSON format of Ethereum's `eth_getTransactionReceipt`.
pub fn parse_receipt(json: &[u8]) -> Result<EthReceipt, ReceiptError> {
    let receipt: Map<String, Value> =
        serde_json::from_slice(json).map_err(|_| ReceiptError::InvalidJson)?;

    let succeeded = match receipt.get("status") {
        None | Some(Value::Null) => false,
        Some(status) => parse_quantity(status, "status")? == 1,
    };

    let logs = receipt
        .get("logs")
        .and_then(Value::as_array)
        .ok_or(ReceiptError::InvalidField("logs"))?
        .iter()
        .map(parse_log)
        .collect::<Result<_, _>>()?;

    Ok(EthReceipt { succeeded, logs })
}

fn parse_log(log: &Value) -> Result<EthLog, ReceiptError> {
    let log = log.as_object().ok_or(ReceiptError::InvalidField("logs"))?;

    let address = parse_hex(log.get("address"), "address")?;
    if address.len() != H160::len_bytes() {
        return Err(ReceiptError::InvalidField("address"))
    }

    let topics = log
        .get("topics")
        .and_then(Value::as_array)
        .ok_or(ReceiptError::InvalidField("topics"))?
        .iter()
        .map(|topic| parse_hex(Some(topic), "topics"))
        .collect::<Result<_, _>>()?;

    Ok(EthLog {
        address: H160::from_slice(&address),
        topics,
        data: parse_hex(log.get("data"), "data")?,
    })
}

fn parse_hex(value: Option<&Value>, field: &'static str) -> Result<Vec<u8>, ReceiptError> {
    let hex_str = value.and_then(Value::as_str).ok_or(ReceiptError::InvalidField(field))?;
    hex::decode(strip_hex_prefix(hex_str)).map_err(|_| ReceiptError::InvalidField(field))
}

fn parse_quantity(value: &Value, field: &'static str) -> Result<u64, ReceiptError> {
    let hex_str = value.as_str().ok_or(ReceiptError::InvalidField(field))?;
    u64::from_str_radix(strip_hex_prefix(hex_str), 16)
        .map_err(|_| ReceiptError::InvalidField(field))
}

fn strip_hex_prefix(hex_str: &str) -> &str {
    hex_str
        .strip_prefix("0x")
        .or_else(|| hex_str.strip_prefix("0X"))
        .unwrap_or(hex_str)
}

#[cfg(test)]
#[path = "tests/test_eth_receipt.rs"]
mod test_eth_receipt;
