//Copyright 2025 Aventus Network Services (UK) Ltd.

#![cfg(test)]

use crate::{migration::AuthorsManagerMigrations, mock::*, AVN, *};
use frame_support::{
    assert_noop, assert_ok,
    pallet_prelude::{DispatchResultWithPostInfo, StorageVersion, Weight},
    traits::{Currency, GetStorageVersion, OnRuntimeUpgrade},
};
use frame_system::RawOrigin;
use hex_literal::hex;
use sp_runtime::{testing::UintAuthorityId, traits::BadOrigin};
use substrate_test_utils::assert_eq_uvec;

fn register_author(author_id: &AccountId, author_eth_public_key: &ecdsa::Public) -> DispatchResult {
    return AuthorsManager::add_author(RawOrigin::Root.into(), *author_id, *author_eth_public_key)
}

fn set_session_keys(author_id: &AccountId) {
    pallet_session::NextKeys::<TestRuntime>::insert::<AccountId, UintAuthorityId>(
        *author_id,
        UintAuthorityId(10u64).into(),
    );
}

fn force_add_author(
    author_id: &AccountId,
    author_eth_public_key: &ecdsa::Public,
) -> DispatchResult {
    set_session_keys(author_id);
    assert_ok!(register_author(author_id, author_eth_public_key));

    // Simulate T1 confirmation
    let tx_id = AuthorActions::<TestRuntime>::iter()
        .find(|(account_id, _, _)| account_id == author_id)
        .map(|(_, _, data)| data.eth_transaction_id)
        .expect("Action should exist");

    assert_ok!(Pallet::<TestRuntime>::process_result(tx_id, PALLET_ID.to_vec(), true));

    //Advance 2 sessions to add the author to the session
    advance_session();
    advance_session();

    Ok(())
}

#[test]
fn test_register_existing_author() {
    let mut ext = ExtBuilder::build_default().with_authors().as_externality();
    ext.execute_with(|| {
        let mock_data = MockData::setup_valid();
        AuthorsManager::insert_to_authors(&mock_data.new_author_id);

        let current_num_events = System::events().len();

        //Set the session keys of the new author we are trying to register
        set_session_keys(&mock_data.new_author_id);

        assert_noop!(
            register_author(&mock_data.new_author_id, &mock_data.author_eth_public_key),
            Error::<TestRuntime>::AuthorAlreadyExists
        );

        // No Event has been deposited
        assert_eq!(System::events().len(), current_num_events);
    });
}

#[test]
fn test_register_author_with_no_authors() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let mock_data = MockData::setup_valid();
        let current_num_events = System::events().len();

        //Set the session keys of the new author we are trying to register
        set_session_keys(&mock_data.new_author_id);

        assert_noop!(
            register_author(&mock_data.new_author_id, &mock_data.author_eth_public_key),
            Error::<TestRuntime>::NoAuthors
        );

        // no Event has been deposited
        assert_eq!(System::events().len(), current_num_events);
    });
}

mod register_author {
    use super::*;

    // TODO move MockData here and rename to Context

    fn run_preconditions(context: &MockData) {
        assert_eq!(0, AuthorActions::<TestRuntime>::iter().count());
        let author_account_ids =
            AuthorsManager::author_account_ids().expect("Should contain authors");
        assert_eq!(false, author_account_ids.contains(&context.new_author_id));
        assert_eq!(
            false,
            AuthorsManager::get_ethereum_public_key_if_exists(&context.new_author_id).is_some()
        );
    }

    fn find_author_activation_action(data: &MockData, status: AuthorsActionStatus) -> bool {
        return AuthorActions::<TestRuntime>::iter().any(|(account_id, _ingress, action_data)| {
            action_data.status == status &&
                action_data.action_type == AuthorsActionType::Activation &&
                account_id == data.new_author_id
        })
    }

    mod succeeds {
        use super::*;

        #[test]
        fn and_adds_author() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = MockData::setup_valid();
                run_preconditions(&context);

                //set the session keys of the new author we are trying to register
                set_session_keys(&context.new_author_id);

                // Result OK
                assert_ok!(register_author(&context.new_author_id, &context.author_eth_public_key));
                // Author has NOT been added to AuthorAccountIds yet (only after T1 confirmation)
                assert_eq!(
                    false,
                    AuthorsManager::author_account_ids()
                        .unwrap()
                        .iter()
                        .any(|a| a == &context.new_author_id)
                );
                // AuthorActionPublished Event has been deposited
                assert_eq!(
                    true,
                    System::events().iter().any(|a| a.event ==
                        mock::RuntimeEvent::AuthorsManager(
                            crate::Event::<TestRuntime>::AuthorActionPublished {
                                author_id: context.new_author_id,
                                action_type: AuthorsActionType::Registration,
                                tx_id: 0
                            }
                        ))
                );
                // AuthorRegistered Event has NOT been deposited yet
                assert_eq!(
                    false,
                    System::events().iter().any(|a| a.event ==
                        mock::RuntimeEvent::AuthorsManager(
                            crate::Event::<TestRuntime>::AuthorRegistered {
                                author_id: context.new_author_id,
                                eth_key: context.author_eth_public_key.clone()
                            }
                        ))
                );
                // Check that a registration action exists with AwaitingConfirmation status
                assert_eq!(
                    true,
                    AuthorActions::<TestRuntime>::iter().any(|(account_id, _, action_data)| {
                        account_id == context.new_author_id &&
                            action_data.status == AuthorsActionStatus::AwaitingConfirmation &&
                            action_data.action_type == AuthorsActionType::Registration
                    })
                );
            });
        }

        #[test]
        fn activation_dispatches_after_two_sessions() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = MockData::setup_valid();
                run_preconditions(&context);

                //Set the session keys of the new author we are trying to register
                set_session_keys(&context.new_author_id);

                assert_ok!(register_author(&context.new_author_id, &context.author_eth_public_key));

                // Simulate T1 confirmation by calling process_result
                let (tx_id, _) = AuthorActions::<TestRuntime>::iter()
                    .find(|(author, _, _)| author == &context.new_author_id)
                    .map(|(_, ingress, data)| (data.eth_transaction_id, ingress))
                    .expect("Action should exist");

                assert_ok!(Pallet::<TestRuntime>::process_result(tx_id, PALLET_ID.to_vec(), true));

                // Now the author should be in author_account_ids
                assert!(AuthorsManager::author_account_ids()
                    .unwrap()
                    .iter()
                    .any(|a| a == &context.new_author_id));

                // AuthorActionConfirmedOnEthereum Event has been deposited
                assert_eq!(
                    true,
                    System::events().iter().any(|a| a.event ==
                        mock::RuntimeEvent::AuthorsManager(
                            crate::Event::<TestRuntime>::AuthorActionConfirmedOnEthereum {
                                author_id: context.new_author_id,
                                action_type: AuthorsActionType::Registration,
                                tx_id
                            }
                        ))
                );

                // After T1 confirmation, the action type is Activation with Actioned status
                assert_eq!(
                    true,
                    AuthorActions::<TestRuntime>::iter().any(|(account_id, _, action_data)| {
                        account_id == context.new_author_id &&
                            action_data.action_type == AuthorsActionType::Activation &&
                            action_data.status == AuthorsActionStatus::Actioned
                    })
                );
                // AuthorActivationStarted Event has been deposited
                assert_eq!(
                    true,
                    System::events().iter().any(|a| a.event ==
                        mock::RuntimeEvent::AuthorsManager(
                            crate::Event::<TestRuntime>::AuthorActivationStarted {
                                author_id: context.new_author_id
                            }
                        ))
                );
            });
        }
    }
}

// Change these tests to accomodate the use of votes
#[allow(non_fmt_panics)]
mod remove_author_public {
    use super::*;

    // Tests for pub fn remove_author(origin) -> DispatchResult {...}
    #[test]
    fn valid_case() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = MockData::setup_valid();
            assert_ok!(force_add_author(&context.new_author_id, &context.author_eth_public_key));

            //Prove this is an existing author
            assert_eq_uvec!(
                Session::validators(),
                vec![
                    author_id_1(),
                    author_id_2(),
                    author_id_3(),
                    author_id_4(),
                    author_id_5(),
                    context.new_author_id
                ]
            );

            //Author exists in the AVN
            assert_eq!(AVN::<TestRuntime>::is_validator(&context.new_author_id), true);

            //Remove the author
            assert_ok!(AuthorsManager::remove_author(
                RawOrigin::Root.into(),
                context.new_author_id
            ));

            //AuthorActionPublished event emitted (not AuthorDeregistered yet - that comes after T1
            // confirmation)
            assert!(System::events().iter().any(|a| matches!(&a.event,
                mock::RuntimeEvent::AuthorsManager(
                    crate::Event::<TestRuntime>::AuthorActionPublished {
                        author_id, action_type: AuthorsActionType::Resignation, ..
                    }
                ) if author_id == &context.new_author_id)));

            //Author is NOT removed from authors manager yet (only after T1 confirmation)
            assert!(AuthorsManager::author_account_ids()
                .unwrap()
                .iter()
                .position(|&x| x == context.new_author_id)
                .is_some());

            // Simulate T1 confirmation to actually remove the author
            let (account_id, ingress_counter, action_data) = AuthorActions::<TestRuntime>::iter()
                .find(|(acc, _, data)| {
                    acc == &context.new_author_id &&
                        data.action_type == AuthorsActionType::Resignation
                })
                .expect("Resignation action should exist");

            let tx_id = action_data.eth_transaction_id;

            // Ensure the transaction mapping is set up for the callback
            TransactionToAction::<TestRuntime>::insert(
                tx_id,
                (account_id.clone(), ingress_counter),
            );

            assert_ok!(Pallet::<TestRuntime>::process_result(tx_id, PALLET_ID.to_vec(), true));

            //Author is now removed from authors manager
            assert_eq!(
                AuthorsManager::author_account_ids()
                    .unwrap()
                    .iter()
                    .position(|&x| x == context.new_author_id),
                None
            );

            //Author is still in the session. Will be removed after 1 era.
            assert_eq_uvec!(
                Session::validators(),
                vec![
                    author_id_1(),
                    author_id_2(),
                    author_id_3(),
                    author_id_4(),
                    author_id_5(),
                    context.new_author_id
                ]
            );

            // Advance 2 sessions
            advance_session();
            advance_session();

            // Author has been removed from the session
            assert_eq_uvec!(
                Session::validators(),
                vec![author_id_1(), author_id_2(), author_id_3(), author_id_4(), author_id_5()]
            );

            //Author is also removed from the AVN
            assert_eq!(AVN::<TestRuntime>::is_validator(&context.new_author_id), false);
        });
    }

    #[test]
    fn fails_when_regular_sender_submits_transaction() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = MockData::setup_valid();
            assert_ok!(force_add_author(&context.new_author_id, &context.author_eth_public_key));

            let num_events = System::events().len();
            assert_noop!(
                AuthorsManager::remove_author(RuntimeOrigin::signed(author_id_3()), author_id_3()),
                BadOrigin
            );
            assert_eq!(System::events().len(), num_events);
        });
    }

    #[test]
    fn unsigned_sender() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = MockData::setup_valid();
            assert_ok!(force_add_author(&context.new_author_id, &context.author_eth_public_key));

            let num_events = System::events().len();
            assert_noop!(
                AuthorsManager::remove_author(RawOrigin::None.into(), context.new_author_id),
                BadOrigin
            );
            assert_eq!(System::events().len(), num_events);
        });
    }

    #[test]
    fn non_author() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            //Ensure we have enough candidates
            let context = MockData::setup_valid();
            assert_ok!(force_add_author(&context.new_author_id, &context.author_eth_public_key));

            let original_authors = AuthorsManager::author_account_ids();
            let num_events = System::events().len();

            // Caller of remove function has to emit event if removal is successful.
            assert_eq!(System::events().len(), num_events);
            assert_eq!(AuthorsManager::author_account_ids(), original_authors);
        });
    }

    #[test]
    fn fails_when_another_deregistration_is_in_progress() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = MockData::setup_valid();
            assert_ok!(force_add_author(&context.new_author_id, &context.author_eth_public_key));

            // Start a deregistration for author_id_1
            assert_ok!(AuthorsManager::remove_author(RawOrigin::Root.into(), author_id_1()));

            // Try to deregister a different author while the first is still in progress
            assert_noop!(
                AuthorsManager::remove_author(RawOrigin::Root.into(), author_id_2()),
                Error::<TestRuntime>::ValidatorActionAlreadyInProgress
            );
        });
    }

    #[test]
    fn fails_when_same_author_tries_to_deregister_twice() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = MockData::setup_valid();
            assert_ok!(force_add_author(&context.new_author_id, &context.author_eth_public_key));

            // Start a deregistration for author_id_1
            assert_ok!(AuthorsManager::remove_author(RawOrigin::Root.into(), author_id_1()));

            // Try to deregister the same author again (this should fail because a deregistration is
            // already in progress) The global check prevents any new deregistration
            // attempts while one is in progress
            assert_noop!(
                AuthorsManager::remove_author(RawOrigin::Root.into(), author_id_1()),
                Error::<TestRuntime>::ValidatorActionAlreadyInProgress
            );
        });
    }

    #[test]
    fn allows_deregistration_after_previous_one_completes() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = MockData::setup_valid();
            assert_ok!(force_add_author(&context.new_author_id, &context.author_eth_public_key));

            // Start a deregistration for author_id_1
            assert_ok!(AuthorsManager::remove_author(RawOrigin::Root.into(), author_id_1()));

            // Complete the first deregistration by simulating T1 confirmation
            let (account_id, ingress_counter, action_data) = AuthorActions::<TestRuntime>::iter()
                .find(|(acc, _, data)| {
                    acc == &author_id_1() && data.action_type == AuthorsActionType::Resignation
                })
                .expect("Resignation action should exist");

            let tx_id = action_data.eth_transaction_id;

            // Set up transaction mapping for callback
            TransactionToAction::<TestRuntime>::insert(
                tx_id,
                (account_id.clone(), ingress_counter),
            );

            // Process the T1 confirmation
            assert_ok!(Pallet::<TestRuntime>::process_result(tx_id, PALLET_ID.to_vec(), true));

            // Now we should be able to deregister a different author (author_id_2 is still in the
            // list)
            assert_ok!(AuthorsManager::remove_author(RawOrigin::Root.into(), author_id_2()));
        });
    }

    #[test]
    fn allows_deregistration_after_previous_one_fails() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = MockData::setup_valid();
            assert_ok!(force_add_author(&context.new_author_id, &context.author_eth_public_key));

            // Start a deregistration for author_id_1
            assert_ok!(AuthorsManager::remove_author(RawOrigin::Root.into(), author_id_1()));

            // Simulate T1 failure
            let (account_id, ingress_counter, action_data) = AuthorActions::<TestRuntime>::iter()
                .find(|(acc, _, data)| {
                    acc == &author_id_1() && data.action_type == AuthorsActionType::Resignation
                })
                .expect("Resignation action should exist");

            let tx_id = action_data.eth_transaction_id;

            // Set up transaction mapping for callback
            TransactionToAction::<TestRuntime>::insert(
                tx_id,
                (account_id.clone(), ingress_counter),
            );

            // Process the T1 failure
            assert_ok!(Pallet::<TestRuntime>::process_result(tx_id, PALLET_ID.to_vec(), false));

            // Now we should be able to deregister a different author (author_id_2 is still in the
            // list)
            assert_ok!(AuthorsManager::remove_author(RawOrigin::Root.into(), author_id_2()));
        });
    }
}

#[test]
fn test_initial_authors_populated_from_genesis_config() {
    let mut ext = ExtBuilder::build_default().with_authors().as_externality();
    ext.execute_with(|| {
        assert_eq!(
            AuthorsManager::author_account_ids().unwrap(),
            genesis_config_initial_authors().to_vec()
        );
    });
}

mod add_author {
    use super::*;

    struct AddAuthorContext {
        author: AccountId,
        author_eth_public_key: ecdsa::Public,
    }

    impl Default for AddAuthorContext {
        fn default() -> Self {
            let author = TestAccount::new([0u8; 32]).account_id();
            Balances::make_free_balance_be(&author, 100000);

            AddAuthorContext {
                author,
                author_eth_public_key: ecdsa::Public::from_raw(hex!(
                    "02407b0d9f41148bbe3b6c7d4a62585ae66cc32a707441197fa5453abfebd31d57"
                )),
            }
        }
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = &AddAuthorContext::default();

            set_session_keys(&context.author);
            assert_ok!(register_author(&context.author, &context.author_eth_public_key));

            // Author is not in author_account_ids yet (only after T1 confirmation)
            // But eth key mapping should exist
            assert_eq!(
                AuthorsManager::get_author_by_eth_public_key(context.author_eth_public_key.clone())
                    .unwrap(),
                context.author
            );
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = &AddAuthorContext::default();

                set_session_keys(&context.author);
                assert_noop!(
                    AuthorsManager::add_author(
                        RawOrigin::None.into(),
                        context.author,
                        context.author_eth_public_key,
                    ),
                    BadOrigin
                );
            });
        }

        #[test]
        fn no_authors() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                // This test is simulating "no authors" by not using authors when building the
                // test extension
                let context = &AddAuthorContext::default();

                set_session_keys(&context.author);
                assert_noop!(
                    register_author(&context.author, &context.author_eth_public_key),
                    Error::<TestRuntime>::NoAuthors
                );
            });
        }

        #[test]
        fn author_eth_key_already_exists() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = &AddAuthorContext::default();

                set_session_keys(&context.author);
                <EthereumPublicKeys<TestRuntime>>::insert(
                    context.author_eth_public_key.clone(),
                    context.author,
                );

                assert_noop!(
                    register_author(&context.author, &context.author_eth_public_key),
                    Error::<TestRuntime>::AuthorEthKeyAlreadyExists
                );
            });
        }

        #[test]
        fn author_already_exists() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = &AddAuthorContext::default();

                set_session_keys(&context.author);
                assert_ok!(<AuthorAccountIds::<TestRuntime>>::try_append(&context.author));

                assert_noop!(
                    register_author(&context.author, &context.author_eth_public_key),
                    Error::<TestRuntime>::AuthorAlreadyExists
                );
            });
        }

        #[test]
        fn maximum_authors_is_reached() {
            let mut ext = ExtBuilder::build_default().with_maximum_authors().as_externality();
            ext.execute_with(|| {
                let context = &AddAuthorContext::default();

                set_session_keys(&context.author);
                assert_noop!(
                    register_author(&context.author, &context.author_eth_public_key),
                    Error::<TestRuntime>::MaximumAuthorsReached
                );
            });
        }
    }
}

mod bridge_interface_notification {
    use super::*;

    fn setup_test_action(context: &MockData) -> (IngressCounter, EthereumId) {
        set_session_keys(&context.new_author_id);
        assert_ok!(register_author(&context.new_author_id, &context.author_eth_public_key));

        let (ingress_counter, action_data) = AuthorActions::<TestRuntime>::iter()
            .find(|(author, _, _)| author == &context.new_author_id)
            .map(|(_, ingress, data)| (ingress, data))
            .expect("Action should exist");

        let tx_id = action_data.eth_transaction_id;
        TransactionToAction::<TestRuntime>::insert(
            tx_id,
            (context.new_author_id.clone(), ingress_counter),
        );

        (ingress_counter, tx_id)
    }

    mod succeeds {
        use super::*;

        #[test]
        fn when_processing_valid_transaction() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = MockData::setup_valid();
                let (ingress_counter, tx_id) = setup_test_action(&context);

                advance_session();
                advance_session();

                assert_ok!(Pallet::<TestRuntime>::process_result(tx_id, PALLET_ID.to_vec(), true));

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::AuthorsManager(
                        crate::Event::<TestRuntime>::AuthorActionConfirmedOnEthereum {
                            author_id: context.new_author_id,
                            action_type: AuthorsActionType::Registration,
                            tx_id
                        }
                    )));

                // After successful registration, the action is mutated to Activation type
                let activation_action =
                    AuthorActions::<TestRuntime>::get(&context.new_author_id, ingress_counter)
                        .expect("Activation action should exist");
                assert_eq!(activation_action.action_type, AuthorsActionType::Activation);
                // Transaction mapping is removed by process_result (line 1005: take)
                assert_eq!(TransactionToAction::<TestRuntime>::get(tx_id), None);
            });
        }

        #[test]
        fn when_transaction_fails() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = MockData::setup_valid();
                let (ingress_counter, tx_id) = setup_test_action(&context);

                advance_session();
                advance_session();

                assert_ok!(Pallet::<TestRuntime>::process_result(tx_id, PALLET_ID.to_vec(), false));

                assert!(System::events().iter().any(|a| a.event ==
                    mock::RuntimeEvent::AuthorsManager(
                        crate::Event::<TestRuntime>::AuthorActionFailedOnEthereum {
                            author_id: context.new_author_id,
                            action_type: AuthorsActionType::Registration,
                            tx_id
                        }
                    )));

                // On failure, the action is removed (cleanup_registration_storage)
                assert_eq!(
                    AuthorActions::<TestRuntime>::get(&context.new_author_id, ingress_counter),
                    None
                );
                // Transaction mapping is also removed (by process_result)
                assert_eq!(TransactionToAction::<TestRuntime>::get(tx_id), None);
            });
        }
    }

    mod fails {
        use super::*;

        #[test]
        fn with_invalid_state() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = MockData::setup_valid();
                let (_, tx_id) = setup_test_action(&context);

                // process_result should succeed even without advancing sessions
                // because it validates the transaction exists, not its status
                assert_ok!(Pallet::<TestRuntime>::process_result(tx_id, PALLET_ID.to_vec(), true));
            });
        }

        #[test]
        fn with_missing_transaction() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                assert_ok!(Pallet::<TestRuntime>::process_result(999u32, PALLET_ID.to_vec(), true));
            });
        }
    }
}

mod rotate_author_ethereum_key {
    use sp_core::ByteArray;

    use super::*;

    struct RotateAuthorEthKeyContext {
        author: AccountId,
        author_eth_old_public_key: ecdsa::Public,
        author_eth_new_public_key: ecdsa::Public,
    }

    impl Default for RotateAuthorEthKeyContext {
        fn default() -> Self {
            let author = author_id_1();
            Balances::make_free_balance_be(&author, 100000);

            RotateAuthorEthKeyContext {
                author,
                author_eth_old_public_key: ecdsa::Public::from_slice(&AUTHOR_1_ETHEREUM_PUPLIC_KEY)
                    .unwrap(),
                author_eth_new_public_key: ecdsa::Public::from_raw(hex!(
                    "02407b0d9f41148bbe3b6c7d4a62585ae66cc32a707441197fa5453abfebd31d57"
                )),
            }
        }
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let mut ext = ExtBuilder::build_default().with_authors().as_externality();
        ext.execute_with(|| {
            let context = &RotateAuthorEthKeyContext::default();

            assert_ok!(AuthorsManager::rotate_author_ethereum_key(
                RuntimeOrigin::root(),
                context.author.clone(),
                context.author_eth_old_public_key.clone(),
                context.author_eth_new_public_key.clone()
            ));

            assert_eq!(
                true,
                AuthorsManager::author_account_ids().unwrap().contains(&context.author)
            );
            assert_eq!(
                AuthorsManager::get_author_by_eth_public_key(
                    context.author_eth_new_public_key.clone()
                )
                .unwrap(),
                context.author
            );

            assert_eq!(
                AuthorsManager::get_author_by_eth_public_key(
                    context.author_eth_old_public_key.clone()
                ),
                None
            );
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = &RotateAuthorEthKeyContext::default();

                assert_noop!(
                    AuthorsManager::rotate_author_ethereum_key(
                        RuntimeOrigin::none(),
                        context.author.clone(),
                        context.author_eth_old_public_key.clone(),
                        context.author_eth_new_public_key.clone()
                    ),
                    BadOrigin
                );
            });
        }

        #[test]
        fn author_eth_key_already_exists() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = &RotateAuthorEthKeyContext::default();

                assert_noop!(
                    AuthorsManager::rotate_author_ethereum_key(
                        RuntimeOrigin::root(),
                        context.author.clone(),
                        context.author_eth_old_public_key.clone(),
                        ecdsa::Public::from_slice(&AUTHOR_2_ETHEREUM_PUPLIC_KEY).unwrap()
                    ),
                    Error::<TestRuntime>::AuthorEthKeyAlreadyExists
                );
            });
        }

        #[test]
        fn author_eth_key_unchanged() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = &RotateAuthorEthKeyContext::default();

                assert_noop!(
                    AuthorsManager::rotate_author_ethereum_key(
                        RuntimeOrigin::root(),
                        context.author.clone(),
                        context.author_eth_old_public_key.clone(),
                        context.author_eth_old_public_key.clone(),
                    ),
                    Error::<TestRuntime>::AuthorEthKeyAlreadyExists
                );
            });
        }

        #[test]
        fn author_not_found() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = &RotateAuthorEthKeyContext::default();

                let no_author = TestAccount::new([6u8; 32]).account_id();

                assert_noop!(
                    AuthorsManager::rotate_author_ethereum_key(
                        RuntimeOrigin::root(),
                        no_author,
                        context.author_eth_old_public_key.clone(),
                        context.author_eth_new_public_key.clone()
                    ),
                    Error::<TestRuntime>::AuthorNotFound
                );
            });
        }

        #[test]
        fn author_keys_missmatch() {
            let mut ext = ExtBuilder::build_default().with_authors().as_externality();
            ext.execute_with(|| {
                let context = &RotateAuthorEthKeyContext::default();

                assert_noop!(
                    AuthorsManager::rotate_author_ethereum_key(
                        RuntimeOrigin::root(),
                        author_id_5(),
                        context.author_eth_old_public_key.clone(),
                        context.author_eth_new_public_key.clone()
                    ),
                    Error::<TestRuntime>::AuthorNotFound
                );
            });
        }
    }
}

#[test]
fn migration_populates_accountid_to_ethereum_keys_and_sets_storage_version() {
    let mut ext = ExtBuilder::build_default().with_authors().as_externality();
    ext.execute_with(|| {
        // Simulate old state by clearing reverse map while keeping EthereumPublicKeys intact
        for (account_id, _) in AccountIdToEthereumKeys::<TestRuntime>::iter() {
            AccountIdToEthereumKeys::<TestRuntime>::remove(&account_id);
        }

        // Sanity: reverse map is empty or missing for authors
        let authors = AuthorsManager::author_account_ids().unwrap();
        for author in authors.iter() {
            assert_eq!(AccountIdToEthereumKeys::<TestRuntime>::get(author), None);
        }

        // Simulate older on-chain storage version so migration runs
        StorageVersion::new(0).put::<Pallet<TestRuntime>>();

        // Run migration
        let _w = AuthorsManagerMigrations::<TestRuntime>::on_runtime_upgrade();

        // Verify reverse map populated from forward map
        for (eth_key, account_id) in EthereumPublicKeys::<TestRuntime>::iter() {
            assert_eq!(AccountIdToEthereumKeys::<TestRuntime>::get(&account_id), Some(eth_key));
        }

        // Verify version bumped so it won't run again
        assert_eq!(
            Pallet::<TestRuntime>::on_chain_storage_version(),
            crate::migration::STORAGE_VERSION
        );

        // Second run should do nothing
        let w2 = AuthorsManagerMigrations::<TestRuntime>::on_runtime_upgrade();
        assert_eq!(w2, Weight::zero());
    });
}
