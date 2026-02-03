// Copyright 2023 Aventus Network Systems (UK) Ltd.

#![cfg(test)]
use alloy_primitives::{Bytes, B256, U256};
use alloy_sol_types::{sol, SolCall};

use crate::{eth::generate_send_calldata, mock::*, request::*, *};
use frame_support::{
    assert_err, assert_noop, assert_ok, dispatch::DispatchResultWithPostInfo, error::BadOrigin,
};
use sp_avn_common::{eth::EthereumId, BridgeContractMethod};
use sp_runtime::{testing::UintAuthorityId, DispatchError};

const ROOT_HASH: &str = "30b83f0d722d1d4308ab4660a72dbaf0a7392d5674eca3cd21d57256d42df7a0";
const REWARDS: u128 = 500_000_000_000_000_000_000;
const AVG_STAKED: u128 = 1_000_000_000_000_000_000_000;
const PERIOD: u32 = 3;
const T2_PUB_KEY: &str = "14aeac90dbd3573458f9e029eb2de122ee94f2f0bc5ee4b6c6c5839894f1a547";
const T1_PUB_KEY: &str = "23d79f6492dddecb436333a5e7a4cfcc969f568e01283fa2964aae15327fb8a3b685a4d0f3ef9b3c2adb20f681dbc74b7f82c1cf8438d37f2c10e9c79591e9ea";

sol! {
    function addAuthor(
        bytes calldata t1PubKey,
        bytes32 t2PubKey,
        uint256 expiry,
        uint32 t2TxId,
        bytes calldata confirmations
    ) external;

    function removeAuthor(
        bytes32 t2PubKey,
        bytes t1PubKey,
        uint256 expiry,
        uint32 t2TxId,
        bytes confirmations
    ) external;

    function publishRoot(
        bytes32 rootHash,
        uint256 expiry,
        uint32 t2TxId,
        bytes calldata confirmations
    ) external;

    function triggerGrowth(
        uint256 rewards,
        uint256 avgStaked,
        uint32 period,
        uint256 expiry,
        uint32 t2TxId,
        bytes calldata confirmations
    );
}

fn corroborate_good_transactions(
    tx_id: EthereumId,
    author: &Validator<UintAuthorityId, AccountId>,
    context: &Context,
) -> DispatchResultWithPostInfo {
    EthBridge::add_corroboration(
        RuntimeOrigin::none(),
        tx_id,
        true,
        true,
        author.clone(),
        context.replay_attempt,
        context.test_signature.clone(),
    )
}

fn corroborate_bad_transactions(
    tx_id: EthereumId,
    author: &Validator<UintAuthorityId, AccountId>,
    context: &Context,
) {
    EthBridge::add_corroboration(
        RuntimeOrigin::none(),
        tx_id,
        false,
        false,
        author.clone(),
        context.replay_attempt,
        context.test_signature.clone(),
    )
    .unwrap();
}

#[test]
fn check_publish_root_encoding() {
    let function_name = BridgeContractMethod::PublishRoot.name_as_bytes().to_vec();
    let params = vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())];
    let expected_msg_hash = "4e6d970f2445d0bc2bd5731553e0996b03a64b8be8e200a2007ff38db9bebb56";
    let expected_calldata = |expected_expiry: u64| -> String {
        let confirmations = vec![];
        hex::encode(
            publishRootCall {
                rootHash: B256::from_slice(&hex::decode(ROOT_HASH).unwrap()[..]),
                expiry: U256::from(expected_expiry),
                t2TxId: 1,
                confirmations: confirmations.into(),
            }
            .abi_encode(),
        )
        .to_ascii_lowercase()
    };

    run_checks(function_name, params, expected_msg_hash, &expected_calldata);
}

#[test]
fn check_trigger_growth_encoding() {
    let function_name = BridgeContractMethod::TriggerGrowth.name_as_bytes().to_vec();
    let params = vec![
        (b"uint256".to_vec(), format!("{}", REWARDS).as_bytes().to_vec()),
        (b"uint256".to_vec(), format!("{}", AVG_STAKED).as_bytes().to_vec()),
        (b"uint32".to_vec(), format!("{}", PERIOD).as_bytes().to_vec()),
    ];
    let expected_msg_hash = "0ab8ebf8d3d8b7e38643e6eab2e47f065be1c0c583c1190af7c36e425029d484";

    let expected_calldata = |expected_expiry: u64| -> String {
        let confirmations = vec![];
        hex::encode(
            triggerGrowthCall {
                rewards: U256::from(REWARDS),
                avgStaked: U256::from(AVG_STAKED),
                period: PERIOD,
                expiry: U256::from(expected_expiry),
                t2TxId: 1,
                confirmations: confirmations.into(),
            }
            .abi_encode(),
        )
        .to_ascii_lowercase()
    };

    run_checks(function_name, params, expected_msg_hash, &expected_calldata);
}

#[test]
fn check_add_author_encoding() {
    let function_name = BridgeContractMethod::AddAuthor.name_as_bytes().to_vec();
    let params = vec![
        (b"bytes".to_vec(), hex::decode(T1_PUB_KEY).unwrap()),
        (b"bytes32".to_vec(), hex::decode(T2_PUB_KEY).unwrap()),
    ];
    let expected_msg_hash = "2cdf5c4ea05f21718a8028baeb214a34e7830b42fbb064054f07271e2c8743df";

    let expected_calldata = |expected_expiry: u64| -> String {
        let confirmations = vec![];
        hex::encode(
            addAuthorCall {
                t1PubKey: Bytes::from(hex::decode(T1_PUB_KEY).unwrap()),
                t2PubKey: B256::from_slice(&hex::decode(T2_PUB_KEY).unwrap()[..]),
                expiry: U256::from(expected_expiry),
                t2TxId: 1,
                confirmations: confirmations.into(),
            }
            .abi_encode(),
        )
        .to_ascii_lowercase()
    };

    run_checks(function_name, params, expected_msg_hash, &expected_calldata);
}

#[test]
fn check_remove_author_encoding() {
    let function_name = BridgeContractMethod::RemoveAuthor.name_as_bytes().to_vec();
    let params = vec![
        (b"bytes32".to_vec(), hex::decode(T2_PUB_KEY).unwrap()),
        (b"bytes".to_vec(), hex::decode(T1_PUB_KEY).unwrap()),
    ];

    let expected_calldata = |expected_expiry: u64| -> String {
        let confirmations = vec![];
        hex::encode(
            removeAuthorCall {
                t2PubKey: B256::from_slice(&hex::decode(T2_PUB_KEY).unwrap()[..]),
                t1PubKey: Bytes::from(hex::decode(T1_PUB_KEY).unwrap()),
                expiry: U256::from(expected_expiry),
                t2TxId: 1,
                confirmations: confirmations.into(),
            }
            .abi_encode(),
        )
        .to_ascii_lowercase()
    };

    let expected_msg_hash = "f032c115ddf96bea32c9b445f53836fcce9b0e6243824063ea76da8b8049ea64";

    run_checks(function_name, params, expected_msg_hash, &expected_calldata);
}

fn run_checks(
    function_name: Vec<u8>,
    params: Vec<(Vec<u8>, Vec<u8>)>,
    expected_msg_hash: &str,
    expected_calldata: &dyn Fn(u64) -> String,
) {
    let mut ext = ExtBuilder::build_default()
        .with_validators()
        .with_genesis_config()
        .as_externality();
    ext.execute_with(|| {
        let current_time = 1_695_809_729_000;
        pallet_timestamp::Pallet::<TestRuntime>::set_timestamp(current_time);
        let tx_id =
            add_new_send_request::<TestRuntime, ()>(&function_name, &params, &vec![]).unwrap();
        let active_tx = ActiveRequest::<TestRuntime>::get()
            .expect("is active")
            .as_active_tx::<TestRuntime, ()>()
            .unwrap();
        assert_eq!(tx_id, active_tx.request.tx_id);

        let eth_tx_lifetime_secs = EthBridge::get_eth_tx_lifetime_secs();
        let expected_expiry = current_time / 1000 + eth_tx_lifetime_secs;
        assert_eq!(active_tx.data.expiry, expected_expiry);

        let msg_hash = hex::encode(active_tx.confirmation.msg_hash);
        assert_eq!(msg_hash, expected_msg_hash);

        let calldata = generate_send_calldata::<TestRuntime, ()>(&active_tx).unwrap();
        let calldata = hex::encode(calldata);
        assert_eq!(calldata, expected_calldata(expected_expiry));
    })
}

#[cfg(test)]
mod set_admin_setting {
    use crate::incoming_events_tests::{init_active_range, EventProcessContext};

    use super::*;
    use frame_support::{
        assert_ok,
        dispatch::{DispatchErrorWithPostInfo, PostDispatchInfo},
    };
    use frame_system::RawOrigin;
    use sp_runtime::{traits::Zero, DispatchError};

    #[test]
    fn set_eth_tx_lifetime_secs_success() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let new_lifetime_secs = 120u64;

            assert_ne!(EthBridge::get_eth_tx_lifetime_secs(), 120u64);

            assert_ok!(EthBridge::set_admin_setting(
                RawOrigin::Root.into(),
                AdminSettings::EthereumTransactionLifetimeSeconds(new_lifetime_secs),
            ));

            assert_eq!(
                EthBridge::get_eth_tx_lifetime_secs(),
                new_lifetime_secs,
                "Eth tx lifetime should be updated"
            );

            assert!(System::events().iter().any(|record| matches!(
                record.event,
                mock::RuntimeEvent::EthBridge(crate::Event::EthTxLifetimeUpdated { eth_tx_lifetime_secs })
                if eth_tx_lifetime_secs == 120u64
            )));
        });
    }

    #[test]
    fn set_eth_tx_lifetime_secs_non_root_fails() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let new_lifetime_secs = 120u64;

            let result = EthBridge::set_admin_setting(
                RawOrigin::None.into(),
                AdminSettings::EthereumTransactionLifetimeSeconds(new_lifetime_secs),
            );

            assert_eq!(
                result,
                Err(DispatchErrorWithPostInfo {
                    post_info: PostDispatchInfo::default(),
                    error: DispatchError::BadOrigin,
                }),
                "Only root can set eth tx lifetime"
            );
        });
    }

    #[test]
    fn set_eth_tx_id_success() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let new_eth_tx_id = 123u32;

            assert_ne!(EthBridge::get_next_tx_id(), new_eth_tx_id);

            assert_ok!(EthBridge::set_admin_setting(
                RawOrigin::Root.into(),
                AdminSettings::EthereumTransactionId(new_eth_tx_id),
            ));

            assert_eq!(EthBridge::get_next_tx_id(), new_eth_tx_id, "Eth tx id should be updated");

            assert!(System::events().iter().any(|record| matches!(
                record.event,
                mock::RuntimeEvent::EthBridge(crate::Event::EthTxIdUpdated { eth_tx_id })
                if eth_tx_id == new_eth_tx_id
            )));
        });
    }

    #[test]
    fn set_eth_tx_id_non_root_fails() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let new_eth_tx_id = 123u32;

            let result = EthBridge::set_admin_setting(
                RawOrigin::None.into(),
                AdminSettings::EthereumTransactionId(new_eth_tx_id),
            );

            assert_eq!(
                result,
                Err(DispatchErrorWithPostInfo {
                    post_info: PostDispatchInfo::default(),
                    error: DispatchError::BadOrigin,
                }),
                "Only root can set eth tx id"
            );
        });
    }

    #[test]
    fn remove_active_request_success() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();

            let tx_id = add_new_send_request::<TestRuntime, ()>(
                &BridgeContractMethod::PublishRoot.name_as_bytes().to_vec(),
                &context.request_params,
                &vec![],
            )
            .unwrap();
            // Show that we have an active request
            let _ = ActiveRequest::<TestRuntime>::get()
                .expect("is active")
                .as_active_tx::<TestRuntime, ()>()
                .unwrap();

            assert_ok!(EthBridge::set_admin_setting(
                RawOrigin::Root.into(),
                AdminSettings::RemoveActiveRequest,
            ));

            assert!(ActiveRequest::<TestRuntime>::get().is_none(), "Eth tx id should be updated");
            assert_eq!(true, request_failed(&tx_id));
            assert!(System::events().iter().any(|record| matches!(
                record.event,
                mock::RuntimeEvent::EthBridge(crate::Event::ActiveRequestRemoved { request_id })
                if request_id == tx_id
            )));
        });
    }

    #[test]
    fn remove_active_request_non_root_fails() {
        let mut ext = ExtBuilder::build_default()
            .with_validators()
            .with_genesis_config()
            .as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let _ = add_new_send_request::<TestRuntime, ()>(
                &BridgeContractMethod::PublishRoot.name_as_bytes().to_vec(),
                &context.request_params,
                &vec![],
            )
            .unwrap();
            // Show that we have an active request
            let _ = ActiveRequest::<TestRuntime>::get()
                .expect("is active")
                .as_active_tx::<TestRuntime, ()>()
                .unwrap();

            assert_noop!(
                EthBridge::set_admin_setting(
                    RawOrigin::None.into(),
                    AdminSettings::RemoveActiveRequest
                ),
                BadOrigin
            );
        });
    }

    #[test]
    fn remove_active_request_missing_request_fails() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            // Show that we don't have an active request
            assert!(ActiveRequest::<TestRuntime>::get().is_none());

            assert_noop!(
                EthBridge::set_admin_setting(
                    RawOrigin::Root.into(),
                    AdminSettings::RemoveActiveRequest,
                ),
                Error::<TestRuntime>::NoActiveRequest
            );
        });
    }

    #[test]
    fn queue_additional_event() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            assert_ok!(EthBridge::set_admin_setting(
                RawOrigin::Root.into(),
                AdminSettings::QueueAdditionalEthereumEvent(H256::zero()),
            ));

            assert!(AdditionalEthereumEventsQueue::<TestRuntime>::get().contains(&H256::zero()));

            assert!(System::events().iter().any(|record| matches!(
                record.event,
                mock::RuntimeEvent::EthBridge(crate::Event::AdditionalEventQueued { transaction_hash })
                if transaction_hash == H256::zero()
            )));

        });
    }

    #[test]
    fn resets_ethereum_events() {
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

            assert!(!EthereumEvents::<TestRuntime>::iter().count().is_zero());

            assert_ok!(EthBridge::set_admin_setting(
                RawOrigin::Root.into(),
                AdminSettings::RestartEventDiscoveryOnRange,
            ));

            assert!(EthereumEvents::<TestRuntime>::iter().count().is_zero());
        });
    }
}

#[cfg(test)]
mod add_confirmation {

    use super::*;
    use frame_support::assert_ok;
    use frame_system::RawOrigin;

    fn setup_confirmation_test(context: &Context) -> (u32, ActiveTransactionData<AccountId>) {
        let tx_id = setup_eth_tx_request(&context);

        assert_ok!(EthBridge::add_confirmation(
            RawOrigin::None.into(),
            tx_id,
            context.confirmation_signature.clone(),
            context.confirming_author.clone(),
            context.test_signature.clone(),
        ));

        let active_request =
            ActiveRequest::<TestRuntime>::get().expect("Active transaction should be present");
        (tx_id, active_request.as_active_tx::<TestRuntime, ()>().unwrap())
    }

    #[test]
    fn adds_confirmation_correctly() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let (_, active_tx) = setup_confirmation_test(&context);

            assert!(
                active_tx.confirmation.confirmations.contains(&context.confirmation_signature),
                "Confirmation should be present"
            );

            assert_eq!(
                active_tx.data.sender, context.third_confirming_author.account_id,
                "Sender should be the author's account_id"
            );
        });
    }

    #[test]
    fn add_confirmation_with_invalid_signature_fails() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let tx_id = setup_eth_tx_request(&context);

            let invalid_signature = ecdsa::Signature::default(); // Invalid signature

            let result = EthBridge::add_confirmation(
                RawOrigin::None.into(),
                tx_id,
                invalid_signature,
                context.confirming_author.clone(),
                context.test_signature.clone(),
            );

            assert_eq!(result, Err(Error::<TestRuntime>::InvalidECDSASignature.into()));
        });
    }
}

#[cfg(test)]
mod add_eth_tx_hash {
    use super::*;
    use frame_system::RawOrigin;

    fn setup_active_transaction_data(setup_fn: Option<fn(&mut ActiveTransactionData<AccountId>)>) {
        if let Some(setup_fn) = setup_fn {
            let mut active_tx = ActiveRequest::<TestRuntime>::get()
                .expect("is active")
                .as_active_tx::<TestRuntime, ()>()
                .unwrap();
            setup_fn(&mut active_tx);
            ActiveRequest::<TestRuntime>::put(ActiveRequestData {
                request: types::Request::Send(active_tx.request),
                confirmation: active_tx.confirmation,
                tx_data: Some(active_tx.data),
                last_updated: 0u64,
            });
        }
    }

    #[test]
    fn eth_tx_hash_already_set_error() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let tx_id = setup_eth_tx_request(&context);

            setup_active_transaction_data(Some(|active_tx| {
                active_tx.data.eth_tx_hash = H256::repeat_byte(1);
            }));

            let result = EthBridge::add_eth_tx_hash(
                RawOrigin::None.into(),
                tx_id,
                context.eth_tx_hash,
                context.author,
                context.test_signature.clone(),
            );

            assert_eq!(result, Err(Error::<TestRuntime>::EthTxHashAlreadySet.into()));
        });
    }

    #[test]
    fn eth_tx_hash_set_by_sender_error() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let tx_id = setup_eth_tx_request(&context);

            setup_active_transaction_data(Some(|active_tx| {
                active_tx.data.sender = Default::default();
            }));

            let result = EthBridge::add_eth_tx_hash(
                RawOrigin::None.into(),
                tx_id,
                context.eth_tx_hash,
                context.author,
                context.test_signature.clone(),
            );

            assert_eq!(result, Err(Error::<TestRuntime>::EthTxHashMustBeSetBySender.into()));
        });
    }
    #[test]
    fn eth_tx_hash_set_correctly() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let tx_id = setup_eth_tx_request(&context);

            let result = EthBridge::add_eth_tx_hash(
                RawOrigin::None.into(),
                tx_id,
                context.eth_tx_hash,
                context.third_confirming_author,
                context.test_signature.clone(),
            );

            assert_eq!(result, Ok(().into()));
        });
    }
}

#[cfg(test)]
mod add_corroboration {
    use super::*;
    use frame_support::assert_ok;
    use frame_system::RawOrigin;
    fn setup_corroboration_test(
        context: &Context,
        is_tx_successful: bool,
        is_hash_valid: bool,
    ) -> ActiveTransactionData<AccountId> {
        let tx_id = setup_eth_tx_request(&context);

        assert_ok!(EthBridge::add_corroboration(
            RawOrigin::None.into(),
            tx_id,
            is_tx_successful,
            is_hash_valid,
            context.confirming_author.clone(),
            context.replay_attempt,
            context.test_signature.clone(),
        ));

        ActiveRequest::<TestRuntime>::get()
            .expect("Active transaction should be present")
            .as_active_tx::<TestRuntime, ()>()
            .unwrap()
    }

    #[test]
    fn adds_invalid_hash_and_successful_corroboration_correctly() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();

            let is_tx_successful = true;
            let is_hash_valid = false;

            let tx_id = setup_eth_tx_request(&context);

            assert_ok!(EthBridge::add_corroboration(
                RawOrigin::None.into(),
                tx_id,
                is_tx_successful,
                is_hash_valid,
                context.confirming_author.clone(),
                context.replay_attempt,
                context.test_signature.clone(),
            ));

            let active_tx = ActiveRequest::<TestRuntime>::get()
                .expect("Active transaction should be present")
                .as_active_tx::<TestRuntime, ()>()
                .unwrap();

            assert_eq!(true, active_tx.data.valid_tx_hash_corroborations.is_empty());
            assert_eq!(false, active_tx.data.invalid_tx_hash_corroborations.is_empty());
        });
    }

    #[test]
    fn adds_invalid_hash_and_failure_corroboration_correctly() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let active_tx = setup_corroboration_test(&context, true, false);

            assert_eq!(false, active_tx.data.invalid_tx_hash_corroborations.is_empty());
        });
    }

    #[test]
    fn adds_successful_corroboration_correctly() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let active_tx = setup_corroboration_test(&context, true, true);

            assert_eq!(false, active_tx.data.valid_tx_hash_corroborations.is_empty());
        });
    }

    #[test]
    fn add_corroboration_after_confirmation() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = setup_context();
            let tx_id = setup_eth_tx_request(&context);

            assert_ok!(EthBridge::add_confirmation(
                RawOrigin::None.into(),
                tx_id,
                context.confirmation_signature.clone(),
                context.confirming_author.clone(),
                context.test_signature.clone(),
            ));

            let result = corroborate_good_transactions(tx_id, &context.confirming_author, &context);
            assert_eq!(result, Ok(().into()));
        });
    }
}

#[test]
fn publish_to_ethereum_creates_new_transaction_request() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
            let function_name = BridgeContractMethod::PublishRoot.name_as_bytes().to_vec();
            let params = vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())];

            let transaction_id = EthBridge::publish(&function_name, &params, vec![]).unwrap();
            let active_tx = ActiveRequest::<TestRuntime>::get().unwrap().as_active_tx::<TestRuntime, ()>().unwrap();
            assert_eq!(active_tx.request.tx_id, transaction_id);
            assert_eq!(active_tx.data.function_name, function_name);

            assert!(System::events().iter().any(|record| matches!(
                &record.event,
                mock::RuntimeEvent::EthBridge(crate::Event::PublishToEthereum { function_name, params, tx_id, caller_id: _ })
                if function_name == &BridgeContractMethod::PublishRoot.name_as_bytes().to_vec() && tx_id == &transaction_id && params == &vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())]
            )));
        });
}

#[test]
fn read_bridge_contract_with_invalid_account_id() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let invalid_account_id_encoded = vec![];

        let result = EthBridge::read_bridge_contract(
            invalid_account_id_encoded,
            BridgeContractMethod::ReferenceRateUpdatedAt.name_as_bytes(),
            &vec![],
            None,
        );
        assert_err!(result, Error::<TestRuntime>::InvalidAccountId);
    });
}

#[test]
fn publish_fails_with_empty_function_name() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let function_name: &[u8] = b"";
        let params = vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())];

        let result = EthBridge::publish(function_name, &params, vec![]);
        assert_err!(result, DispatchError::Other(Error::<TestRuntime>::EmptyFunctionName.into()));
    });
}

#[test]
fn publish_fails_with_exceeding_params_limit() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let function_name: &[u8] = BridgeContractMethod::PublishRoot.name_as_bytes();
        let params = vec![(b"param1".to_vec(), b"value1".to_vec()); 6]; // ParamsLimit is 5

        let result = EthBridge::publish(function_name, &params, vec![]);
        assert_err!(result, DispatchError::Other(Error::<TestRuntime>::ParamsLimitExceeded.into()));
    });
}

#[test]
fn publish_fails_with_exceeding_type_limit_in_params() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let function_name: &[u8] = BridgeContractMethod::PublishRoot.name_as_bytes();
        let params = vec![(vec![b'a'; 8], b"value1".to_vec())]; // TypeLimit is 7

        let result = EthBridge::publish(function_name, &params, vec![]);

        assert_err!(
            result,
            DispatchError::Other(Error::<TestRuntime>::TypeNameLengthExceeded.into())
        );
    });
}

#[test]
fn publish_and_confirm_transaction() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let context = setup_context();

        let function_name = BridgeContractMethod::PublishRoot.name_as_bytes().to_vec();
        let params = vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())];

        let tx_id = EthBridge::publish(&function_name, &params, vec![]).unwrap();

        // Simulate confirming the transaction by an author

        let result = EthBridge::add_confirmation(
            RuntimeOrigin::none(),
            tx_id,
            context.confirmation_signature.clone(),
            context.confirming_author.clone(),
            context.test_signature.clone(),
        );

        assert_ok!(result);

        // Verify that the confirmation was added to the transaction
        let tx = ActiveRequest::<TestRuntime>::get().unwrap();
        assert_eq!(tx.confirmation.confirmations.len(), 1);
        assert_eq!(tx.confirmation.confirmations[0], context.confirmation_signature.clone());
    });
}

#[test]
fn publish_and_send_transaction() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let context = setup_context();

        let function_name = BridgeContractMethod::PublishRoot.name_as_bytes().to_vec();
        let params = vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())];

        let tx_id = EthBridge::publish(&function_name, &params, vec![]).unwrap();

        EthBridge::add_confirmation(
            RuntimeOrigin::none(),
            tx_id,
            context.confirmation_signature.clone(),
            context.third_confirming_author.clone(),
            context.test_signature.clone(),
        )
        .unwrap();

        let result = EthBridge::add_eth_tx_hash(
            RuntimeOrigin::none(),
            tx_id,
            context.eth_tx_hash.clone(),
            context.third_confirming_author.clone(),
            context.test_signature.clone(),
        );

        assert_ok!(result);

        let tx = ActiveRequest::<TestRuntime>::get()
            .unwrap()
            .as_active_tx::<TestRuntime, ()>()
            .unwrap();
        assert_eq!(tx.data.eth_tx_hash, context.eth_tx_hash);
    });
}

#[test]
fn publish_and_corroborate_transaction() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let context = setup_context();

        let function_name = BridgeContractMethod::PublishRoot.name_as_bytes().to_vec();
        let params = vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())];

        let tx_id = EthBridge::publish(&function_name, &params, vec![]).unwrap();

        EthBridge::add_confirmation(
            RuntimeOrigin::none(),
            tx_id,
            context.confirmation_signature.clone(),
            context.third_confirming_author.clone(),
            context.test_signature.clone(),
        )
        .unwrap();
        EthBridge::add_eth_tx_hash(
            RuntimeOrigin::none(),
            tx_id,
            context.eth_tx_hash.clone(),
            context.third_confirming_author.clone(),
            context.test_signature.clone(),
        )
        .unwrap();

        corroborate_good_transactions(tx_id, &context.confirming_author, &context).unwrap();
        corroborate_good_transactions(tx_id, &context.second_confirming_author, &context).unwrap();

        // Verify that the transaction is finalized
        assert_eq!(ActiveRequest::<TestRuntime>::get(), None);
    });
}

#[test]
fn unsent_transactions_are_replayed() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let context = setup_context();

        let function_name = BridgeContractMethod::PublishRoot.name_as_bytes().to_vec();
        let params = vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())];

        let tx_id = EthBridge::publish(&function_name, &params, vec![]).unwrap();
        let initial_replay_attempt = ActiveRequest::<TestRuntime>::get()
            .unwrap()
            .as_active_tx::<TestRuntime, ()>()
            .unwrap()
            .replay_attempt;

        EthBridge::add_confirmation(
            RuntimeOrigin::none(),
            tx_id,
            context.confirmation_signature.clone(),
            context.third_confirming_author.clone(),
            context.test_signature.clone(),
        )
        .unwrap();
        EthBridge::add_eth_tx_hash(
            RuntimeOrigin::none(),
            tx_id,
            context.eth_tx_hash.clone(),
            context.third_confirming_author.clone(),
            context.test_signature.clone(),
        )
        .unwrap();

        corroborate_bad_transactions(tx_id, &context.confirming_author, &context);
        corroborate_bad_transactions(tx_id, &context.second_confirming_author, &context);

        // the active request is retried with the same id, different sender
        let tx = ActiveRequest::<TestRuntime>::get()
            .unwrap()
            .as_active_tx::<TestRuntime, ()>()
            .unwrap();
        assert_eq!(tx_id, tx.request.tx_id);
        assert_eq!(initial_replay_attempt + 1, tx.replay_attempt);
        assert_ne!(&context.author.account_id, &tx.data.sender);

        assert!(System::events().iter().any(|record| record.event ==
            mock::RuntimeEvent::EthBridge(crate::Event::ActiveRequestRetried {
                function_name: tx.request.function_name.clone(),
                params: tx.request.params.clone(),
                caller_id: tx.request.caller_id.clone(),
            })));
    });
}

#[test]
fn self_corroborate_fails() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let context = setup_context();

        let function_name = BridgeContractMethod::PublishRoot.name_as_bytes().to_vec();
        let params = vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())];

        let tx_id = EthBridge::publish(&function_name, &params, vec![]).unwrap();

        let primary_author_account_id =
            AVN::<TestRuntime>::get_primary_validator_for_sending().unwrap();
        let primary_author = Author::<TestRuntime> {
            key: UintAuthorityId(primary_author_account_id),
            account_id: primary_author_account_id,
        };

        EthBridge::add_confirmation(
            RuntimeOrigin::none(),
            tx_id,
            context.confirmation_signature.clone(),
            context.confirming_author.clone(),
            context.test_signature.clone(),
        )
        .unwrap();
        EthBridge::add_eth_tx_hash(
            RuntimeOrigin::none(),
            tx_id,
            context.eth_tx_hash.clone(),
            primary_author.clone(),
            context.test_signature.clone(),
        )
        .unwrap();

        assert_noop!(
            EthBridge::add_corroboration(
                RuntimeOrigin::none(),
                tx_id,
                true,
                true,
                primary_author,
                context.replay_attempt,
                context.test_signature,
            ),
            Error::<TestRuntime>::CannotCorroborateOwnTransaction
        );
    });
}
