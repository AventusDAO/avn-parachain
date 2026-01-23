//! # Ethereum events pallet
// Copyright 2022 Aventus Systems (UK) Ltd.

//! ethereum-events pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;

use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite, whitelisted_caller};
use frame_system::{EventRecord, RawOrigin};
use hex_literal::hex;
use pallet_avn::{self as avn};
use sp_core::{sr25519, ByteArray};
use sp_runtime::WeakBoundedVec;

pub type AVN<T> = avn::Pallet<T>;

const MAX_NUMBER_OF_UNCHECKED_EVENTS_BENCH: u32 = MAX_NUMBER_OF_UNCHECKED_EVENTS - 1;
const MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES_BENCH: u32 =
    MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES - 1;
const MAX_CHALLENGES_BENCH: u32 = MAX_CHALLENGES - 1;

fn setup_unchecked_events<T: Config>(event_type: &ValidEvents, number_of_unchecked_events: u32) {
    let mut unchecked_added_validator_events: Vec<(EthEventId, IngressCounter, BlockNumberFor<T>)> =
        Vec::new();
    for i in 1..=number_of_unchecked_events {
        unchecked_added_validator_events.push((
            EthEventId { signature: event_type.signature(), transaction_hash: H256::from([2; 32]) },
            i as IngressCounter,
            0u32.into(),
        ));
    }

    UncheckedEvents::<T>::put(BoundedVec::truncate_from(unchecked_added_validator_events));
}

fn setup_events_pending_challenge<T: Config>(
    event_type: &ValidEvents,
    number_of_events_pending_challenge: u32,
) {
    let mut events_pending_challenge: Vec<(
        EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
        IngressCounter,
        BlockNumberFor<T>,
    )> = Vec::new();
    for i in 1..=number_of_events_pending_challenge {
        events_pending_challenge.push((
            EthEventCheckResult::new(
                0u32.into(),
                CheckResult::Ok,
                &EthEventId {
                    signature: event_type.signature(),
                    transaction_hash: H256::from([3; 32]),
                },
                &EventData::EmptyEvent,
                account("dummy account", i, i),
                10u32.into(),
                Default::default(),
            ),
            i as IngressCounter,
            0u32.into(),
        ));
    }
    EventsPendingChallenge::<T>::put(BoundedVec::truncate_from(events_pending_challenge));
}

fn setup_challenges<T: Config>(
    event_id: &EthEventId,
    validators: Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>>,
    number_of_challenges: u32,
) {
    let validators_account_ids: Vec<T::AccountId> =
        validators.iter().map(|v| v.account_id.clone()).collect::<Vec<T::AccountId>>();
    let mut challengers: Vec<T::AccountId> = Vec::new();
    for _ in 0..number_of_challenges {
        challengers.push(validators_account_ids[0 as usize].clone());
    }
    Challenges::<T>::insert(event_id, BoundedVec::truncate_from(challengers));
}

fn setup_validators<T: Config>(
    number_of_validator_account_ids: u32,
) -> Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>> {
    let mnemonic: &str =
        "basic anxiety marine match castle rival moral whisper insane away avoid bike";
    let mut validators: Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>> =
        Vec::new();
    for i in 0..number_of_validator_account_ids {
        let account = account("dummy_validator", i, i);
        let key =
            <T as avn::Config>::AuthorityId::generate_pair(Some(mnemonic.as_bytes().to_vec()));
        validators.push(Validator::new(account, key));
    }

    // Setup validators in avn pallet
    avn::Validators::<T>::put(WeakBoundedVec::force_from(
        validators.clone(),
        Some("Too many validators for session"),
    ));

    return validators
}

fn setup_extrinsics_inputs<T: Config>(
    validators: Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>>,
) -> (
    EthEventCheckResult<BlockNumberFor<T>, T::AccountId>,
    u64,
    <T::AuthorityId as RuntimeAppPublic>::Signature,
    Validator<T::AuthorityId, T::AccountId>,
) {
    let event_id = EthEventId {
        signature: ValidEvents::AddedValidator.signature(),
        transaction_hash: H256::from([4; 32]),
    };
    let result: EthEventCheckResult<BlockNumberFor<T>, T::AccountId> = EthEventCheckResult::new(
        0u32.into(),
        CheckResult::Ok,
        &event_id.clone(),
        &EventData::EmptyEvent,
        validators[validators.len() - (1 as usize)].account_id.clone(),
        10u32.into(),
        Default::default(),
    );
    let ingress_counter: u64 = 2000;
    let signature: <T::AuthorityId as RuntimeAppPublic>::Signature = generate_signature::<T>();
    let validator: Validator<T::AuthorityId, T::AccountId> =
        validators[validators.len() - (1 as usize)].clone();

    (result, ingress_counter, signature, validator)
}

fn generate_signature<T: pallet_avn::Config>(
) -> <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature {
    let encoded_data = 0.encode();
    let authority_id = T::AuthorityId::generate_pair(None);
    let signature = authority_id.sign(&encoded_data).expect("able to make signature");
    return signature
}

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1 as usize)];
    assert_eq!(event, &system_event);
}

benchmarks! {
    add_ethereum_log {
        let u in 1 .. MAX_NUMBER_OF_UNCHECKED_EVENTS_BENCH;
        let e in 1 .. MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES_BENCH;

        let event_type = ValidEvents::NftMint;
        setup_unchecked_events::<T>(&event_type, u);
        setup_events_pending_challenge::<T>(&event_type, e);

        let tx_hash = H256::from([1; 32]);
        let account_id: T::AccountId = whitelisted_caller();
    }: _(RawOrigin::<T::AccountId>::Signed(account_id.clone()), event_type, tx_hash)
    verify {
        let eth_event_id = EthEventId {
            signature: ValidEvents::NftMint.signature(),
            transaction_hash: tx_hash,
        };
        let ingress_counter = <TotalIngresses<T>>::get();

        assert_eq!(true, UncheckedEvents::<T>::get().contains(&(eth_event_id.clone(), ingress_counter, 1u32.into())));
        assert_last_event::<T>(Event::<T>::NftEthereumEventAdded {
            eth_event_id: eth_event_id,
            account_id: account_id,
        }.into());
    }

    signed_add_ethereum_log {
        let u in 1 .. MAX_NUMBER_OF_UNCHECKED_EVENTS_BENCH;
        let e in 1 .. MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES_BENCH;

        let event_type = ValidEvents::NftMint;
        setup_unchecked_events::<T>(&event_type, u);
        setup_events_pending_challenge::<T>(&event_type, e);

        // This is generated from scripts/benchmarking/sign_add_ethereum_log.js
        let signer_raw: H256 = H256(hex!("482eae97356cdfd3b12774db1e5950471504d28b89aa169179d6c0527a04de23"));
        let signer = T::AccountId::decode(&mut signer_raw.as_bytes()).expect("valid account id");

        // Signature is generated using the script in `scripts/benchmarking`.
        let signature = &hex!("a644590556915ea752559d52aded20e0fb2c586d478717f075d938fb18462373677042b0a202e048069b24ac76c9115e0222411d72da11a92337c5d67ec7d085");
        let tx_hash = H256::from([1; 32]);

        let proof: Proof<T::Signature, T::AccountId> = Proof {
            signer: signer.clone(),
            relayer: whitelisted_caller(),
            signature: T::Signature::decode(&mut &signature[..])
                .expect("signature bytes must decode into T::Signature"),

        };
    }: _ (RawOrigin::<T::AccountId>::Signed(signer.clone()), proof.clone(), event_type, tx_hash)
    verify {
        let eth_event_id = EthEventId {
            signature: ValidEvents::NftMint.signature(),
            transaction_hash: tx_hash,
        };
        let ingress_counter = <TotalIngresses<T>>::get();

        assert_eq!(true, UncheckedEvents::<T>::get().contains(&(eth_event_id.clone(), ingress_counter, 1u32.into())));
        assert_last_event::<T>(Event::<T>::NftEthereumEventAdded {
            eth_event_id: eth_event_id,
            account_id: signer,
        }.into());
    }

    set_nft_contract_map_storage {
        let contract_address = H160::from([1; 20]);
    }: insert_nft_contract(RawOrigin::Root, contract_address.clone())
    verify {
        assert_eq!(true, <NftT1Contracts<T>>::contains_key(contract_address));
    }

    submit_checkevent_result {
        let v in 1 .. MAX_VALIDATOR_ACCOUNTS;
        let u in 1 .. MAX_NUMBER_OF_UNCHECKED_EVENTS_BENCH;

        let event_type = ValidEvents::Lifted;
        setup_unchecked_events::<T>(&event_type, u);
        let validators = setup_validators::<T>(v);
        let (mut result, ingress_counter, signature, validator) = setup_extrinsics_inputs::<T>(validators.clone());
        UncheckedEvents::<T>::mutate(|events| events.try_push((result.event.event_id.clone(), ingress_counter as IngressCounter, 0u32.into())).expect("Cannot push"));

        let unchecked_events_length = UncheckedEvents::<T>::get().len();
        let events_pending_challenge_length = EventsPendingChallenge::<T>::get().len();
    }: _(RawOrigin::None, result.clone(), ingress_counter, signature, validator)
    verify {
        result.ready_for_processing_after_block = <frame_system::Pallet<T>>::block_number()
            .checked_add(&EventChallengePeriod::<T>::get())
            .ok_or(Error::<T>::Overflow).unwrap();
        result.min_challenge_votes = (validators.len() as u32) / <QuorumFactor<T>>::get();

        assert_eq!(UncheckedEvents::<T>::get().len(), unchecked_events_length - (1 as usize));
        assert_eq!(EventsPendingChallenge::<T>::get().len(), events_pending_challenge_length + (1 as usize));
        assert_eq!(true, EventsPendingChallenge::<T>::get().contains(&(result.clone(), ingress_counter, 1u32.into())));

        assert_last_event::<T>(Event::<T>::EventValidated{
            eth_event_id: result.event.event_id,
            check_result: result.result,
            validated_by: result.checked_by
        }.into());
    }

    process_event_with_successful_challenge {
        let v in 1 .. MAX_VALIDATOR_ACCOUNTS;
        let e in 1 .. MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES_BENCH;

        let validators = setup_validators::<T>(v);
        let (result, ingress_counter, signature, validator) = setup_extrinsics_inputs::<T>(validators.clone());

        setup_events_pending_challenge::<T>(&ValidEvents::AddedValidator, e);
        EventsPendingChallenge::<T>::mutate(|events| events.try_push((result.clone(), ingress_counter, 0u32.into())).expect("Cannot push"));
        let required_challenge_votes = (AVN::<T>::active_validators().len() as u32) / <QuorumFactor<T>>::get();
        setup_challenges::<T>(&result.event.event_id.clone(), validators.clone(), required_challenge_votes + 1);
    }: process_event(RawOrigin::None, result.event.event_id.clone(), ingress_counter, validator.clone(), signature)
    verify {
        assert_last_event::<T>(
            Event::<T>::EventRejected {
                eth_event_id: result.event.event_id,
                check_result: result.result,
                successful_challenge:true
            }.into()
        );
    }

    process_event_without_successful_challenge {
        let v in 1 .. MAX_VALIDATOR_ACCOUNTS;
        let e in 1 .. MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES_BENCH;

        let validators = setup_validators::<T>(v);
        let (mut result, ingress_counter, signature, validator) = setup_extrinsics_inputs::<T>(validators.clone());

        setup_events_pending_challenge::<T>(&ValidEvents::AddedValidator, e);
        result.min_challenge_votes = 3;
        EventsPendingChallenge::<T>::mutate(|events| events.try_push((result.clone(), ingress_counter, 0u32.into())).expect("Cannot push"));
        let required_challenge_votes = (AVN::<T>::active_validators().len() as u32) / <QuorumFactor<T>>::get();
        setup_challenges::<T>(&result.event.event_id.clone(), validators.clone(), 1);
    }: process_event(RawOrigin::None, result.event.event_id.clone(), ingress_counter, validator.clone(), signature)
    verify {
        assert_last_event::<T>(
            Event::<T>::EventAccepted { eth_event_id: result.event.event_id }.into()
        );
    }

    challenge_event {
        let v in 3 .. MAX_VALIDATOR_ACCOUNTS;
        let e in 1 .. MAX_NUMBER_OF_EVENTS_PENDING_CHALLENGES_BENCH;
        let c in 1 .. MAX_CHALLENGES_BENCH;

        let mut validators = setup_validators::<T>(v);
        let (result, ingress_counter, signature, validator) = setup_extrinsics_inputs::<T>(validators.clone());

        setup_events_pending_challenge::<T>(&ValidEvents::AddedValidator, e);
        EventsPendingChallenge::<T>::mutate(|events| events.try_push((result.clone(), ingress_counter as IngressCounter, 0u32.into())).expect("Cannot push"));

        let challenged_by = validators[validators.len()-2].account_id.clone();
        validators.remove(validators.len()-1); // remove validator
        validators.remove(validators.len()-1); // remove challenged_by
        setup_challenges::<T>(&result.event.event_id, validators.clone(), c);

        let challenge: Challenge<T::AccountId> = Challenge::new(
            result.event.event_id.clone(),
            ChallengeReason::IncorrectResult,
            challenged_by.clone()
        );
    }: _(RawOrigin::None, challenge.clone(), ingress_counter, signature, validator.clone())
    verify {
        assert_eq!(true, Challenges::<T>::get(result.event.event_id).contains(&challenged_by));
        assert_last_event::<T>(Event::<T>::EventChallenged {
            eth_event_id: challenge.event_id,
            challenger: challenge.challenged_by,
            challenge_reason: challenge.challenge_reason
        }.into());
    }

    set_event_challenge_period {
        let new_event_challenge_period = 1200u32.into();
        assert_ne!(new_event_challenge_period, EventChallengePeriod::<T>::get());
    }: _(RawOrigin::Root, new_event_challenge_period)
    verify {
        assert_eq!(new_event_challenge_period, EventChallengePeriod::<T>::get());
        assert_last_event::<T>(Event::<T>::EventChallengePeriodUpdated{ block: new_event_challenge_period }.into());
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().as_externality(),
    crate::mock::TestRuntime,
);
