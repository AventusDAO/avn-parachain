#![cfg(test)]

use crate::*;
use mock::{EthBridge, ExtBuilder, TestRuntime};
use sp_avn_common::event_types::{EthEvent, EthEventId, EventData, LiftedData, ValidEvents};
use sp_core::{H160, U256};

use sp_runtime::testing::{TestSignature, UintAuthorityId};

use self::mock::RuntimeOrigin;

fn event_data_set() -> Vec<DiscoveredEvent> {
    let events = (1..=100)
        .map(|i| DiscoveredEvent {
            event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256::from([i; 32]),
                },
                event_data: EventData::LogLifted(LiftedData {
                    token_contract: H160::from([1u8; 20]),
                    sender_address: H160::from([2u8; 20]),
                    receiver_address: H256::from([3u8; 32]),
                    amount: 1000,
                    nonce: U256::from(1),
                }),
            },
            block: i as u64,
        })
        .collect();
    events
}

fn alternative_event_data_set() -> Vec<DiscoveredEvent> {
    let events = (1..=20)
        .map(|i| DiscoveredEvent {
            event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256::from([i * 2; 32]),
                },
                event_data: EventData::LogLifted(LiftedData {
                    token_contract: H160::from([1u8; 20]),
                    sender_address: H160::from([2u8; 20]),
                    receiver_address: H256::from([3u8; 32]),
                    amount: 1000,
                    nonce: U256::from(1),
                }),
            },
            block: i as u64,
        })
        .collect();
    events
}

fn empty_event_data_set() -> Vec<DiscoveredEvent> {
    Default::default()
}

fn init_active_range() {
    ActiveEthereumRange::<TestRuntime>::put(ActiveEthRange {
        range: EthBlockRange { start_block: 1, length: 1000 },
        partition: 0,
        ..Default::default()
    });
}

#[derive(Clone, Debug)]
pub struct DiscoveredEthContext {
    pub discovered_events: Vec<DiscoveredEvent>,
    pub author: Author<TestRuntime>,
    range: EthBlockRange,
}

impl Default for DiscoveredEthContext {
    fn default() -> Self {
        let primary_validator_id = 1;
        let author = Author::<TestRuntime> {
            key: UintAuthorityId(primary_validator_id),
            account_id: primary_validator_id,
        };
        let events = event_data_set();

        Self {
            author,
            discovered_events: events,
            range: EthBlockRange { start_block: 1, length: 1000 },
        }
    }
}

impl DiscoveredEthContext {
    fn next_range() {
        let active_range = ActiveEthereumRange::<TestRuntime>::get().unwrap_or_default();
        ActiveEthereumRange::<TestRuntime>::put(ActiveEthRange {
            range: active_range.range.next_range(),
            partition: 0,
            ..Default::default()
        });
    }
    fn partitions(&self) -> Vec<EthereumEventsPartition> {
        events_helpers::EthereumEventsPartitionFactory::create_partitions(
            self.range.clone(),
            self.discovered_events.clone(),
        )
    }
}

impl DiscoveredEthContext {
    fn submit_events_partition(&self, index: usize) -> DispatchResultWithPostInfo {
        EthBridge::submit_ethereum_events(
            RuntimeOrigin::none(),
            self.author.clone(),
            self.partitions().get(index).expect("index exists").clone(),
            self.generate_signature(index),
        )
    }

    fn generate_signature(&self, index: usize) -> TestSignature {
        self.author
            .key
            .sign(&encode_eth_event_submission_data(
                Some(&Instance::<TestRuntime, ()>::get()),
                &SUBMIT_ETHEREUM_EVENTS_HASH_CONTEXT,
                &self.author.account_id,
                self.partitions().get(index).expect("Index should exist"),
            ))
            .expect("Signature is signed")
    }
}

mod submit_discovered_events {

    use super::{DiscoveredEthContext as Context, *};
    use frame_support::{assert_noop, assert_ok};
    use sp_runtime::traits::Saturating;

    fn expected_votes_for_id(id: &H256) -> usize {
        let mut votes_count = 0usize;
        EthereumEvents::<TestRuntime>::iter().for_each(|(partition, _votes)| {
            if partition.id() == *id {
                votes_count.saturating_inc();
            }
        });
        votes_count
    }

    #[test]
    fn adds_vote_correctly() {
        let mut ext =
            ExtBuilder::build_default().with_validators().with_genesis_config().as_externality();
        ext.execute_with(|| {
            init_active_range();
            let context: Context = Default::default();

            assert_ok!(context.submit_events_partition(0));

            assert_eq!(
                expected_votes_for_id(&context.partitions()[0].id()),
                1,
                "Should be a single vote."
            );
        });
    }

    #[test]
    fn adds_empty_vote_correctly() {
        let mut ext =
            ExtBuilder::build_default().with_validators().with_genesis_config().as_externality();
        ext.execute_with(|| {
            init_active_range();
            let context =
                Context { discovered_events: empty_event_data_set(), ..Default::default() };

            assert_ok!(context.submit_events_partition(0));

            assert_eq!(
                expected_votes_for_id(&context.partitions()[0].id()),
                1,
                "Should be a single vote."
            );
        });
    }

    #[test]
    fn finalises_vote() {
        let mut ext =
            ExtBuilder::build_default().with_validators().with_genesis_config().as_externality();
        ext.execute_with(|| {
            // given
            init_active_range();
            let contexts = (1..6 as u64)
                .map(|id| Context {
                    author: Author::<TestRuntime> { key: UintAuthorityId(id), account_id: id },
                    ..Default::default()
                })
                .take(<TestRuntime as crate::Config>::Quorum::get_quorum() as usize)
                .collect::<Vec<Context>>();

            let first_partition_index = 0usize;

            // when
            // Cast all votes
            for context in contexts.iter() {
                assert_ok!(context.submit_events_partition(first_partition_index));
            }

            // then
            let new_active_range = EthBridge::active_ethereum_range().expect("Should be ok");

            let second_partition_index = first_partition_index.saturating_add(1);
            assert_eq!(
                contexts[0].partitions()[second_partition_index].range(),
                &new_active_range.range,
            );
            assert_eq!(
                contexts[0].partitions()[second_partition_index].partition(),
                new_active_range.partition,
            );
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn another_range_is_active() {
            let mut ext = ExtBuilder::build_default()
                .with_validators()
                .with_genesis_config()
                .as_externality();
            ext.execute_with(|| {
                init_active_range();
                let context: Context = Default::default();
                Context::next_range();

                assert_noop!(
                    context.submit_events_partition(0),
                    Error::<TestRuntime>::NonActiveEthereumRange,
                );
            });
        }

        #[test]
        fn author_has_voted_the_partition() {
            let mut ext = ExtBuilder::build_default()
                .with_validators()
                .with_genesis_config()
                .as_externality();
            ext.execute_with(|| {
                init_active_range();
                let context: Context = Default::default();

                assert_ok!(context.submit_events_partition(0));

                // Resubmit vote
                assert_noop!(
                    context.submit_events_partition(0),
                    Error::<TestRuntime>::EventVoteExists,
                );
            });
        }

        #[test]
        fn author_has_voted_another_partition() {
            let mut ext = ExtBuilder::build_default()
                .with_validators()
                .with_genesis_config()
                .as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                init_active_range();
                let other_context = Context {
                    discovered_events: alternative_event_data_set(),
                    ..Default::default()
                };

                assert_ok!(other_context.submit_events_partition(0));

                // try to submit another vote
                assert_noop!(
                    context.submit_events_partition(0),
                    Error::<TestRuntime>::EventVoteExists,
                );
            });
        }
    }
}

#[derive(Clone)]
pub struct LatestEthBlockContext {
    pub discovered_block: u32,
    pub author: Author<TestRuntime>,
    pub eth_start_block: u32,
}

impl Default for LatestEthBlockContext {
    fn default() -> Self {
        let discovered_block = 100;
        let primary_validator_id = 1;
        let author = Author::<TestRuntime> {
            key: UintAuthorityId(primary_validator_id),
            account_id: primary_validator_id,
        };
        let eth_start_block = events_helpers::compute_start_block_from_finalised_block_number(
            discovered_block,
            EthBlockRangeSize::<TestRuntime>::get(),
        )
        .expect("set on genesis");

        Self { author, discovered_block, eth_start_block }
    }
}

impl LatestEthBlockContext {
    fn submit_latest_block(&self) -> DispatchResultWithPostInfo {
        EthBridge::submit_latest_ethereum_block(
            RuntimeOrigin::none(),
            self.author.clone(),
            self.discovered_block,
            self.generate_signature(),
        )
    }

    fn generate_signature(&self) -> TestSignature {
        self.author
            .key
            .sign(&encode_eth_event_submission_data(
                Some(&Instance::<TestRuntime, ()>::get()),
                &SUBMIT_LATEST_ETH_BLOCK_CONTEXT,
                &self.author.account_id,
                self.discovered_block,
            ))
            .expect("Signature is signed")
    }
}

mod initial_range_consensus {

    use super::{LatestEthBlockContext as Context, *};
    use frame_support::{assert_noop, assert_ok};
    use sp_runtime::traits::Saturating;

    fn get_votes_for_initial_range(eth_block_num: &u32) -> usize {
        let mut votes_count = 0usize;
        SubmittedEthBlocks::<TestRuntime>::iter().for_each(|(submitted_block_num, _votes)| {
            if *eth_block_num == submitted_block_num {
                votes_count.saturating_inc();
            }
        });
        votes_count
    }

    #[test]
    fn adds_latest_block_successfully() {
        let mut ext =
            ExtBuilder::build_default().with_genesis_config().with_validators().as_externality();
        ext.execute_with(|| {
            let context: Context = Default::default();

            assert_ok!(context.submit_latest_block());

            assert_eq!(
                get_votes_for_initial_range(&context.eth_start_block),
                1,
                "Should be a single vote."
            );
        });
    }

    #[test]
    fn finalises_initial_range() {
        let mut ext =
            ExtBuilder::build_default().with_genesis_config().with_validators().as_externality();
        ext.execute_with(|| {
            let contexts = (1..5 as u64)
                .map(|id| Context {
                    author: Author::<TestRuntime> { key: UintAuthorityId(id), account_id: id },
                    discovered_block: id as u32 * 100,
                    eth_start_block: 1,
                })
                .take(<TestRuntime as crate::Config>::Quorum::get_supermajority_quorum() as usize)
                .collect::<Vec<Context>>();

            // Cast all votes
            for context in contexts.iter() {
                assert_ok!(context.submit_latest_block());
            }

            let active_range = EthBridge::active_ethereum_range().expect("Should be set");
            // Given that the submitted blocks are [100,200,300,400] the expected consensus
            assert_eq!(
                active_range.range,
                EthBlockRange {
                    start_block: events_helpers::compute_start_block_from_finalised_block_number(
                        300,
                        EthBlockRangeSize::<TestRuntime>::get()
                    )
                    .expect("set on genesis"),
                    length: EthBlockRangeSize::<TestRuntime>::get()
                }
            );
            // Ensure that cleanup has occured
            for context in contexts.iter() {
                assert_eq!(
                    get_votes_for_initial_range(&context.eth_start_block),
                    0,
                    "Should be no votes."
                );
            }
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn a_range_is_active() {
            let mut ext = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .as_externality();
            ext.execute_with(|| {
                init_active_range();
                let context: Context = Default::default();

                assert_noop!(context.submit_latest_block(), Error::<TestRuntime>::VotingEnded,);
            });
        }

        #[test]
        fn author_has_voted_the_partition() {
            let mut ext = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();

                assert_ok!(context.submit_latest_block());

                // Attempt to resubmit vote
                assert_noop!(context.submit_latest_block(), Error::<TestRuntime>::EventVoteExists,);
            });
        }

        #[test]
        fn author_has_voted_another_block() {
            let mut ext = ExtBuilder::build_default()
                .with_genesis_config()
                .with_validators()
                .as_externality();
            ext.execute_with(|| {
                let context: Context = Default::default();
                let other_context = Context {
                    discovered_block: context.discovered_block.saturating_add(1),
                    ..Default::default()
                };

                assert_ok!(other_context.submit_latest_block());

                // try to submit another vote
                assert_noop!(context.submit_latest_block(), Error::<TestRuntime>::EventVoteExists,);
            });
        }
    }
}
