#![cfg(test)]

use crate::{mock::*, *};
use sp_core::hash::H256;

use sp_avn_common::{avn_tests_helpers::ethereum_converters::*, event_types::EventData};

#[test]
fn test_parse_tier1_event_valid_case_added_validator() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let data = into_32_be_bytes(&10000u32.to_le_bytes());

        let topics = vec![vec![10; 32], vec![10; 32], vec![10; 32], vec![10; 32]];

        let validator_event_to_parse = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::random(),
        };
        let ok_result =
            EthereumEvents::parse_tier1_event(validator_event_to_parse, Some(data), topics);
        assert!(ok_result.is_ok(), "Parse of valid tier1 event failed");
        assert!(matches!(ok_result.unwrap(), EventData::LogAddedValidator(_)));
    });
}

#[test]
fn test_parse_tier1_event_valid_case_lifted() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let data = into_32_be_bytes(&10000u32.to_le_bytes());

        let mut token_contract = vec![0; 12];
        token_contract.extend(vec![1; 20]);

        let topics = vec![vec![20; 32], token_contract, vec![40; 32]];

        let validator_event_to_parse = EthEventId {
            signature: ValidEvents::Lifted.signature(),
            transaction_hash: H256::random(),
        };

        let ok_result =
            EthereumEvents::parse_tier1_event(validator_event_to_parse, Some(data), topics);
        assert!(ok_result.is_ok(), "Parse of valid tier1 event failed");
        assert!(matches!(ok_result.unwrap(), EventData::LogLifted(_)));
    });
}

#[test]
fn test_parse_tier1_event_invalid_signature() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let lift_event_to_parse =
            EthEventId { signature: H256::zero(), transaction_hash: H256::random() };
        // TODO [TYPE: test][PRI: medium]: Error::<TestRuntime>::UnrecognizedEventSignature
        assert!(
            EthereumEvents::parse_tier1_event(lift_event_to_parse, None, Vec::<Vec<u8>>::new())
                .is_err(),
            "Parse should fail due to unrecognized event signature"
        );
    });
}

mod parse_nft_mint_log {
    use super::*;

    struct Context {
        topics: Vec<Vec<u8>>,
        data: Option<Vec<u8>>,
    }

    impl Context {
        pub fn nft_mint_eth_event_id() -> EthEventId {
            EthEventId {
                signature: ValidEvents::NftMint.signature(),
                transaction_hash: H256::from([5u8; 32]),
            }
        }

        pub fn setup() -> Context {
            let sale_index = into_32_be_bytes(&10000u64.to_le_bytes());

            let topics = vec![vec![0; 32], vec![11; 32], sale_index, vec![10; 32]];

            let mut data = vec![0; 64];
            data.append(&mut vec![8; 32]);
            data.append(&mut vec![8; 4]);
            data.append(&mut vec![0; 28]);

            return Context { topics, data: Some(data) };
        }
    }

    mod succeeds_when {
        use super::*;

        #[test]
        fn input_event_is_valid_context() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let valid_context = Context::setup();
                let ok_result = EthereumEvents::parse_tier1_event(
                    Context::nft_mint_eth_event_id(),
                    valid_context.data.clone(),
                    valid_context.topics.clone(),
                );
                assert!(ok_result.is_ok(), "Parse of valid tier1 event failed");
                assert!(matches!(ok_result.unwrap(), EventData::LogNftMinted(_)));
            });
        }
    }

    mod fails_when {
        use super::*;
        #[test]
        fn has_some_data() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let mut invalid_context = Context::setup();
                invalid_context.data = Some(vec![40u8; 1]);

                assert!(
                    EthereumEvents::parse_tier1_event(
                        Context::nft_mint_eth_event_id(),
                        invalid_context.data.clone(),
                        invalid_context.topics.clone()
                    )
                    .is_err(),
                    "Parse should have failed due to data existing in an nft mint event"
                );
            });
        }

        #[test]
        fn has_defined_but_empty_data() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let mut invalid_context = Context::setup();
                invalid_context.data = Some(Vec::new());

                assert!(
                    EthereumEvents::parse_tier1_event(
                        Context::nft_mint_eth_event_id(),
                        invalid_context.data.clone(),
                        invalid_context.topics.clone()
                    )
                    .is_err(),
                    "Parse should have failed due to defined & empty data in an nft mint event"
                );
            });
        }
    }
}

mod parse_nft_transfer_to_log {
    use super::*;

    struct Context {
        topics: Vec<Vec<u8>>,
        data: Option<Vec<u8>>,
    }

    impl Context {
        pub fn nft_transfer_to_eth_event_id() -> EthEventId {
            EthEventId {
                signature: ValidEvents::NftTransferTo.signature(),
                transaction_hash: H256::from([5u8; 32]),
            }
        }

        pub fn setup() -> Context {
            let transfer_nonce = into_32_be_bytes(&10000u64.to_le_bytes());

            let topics = vec![vec![0; 32], vec![11; 32], vec![10; 32], transfer_nonce];

            return Context { topics, data: None };
        }
    }

    mod succeeds_when {
        use super::*;

        #[test]
        fn input_event_is_valid_context() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let valid_context = Context::setup();
                let ok_result = EthereumEvents::parse_tier1_event(
                    Context::nft_transfer_to_eth_event_id(),
                    valid_context.data.clone(),
                    valid_context.topics.clone(),
                );
                assert!(ok_result.is_ok(), "Parse of valid tier1 event failed");
                assert!(matches!(ok_result.unwrap(), EventData::LogNftTransferTo(_)));
            });
        }
    }

    mod fails_when {
        use super::*;
        #[test]
        fn has_some_data() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let mut invalid_context = Context::setup();
                invalid_context.data = Some(vec![40u8; 1]);

                assert!(
                    EthereumEvents::parse_tier1_event(
                        Context::nft_transfer_to_eth_event_id(),
                        invalid_context.data.clone(),
                        invalid_context.topics.clone()
                    )
                    .is_err(),
                    "Parse should have failed due to data existing in an nft transfer to event"
                );
            });
        }

        #[test]
        fn has_defined_but_empty_data() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(||{
                let mut invalid_context = Context::setup();
                invalid_context.data = Some(Vec::new());

                assert!(
                    EthereumEvents::parse_tier1_event(
                        Context::nft_transfer_to_eth_event_id(),
                        invalid_context.data.clone(),
                        invalid_context.topics.clone()
                    ).is_err(),
                    "Parse should have failed due to defined & empty data in an nft transfer to event"
                );
            });
        }
    }
}

mod parse_avt_growth_lifted_log {
    use super::*;

    fn get_topic_8_bytes(bytes: Vec<u8>) -> Vec<u8> {
        let mut topic = vec![0; 28];
        let mut values = bytes;
        topic.append(&mut values);
        return topic;
    }

    struct Context {
        topics: Vec<Vec<u8>>,
        data: Option<Vec<u8>>,
    }

    impl Context {
        pub fn avt_growth_lifted_eth_event_id() -> EthEventId {
            EthEventId {
                signature: ValidEvents::AvtGrowthLifted.signature(),
                transaction_hash: H256::from([5u8; 32]),
            }
        }

        pub fn setup() -> Context {
            let topics = vec![
                vec![0; 32],
                into_32_be_bytes(&20u128.to_le_bytes()),
                get_topic_8_bytes(vec![1; 4]),
            ];
            return Context { topics, data: None };
        }
    }

    mod succeeds_when {
        use super::*;

        #[test]
        fn input_event_is_valid() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let valid_context = Context::setup();
                let result = EthereumEvents::parse_tier1_event(
                    Context::avt_growth_lifted_eth_event_id(),
                    valid_context.data.clone(),
                    valid_context.topics.clone(),
                );
                assert!(result.is_ok(), "Parse of valid tier1 event failed");
                assert!(matches!(result.unwrap(), EventData::LogAvtGrowthLifted(_)));
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn event_has_some_data() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let bad_data = Some(vec![40u8; 1]);
                let context = Context::setup();

                assert!(
                    EthereumEvents::parse_tier1_event(
                        Context::avt_growth_lifted_eth_event_id(),
                        bad_data,
                        context.topics.clone()
                    )
                    .is_err(),
                    "Parse should have failed because data should be empty"
                );
            });
        }

        #[test]
        fn has_defined_but_empty_data() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let bad_data = Some(Vec::new());
                let context = Context::setup();

                assert!(
                    EthereumEvents::parse_tier1_event(
                        Context::avt_growth_lifted_eth_event_id(),
                        bad_data,
                        context.topics.clone()
                    )
                    .is_err(),
                    "Parse should have failed because data should be empty"
                );
            });
        }
    }
}
