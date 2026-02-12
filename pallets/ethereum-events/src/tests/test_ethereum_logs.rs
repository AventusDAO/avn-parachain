// Copyright 2021 Aventus (UK) Ltd.

#![cfg(test)]

use crate::{
    mock::{RuntimeOrigin, *},
    *,
};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_avn_common::event_types::{EthEventId, ValidEvents};
use sp_core::hash::H256;
use sp_runtime::traits::BadOrigin;

mod test_add_ethereum_log {
    use super::*;

    struct Context {
        origin: RuntimeOrigin,
        tx_hash: H256,
        nft_event_type: ValidEvents,
        current_block: BlockNumber,
        current_ingress_counter: IngressCounter,
        expected_ingress_counter: IngressCounter,
    }

    impl Default for Context {
        fn default() -> Self {
            Context {
                origin: RuntimeOrigin::signed(account_id_0()),
                tx_hash: H256::from([5u8; 32]),
                nft_event_type: ValidEvents::NftMint,
                current_block: 1,
                current_ingress_counter: EthereumEvents::ingress_counter(),
                expected_ingress_counter: EthereumEvents::ingress_counter() + 1,
            }
        }
    }

    impl Context {
        fn create_ethereum_event_id(&self) -> EthEventId {
            return EthEventId {
                signature: self.nft_event_type.signature(),
                transaction_hash: self.tx_hash,
            };
        }

        fn insert_to_unchecked_events(&self) {
            EthereumEvents::insert_to_unchecked_events(
                &EthEventId {
                    signature: self.nft_event_type.signature(),
                    transaction_hash: self.tx_hash.clone(),
                },
                self.current_ingress_counter,
            );
        }

        fn dispatch_add_ethereum_log(&self) -> DispatchResult {
            return EthereumEvents::add_ethereum_log(
                self.origin.clone(),
                self.nft_event_type.clone(),
                self.tx_hash,
            );
        }
    }

    fn perform_success_case(event_type: ValidEvents) {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let context: Context = Context { nft_event_type: event_type, ..Default::default() };

            assert_ok!(EthereumEvents::add_ethereum_log(
                context.origin.clone(),
                context.nft_event_type.clone(),
                context.tx_hash
            ));
            let ethereum_event = context.create_ethereum_event_id();

            assert_eq!(1, EthereumEvents::unchecked_events().len());
            assert_ne!(context.current_ingress_counter, EthereumEvents::ingress_counter());
            assert_eq!(
                true,
                EthereumEvents::unchecked_events().contains(&(
                    ethereum_event.clone(),
                    context.expected_ingress_counter,
                    context.current_block
                ))
            );

            let event;
            if context.nft_event_type.is_nft_event() {
                event = RuntimeEvent::EthereumEvents(
                    crate::Event::<TestRuntime>::NftEthereumEventAdded {
                        eth_event_id: ethereum_event.clone(),
                        account_id: account_id_0(),
                    },
                );
            } else {
                event =
                    RuntimeEvent::EthereumEvents(crate::Event::<TestRuntime>::EthereumEventAdded {
                        eth_event_id: ethereum_event.clone(),
                        added_by: account_id_0(),
                        t1_contract_address: AVN::<TestRuntime>::get_bridge_contract_address(),
                    });
            }

            assert!(EthereumEvents::event_emitted(&event));
            assert_eq!(1, System::events().len());
        });
    }

    mod success_cases {
        use super::*;
        #[test]
        fn add_validator_log() {
            perform_success_case(ValidEvents::AddedValidator);
        }

        #[test]
        fn add_lift_log() {
            perform_success_case(ValidEvents::Lifted);
        }

        #[test]
        fn add_nft_mint_log() {
            perform_success_case(ValidEvents::NftMint);
        }

        #[test]
        fn add_nft_transfer_log() {
            perform_success_case(ValidEvents::NftTransferTo);
        }

        #[test]
        fn add_nft_cancel_listing_log() {
            perform_success_case(ValidEvents::NftCancelListing);
        }

        #[test]
        fn add_cancel_batch_listing_log() {
            perform_success_case(ValidEvents::NftEndBatchListing);
        }
    }

    mod successive_events {
        use super::*;

        fn create_two_successive_events(
            signer: AccountId,
            first_event_type: ValidEvents,
            second_event_type: ValidEvents,
        ) -> (EthEventId, EthEventId) {
            let first_tx_hash: H256 = H256::from([1u8; 32]);
            let second_tx_hash: H256 = H256::from([2u8; 32]);

            assert_ok!(EthereumEvents::add_ethereum_log(
                RuntimeOrigin::signed(signer),
                first_event_type.clone(),
                first_tx_hash
            ));
            let validator_event_1 = EthEventId {
                signature: first_event_type.signature(),
                transaction_hash: first_tx_hash,
            };

            assert_ok!(EthereumEvents::add_ethereum_log(
                RuntimeOrigin::signed(signer),
                second_event_type.clone(),
                second_tx_hash
            ));
            let validator_event_2 = EthEventId {
                signature: second_event_type.signature(),
                transaction_hash: second_tx_hash,
            };

            return (validator_event_1, validator_event_2);
        }

        #[test]
        fn have_consecutive_ingress_counters() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                EthereumEvents::set_ingress_counter(DEFAULT_INGRESS_COUNTER);
                let (validator_event_1, validator_event_2) = create_two_successive_events(
                    account_id_0(),
                    ValidEvents::NftMint,
                    ValidEvents::NftMint,
                );

                assert_eq!(EthereumEvents::unchecked_events().len(), 2);
                assert!(EthereumEvents::unchecked_events().contains(&(
                    validator_event_1.clone(),
                    DEFAULT_INGRESS_COUNTER + 1,
                    1
                )));
                assert!(EthereumEvents::unchecked_events().contains(&(
                    validator_event_2.clone(),
                    DEFAULT_INGRESS_COUNTER + 2,
                    1
                )));
            });
        }

        #[test]
        fn generate_several_system_events() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                EthereumEvents::set_ingress_counter(DEFAULT_INGRESS_COUNTER);
                let (validator_event_1, validator_event_2) = create_two_successive_events(
                    account_id_0(),
                    ValidEvents::NftCancelListing,
                    ValidEvents::NftCancelListing,
                );

                let event = RuntimeEvent::EthereumEvents(
                    crate::Event::<TestRuntime>::NftEthereumEventAdded {
                        eth_event_id: validator_event_1.clone(),
                        account_id: account_id_0(),
                    },
                );
                assert!(EthereumEvents::event_emitted(&event));

                let event = RuntimeEvent::EthereumEvents(
                    crate::Event::<TestRuntime>::NftEthereumEventAdded {
                        eth_event_id: validator_event_2.clone(),
                        account_id: account_id_0(),
                    },
                );
                assert!(EthereumEvents::event_emitted(&event));

                assert_eq!(2, System::events().len());
            });
        }
    }

    #[test]
    fn zero_hash_should_fail() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let invalid_context: Context = Context { tx_hash: H256::zero(), ..Default::default() };

            assert_noop!(
                EthereumEvents::add_ethereum_log(
                    invalid_context.origin,
                    ValidEvents::NftMint,
                    invalid_context.tx_hash
                ),
                Error::<TestRuntime>::MalformedHash
            );
            // Ensure no events were emitted in avn
            assert_eq!(0, System::events().len());
        });
    }

    #[test]
    fn duplicate_event_should_fail() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();

        ext.execute_with(|| {
            let context: Context = Default::default();

            context.insert_to_unchecked_events();

            assert_noop!(context.dispatch_add_ethereum_log(), Error::<TestRuntime>::DuplicateEvent);
            // Ensure no events were emitted in avn
            assert_eq!(0, System::events().len());
        });
    }

    #[test]
    fn unsigned_origin_should_fail() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let invalid_context: Context =
                Context { origin: RawOrigin::None.into(), ..Default::default() };
            assert_eq!(0, System::events().len());
            assert_noop!(invalid_context.dispatch_add_ethereum_log(), BadOrigin);
            // Ensure no events were emitted in avn
            assert_eq!(0, System::events().len());
        });
    }

    #[test]
    fn filtered_event_should_not_be_added() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let context: Context = Default::default();
            assert_noop!(
                EthereumEvents::add_ethereum_log(
                    context.origin.clone(),
                    ValidEvents::AvtLowerClaimed,
                    context.tx_hash
                ),
                Error::<TestRuntime>::ErrorAddingEthereumLog
            );
            // Ensure no events were emitted in avn
            assert_eq!(0, System::events().len());
        });
    }
}

mod add_event {
    use super::*;

    #[test]
    fn valid_lift() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let account_id = account_id_1();
            let tx_lift_hash: H256 = H256::random();
            assert_ok!(EthereumEvents::add_event(
                ValidEvents::Lifted,
                tx_lift_hash.clone(),
                account_id
            ));
            let lift_event = EthEventId {
                signature: ValidEvents::Lifted.signature(),
                transaction_hash: tx_lift_hash,
            };
            // Check that the event is added to unchecked queue
            assert_eq!(EthereumEvents::unchecked_events().len(), 1);
            assert!(EthereumEvents::unchecked_events().contains(&(
                lift_event.clone(),
                FIRST_INGRESS_COUNTER,
                1
            )));
            // Check that the event is deposited with correct data

            let event =
                RuntimeEvent::EthereumEvents(crate::Event::<TestRuntime>::EthereumEventAdded {
                    eth_event_id: lift_event.clone(),
                    added_by: account_id,
                    t1_contract_address: AVN::<TestRuntime>::get_bridge_contract_address(),
                });
            assert!(EthereumEvents::event_emitted(&event));
            assert_eq!(1, System::events().len());
        });
    }

    #[test]
    fn valid_validator() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let tx_validator_hash: H256 = H256::random();
            let account_id = account_id_1();
            assert_ok!(EthereumEvents::add_event(
                ValidEvents::AddedValidator,
                tx_validator_hash.clone(),
                account_id
            ));
            let validator_event = EthEventId {
                signature: ValidEvents::AddedValidator.signature(),
                transaction_hash: tx_validator_hash,
            };
            // Check that the event is added to unchecked queue
            assert_eq!(EthereumEvents::unchecked_events().len(), 1);
            assert!(EthereumEvents::unchecked_events().contains(&(
                validator_event.clone(),
                FIRST_INGRESS_COUNTER,
                1
            )));
            // Check that the event is deposited with correct data
            let event =
                RuntimeEvent::EthereumEvents(crate::Event::<TestRuntime>::EthereumEventAdded {
                    eth_event_id: validator_event.clone(),
                    added_by: account_id,
                    t1_contract_address: AVN::<TestRuntime>::get_bridge_contract_address(),
                });
            assert!(EthereumEvents::event_emitted(&event));
            assert_eq!(1, System::events().len());
        });
    }

    #[test]
    fn existing_event_should_fail() {
        let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
        ext.execute_with(|| {
            let account_id = account_id_1();
            let event_id = EthEventId {
                signature: ValidEvents::AddedValidator.signature(),
                transaction_hash: H256::random(),
            };
            EthereumEvents::insert_to_unchecked_events(&event_id, DEFAULT_INGRESS_COUNTER);
            assert_noop!(
                EthereumEvents::add_event(
                    ValidEvents::AddedValidator,
                    event_id.transaction_hash,
                    account_id
                ),
                Error::<TestRuntime>::DuplicateEvent
            );
            // Ensure no events were emitted in avn
            assert_eq!(System::events(), vec![]);
        });
    }
    // TODO [TYPE: test][PRI: medium]: add_event and check for vector overflow (too many events)
}
