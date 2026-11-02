#[cfg(test)]
use crate::event_types::{EventData, LiftedData, ValidEvents};
use crate::{
    event_discovery::{
        events_helpers::EthereumEventsPartitionFactory, DiscoveredEvent, EthBlockRange,
    },
    event_types::{EthEvent, EthEventId},
};
use hex_literal::hex;
use sp_core::{H160, H256, U256};

#[test]
pub fn event_id_comparison_is_case_insensitive() {
    let left = EthEventId {
        signature: H256(hex!("000000000000000000000000000000000000000000000000000000000000dddd")),
        transaction_hash: H256(hex!(
            "000000000000000000000000000000000000000000000000000000000000eeee"
        )),
    };

    let right = EthEventId {
        signature: H256(hex!("000000000000000000000000000000000000000000000000000000000000DDDD")),
        transaction_hash: H256(hex!(
            "000000000000000000000000000000000000000000000000000000000000EEEE"
        )),
    };

    assert_eq!(left, right);
}

#[test]
pub fn discovered_event_ordering_works() {
    let mock_event_data = EventData::LogLifted(LiftedData {
        token_contract: H160::zero(),
        sender_address: H160::zero(),
        receiver_address: H256::zero(),
        amount: 1,
        nonce: U256::zero(),
    });
    let first_event_set = vec![
        DiscoveredEvent {
            event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256(hex!(
                        "000000000000000000000000000000000000000000000000000000000000aaaa"
                    )),
                },
                event_data: mock_event_data.clone(),
            },
            block: 1,
        },
        DiscoveredEvent {
            event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256(hex!(
                        "000000000000000000000000000000000000000000000000000000000000bbbb"
                    )),
                },
                event_data: mock_event_data.clone(),
            },
            block: 1,
        },
        DiscoveredEvent {
            event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256(hex!(
                        "000000000000000000000000000000000000000000000000000000000000cccc"
                    )),
                },
                event_data: mock_event_data.clone(),
            },
            block: 1,
        },
        DiscoveredEvent {
            event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256(hex!(
                        "000000000000000000000000000000000000000000000000000000000000dddd"
                    )),
                },
                event_data: mock_event_data.clone(),
            },
            block: 1,
        },
        DiscoveredEvent {
            event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256(hex!(
                        "000000000000000000000000000000000000000000000000000000000000eeee"
                    )),
                },
                event_data: mock_event_data.clone(),
            },
            block: 1,
        },
    ];

    let second_event_set = vec![
        first_event_set[4].clone(),
        first_event_set[3].clone(),
        first_event_set[2].clone(),
        first_event_set[1].clone(),
        first_event_set[0].clone(),
    ];

    let range = EthBlockRange { start_block: 1, length: 10 };
    assert_eq!(
        EthereumEventsPartitionFactory::create_partitions(range.clone(), first_event_set)
            .first()
            .unwrap(),
        EthereumEventsPartitionFactory::create_partitions(range.clone(), second_event_set)
            .first()
            .unwrap()
    );
}

#[test]
pub fn core_primary_events_filter_excludes_avt_growth() {
    use crate::event_discovery::{filters::CorePrimaryEventsFilter, EthereumEventsFilterTrait};

    let filter = CorePrimaryEventsFilter::get();
    assert!(!filter.contains(&ValidEvents::AvtGrowthLifted));
    assert!(filter.contains(&ValidEvents::Lifted));
}
