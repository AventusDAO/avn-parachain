// Copyright 2025 Aventus Network Systems (UK) Ltd.

#![cfg(test)]
use crate::{mock::*, *};
use frame_support::assert_ok;
use sp_core::{H160, U256};
use sp_runtime::{
    testing::{TestSignature, UintAuthorityId},
    DispatchError,
};
pub extern crate alloc;
use alloc::collections::BTreeSet;
use sp_avn_common::{
    event_discovery::EthBridgeEventsFilter,
    event_types::{EthEvent, EventData, LiftedData, ValidEvents},
};

#[derive(Clone)]
pub struct EventProcessContext {
    pub mock_event_partition: EthereumEventsPartition,
    pub bad_mock_event_partition: EthereumEventsPartition,
    pub second_mock_event_partition: EthereumEventsPartition,
    pub author: Author<TestRuntime>,
    pub author_two: Author<TestRuntime>,
    pub test_signature: TestSignature,
    pub test_signature_two: TestSignature,
    pub eth_event_id: EthEventId,
    pub bad_eth_event_id: EthEventId,
}

impl EventProcessContext {
    pub fn setup() -> EventProcessContext {
        let primary_validator_id =
            AVN::<TestRuntime>::advance_primary_validator_for_sending().unwrap();
        let author = Author::<TestRuntime> {
            key: UintAuthorityId(primary_validator_id),
            account_id: primary_validator_id,
        };
        let author_two = Author::<TestRuntime> { key: UintAuthorityId(22), account_id: 22 };

        let test_signature = generate_signature(author.clone(), b"test context");
        let test_signature_two = generate_signature(author.clone(), b"test context");
        let eth_tx_hash = H256::from_slice(&[0u8; 32]);
        let eth_event_id = EthEventId {
            signature: ValidEvents::Lifted.signature(),
            transaction_hash: eth_tx_hash,
        };
        let bad_eth_event_id = EthEventId {
            signature: ValidEvents::Lifted.signature(),
            transaction_hash: H256::from_slice(&[6u8; 32]),
        };
        let bad_eth_event = EthEvent {
            event_id: bad_eth_event_id.clone(),
            event_data: sp_avn_common::event_types::EventData::LogLifted(LiftedData {
                token_contract: H160::zero(),
                sender_address: H160::zero(),
                receiver_address: H256::zero(),
                amount: 1,
                nonce: U256::zero(),
            }),
        };
        let mock_event_partition = create_mock_event_partition(
            EthEvent { event_id: eth_event_id.clone(), event_data: Self::mock_lift_event() },
            2,
            0,
        );
        let bad_mock_event_partition = create_mock_event_partition(bad_eth_event, 2, 0);

        let second_mock_event_partition = create_mock_event_partition(
            EthEvent { event_id: eth_event_id.clone(), event_data: Self::mock_lift_event() },
            2,
            1,
        );

        UintAuthorityId::set_all_keys(vec![UintAuthorityId(primary_validator_id)]);

        EventProcessContext {
            mock_event_partition,
            bad_mock_event_partition,
            second_mock_event_partition,
            test_signature,
            test_signature_two,
            author: author.clone(),
            author_two: author_two.clone(),
            eth_event_id,
            bad_eth_event_id,
        }
    }

    fn mock_lift_event() -> EventData {
        sp_avn_common::event_types::EventData::LogLifted(LiftedData {
            token_contract: H160::zero(),
            sender_address: H160::zero(),
            receiver_address: H256::zero(),
            amount: 1,
            nonce: U256::zero(),
        })
    }
}

pub fn create_mock_event_partition(
    events: EthEvent,
    block: u64,
    part: u16,
) -> EthereumEventsPartition {
    let mut partition: BoundedBTreeSet<DiscoveredEvent, IncomingEventsBatchLimit> =
        BoundedBTreeSet::new();
    partition.try_insert(DiscoveredEvent { event: events.clone(), block }).unwrap();
    EthereumEventsPartition::new(
        EthBlockRange { start_block: 1, length: 1000 },
        part,
        false,
        partition,
    )
}

// Added this function as in event_listener_tests to initialize the active event range
pub(crate) fn init_active_range() {
    ActiveEthereumRange::<TestRuntime>::put(ActiveEthRange {
        range: EthBlockRange { start_block: 1, length: 1000 },
        partition: 0,
        event_types_filter: EthBridgeEventsFilter::try_from(
            vec![
                ValidEvents::AddedValidator,
                ValidEvents::Lifted,
                ValidEvents::AvtGrowthLifted,
                ValidEvents::AvtLowerClaimed,
                ValidEvents::LowerReverted,
            ]
            .into_iter()
            .collect::<BTreeSet<ValidEvents>>(),
        )
        .unwrap(),
        additional_transactions: Default::default(),
    });
}

mod process_events {
    use super::*;

    // successfully process the specified ethereum_event
    #[test]
    fn successful_event_processing_accepted() {
        let mut ext = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .as_externality();
        ext.execute_with(|| {
            let context = EventProcessContext::setup();
            init_active_range();

            // Two calls needed as upon the first there are not enough votes to pass the condition
            // in lib.rs line 563, to reach the call of process_ethereum_events_partition()
            assert_ok!(EthBridge::submit_ethereum_events(
                RuntimeOrigin::none(),
                context.author.clone(),
                context.mock_event_partition.clone(),
                context.test_signature.clone()
            ));
            assert_ok!(EthBridge::submit_ethereum_events(
                RuntimeOrigin::none(),
                context.author_two.clone(),
                context.mock_event_partition.clone(),
                context.test_signature_two.clone()
            ));

            assert!(System::events().iter().any(|record| record.event ==
                mock::RuntimeEvent::EthBridge(Event::<TestRuntime>::EventAccepted {
                    eth_event_id: context.eth_event_id.clone(),
                })));
        });
    }

    // This test should fail processing the ethereum_event and emit the specified event
    #[test]
    fn successful_event_processing_not_accepted() {
        let mut ext = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .as_externality();
        ext.execute_with(|| {
            let context = EventProcessContext::setup();
            init_active_range();
            assert_ok!(EthBridge::submit_ethereum_events(
                RuntimeOrigin::none(),
                context.author.clone(),
                context.bad_mock_event_partition.clone(),
                context.test_signature.clone()
            ));
            assert_ok!(EthBridge::submit_ethereum_events(
                RuntimeOrigin::none(),
                context.author_two.clone(),
                context.bad_mock_event_partition.clone(),
                context.test_signature.clone()
            ));

            assert!(System::events().iter().any(|record| record.event ==
                mock::RuntimeEvent::EthBridge(Event::<TestRuntime>::EventRejected {
                    eth_event_id: context.bad_eth_event_id.clone(),
                    reason: DispatchError::Other("").into(),
                })));
        });
    }

    // This test should fail on the check
    // T::ProcessedEventsChecker::processed_event_exists(&event.event_id.clone()), if the event is
    // already in the system
    #[test]
    fn event_already_processed() {
        let mut ext = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .as_externality();
        ext.execute_with(|| {
            let context = EventProcessContext::setup();
            init_active_range();
            assert_ok!(EthBridge::submit_ethereum_events(
                RuntimeOrigin::none(),
                context.author.clone(),
                context.mock_event_partition.clone(),
                context.test_signature.clone()
            ));
            assert_ok!(EthBridge::submit_ethereum_events(
                RuntimeOrigin::none(),
                context.author_two.clone(),
                context.mock_event_partition.clone(),
                context.test_signature_two.clone()
            ));
            assert_ok!(EthBridge::submit_ethereum_events(
                RuntimeOrigin::none(),
                context.author.clone(),
                context.second_mock_event_partition.clone(),
                context.test_signature.clone()
            ));
            assert_ok!(EthBridge::submit_ethereum_events(
                RuntimeOrigin::none(),
                context.author_two.clone(),
                context.second_mock_event_partition.clone(),
                context.test_signature_two.clone()
            ));
            assert!(System::events().iter().any(|record| record.event ==
                mock::RuntimeEvent::EthBridge(Event::<TestRuntime>::EventRejected {
                    eth_event_id: context.eth_event_id.clone(),
                    reason: Error::<TestRuntime>::EventAlreadyProcessed.into(),
                })));
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn future_event_gets_rejected() {
            let mut ext = ExtBuilder::build_default()
                .with_validators()
                .with_genesis_config()
                .as_externality();
            ext.execute_with(|| {
                let context = EventProcessContext::setup();
                init_active_range();

                let partition_with_future_events = create_mock_event_partition(
                    EthEvent {
                        event_id: context.eth_event_id.clone(),
                        event_data: EventProcessContext::mock_lift_event(),
                    },
                    1001,
                    0,
                );

                assert_ok!(EthBridge::submit_ethereum_events(
                    RuntimeOrigin::none(),
                    context.author.clone(),
                    partition_with_future_events.clone(),
                    context.test_signature.clone()
                ));
                assert_ok!(EthBridge::submit_ethereum_events(
                    RuntimeOrigin::none(),
                    context.author_two.clone(),
                    partition_with_future_events.clone(),
                    context.test_signature_two.clone()
                ));

                assert!(mock::contains_event(mock::RuntimeEvent::EthBridge(
                    Event::<TestRuntime>::EventRejected {
                        eth_event_id: context.eth_event_id.clone(),
                        reason: Error::<TestRuntime>::EventBelongsInFutureRange.into(),
                    }
                )));
            });
        }
    }
}
