// Copyright 2026 Aventus DAO Ltd
#[cfg(test)]
use super::*; // event_types
use crate::avn_tests_helpers::ethereum_converters::*;
use byte_slice_cast::AsByteSlice;
use hex_literal::hex;
use sp_core::{H160, H256};
use sp_runtime::traits::{BlakeTwo256, Hash};
use sp_std::vec::Vec;

struct TestConsts {
    topic1: Vec<u8>,
    topic2: Vec<u8>,
    topic3: Vec<u8>,
    topic4: Vec<u8>,

    bad_topic1: Vec<u8>,
    bad_topic2: Vec<u8>,

    data: Vec<u8>,
    bad_data1: Vec<u8>,
    bad_data2: Vec<u8>,
}

impl TestConsts {
    fn setup() -> Self {
        let topic2_part1 = vec![20; 32];
        let topic2_part2 = vec![20; 32];

        let data = into_32_be_bytes(&10000u32.to_le_bytes());

        let mut bad_data1 = data.clone();
        bad_data1.pop(); // remove one byte

        let mut bad_data2 = data.clone();
        bad_data2.push(10); // add another byte

        TestConsts {
            topic1: vec![10; 32],
            topic2: topic2_part1,
            topic3: topic2_part2,
            topic4: vec![30; 32],

            bad_topic1: vec![10; 16],
            bad_topic2: vec![10; 64],
            data,
            bad_data1,
            bad_data2,
        }
    }
}

fn get_lifted_avt_data() -> Vec<u8> {
    let amount = 10000u32;
    let mut data = Vec::new();

    let amount_vec = into_32_be_bytes(&amount.to_le_bytes());

    data.extend(&amount_vec);

    return data
}

fn get_lifted_avt_data_with_max_value() -> Vec<u8> {
    let amount = u128::max_value();
    let mut data = Vec::new();

    let amount_vec = into_32_be_bytes(&amount.to_le_bytes());

    data.extend(&amount_vec);

    return data
}

fn get_lifted_avt_data_with_max_bits() -> Vec<u8> {
    let amount = 1u128 << 127;
    let mut data = Vec::new();

    let amount_vec = into_32_be_bytes(&amount.to_le_bytes());

    data.extend(&amount_vec);

    return data
}

fn get_lifted_avt_data_with_too_large_amount() -> Vec<u8> {
    let amount = U256::from(u128::max_value()) + 1;
    let mut data = Vec::new();

    let amount_vec = into_32_be_bytes(amount.as_byte_slice());

    data.extend(&amount_vec);

    return data
}

fn get_lifted_avt_short_data() -> Vec<u8> {
    let mut data = get_lifted_avt_data();
    data.pop();
    return data
}

fn get_lifted_avt_long_data() -> Vec<u8> {
    let mut data = get_lifted_avt_data();
    data.push(10);
    return data
}

fn get_topic_20_bytes(n: u8) -> Vec<u8> {
    let mut topic = vec![0; 12];
    topic.append(&mut vec![n; 20]);

    return topic
}

fn get_lifted_avt_topics() -> Vec<Vec<u8>> {
    let topic_event_signature = get_topic_32_bytes(10);
    let topic_contract = get_topic_20_bytes(20);
    let topic_receiver = get_topic_32_bytes(30);
    return vec![topic_event_signature, topic_contract, topic_receiver]
}

fn get_lifted_avt_few_topics() -> Vec<Vec<u8>> {
    let mut topics = get_lifted_avt_topics();
    topics.pop();
    return topics
}

fn get_lifted_avt_many_topics() -> Vec<Vec<u8>> {
    let mut topics = get_lifted_avt_topics();
    topics.push(get_topic_20_bytes(20));
    return topics
}

fn get_lifted_avt_with_short_topic() -> Vec<Vec<u8>> {
    let mut topics = get_lifted_avt_topics();
    topics[1].pop();
    return topics
}

fn get_lifted_avt_with_long_topic() -> Vec<Vec<u8>> {
    let mut topics = get_lifted_avt_topics();
    topics[1].push(30);
    return topics
}

// ===================================== LiftedAVT related tests
// =============================================

#[test]
fn test_lifted_avt_parse_bytes_good_case() {
    let expected_contract_address = H160(hex!("1414141414141414141414141414141414141414"));
    let expected_sender_address = H160::zero();
    let expected_receiver_address =
        H256(hex!("1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e"));
    let expected_amount = 10000u32;

    let data = Some(get_lifted_avt_data());
    let topics = get_lifted_avt_topics();
    let result = LiftedData::parse_bytes(data, topics);

    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.is_valid());

    assert_eq!(result.token_contract, expected_contract_address);
    assert_eq!(result.sender_address, expected_sender_address);
    assert_eq!(result.receiver_address, expected_receiver_address);
    assert_eq!(result.amount, expected_amount.into());
    assert!(result.nonce.is_zero());
}

#[test]
fn test_lifted_avt_parse_bytes_max_values() {
    let expected_contract_address = H160(hex!("1414141414141414141414141414141414141414"));
    let expected_sender_address = H160::zero();
    let expected_receiver_address =
        H256(hex!("1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e"));
    let expected_amount = u128::max_value();

    let data = Some(get_lifted_avt_data_with_max_value());
    let topics = get_lifted_avt_topics();
    let result = LiftedData::parse_bytes(data, topics);

    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.is_valid());

    assert_eq!(result.token_contract, expected_contract_address);
    assert_eq!(result.sender_address, expected_sender_address);
    assert_eq!(result.receiver_address, expected_receiver_address);
    assert_eq!(result.amount, expected_amount.into());
    assert!(result.nonce.is_zero());
}

#[test]
fn test_lifted_avt_parse_bytes_endianness() {
    let expected_contract_address = H160(hex!("1414141414141414141414141414141414141414"));
    let expected_sender_address = H160::zero();
    let expected_receiver_address =
        H256(hex!("1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e"));
    let expected_amount = 1u128 << 127;

    let data = Some(get_lifted_avt_data_with_max_bits());
    let topics = get_lifted_avt_topics();
    let result = LiftedData::parse_bytes(data, topics);

    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.is_valid());

    assert_eq!(result.token_contract, expected_contract_address);
    assert_eq!(result.sender_address, expected_sender_address);
    assert_eq!(result.receiver_address, expected_receiver_address);
    assert_eq!(result.amount, expected_amount.into());
    assert!(result.nonce.is_zero());
}

#[test]
fn test_lifted_avt_parse_bytes_overflow_values() {
    let bad_data = Some(get_lifted_avt_data_with_too_large_amount());
    let topics = get_lifted_avt_topics();
    let result = LiftedData::parse_bytes(bad_data, topics);

    assert_eq!(result, Err(Error::LiftedEventDataOverflow));
}

#[test]
fn test_lifted_avt_parse_bytes_no_data() {
    let data = None;
    let topics = get_lifted_avt_topics();
    let result = LiftedData::parse_bytes(data, topics);

    assert_eq!(result, Err(Error::LiftedEventMissingData));
}

#[test]
fn test_lifted_avt_parse_bytes_short_data() {
    let bad_data = Some(get_lifted_avt_short_data());
    let topics = get_lifted_avt_topics();
    let result = LiftedData::parse_bytes(bad_data, topics);

    assert_eq!(result, Err(Error::LiftedEventBadDataLength));
}

#[test]
fn test_lifted_avt_parse_bytes_long_data() {
    let bad_data = Some(get_lifted_avt_long_data());
    let topics = get_lifted_avt_topics();
    let result = LiftedData::parse_bytes(bad_data, topics);

    assert_eq!(result, Err(Error::LiftedEventBadDataLength));
}

#[test]
fn test_lifted_avt_parse_bytes_few_topics() {
    let data = Some(get_lifted_avt_data());
    let bad_topics = get_lifted_avt_few_topics();

    let result = LiftedData::parse_bytes(data, bad_topics);

    assert_eq!(result, Err(Error::LiftedEventWrongTopicCount));
}

#[test]
fn test_lifted_avt_parse_bytes_too_many_topics() {
    let data = Some(get_lifted_avt_data());
    let bad_topics = get_lifted_avt_many_topics();

    let result = LiftedData::parse_bytes(data, bad_topics);

    assert_eq!(result, Err(Error::LiftedEventWrongTopicCount));
}

#[test]
fn test_lifted_avt_parse_bytes_short_topic() {
    let data = Some(get_lifted_avt_data());
    let bad_topics = get_lifted_avt_with_short_topic();

    let result = LiftedData::parse_bytes(data, bad_topics);

    assert_eq!(result, Err(Error::LiftedEventBadTopicLength));
}

#[test]
fn test_lifted_avt_parse_bytes_long_topic() {
    let data = Some(get_lifted_avt_data());
    let bad_topics = get_lifted_avt_with_long_topic();

    let result = LiftedData::parse_bytes(data, bad_topics);

    assert_eq!(result, Err(Error::LiftedEventBadTopicLength));
}

#[test]
fn test_prediction_market_lifted_avt_parse_bytes_good_case() {
    let expected_contract_address = H160(hex!("1414141414141414141414141414141414141414"));
    let expected_receiver_address =
        H256(hex!("1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e"));
    let expected_amount = 10000u32;

    let data = Some(get_lifted_avt_data());
    let topics = get_lifted_avt_topics();
    let result = LiftedData::parse_bytes(data, topics);

    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.is_valid());

    assert_eq!(result.token_contract, expected_contract_address);
    assert_eq!(result.receiver_address, expected_receiver_address);
    assert_eq!(result.amount, expected_amount.into());
}

// ===================================== AddedValidator related tests
// ========================================

#[test]
fn test_added_validator_parse_bytes_good_case() {
    let c = TestConsts::setup();

    let expected_eth_public_key = H512(hex!("14141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414"));
    let expected_t2_address =
        H256(hex!("1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e"));

    let data = Some(c.data);
    let topics = vec![c.topic1.clone(), c.topic2.clone(), c.topic3.clone(), c.topic4.clone()];
    let result = AddedValidatorData::parse_bytes(data, topics);

    assert!(result.is_ok());
    let result = result.unwrap();

    assert_eq!(result.eth_public_key, expected_eth_public_key);
    assert_eq!(result.t2_address, expected_t2_address);
    assert_eq!(result.validator_account_id, 10000.into());
}

#[test]
fn test_added_validator_parse_bytes_no_data() {
    let c = TestConsts::setup();

    let data = None;
    let topics = vec![c.topic1.clone(), c.topic2.clone(), c.topic3.clone()];
    let result = AddedValidatorData::parse_bytes(data, topics);

    assert_eq!(result, Err(Error::AddedValidatorEventMissingData));
}

#[test]
fn test_added_validator_parse_bytes_short_data() {
    let c = TestConsts::setup();

    let data = Some(c.bad_data1);
    let topics = vec![c.topic1.clone(), c.topic2.clone(), c.topic3.clone()];
    let result = AddedValidatorData::parse_bytes(data, topics);

    assert_eq!(result, Err(Error::AddedValidatorEventBadDataLength));
}

#[test]
fn test_added_validator_parse_bytes_long_data() {
    let c = TestConsts::setup();

    let data = Some(c.bad_data2);
    let topics = vec![c.topic1.clone(), c.topic2.clone(), c.topic3.clone()];
    let result = AddedValidatorData::parse_bytes(data, topics);

    assert_eq!(result, Err(Error::AddedValidatorEventBadDataLength));
}

#[test]
fn test_added_validator_parse_bytes_few_topics() {
    let c = TestConsts::setup();

    let data = Some(c.data);
    let topics = vec![c.topic1.clone(), c.topic2.clone()];
    let result = AddedValidatorData::parse_bytes(data, topics);

    assert_eq!(result, Err(Error::AddedValidatorEventWrongTopicCount));
}

#[test]
fn test_added_validator_parse_bytes_too_many_topics() {
    let c = TestConsts::setup();

    let data = Some(c.data);
    let topics = vec![
        c.topic1.clone(),
        c.topic2.clone(),
        c.topic1.clone(),
        c.topic3.clone(),
        c.topic3.clone(),
    ];
    let result = AddedValidatorData::parse_bytes(data, topics);

    assert_eq!(result, Err(Error::AddedValidatorEventWrongTopicCount));
}

#[test]
fn test_added_validator_parse_bytes_short_topic() {
    let c = TestConsts::setup();

    let data = Some(c.data);
    let topics = vec![c.topic1.clone(), c.bad_topic1.clone(), c.topic2.clone(), c.topic3.clone()];
    let result = AddedValidatorData::parse_bytes(data, topics);

    assert_eq!(result, Err(Error::AddedValidatorEventBadTopicLength));
}

#[test]
fn test_added_validator_parse_bytes_long_topic() {
    let c = TestConsts::setup();

    let data = Some(c.data);
    let topics = vec![c.topic1.clone(), c.bad_topic2.clone(), c.topic2.clone(), c.topic2.clone()];
    let result = AddedValidatorData::parse_bytes(data, topics);

    assert_eq!(result, Err(Error::AddedValidatorEventBadTopicLength));
}

#[test]
fn test_hashed() {
    let event_id = EthEventId { signature: H256::zero(), transaction_hash: H256::zero() };

    let actual = event_id.hashed(BlakeTwo256::hash);
    let expected = BlakeTwo256::hash_of(&event_id);
    assert_eq!(expected, actual);
}

#[test]
fn test_module_hasher_uses_hash_of() {
    const EMPTY_STRING: &str = "";
    const HELLO_WORLD: &str = "hello world";

    let expected = BlakeTwo256::hash_of(&HELLO_WORLD);
    let actual = HELLO_WORLD.using_encoded(BlakeTwo256::hash);
    assert_eq!(expected, actual);

    let expected = BlakeTwo256::hash_of(&EMPTY_STRING);
    let actual = EMPTY_STRING.using_encoded(BlakeTwo256::hash);
    assert_eq!(expected, actual);
}
