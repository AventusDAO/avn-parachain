#[cfg(not(feature = "std"))]
extern crate alloc;
use crate::EthQueryResponse;
#[cfg(not(feature = "std"))]
use alloc::{format, string::String};
use codec::Decode;
use hex::FromHex;
use simple_json2::{
    self as json,
    impls::SimpleError,
    json::{JsonObject, JsonValue},
    parser::Error,
};
use sp_core::{H160, H256};
use sp_std::prelude::*;
// TODO [TYPE: refactoring][PRI: unknown][NOTE: clarify]: extend the parser to work with named
// properties
const INDEX_EVENT_SIGNATURE_TOPIC: usize = 0;

fn get_value_of(key: String, object: &JsonObject) -> Result<&JsonValue, SimpleError> {
    let key_char: Vec<char> = key.chars().collect();
    let value = object.into_iter().find(|v| v.0 == key_char);
    if let Some(value) = value {
        return Ok(&value.1);
    }

    return Err(SimpleError::plain_str("key not found in object"));
}

pub fn parse_response_to_json(response_body: Vec<u8>) -> Result<(JsonObject, u64), ()> {
    let body = hex::decode(&response_body).map_err(|e| {
        log::error!("❌ Error decoding hex response {:?} - {:?}", response_body, e);
    })?;

    let eth_query_response = EthQueryResponse::decode(&mut &body[..]).map_err(|e| {
        log::error!("❌ Error decoding eth query response {:?} - {:?}", body, e);
    })?;

    let response_data_bytes: Vec<u8> =
        Decode::decode(&mut &eth_query_response.data[..]).map_err(|e| {
            log::error!(
                "❌ Invalid response data from ethereum: {:?} - {:?}",
                eth_query_response.data,
                e
            );
        })?;

    let response_data_string = &core::str::from_utf8(&response_data_bytes).map_err(|e| {
        log::error!(
            "❌ Invalid (non utf8) response data bytes {:?} - {:?}",
            response_data_bytes,
            e
        );
    })?;

    let response_data_json = json::parse_json(&response_data_string).map_err(|e| {
        log::error!(
            "❌ Response from ethereum is not a valid json {:?} - {:?}",
            response_data_string,
            e
        );
    })?;

    let response_json_object = response_data_json.get_object().map_err(|e| {
        log::error!(
            "❌ Error converting json {:?} into a json object - {:?}",
            response_data_json,
            e
        );
    })?;

    return Ok((response_json_object.clone(), eth_query_response.num_confirmations));
}

pub fn find_event(
    response: &JsonObject,
    topic: H256,
) -> Option<(Option<Vec<u8>>, Vec<Vec<u8>>, H160)> {
    let empty_events = &vec![];
    let events = get_events(response).unwrap_or(empty_events);
    let event =
        events.into_iter().find(|event| topic_matches(event, topic).map_or_else(|_| false, |v| v));

    if let Some(event) = event {
        if let Ok(contract_address) = get_contract_address(event) {
            if let Ok((data, topics)) = get_topics_with_data(&event) {
                return Some((data, topics, contract_address));
            }
        }
    }

    return None;
}

pub fn get_status(response: &JsonObject) -> Result<u8, SimpleError> {
    let status = get_value_of(String::from("status"), response)?.get_string()?;
    match u8::from_str_radix(status.trim_start_matches("0x"), 16) {
        Ok(s) => Ok(s),
        Err(e) => {
            log::error!("❌ Status {:?} is not a valid hex number - {:?}", status, e);
            Err(SimpleError::plain_str("not a valid hex number"))
        },
    }
}

fn get_topics_with_data(event: &JsonValue) -> Result<(Option<Vec<u8>>, Vec<Vec<u8>>), SimpleError> {
    let topics = get_topics(event)?;
    let data = get_data(event)?;
    return Ok((data, topics));
}

fn get_events(response: &JsonObject) -> Result<&Vec<JsonValue>, SimpleError> {
    let events = get_value_of(String::from("logs"), response)?.get_array().map_err(|e| {
        log::error!("❌ Unable to find events from response {:?} - {:?}", response, e);
        e
    })?;

    return Ok(events);
}

fn get_data(event: &JsonValue) -> Result<Option<Vec<u8>>, SimpleError> {
    let event = event.get_object()?;
    let data = get_value_of(String::from("data"), event)?.get_string().map_err(|e| {
        log::error!("❌ Unable to extract data from event {:?} - {:?}", event, e);
        e
    })?;

    let bytes = hex_to_bytes(data)?;

    if !bytes.is_empty() {
        return Ok(Some(bytes));
    }

    return Ok(None);
}

fn get_topics(event: &JsonValue) -> Result<Vec<Vec<u8>>, SimpleError> {
    let event = event.get_object()?;
    let topics = get_value_of(String::from("topics"), event)?.get_array().map_err(|e| {
        log::error!("❌ Unable to extract topics from event {:?} - {:?}", event, e);
        e
    })?;

    let mut topics_bytes: Vec<Vec<u8>> = Vec::<Vec<u8>>::new();
    for topic in topics.into_iter() {
        let topic_string = topic.get_string()?;
        let topic_bytes = hex_to_bytes(topic_string)?;
        topics_bytes.push(topic_bytes);
    }

    return Ok(topics_bytes);
}

fn get_event_signature(event: &JsonValue) -> Result<String, SimpleError> {
    let event = event.get_object()?;
    let topics = get_value_of(String::from("topics"), event)?.get_array()?;
    let event_signature = topics[INDEX_EVENT_SIGNATURE_TOPIC].get_string()?;

    return Ok(event_signature);
}

fn get_contract_address(event: &JsonValue) -> Result<H160, SimpleError> {
    let event = event.get_object()?;
    let address = get_value_of(String::from("address"), event)?.get_string()?;
    let bytes = hex_to_bytes(address)?;

    return Ok(H160::from_slice(&bytes));
}

fn hex_to_bytes(hex_string: String) -> Result<Vec<u8>, SimpleError> {
    let mut hex_string = hex_string.to_lowercase();
    if hex_string.starts_with("0x") {
        hex_string = hex_string[2..].into();
    }

    return Vec::from_hex(hex_string)
        .map_or_else(|_error| Err(SimpleError::plain_str("hex_to_bytes error")), |bytes| Ok(bytes));
}

fn to_bytes32(hex_topic: String) -> Result<[u8; 32], SimpleError> {
    let mut hex_topic = hex_topic.to_lowercase();
    if hex_topic.starts_with("0x") {
        hex_topic = hex_topic[2..].into();
    }

    return <[u8; 32]>::from_hex(hex_topic).map_or_else(
        |_error| Err(SimpleError::plain_str("to_bytes32 error")),
        |bytes32| Ok(bytes32),
    );
}

fn topic_matches(event: &JsonValue, topic: H256) -> Result<bool, SimpleError> {
    let event_signature_bytes = to_bytes32(get_event_signature(event)?)?;
    return Ok(H256(event_signature_bytes) == topic);
}

#[cfg(test)]
#[path = "tests/test_event_parser.rs"]
mod test_event_parser;
