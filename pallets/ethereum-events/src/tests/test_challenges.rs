// Copyright 2022 Aventus Systems (UK) Ltd.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_err, assert_noop, assert_ok, dispatch, unsigned::ValidateUnsigned};
use frame_system::RawOrigin;
use sp_avn_common::event_types::{EthEventId, ValidEvents};
use sp_core::{
    offchain::{
        testing::{TestOffchainExt, TestTransactionPoolExt},
        OffchainDbExt as OffchainExt, TransactionPoolExt,
    },
    H256, H512, U256,
};
use sp_runtime::testing::{TestSignature, UintAuthorityId};

fn with_offchain_worker(externality: sp_io::TestExternalities) -> sp_io::TestExternalities {
    let mut ext = externality;
    let (offchain, _state) = TestOffchainExt::new();
    let (pool, _pool_state) = TestTransactionPoolExt::new();
    ext.register_extension(OffchainExt::new(offchain));
    ext.register_extension(TransactionPoolExt::new(pool));
    return ext;
}

fn remove_from_events_pending_challenge(index: usize) {
    <EventsPendingChallenge<TestRuntime>>::mutate(|pending_events| pending_events.remove(index));
}

fn get_event_check_result(
    id: &EthEventId,
    data: &EventData,
    result: CheckResult,
) -> EthEventCheckResult<BlockNumberFor<TestRuntime>, AccountId> {
    return EthEventCheckResult::new(10, result, id, data, account_id_0(), 5, 0);
}

fn get_added_validator_data() -> AddedValidatorData {
    return AddedValidatorData {
        eth_public_key: H512::random(),
        t2_address: H256::random(),
        validator_account_id: U256::one(),
    };
}

fn create_challenge(
    id: EthEventId,
    reason: ChallengeReason,
    challenger: AccountId,
) -> Challenge<AccountId> {
    return Challenge::new(id, reason, challenger);
}

fn get_validator(index: usize) -> Validator<AuthorityId, AccountId> {
    return EthereumEvents::validators()[index].clone();
}

fn mock_send_challenge_transaction_from_ocw(
    challenge: Challenge<AccountId>,
    ingress_counter: IngressCounter,
    signature: TestSignature,
    validator: Validator<AuthorityId, AccountId>,
) -> dispatch::DispatchResult {
    EthereumEvents::pre_dispatch(&crate::Call::challenge_event {
        challenge: challenge.clone(),
        ingress_counter,
        signature: signature.clone(),
        validator: validator.clone(),
    })
    .map_err(|e| <&'static str>::from(e))?;
    return EthereumEvents::challenge_event(
        RawOrigin::None.into(),
        challenge,
        ingress_counter,
        signature,
        validator,
    );
}

// Tests for `fn get_next_event_to_validate` (also covers `fn can_validate_this_event`)
/*
    * when pending challenge queue is empty
    * when pending challenge queue is not empty but all events are checked by us
    * when pending challenge queue is not empty but all events are past the challenge window
    * when there is exactly 1 event to validate
        - Good case
        - Already validated by us
        - Challenge window passed
    * when there are more than 1 event to validate
       - first event can be validated
       - Third event can be validated
*/

// TODO [TYPE: test refactoring][PRI: low]: Review these tests to work with the builder's pattern
// setup Currently, a straightforward substitution makes several of these tests fail
#[test]
// * when pending challenge queue is empty
fn test_get_event_to_validate_empty_pending_queue() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        assert!(!EthereumEvents::has_events_to_validate());
        assert!(EthereumEvents::get_next_event_to_validate(&account_id_0(), 0u64.into()).is_none());
    });
}

#[test]
// * when pending challenge queue is not empty but all events are checked by us
fn test_get_event_to_validate_all_checked_by_us() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        let this_validator_account_id = account_id_0();

        assert!(!EthereumEvents::has_events_to_validate());
        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 5);
        assert_eq!(EthereumEvents::events_pending_challenge().len(), 5);

        assert!(EthereumEvents::get_next_event_to_validate(
            &this_validator_account_id,
            0u64.into()
        )
        .is_none());
    });
}

#[test]
#[ignore]
// * when pending challenge queue is not empty but all events are past the challenge window
fn test_get_event_to_validate_past_challenge_window() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        let this_validator_account_id = account_id_0();
        let new_validator_account_id = account_id_1();

        assert!(!EthereumEvents::has_events_to_validate());
        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 5);
        assert_eq!(EthereumEvents::events_pending_challenge().len(), 5);

        // Increase the current block_number so its passed the challenge period
        System::set_block_number(EVENT_CHALLENGE_PERIOD + 1);
        assert!(EthereumEvents::get_next_event_to_validate(&new_validator_account_id, 0u64.into())
            .is_none());
    });
}

#[test]
// * when there is exactly 1 event to validate (good event)
fn test_get_event_to_validate_1_good_event() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        let this_validator_account_id = account_id_0();
        let new_validator_account_id = account_id_1();

        assert!(!EthereumEvents::has_events_to_validate());
        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 1);
        assert_eq!(EthereumEvents::events_pending_challenge().len(), 1);

        assert!(EthereumEvents::get_next_event_to_validate(&new_validator_account_id, 0u64.into())
            .is_some());
    });
}

#[test]
// * when there is exactly 1 event checked by us
fn test_get_event_to_validate_1_event_checked_by_us() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        let this_validator_account_id = account_id_0();

        assert!(!EthereumEvents::has_events_to_validate());
        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 1);
        assert_eq!(EthereumEvents::events_pending_challenge().len(), 1);

        assert!(EthereumEvents::get_next_event_to_validate(
            &this_validator_account_id,
            0u64.into()
        )
        .is_none());
    });
}

#[test]
#[ignore]
// * when there is exactly 1 event past its challenge window
fn test_get_event_to_validate_1_event_past_challenge_window() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        let this_validator_account_id = account_id_0();
        let new_validator_account_id = account_id_1();

        assert!(!EthereumEvents::has_events_to_validate());
        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 1);
        assert_eq!(EthereumEvents::events_pending_challenge().len(), 1);

        // Increase the current block_number so its after the challenge period
        System::set_block_number(EVENT_CHALLENGE_PERIOD + 1);
        assert!(EthereumEvents::get_next_event_to_validate(&new_validator_account_id, 0u64.into())
            .is_none());
    });
}

#[test]
// * when pending challenge queue is not empty and the first event can be validated but the second
//   one is checked by us
fn test_get_event_to_validate_first_good_event() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        let this_validator_account_id = account_id_0();
        let new_validator_account_id = account_id_1();

        assert!(!EthereumEvents::has_events_to_validate());
        EthereumEvents::populate_events_pending_challenge(&new_validator_account_id, 1);
        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 2);
        assert_eq!(EthereumEvents::events_pending_challenge().len(), 3);

        let (next_event_to_validate, counter, _) =
            EthereumEvents::get_next_event_to_validate(&this_validator_account_id, 0u64.into())
                .unwrap();
        let expected_event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::from([0; 32]), //0 is the first item of the vector
        };

        assert!(expected_event_id == next_event_to_validate.event.event_id);
        assert_eq!(counter, 1);
    });
}

#[test]
// * when pending challenge queue is not empty but the first 2 events are checked by us
fn test_get_event_to_validate_third_good_event() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        let this_validator_account_id = account_id_0();
        let new_validator_account_id = account_id_1();

        assert!(!EthereumEvents::has_events_to_validate());
        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 2);
        EthereumEvents::populate_events_pending_challenge(&new_validator_account_id, 1);
        assert_eq!(EthereumEvents::events_pending_challenge().len(), 3);

        let (next_event_to_validate, counter, _) =
            EthereumEvents::get_next_event_to_validate(&this_validator_account_id, 0u64.into())
                .unwrap();
        let expected_event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::from([2; 32]), //2 is the zero based index of the vector
        };

        assert!(expected_event_id == next_event_to_validate.event.event_id);
        assert_eq!(counter, 3);
    });
}

#[test]
// * when pending challenge queue is not empty and event 2 can be validated by
//   `this_validator_account_id` and
// event 4 can be validated by `new_validator_account_id`
fn test_get_event_to_validate_mixed() {
    with_offchain_worker(eth_events_test_with_validators()).execute_with(|| {
        let this_validator_account_id = account_id_0();
        let new_validator_account_id = account_id_1();

        assert!(!EthereumEvents::has_events_to_validate());

        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 1);
        EthereumEvents::populate_events_pending_challenge(&new_validator_account_id, 2);
        EthereumEvents::populate_events_pending_challenge(&this_validator_account_id, 1);
        assert_eq!(EthereumEvents::events_pending_challenge().len(), 4);

        let (next_event_to_validate, counter, _) =
            EthereumEvents::get_next_event_to_validate(&this_validator_account_id, 0u64.into())
                .unwrap();
        let expected_event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::from([1; 32]),
        };
        assert_eq!(expected_event_id, next_event_to_validate.event.event_id);
        assert_eq!(counter, 2);

        let (next_event_to_validate, counter, _) =
            EthereumEvents::get_next_event_to_validate(&new_validator_account_id, 0u64.into())
                .unwrap();
        let expected_event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::from([0; 32]),
        };
        assert!(expected_event_id == next_event_to_validate.event.event_id);
        assert_eq!(counter, 1);

        //Remove the first event from the queue
        remove_from_events_pending_challenge(0);

        let (next_event_to_validate, counter, _) =
            EthereumEvents::get_next_event_to_validate(&new_validator_account_id, 0u64.into())
                .unwrap();
        let expected_event_id = EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::from([3; 32]),
        };
        assert!(expected_event_id == next_event_to_validate.event.event_id);
        assert_eq!(counter, 4);
    });
}

// Tests for `validate_event`
/*
 * when there is an HTTP error while validating
 * when there is an error signing ??
 * when there is an error submitting transaction ??
 */

// Tests for `get_challenge_if_required`
/*
 * when check.result and validation.result both Ok with same event data
 * when check.result and validation.result both Ok but different event data
 * when check.result and validation.result both Invalid with same event data
 * when check.result and validation.result both Invalid but different event data
 * when check.result = Ok but validation.result = Invalid
 * when check.result = Invalid but validation.result = Ok
 */

#[test]
// * when check.result and validation.result both Ok with same event data
fn test_challenge_validation_match_check() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let event_id = EthereumEvents::get_event_id(1);
        let validator_account_id = account_id_0();

        let checked = get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Ok);
        let validated = get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Ok);

        let challenge =
            EthereumEvents::get_challenge_if_required(checked, validated, validator_account_id);
        assert!(challenge.is_none());
    });
}

#[test]
// * when check.result and validation.result both Ok but different event data
fn test_challenge_ok_different_event_data() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let add_validator_data = EventData::LogAddedValidator(get_added_validator_data());
        let event_id = EthereumEvents::get_event_id(1);
        let validator_account_id = account_id_0();

        let checked = get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Ok);
        let validated = get_event_check_result(&event_id, &add_validator_data, CheckResult::Ok);

        let challenge =
            EthereumEvents::get_challenge_if_required(checked, validated, validator_account_id);

        assert!(challenge.is_some());
        assert_eq!(challenge.unwrap().challenge_reason, ChallengeReason::IncorrectEventData);
    });
}

#[test]
// * wwhen check.result and validation.result both Invalid with same event data
fn test_challenge_invalid_same_event_data() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let event_id = EthereumEvents::get_event_id(1);
        let validator_account_id = account_id_0();

        let checked =
            get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Invalid);
        let validated =
            get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Invalid);

        let challenge =
            EthereumEvents::get_challenge_if_required(checked, validated, validator_account_id);

        assert!(challenge.is_none());
    });
}

#[test]
// * when check.result and validation.result both Invalid but different event data
fn test_challenge_invalid_different_event_data() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let add_validator_data = EventData::LogAddedValidator(get_added_validator_data());
        let event_id = EthereumEvents::get_event_id(1);
        let validator_account_id = account_id_0();

        let checked =
            get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Invalid);
        let validated =
            get_event_check_result(&event_id, &add_validator_data, CheckResult::Invalid);

        let challenge =
            EthereumEvents::get_challenge_if_required(checked, validated, validator_account_id);

        // If both checked and validated are invalid, we dont care about event data
        assert!(challenge.is_none());
    });
}

#[test]
// * when check.result = Ok but validation.result = Invalid
fn test_challenge_validation_invalid_check_ok() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let event_id = EthereumEvents::get_event_id(1);
        let validator_account_id = account_id_0();

        let checked = get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Ok);
        let validated =
            get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Invalid);

        let challenge =
            EthereumEvents::get_challenge_if_required(checked, validated, validator_account_id);

        assert!(challenge.is_some());
        assert_eq!(challenge.unwrap().challenge_reason, ChallengeReason::IncorrectResult);
    });
}

#[test]
// * when check.result = Invalid but validation.result = Ok
fn test_challenge_validation_ok_check_invalid() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let event_id = EthereumEvents::get_event_id(1);
        let validator_account_id = account_id_0();

        let checked =
            get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Invalid);
        let validated = get_event_check_result(&event_id, &EventData::EmptyEvent, CheckResult::Ok);

        let challenge =
            EthereumEvents::get_challenge_if_required(checked, validated, validator_account_id);

        assert!(challenge.is_some());
        assert_eq!(challenge.unwrap().challenge_reason, ChallengeReason::IncorrectResult);
    });
}

// Tests for `save_validated_event_in_local_storage`
/*
    * when this is the first time we are saving (LocalDB was empty)
        - valid EthEventId
        - Invalid EthEventId
    * when appending a valid EthEventId that doesnt exist in LocalDB
    * when appending an existing EthEventId in LocalDB (Duplicate)
    * when appending an invalid input (not EthEventId) ??
*/

// Tests for `fn challenge_event`
/*
    * when event doesn't exist in the pending challenge queue
    * when the challenge window of the event has passed
    * when the challenger is not a validator
    * when the signature is not valid
    * when challenging your own event
    * when challenging more than once
    * when a valid challenge is added (good case)
        - First challenge to be added
        - Additional challenge to be added (there were existing challenges in the system)
        - challenge reason: InvalidResult
        - challenge reason: InvalidEventData
        - check correct event deposited
*/
#[test]
fn test_challenge_missing_event() {
    eth_events_test_with_validators().execute_with(|| {
        let validator = get_validator(0);
        let bad_event_id = EthereumEvents::get_event_id(1);
        let challenge =
            create_challenge(bad_event_id, ChallengeReason::IncorrectResult, account_id_1());
        let signature =
            validator.key.sign(&(CHALLENGE_EVENT_CONTEXT, challenge.clone()).encode()).unwrap();

        assert_eq!(EthereumEvents::events_pending_challenge().len(), 0);
        assert_noop!(
            EthereumEvents::challenge_event(
                RawOrigin::None.into(),
                challenge,
                DEFAULT_INGRESS_COUNTER,
                signature,
                validator
            ),
            Error::<TestRuntime>::InvalidEventToChallenge
        );
    });
}

#[test]
#[ignore]
fn test_challenge_out_of_challenge_window() {
    eth_events_test_with_validators().execute_with(|| {
        EthereumEvents::populate_events_pending_challenge(&account_id_0(), 1);

        let validator = get_validator(1);
        let challenge = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectResult,
            validator.account_id,
        );
        let signature =
            validator.key.sign(&(CHALLENGE_EVENT_CONTEXT, challenge.clone()).encode()).unwrap();

        // Move block_number past challenge window
        System::set_block_number(EVENT_CHALLENGE_PERIOD + 1);

        assert_noop!(
            EthereumEvents::challenge_event(
                RawOrigin::None.into(),
                challenge,
                DEFAULT_INGRESS_COUNTER,
                signature,
                validator
            ),
            Error::<TestRuntime>::InvalidEventToChallenge
        );
    });
}

#[test]
fn test_challenge_invalid_validator() {
    eth_events_test_with_validators().execute_with(|| {
        EthereumEvents::populate_events_pending_challenge(&account_id_0(), 1);

        let bad_validator =
            Validator::new(TestAccount::new([10u8; 32]).account_id(), UintAuthorityId(10));
        let challenge = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectResult,
            bad_validator.account_id,
        );
        let signature =
            bad_validator.key.sign(&(CHALLENGE_EVENT_CONTEXT, challenge.clone()).encode()).unwrap();

        assert_noop!(
            EthereumEvents::challenge_event(
                RawOrigin::None.into(),
                challenge,
                DEFAULT_INGRESS_COUNTER,
                signature,
                bad_validator
            ),
            Error::<TestRuntime>::InvalidKey
        );
    });
}

#[test]
fn test_challenge_invalid_signature() {
    eth_events_test_with_validators().execute_with(|| {
        let ingress_counter = EthereumEvents::populate_events_pending_challenge(&account_id_0(), 1);

        let validator = get_validator(1);
        let challenge = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectResult,
            validator.account_id,
        );
        let bad_data_to_sign = "Bad signature data";

        // Note: the implementation of sign() on UintAuthorityId will return the same signature for
        // different signer as long as the data being signed is the same.
        let bad_signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, bad_data_to_sign, ingress_counter).encode())
            .unwrap();

        // Signature validation is done in Validate_unsigned which is called when the OCW sends
        // transactions
        assert_err!(
            mock_send_challenge_transaction_from_ocw(
                challenge.clone(),
                ingress_counter,
                bad_signature,
                validator
            ),
            <&str>::from(InvalidTransaction::BadProof)
        );
    });
}

#[test]
fn test_challenge_own_event_challenges() {
    eth_events_test_with_validators().execute_with(|| {
        let validator = get_validator(0);

        let ingress_counter =
            EthereumEvents::populate_events_pending_challenge(&validator.account_id, 1);

        let challenge = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectResult,
            validator.account_id,
        );
        let signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, &challenge, ingress_counter).encode())
            .unwrap();

        assert_noop!(
            EthereumEvents::challenge_event(
                RawOrigin::None.into(),
                challenge,
                ingress_counter,
                signature,
                validator
            ),
            Error::<TestRuntime>::ChallengingOwnEvent
        );
    });
}

#[test]
fn test_challenge_duplicate_challenges() {
    eth_events_test_with_validators().execute_with(|| {
        let ingress_counter = EthereumEvents::populate_events_pending_challenge(&account_id_1(), 1);

        let validator = get_validator(1);
        let challenge = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectResult,
            validator.account_id,
        );
        let signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, &challenge, ingress_counter).encode())
            .unwrap();

        assert_ok!(EthereumEvents::challenge_event(
            RawOrigin::None.into(),
            challenge.clone(),
            ingress_counter,
            signature.clone(),
            validator.clone()
        ));

        assert_noop!(
            EthereumEvents::challenge_event(
                RawOrigin::None.into(),
                challenge,
                ingress_counter,
                signature,
                validator
            ),
            Error::<TestRuntime>::DuplicateChallenge
        );
    });
}

#[test]
fn test_challenge_valid_challenge_first() {
    eth_events_test_with_validators().execute_with(|| {
        let ingress_counter = EthereumEvents::populate_events_pending_challenge(&account_id_1(), 1);

        let validator = get_validator(1);
        let challenge = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectResult,
            validator.account_id,
        );
        let signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, &challenge, ingress_counter).encode())
            .unwrap();

        assert_eq!(EthereumEvents::challenges(challenge.event_id.clone()).len(), 0);
        assert_ok!(EthereumEvents::challenge_event(
            RawOrigin::None.into(),
            challenge.clone(),
            ingress_counter,
            signature.clone(),
            validator
        ));
        assert_eq!(EthereumEvents::challenges(challenge.event_id).len(), 1);
    });
}

#[test]
fn test_challenge_valid_challenge_multiple() {
    eth_events_test_with_validators().execute_with(|| {
        let mut ingress_counter =
            EthereumEvents::populate_events_pending_challenge(&account_id_1(), 3);

        let validator = get_validator(1);
        let challenge1 = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectResult,
            validator.account_id,
        );
        let signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, &challenge1, ingress_counter).encode())
            .unwrap();
        assert_eq!(EthereumEvents::challenges(challenge1.event_id.clone()).len(), 0);
        assert_ok!(EthereumEvents::challenge_event(
            RawOrigin::None.into(),
            challenge1.clone(),
            ingress_counter,
            signature.clone(),
            validator.clone()
        ));
        assert_eq!(EthereumEvents::challenges(challenge1.clone().event_id).len(), 1);

        ingress_counter += 1;
        let challenge2 = create_challenge(
            EthereumEvents::get_event_id(1),
            ChallengeReason::IncorrectResult,
            validator.account_id,
        );
        let signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, &challenge2, ingress_counter).encode())
            .unwrap();
        assert_ok!(EthereumEvents::challenge_event(
            RawOrigin::None.into(),
            challenge2.clone(),
            ingress_counter,
            signature.clone(),
            validator
        ));
        assert_eq!(EthereumEvents::challenges(challenge2.event_id).len(), 1);

        // Make sure the first challenge is still there
        assert_eq!(EthereumEvents::challenges(challenge1.event_id).len(), 1);
    });
}

#[test]
fn test_challenge_valid_challenge_invalid_result() {
    eth_events_test_with_validators().execute_with(|| {
        let ingress_counter = EthereumEvents::populate_events_pending_challenge(&account_id_1(), 1);

        let validator = get_validator(1);
        let challenge = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectResult,
            validator.account_id,
        );
        let signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, &challenge, ingress_counter).encode())
            .unwrap();

        assert_eq!(EthereumEvents::challenges(challenge.event_id.clone()).len(), 0);
        assert_ok!(EthereumEvents::challenge_event(
            RawOrigin::None.into(),
            challenge.clone(),
            ingress_counter,
            signature.clone(),
            validator.clone()
        ));
        assert_eq!(EthereumEvents::challenges(challenge.event_id.clone()).len(), 1);

        assert!(System::events().iter().any(|a| a.event
            == mock::RuntimeEvent::EthereumEvents(crate::Event::<TestRuntime>::EventChallenged {
                eth_event_id: challenge.event_id.clone(),
                challenger: validator.account_id,
                challenge_reason: ChallengeReason::IncorrectResult
            })));
        assert_eq!(System::events().len(), 1);
    });
}

#[test]
fn test_challenge_valid_challenge_invalid_event_data() {
    eth_events_test_with_validators().execute_with(|| {
        let ingress_counter = EthereumEvents::populate_events_pending_challenge(&account_id_1(), 1);

        let validator = get_validator(1);
        let challenge = create_challenge(
            EthereumEvents::get_event_id(0),
            ChallengeReason::IncorrectEventData,
            validator.account_id,
        );
        let signature = validator
            .key
            .sign(&(CHALLENGE_EVENT_CONTEXT, &challenge, ingress_counter).encode())
            .unwrap();

        assert_eq!(EthereumEvents::challenges(challenge.event_id.clone()).len(), 0);
        assert_ok!(EthereumEvents::challenge_event(
            RawOrigin::None.into(),
            challenge.clone(),
            ingress_counter,
            signature.clone(),
            validator.clone()
        ));
        assert_eq!(EthereumEvents::challenges(challenge.event_id.clone()).len(), 1);

        assert!(System::events().iter().any(|a| a.event
            == mock::RuntimeEvent::EthereumEvents(crate::Event::<TestRuntime>::EventChallenged {
                eth_event_id: challenge.event_id.clone(),
                challenger: validator.account_id,
                challenge_reason: ChallengeReason::IncorrectEventData
            })));
        assert_eq!(System::events().len(), 1);
    });
}

#[test]
#[should_panic]
fn test_invalid_config_validator_threshold() {
    ExtBuilder::build_default().invalid_config_with_zero_validator_threshold().as_externality();
}

// Test "increase the min_challenge_votes of any open challenges to 1/3 of the new validators"
/*
TBD
*/

// Tests for "fn process_event" (challenge outcome related tests only)
/*
TBD
*/
