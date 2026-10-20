// Copyright 2026 Aventus DAO Ltd
#[cfg(test)]
use super::*;
use sha3::{Digest, Keccak256};
use sp_std::vec::Vec;

mod lower_cancelled {
    use super::*;

    struct LowerCancelledConfig {
        topic1: Vec<u8>,
        topic2_lower_id: Vec<u8>,
        data: Option<Vec<u8>>,
        bad_topic_short: Vec<u8>,
        bad_topic_long: Vec<u8>,
        bad_data: Option<Vec<u8>>,
    }

    impl LowerCancelledConfig {
        fn setup() -> Self {
            LowerCancelledConfig {
                topic1: vec![1; 32],
                topic2_lower_id: vec![1u8; 32],
                data: None,
                bad_topic_short: vec![10; 16],
                bad_topic_long: vec![10; 64],
                bad_data: Some(vec![2; 1]),
            }
        }
    }

    #[test]
    fn event_signature_should_match() {
        let mut hasher = Keccak256::new();

        hasher.input(b"LogLowerCancelled(uint32)");
        let result = hasher.result();

        assert_eq!(result[..], *ValidEvents::LowerCancelled.signature().as_bytes());
    }

    #[test]
    fn event_is_primary() {
        assert!(ValidEvents::LowerCancelled.is_primary());
    }

    #[test]
    fn lower_event_data_is_valid_for_any_lower_id() {
        for lower_id in [0, 1, u32::MAX] {
            assert!(EventData::LogLowerCancelled(LowerCancelledData { lower_id }).is_valid());
            assert!(EventData::LogLowerClaimed(AvtLowerClaimedData { lower_id }).is_valid());
        }
    }

    mod can_successfully_be_parsed {
        use super::*;

        #[test]
        fn when_event_contains_correct_topics_and_data() {
            let config = LowerCancelledConfig::setup();

            let expected_lower_id = u32::from_be_bytes([1u8; 4]);

            let topics = vec![config.topic1, config.topic2_lower_id];
            let result = LowerCancelledData::parse_bytes(config.data, topics);

            assert_eq!(result, Ok(LowerCancelledData { lower_id: expected_lower_id }));
        }
    }

    mod fails_parsing_when {
        use super::*;

        #[test]
        fn non_topic_data_is_not_empty() {
            let config = LowerCancelledConfig::setup();

            let topics = vec![config.topic1, config.topic2_lower_id];
            let result = LowerCancelledData::parse_bytes(config.bad_data, topics);

            assert_eq!(result, Err(Error::LowerCancelledEventShouldOnlyContainTopics));
        }

        #[test]
        fn event_contains_few_topics() {
            let config = LowerCancelledConfig::setup();

            let topics = vec![];
            let result = LowerCancelledData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::LowerCancelledEventWrongTopicCount));
        }

        #[test]
        fn event_contains_too_many_topics() {
            let config = LowerCancelledConfig::setup();

            let topics = vec![config.topic1.clone(), config.topic2_lower_id, config.topic1];
            let result = LowerCancelledData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::LowerCancelledEventWrongTopicCount));
        }

        #[test]
        fn event_contains_short_topic() {
            let config = LowerCancelledConfig::setup();

            let topics = vec![config.topic1, config.bad_topic_short];
            let result = LowerCancelledData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::LowerCancelledEventBadTopicLength));
        }

        #[test]
        fn event_contains_long_topic() {
            let config = LowerCancelledConfig::setup();

            let topics = vec![config.topic1, config.bad_topic_long];
            let result = LowerCancelledData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::LowerCancelledEventBadTopicLength));
        }
    }
}
