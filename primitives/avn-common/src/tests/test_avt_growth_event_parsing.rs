// Copyright 2022 Aventus Systems (UK) Ltd.
#[cfg(test)]
use super::*;
use crate::{avn_tests_helpers::ethereum_converters::*, event_types::AvtGrowthLiftedData};
use byte_slice_cast::AsByteSlice;
use sp_core::U256;
use sp_std::vec::Vec;

pub fn into_4_be_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut vec = Vec::new();
    vec.extend(bytes.iter().copied());
    vec.resize(4, 0);
    vec.reverse();
    return vec;
}

fn get_topic_8_bytes(bytes: Vec<u8>) -> Vec<u8> {
    let mut topic = vec![0; 28];
    let mut values = bytes;
    topic.append(&mut values);
    return topic;
}

fn get_avt_growth_lifted_topics() -> Vec<Vec<u8>> {
    let topic_event_signature = get_topic_32_bytes(10);
    let topic_amount = into_32_be_bytes(&20u128.to_le_bytes());
    let topic_period = get_topic_8_bytes(vec![1; 4]);
    return vec![topic_event_signature, topic_amount, topic_period];
}

fn get_avt_growth_lifted_topics_max_value() -> Vec<Vec<u8>> {
    let topic_event_signature = get_topic_32_bytes(10);
    let topic_amount = into_32_be_bytes(&u128::max_value().to_le_bytes());
    let topic_period = get_topic_8_bytes(into_4_be_bytes(&u32::max_value().to_le_bytes()));
    return vec![topic_event_signature, topic_amount.to_vec(), topic_period];
}

fn get_lifted_avt_few_topics() -> Vec<Vec<u8>> {
    let mut topics = get_avt_growth_lifted_topics();
    topics.pop();
    return topics;
}

fn get_lifted_avt_many_topics() -> Vec<Vec<u8>> {
    let mut topics = get_avt_growth_lifted_topics();
    topics.push(get_topic_32_bytes(20));
    return topics;
}

fn get_lifted_avt_with_short_topic() -> Vec<Vec<u8>> {
    let mut topics = get_avt_growth_lifted_topics();
    topics[1].pop();
    return topics;
}

fn get_lifted_avt_with_long_topic() -> Vec<Vec<u8>> {
    let mut topics = get_avt_growth_lifted_topics();
    topics[2].push(30);
    return topics;
}

#[test]
fn test_lifted_avn_growth_parse_bytes_good_case() {
    let expected_amount = 20u128;
    let expected_period = u32::from_be_bytes([1u8; 4]);

    let topics = get_avt_growth_lifted_topics();
    let result = AvtGrowthLiftedData::parse_bytes(None, topics);
    assert!(result.is_ok());

    let result = result.unwrap();
    assert!(result.is_valid());
    assert_eq!(result.amount, expected_amount);
    assert_eq!(result.period, expected_period);
}

#[test]
fn test_lifted_avn_growth_parse_bytes_max_values() {
    let expected_amount = u128::max_value();
    let expected_period = u32::max_value();

    let topics = get_avt_growth_lifted_topics_max_value();
    let result = AvtGrowthLiftedData::parse_bytes(None, topics);
    assert!(result.is_ok());

    let result = result.unwrap();
    assert!(result.is_valid());
    assert_eq!(result.amount, expected_amount);
    assert_eq!(result.period, expected_period);
}

#[test]
fn test_lifted_avt_parse_bytes_endianness() {
    let expected_amount = 1u128 << 127;
    let expected_period = u32::from_be_bytes([1u8; 4]);

    let mut topics = get_avt_growth_lifted_topics();
    topics[1] = into_32_be_bytes(&(expected_amount).to_le_bytes());

    let result = AvtGrowthLiftedData::parse_bytes(None, topics);
    assert!(result.is_ok());

    let result = result.unwrap();
    assert!(result.is_valid());
    assert_eq!(result.amount, expected_amount);
    assert_eq!(result.period, expected_period);
}

#[test]
fn test_lifted_avt_parse_bytes_overflow_values() {
    let bad_amount = U256::from(u128::max_value()) + 1;
    let mut topics = get_avt_growth_lifted_topics();
    topics[1] = into_32_be_bytes(bad_amount.as_byte_slice());

    let result = AvtGrowthLiftedData::parse_bytes(None, topics);

    assert_eq!(result, Err(Error::AvtGrowthLiftedEventDataOverflow));
}

#[test]
fn test_lifted_avt_parse_bytes_few_topics() {
    let bad_topics = get_lifted_avt_few_topics();
    let result = AvtGrowthLiftedData::parse_bytes(None, bad_topics);
    assert_eq!(result, Err(Error::AvtGrowthLiftedEventWrongTopicCount));
}

#[test]
fn test_lifted_avt_parse_bytes_too_many_topics() {
    let bad_topics = get_lifted_avt_many_topics();
    let result = AvtGrowthLiftedData::parse_bytes(None, bad_topics);
    assert_eq!(result, Err(Error::AvtGrowthLiftedEventWrongTopicCount));
}

#[test]
fn test_lifted_avt_parse_bytes_short_topic() {
    let bad_topics = get_lifted_avt_with_short_topic();
    let result = AvtGrowthLiftedData::parse_bytes(None, bad_topics);
    assert_eq!(result, Err(Error::AvtGrowthLiftedEventBadTopicLength));
}

#[test]
fn test_lifted_avt_parse_bytes_long_topic() {
    let bad_topics = get_lifted_avt_with_long_topic();
    let result = AvtGrowthLiftedData::parse_bytes(None, bad_topics);
    assert_eq!(result, Err(Error::AvtGrowthLiftedEventBadTopicLength));
}
