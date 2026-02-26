// Copyright 2026 Aventus DAO Ltd
#[cfg(test)]
use super::*; // event_types
use hex_literal::hex;
use sha3::{Digest, Keccak256};
use sp_core::H256;
use sp_std::vec::Vec;

mod mint_nft {
    use super::*;

    struct MintNftTestConfig {
        topic1: Vec<u8>,
        topic_batch_id: Vec<u8>,
        topic_sale_index: Vec<u8>,
        topic_owner_pk: Vec<u8>,
        data: Option<Vec<u8>>,
        bad_topic_short: Vec<u8>,
        bad_topic_long: Vec<u8>,
        bad_data: Option<Vec<u8>>,
        missing_data: Option<Vec<u8>>,
    }

    impl MintNftTestConfig {
        fn setup() -> Self {
            let mut topic_sale_index = vec![0; 24];
            topic_sale_index.append(&mut vec![1; 8]);

            // this is the encoding of a UUID string we expect from ethereum
            let mut data = vec![0; 64];
            data.append(&mut vec![8; 32]);
            data.append(&mut vec![8; 4]);
            data.append(&mut vec![0; 28]);

            let topic_batch_id = vec![10; 32];
            let topic_owner_pk = vec![20; 32];

            MintNftTestConfig {
                topic1: vec![1; 32],
                topic_batch_id,
                topic_sale_index,
                topic_owner_pk,
                data: Some(data),
                bad_topic_short: vec![10; 16],
                bad_topic_long: vec![10; 64],
                bad_data: Some(vec![2; 1]),
                missing_data: None,
            }
        }
    }

    #[test]
    fn event_signature_should_match() {
        let mut hasher = Keccak256::new();

        hasher.input(b"AvnMintTo(uint256,uint64,bytes32,string)");
        let result = hasher.result();

        assert_eq!(result[..], *ValidEvents::NftMint.signature().as_bytes());
        assert_eq!(
            result[..],
            hex!("242e8a2c5335295f6294a23543699a458e6d5ed7a5839f93cc420116e0a31f99")[..]
        );
    }

    mod can_successfully_be_parsed {
        use super::*;

        #[test]
        fn when_event_contains_correct_topics_and_data() {
            let config = MintNftTestConfig::setup();

            let expected_batch_id = U256::from_big_endian(vec![10; 32].as_slice());
            let expected_t2_owner_public_key =
                H256(hex!("1414141414141414141414141414141414141414141414141414141414141414"));
            let expected_sale_index = u64::from_be_bytes([1u8; 8]);
            let expected_unique_external_ref: Vec<u8> = vec![8; 36];

            let topics = vec![
                config.topic1,
                config.topic_batch_id,
                config.topic_sale_index,
                config.topic_owner_pk,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert!(result.is_ok());
            let result = result.unwrap();

            assert_eq!(result.batch_id, expected_batch_id);
            assert_eq!(result.t2_owner_public_key, expected_t2_owner_public_key);
            assert_eq!(result.sale_index, expected_sale_index);
            assert_eq!(result.unique_external_ref, expected_unique_external_ref);
        }
    }

    mod fails_parsing_when {
        use super::*;

        #[test]
        fn non_topic_data_is_empty() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1,
                config.topic_batch_id,
                config.topic_sale_index,
                config.topic_owner_pk,
            ];
            let result = NftMintData::parse_bytes(config.missing_data, topics);

            assert_eq!(result, Err(Error::NftMintedEventMissingData));
        }

        #[test]
        fn non_topic_data_is_too_short() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1,
                config.topic_batch_id,
                config.topic_sale_index,
                config.topic_owner_pk,
            ];
            let result = NftMintData::parse_bytes(config.bad_data, topics);

            assert_eq!(result, Err(Error::NftMintedEventBadDataLength));
        }

        #[test]
        fn event_contains_few_topics() {
            let config = MintNftTestConfig::setup();

            let topics = vec![config.topic1, config.topic_batch_id];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftMintedEventWrongTopicCount));
        }

        #[test]
        fn event_contains_too_many_topics() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1.clone(),
                config.topic_batch_id,
                config.topic1,
                config.topic_sale_index.clone(),
                config.topic_sale_index,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftMintedEventWrongTopicCount));
        }

        #[test]
        fn event_contains_short_topic_batch_id() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1,
                config.bad_topic_short,
                config.topic_sale_index,
                config.topic_owner_pk,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftMintedEventBadTopicLength));
        }

        #[test]
        fn event_contains_short_topic_sale_index() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1,
                config.topic_batch_id,
                config.bad_topic_short,
                config.topic_owner_pk,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftMintedEventBadTopicLength));
        }

        #[test]
        fn event_contains_short_topic_owner() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1,
                config.topic_batch_id,
                config.topic_sale_index,
                config.bad_topic_short,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftMintedEventBadTopicLength));
        }

        #[test]
        fn event_contains_long_topic_batch_id() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1,
                config.bad_topic_long,
                config.topic_sale_index,
                config.topic_owner_pk,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftMintedEventBadTopicLength));
        }

        #[test]
        fn event_contains_long_topic_sale_index() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1,
                config.topic_batch_id,
                config.bad_topic_long,
                config.topic_owner_pk,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftMintedEventBadTopicLength));
        }

        #[test]
        fn event_contains_long_topic_owner() {
            let config = MintNftTestConfig::setup();

            let topics = vec![
                config.topic1,
                config.topic_batch_id,
                config.topic_sale_index,
                config.bad_topic_long,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftMintedEventBadTopicLength));
        }
    }

    mod result_is_invalid_when {
        use super::*;

        #[test]
        fn event_contains_zero_address_for_owner() {
            let config = MintNftTestConfig::setup();
            let zero_address_owner = vec![0; 32];

            let topics = vec![
                config.topic1,
                config.topic_batch_id,
                config.topic_sale_index,
                zero_address_owner,
            ];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert!(result.is_ok());
            let result = result.unwrap();

            assert_eq!(false, result.is_valid())
        }

        #[test]
        fn event_contains_zero_batch_id() {
            let config = MintNftTestConfig::setup();
            let zero_batch_id = vec![0; 32];

            let topics =
                vec![config.topic1, zero_batch_id, config.topic_sale_index, config.topic_owner_pk];
            let result = NftMintData::parse_bytes(config.data, topics);

            assert!(result.is_ok());
            let result = result.unwrap();

            assert_eq!(false, result.is_valid())
        }
    }
}

mod transfer_to {
    use super::*;

    struct NftTransferToTestConfig {
        topic1: Vec<u8>,
        topic2: Vec<u8>,
        topic3: Vec<u8>,
        topic4: Vec<u8>,
        data: Option<Vec<u8>>,
        bad_topic_short: Vec<u8>,
        bad_topic_long: Vec<u8>,
        bad_data: Option<Vec<u8>>,
    }

    impl NftTransferToTestConfig {
        fn setup() -> Self {
            let mut index_sale_topic = vec![0; 24];
            index_sale_topic.append(&mut vec![1; 8]);

            let topic2 = vec![10; 32];
            let topic3 = vec![20; 32];

            NftTransferToTestConfig {
                topic1: vec![1; 32],
                topic2,
                topic3,
                topic4: index_sale_topic,
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

        hasher.input(b"AvnTransferTo(uint256,bytes32,uint64)");
        let result = hasher.result();

        assert_eq!(result[..], *ValidEvents::NftTransferTo.signature().as_bytes());
        assert_eq!(
            result[..],
            hex!("fff226ba128aca9718a568817388f3711cfeedd8c81cec4d02dcefc50f3c67bb")[..]
        );
    }

    mod can_successfully_be_parsed {
        use super::*;

        #[test]
        fn when_event_contains_correct_topics_and_data() {
            let config = NftTransferToTestConfig::setup();

            let expected_nft_id = U256::from_big_endian(vec![10; 32].as_slice());
            let expected_t2_transfer_to_public_key =
                H256(hex!("1414141414141414141414141414141414141414141414141414141414141414"));
            let expected_op_id = u64::from_be_bytes([1u8; 8]);

            let topics = vec![config.topic1, config.topic2, config.topic3, config.topic4];
            let result = NftTransferToData::parse_bytes(config.data, topics);

            assert!(result.is_ok());
            let result = result.unwrap();

            assert_eq!(result.nft_id, expected_nft_id);
            assert_eq!(result.t2_transfer_to_public_key, expected_t2_transfer_to_public_key);
            assert_eq!(result.op_id, expected_op_id);
        }
    }

    mod fails_parsing_when {
        use super::*;

        #[test]
        fn non_topic_data_is_not_empty() {
            let config = NftTransferToTestConfig::setup();

            let topics = vec![config.topic1, config.topic2, config.topic3, config.topic4];
            let result = NftTransferToData::parse_bytes(config.bad_data, topics);

            assert_eq!(result, Err(Error::NftTransferToEventShouldOnlyContainTopics));
        }

        #[test]
        fn event_contains_few_topics() {
            let config = NftTransferToTestConfig::setup();

            let topics = vec![config.topic1, config.topic2];
            let result = NftTransferToData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftTransferToEventWrongTopicCount));
        }

        #[test]
        fn event_contains_too_many_topics() {
            let config = NftTransferToTestConfig::setup();

            let topics = vec![
                config.topic1.clone(),
                config.topic2,
                config.topic1,
                config.topic3.clone(),
                config.topic3,
            ];
            let result = NftTransferToData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftTransferToEventWrongTopicCount));
        }

        #[test]
        fn event_contains_short_topic() {
            let config = NftTransferToTestConfig::setup();

            let topics = vec![config.topic1, config.bad_topic_short, config.topic3, config.topic4];
            let result = NftTransferToData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftTransferToEventBadTopicLength));
        }

        #[test]
        fn event_contains_long_topic() {
            let config = NftTransferToTestConfig::setup();

            let topics = vec![config.topic1, config.bad_topic_long, config.topic3, config.topic4];
            let result = NftTransferToData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftTransferToEventBadTopicLength));
        }
    }

    mod result_is_invalid_when {
        use super::*;

        #[test]
        fn event_contains_zero_address_for_owner() {
            let config = NftTransferToTestConfig::setup();
            let zero_address_owner = vec![0; 32];

            let topics = vec![config.topic1, config.topic2, zero_address_owner, config.topic4];
            let result = NftTransferToData::parse_bytes(config.data, topics);

            assert!(result.is_ok());
            let result = result.unwrap();

            assert_eq!(false, result.is_valid())
        }
    }
}

mod cancel_single_listing {
    use super::*;

    struct NftCancelSingleListingConfig {
        topic1: Vec<u8>,
        topic2_nft_id: Vec<u8>,
        topic3_op_id: Vec<u8>,
        data: Option<Vec<u8>>,
        bad_topic_short: Vec<u8>,
        bad_topic_long: Vec<u8>,
        bad_data: Option<Vec<u8>>,
    }

    impl NftCancelSingleListingConfig {
        fn setup() -> Self {
            let mut index_op_id = vec![0; 24];
            index_op_id.append(&mut vec![1; 8]);

            let topic2 = vec![10; 32];

            NftCancelSingleListingConfig {
                topic1: vec![1; 32],
                topic2_nft_id: topic2,
                topic3_op_id: index_op_id,
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

        hasher.input(b"AvnCancelNftListing(uint256,uint64)");
        let result = hasher.result();

        assert_eq!(result[..], *ValidEvents::NftCancelListing.signature().as_bytes());
        assert_eq!(
            result[..],
            hex!("eb0a71ca01b1505be834cafcd54b651d77eafd1ca915d21c0898575bcab53358")[..]
        );
    }

    mod can_successfully_be_parsed {
        use super::*;

        #[test]
        fn when_event_contains_correct_topics_and_data() {
            let config = NftCancelSingleListingConfig::setup();

            let expected_nft_id = U256::from_big_endian(vec![10; 32].as_slice());
            let expected_op_id = u64::from_be_bytes([1u8; 8]);

            let topics = vec![config.topic1, config.topic2_nft_id, config.topic3_op_id];
            let result = NftCancelListingData::parse_bytes(config.data, topics);

            assert!(result.is_ok());
            let result = result.unwrap();

            assert_eq!(result.nft_id, expected_nft_id);
            assert_eq!(result.op_id, expected_op_id);
        }
    }

    mod fails_parsing_when {
        use super::*;

        #[test]
        fn non_topic_data_is_not_empty() {
            let config = NftCancelSingleListingConfig::setup();

            let topics = vec![config.topic1, config.topic2_nft_id, config.topic3_op_id];
            let result = NftCancelListingData::parse_bytes(config.bad_data, topics);

            assert_eq!(result, Err(Error::NftCancelListingEventShouldOnlyContainTopics));
        }

        #[test]
        fn event_contains_few_topics() {
            let config = NftCancelSingleListingConfig::setup();

            let topics = vec![config.topic1, config.topic2_nft_id];
            let result = NftCancelListingData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftCancelListingEventWrongTopicCount));
        }

        #[test]
        fn event_contains_too_many_topics() {
            let config = NftCancelSingleListingConfig::setup();

            let topics = vec![
                config.topic1,
                config.topic2_nft_id,
                config.topic3_op_id.clone(),
                config.topic3_op_id,
            ];
            let result = NftCancelListingData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftCancelListingEventWrongTopicCount));
        }

        #[test]
        fn event_contains_short_topic() {
            let config = NftCancelSingleListingConfig::setup();

            let topics = vec![config.topic1, config.bad_topic_short, config.topic3_op_id];
            let result = NftCancelListingData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftCancelListingEventBadTopicLength));
        }

        #[test]
        fn event_contains_long_topic() {
            let config = NftCancelSingleListingConfig::setup();

            let topics = vec![config.topic1, config.bad_topic_long, config.topic3_op_id];
            let result = NftCancelListingData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftCancelListingEventBadTopicLength));
        }
    }
}

mod end_batch_listing {
    use super::*;

    struct NftEndBatchListingConfig {
        topic1: Vec<u8>,
        topic2_batch_id: Vec<u8>,
        data: Option<Vec<u8>>,
        bad_topic_short: Vec<u8>,
        bad_topic_long: Vec<u8>,
        bad_data: Option<Vec<u8>>,
    }

    impl NftEndBatchListingConfig {
        fn setup() -> Self {
            let topic2_batch_id = vec![10; 32];

            NftEndBatchListingConfig {
                topic1: vec![1; 32],
                topic2_batch_id,
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

        hasher.input(b"AvnEndBatchListing(uint256)");
        let result = hasher.result();

        assert_eq!(result[..], *ValidEvents::NftEndBatchListing.signature().as_bytes());
        assert_eq!(
            result[..],
            hex!("20c46236a16e176bc83a795b3a64ad94e5db8bc92afc8cc6d3fd4a3864211f8f")[..]
        );
    }

    mod can_successfully_be_parsed {
        use super::*;

        #[test]
        fn when_event_contains_correct_topics_and_data() {
            let config = NftEndBatchListingConfig::setup();

            let expected_batch_id = U256::from_big_endian(vec![10; 32].as_slice());

            let topics = vec![config.topic1, config.topic2_batch_id];
            let result = NftEndBatchListingData::parse_bytes(config.data, topics);

            assert!(result.is_ok());
            let result = result.unwrap();

            assert_eq!(result.batch_id, expected_batch_id);
        }
    }

    mod fails_parsing_when {
        use super::*;

        #[test]
        fn non_topic_data_is_not_empty() {
            let config = NftEndBatchListingConfig::setup();

            let topics = vec![config.topic1, config.topic2_batch_id];
            let result = NftEndBatchListingData::parse_bytes(config.bad_data, topics);

            assert_eq!(result, Err(Error::NftEndBatchListingEventShouldOnlyContainTopics));
        }

        #[test]
        fn event_contains_few_topics() {
            let config = NftEndBatchListingConfig::setup();

            let topics = vec![config.topic1];
            let result = NftEndBatchListingData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftEndBatchListingEventWrongTopicCount));
        }

        #[test]
        fn event_contains_too_many_topics() {
            let config = NftEndBatchListingConfig::setup();

            let topics =
                vec![config.topic1, config.topic2_batch_id.clone(), config.topic2_batch_id];
            let result = NftEndBatchListingData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftEndBatchListingEventWrongTopicCount));
        }

        #[test]
        fn event_contains_short_topic() {
            let config = NftEndBatchListingConfig::setup();

            let topics = vec![config.topic1, config.bad_topic_short];
            let result = NftEndBatchListingData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftEndBatchListingEventBadTopicLength));
        }

        #[test]
        fn event_contains_long_topic() {
            let config = NftEndBatchListingConfig::setup();

            let topics = vec![config.topic1, config.bad_topic_long];
            let result = NftEndBatchListingData::parse_bytes(config.data, topics);

            assert_eq!(result, Err(Error::NftEndBatchListingEventBadTopicLength));
        }
    }
}
