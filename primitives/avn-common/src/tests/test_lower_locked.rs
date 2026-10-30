// Copyright 2026 Aventus DAO Ltd
#[cfg(test)]
use super::*;
use sha3::{Digest, Keccak256};
use sp_std::vec::Vec;

mod lower_locked {
    use super::*;

    // An ABI-encoded LogLowerLocked(22274, 0x2222..22, 0x1111..11, 123, 0xaa..aa, 1700000000) log
    const TOPIC_LOWER_ID: [u8; 32] =
        hex!("0000000000000000000000000000000000000000000000000000000000005702");
    const TOPIC_RECIPIENT: [u8; 32] =
        hex!("0000000000000000000000002222222222222222222222222222222222222222");
    const DATA: [u8; 128] = hex!(
        "0000000000000000000000001111111111111111111111111111111111111111"
        "000000000000000000000000000000000000000000000000000000000000007b"
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        "000000000000000000000000000000000000000000000000000000006553f100"
    );

    fn topics() -> Vec<Vec<u8>> {
        vec![
            ValidEvents::LowerLocked.signature().as_bytes().to_vec(),
            TOPIC_LOWER_ID.to_vec(),
            TOPIC_RECIPIENT.to_vec(),
        ]
    }

    fn data() -> Option<Vec<u8>> {
        Some(DATA.to_vec())
    }

    #[test]
    fn event_signature_should_match() {
        let mut hasher = Keccak256::new();

        hasher.input(b"LogLowerLocked(uint32,address,address,uint256,bytes32,uint64)");
        let result = hasher.result();

        assert_eq!(result[..], *ValidEvents::LowerLocked.signature().as_bytes());
    }

    #[test]
    fn event_is_primary() {
        assert!(ValidEvents::LowerLocked.is_primary());
    }

    #[test]
    fn lower_event_data_is_always_valid() {
        assert!(EventData::LogLowerLocked(LowerLockedData::default()).is_valid());
    }

    mod can_successfully_be_parsed {
        use super::*;

        #[test]
        fn when_event_contains_correct_topics_and_data() {
            let result = LowerLockedData::parse_bytes(data(), topics());

            assert_eq!(
                result,
                Ok(LowerLockedData {
                    lower_id: 22274,
                    t1_recipient: H160::repeat_byte(0x22),
                    token_contract: H160::repeat_byte(0x11),
                    amount: U256::from(123u8),
                    t2_sender: H256::repeat_byte(0xaa),
                    unlock_time: 1_700_000_000,
                })
            );
        }

        #[test]
        fn when_amount_does_not_fit_in_a_u128() {
            let mut data = DATA.to_vec();
            data[WORD_LENGTH..2 * WORD_LENGTH].copy_from_slice(&[0xff; WORD_LENGTH]);

            let result = LowerLockedData::parse_bytes(Some(data), topics()).unwrap();

            assert_eq!(result.amount, U256::MAX);
        }
    }

    mod fails_parsing_when {
        use super::*;

        #[test]
        fn data_is_missing() {
            let result = LowerLockedData::parse_bytes(None, topics());

            assert_eq!(result, Err(Error::LowerLockedEventMissingData));
        }

        #[test]
        fn data_is_too_short() {
            let result = LowerLockedData::parse_bytes(Some(DATA[..96].to_vec()), topics());

            assert_eq!(result, Err(Error::LowerLockedEventBadDataLength));
        }

        #[test]
        fn data_is_too_long() {
            let mut data = DATA.to_vec();
            data.extend_from_slice(&[0u8; WORD_LENGTH]);

            let result = LowerLockedData::parse_bytes(Some(data), topics());

            assert_eq!(result, Err(Error::LowerLockedEventBadDataLength));
        }

        #[test]
        fn event_contains_few_topics() {
            let mut topics = topics();
            topics.pop();

            let result = LowerLockedData::parse_bytes(data(), topics);

            assert_eq!(result, Err(Error::LowerLockedEventWrongTopicCount));
        }

        #[test]
        fn event_contains_too_many_topics() {
            let mut topics = topics();
            topics.push(TOPIC_RECIPIENT.to_vec());

            let result = LowerLockedData::parse_bytes(data(), topics);

            assert_eq!(result, Err(Error::LowerLockedEventWrongTopicCount));
        }

        #[test]
        fn event_contains_short_topic() {
            let mut topics = topics();
            topics[1] = vec![10; 16];

            let result = LowerLockedData::parse_bytes(data(), topics);

            assert_eq!(result, Err(Error::LowerLockedEventBadTopicLength));
        }

        #[test]
        fn event_contains_long_topic() {
            let mut topics = topics();
            topics[2] = vec![10; 64];

            let result = LowerLockedData::parse_bytes(data(), topics);

            assert_eq!(result, Err(Error::LowerLockedEventBadTopicLength));
        }
    }
}
