// Copyright 2022 Aventus Systems (UK) Ltd.

#![cfg(test)]

use crate::{mock::*, *};

pub type BlockNumber = BlockNumberFor<TestRuntime>;

mod initial_lifts {
    use super::*;

    struct Context {
        pub initial_lifts: Vec<(EthEventId, IngressCounter, BlockNumber)>,
    }
    impl Context {
        pub fn create() -> Self {
            let mut ingress_counter: IngressCounter = 0;
            let initial_lifts = INITIAL_LIFTS
                .iter()
                .map(|x| {
                    ingress_counter = ingress_counter + 1;
                    (
                        EthEventId {
                            signature: ValidEvents::Lifted.signature(),
                            transaction_hash: H256::from(x),
                        },
                        ingress_counter,
                        0,
                    )
                })
                .collect::<Vec<(EthEventId, IngressCounter, BlockNumber)>>();

            assert_eq!(INITIAL_LIFTS.len(), initial_lifts.len());

            return Context { initial_lifts };
        }
    }

    mod are_empty_when {
        use super::*;

        #[test]
        fn genesis_config_has_none() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let unchecked_events = EthereumEvents::unchecked_events();

                assert_eq!(unchecked_events.len(), 0);
            });
        }
    }

    mod are_populated_when {
        use super::*;

        #[test]
        fn genesis_config_has_some() {
            let mut ext =
                ExtBuilder::build_default().with_genesis_and_initial_lifts().as_externality();
            ext.execute_with(|| {
                let context = Context::create();
                let unchecked_events = EthereumEvents::unchecked_events();

                assert_eq!(unchecked_events.len(), context.initial_lifts.len());
                for i in 0..unchecked_events.len() {
                    assert_eq!(unchecked_events[i], context.initial_lifts[i]);
                }
            });
        }
    }
}

mod initial_processed_events {
    use super::*;
    struct Context {
        pub initial_processed_events: Vec<EthEventId>,
    }

    impl Context {
        pub fn create() -> Self {
            let initial_processed_events = create_initial_processed_events()
                .iter()
                .map(|(x, y, _)| EthEventId { signature: x.clone(), transaction_hash: y.clone() })
                .collect::<Vec<EthEventId>>();
            return Context { initial_processed_events };
        }
    }

    mod are_empty_when {
        use super::*;

        #[test]
        fn genesis_config_has_none() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                assert_eq!(<ProcessedEvents<TestRuntime>>::iter().count(), 0);
            });
        }
    }

    mod are_populated_when {
        use super::*;

        #[test]
        fn genesis_config_has_some() {
            let mut ext =
                ExtBuilder::build_default().with_genesis_and_initial_lifts().as_externality();
            ext.execute_with(|| {
                let context = Context::create();
                assert_eq!(
                    <ProcessedEvents<TestRuntime>>::iter().count(),
                    INITIAL_PROCESSED_EVENTS.len()
                );
                for event in context.initial_processed_events {
                    assert_eq!(<ProcessedEvents<TestRuntime>>::contains_key(event), true);
                }
            });
        }
    }
}
