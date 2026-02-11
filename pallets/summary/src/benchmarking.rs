//! # Summary pallet
// Copyright 2026 Aventus DAO Ltd

//! summary pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;

use crate::offence::create_offenders_identification;
use frame_benchmarking::{account, benchmarks_instance_pallet, impl_benchmark_test_suite};
use frame_system::{pallet_prelude::BlockNumberFor, EventRecord, Pallet as System, RawOrigin};
use hex_literal::hex;
use pallet_avn::{self as avn};
use sp_runtime::WeakBoundedVec;

pub type AVN<T> = avn::Pallet<T>;
pub const ROOT_HASH_BYTES: [u8; 32] = [
    135, 54, 201, 230, 113, 254, 88, 31, 228, 239, 70, 49, 17, 32, 56, 41, 125, 205, 236, 174, 22,
    62, 135, 36, 194, 129, 236, 232, 173, 148, 200, 195,
];

fn setup_publish_root_voting<T: Config<I>, I: 'static>(
    validators: Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>>,
) -> (
    Validator<T::AuthorityId, T::AccountId>,
    RootId<BlockNumberFor<T>>,
    <T::AuthorityId as RuntimeAppPublic>::Signature,
    u32,
) {
    let sender: Validator<T::AuthorityId, T::AccountId> =
        validators[validators.len() - (1 as usize)].clone();
    let root_id: RootId<BlockNumberFor<T>> =
        RootId::new(RootRange::new(0u32.into(), 60u32.into()), 1);
    let signature: <T::AuthorityId as RuntimeAppPublic>::Signature = generate_signature::<T>();
    let quorum = setup_voting_session::<T, I>(&root_id);

    (sender, root_id, signature, quorum)
}

fn setup_voting_session<T: Config<I>, I: 'static>(root_id: &RootId<BlockNumberFor<T>>) -> u32 {
    PendingApproval::<T, I>::insert(root_id.range.clone(), root_id.ingress_counter);

    let quorum = AVN::<T>::quorum();
    let voting_period_end =
        safe_add_block_numbers(<system::Pallet<T>>::block_number(), VotingPeriod::<T, I>::get());
    let current_block_number: BlockNumberFor<T> = 0u32.into();
    VotesRepository::<T, I>::insert(
        root_id,
        VotingSessionData::<T::AccountId, BlockNumberFor<T>>::new(
            root_id.session_id(),
            quorum,
            voting_period_end.expect("already checked"),
            current_block_number,
        ),
    );

    return quorum
}

fn setup_approval_votes<T: Config<I>, I: 'static>(
    validators: &Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>>,
    number_of_votes: u32,
    root_id: &RootId<BlockNumberFor<T>>,
) {
    setup_votes::<T, I>(validators, number_of_votes, root_id, true);
}

fn setup_reject_votes<T: Config<I>, I: 'static>(
    validators: &Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>>,
    number_of_votes: u32,
    root_id: &RootId<BlockNumberFor<T>>,
) {
    setup_votes::<T, I>(validators, number_of_votes, root_id, false);
}

fn setup_votes<T: Config<I>, I: 'static>(
    validators: &Vec<Validator<<T as pallet_avn::Config>::AuthorityId, T::AccountId>>,
    number_of_votes: u32,
    root_id: &RootId<BlockNumberFor<T>>,
    is_approval: bool,
) {
    for i in 0..validators.len() {
        if i < (number_of_votes as usize) {
            match is_approval {
                true => VotesRepository::<T, I>::mutate(root_id, |vote| {
                    vote.ayes
                        .try_push(validators[i].account_id.clone())
                        .expect("Failed to add mock aye vote");
                }),
                false => VotesRepository::<T, I>::mutate(root_id, |vote| {
                    vote.nays
                        .try_push(validators[i].account_id.clone())
                        .expect("Failed to add mock nay vote");
                }),
            }
        }
    }
}

fn advance_block<T: Config<I>, I: 'static>(number: BlockNumberFor<T>) {
    let now = System::<T>::block_number();
    System::<T>::set_block_number(now + number);
}

fn setup_validators<T: Config<I>, I: 'static>(
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

    // setup sender account id and key
    let sender_index = validators.len() - (1 as usize);
    let sender: Validator<T::AuthorityId, T::AccountId> = validators[sender_index].clone();
    let mut account_bytes: [u8; 32] = [0u8; 32];
    account_bytes
        .copy_from_slice(&hex!("be5ddb1579b72e84524fc29e78609e3caf42e85aa118ebfe0b0ad404b5bdd25f")); //Alice stash
    let account_id = T::AccountId::decode(&mut &account_bytes.encode()[..]).unwrap();
    validators[sender_index] = Validator::new(account_id, sender.key);

    // Setup validators in avn pallet
    avn::Validators::<T>::put(WeakBoundedVec::force_from(
        validators.clone(),
        Some("Too many validators for session"),
    ));

    return validators
}

fn setup_roots<T: Config<I>, I: 'static>(
    number_of_roots: u32,
    account_id: T::AccountId,
    start_ingress_counter: IngressCounter,
) {
    ExternalValidationThreshold::<T, I>::put(50u32);
    for i in 0..number_of_roots + 1 {
        Roots::<T, I>::insert(
            RootRange::new(0u32.into(), 60u32.into()),
            start_ingress_counter + i as IngressCounter,
            RootData::new(H256::from([1u8; 32]), account_id.clone(), None),
        );
    }
}

fn setup_record_summary_calculation<T: Config<I>, I: 'static>() -> (
    BlockNumberFor<T>,
    H256,
    IngressCounter,
    <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature,
) {
    let new_block_number: BlockNumberFor<T> = SchedulePeriod::<T, I>::get();
    let root_hash = H256::from(ROOT_HASH_BYTES);
    let ingress_counter: IngressCounter = 100u64.into();
    <TotalIngresses<T, I>>::put(ingress_counter - 1);

    let signature: <T::AuthorityId as RuntimeAppPublic>::Signature = generate_signature::<T>();

    (new_block_number, root_hash, ingress_counter, signature)
}

fn generate_signature<T: pallet_avn::Config>(
) -> <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature {
    let encoded_data = 0.encode();
    let authority_id = T::AuthorityId::generate_pair(None);
    let signature = authority_id.sign(&encoded_data).expect("able to make signature");
    return signature
}

fn assert_last_event<T: Config<I>, I: 'static>(generic_event: <T as Config<I>>::RuntimeEvent) {
    assert_last_nth_event::<T, I>(generic_event, 1);
}

fn assert_last_nth_event<T: Config<I>, I: 'static>(
    generic_event: <T as Config<I>>::RuntimeEvent,
    n: u32,
) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(n as usize)];
    assert_eq!(event, &system_event);
}

fn assert_event_exists<T: Config<I>, I: 'static>(generic_event: <T as Config<I>>::RuntimeEvent) {
    let all_emitted_events = frame_system::Pallet::<T>::events();
    let summary_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();

    assert_eq!(
        true,
        all_emitted_events
            .into_iter()
            .find(|e| {
                let EventRecord { event, .. } = &e;
                event == &summary_event
            })
            .is_some()
    );
}

fn assert_event_not_emitted<T: Config<I>, I: 'static>(
    generic_event: <T as Config<I>>::RuntimeEvent,
) {
    let all_emitted_events = frame_system::Pallet::<T>::events();
    let summary_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();

    assert_eq!(
        false,
        all_emitted_events
            .into_iter()
            .find(|e| {
                let EventRecord { event, .. } = &e;
                event == &summary_event
            })
            .is_some()
    );
}

#[cfg(test)]
fn set_recovered_account_for_tests<T: Config<I>, I: 'static>(
    sender_account_id: &<T as system::Config>::AccountId,
) {
    // AccountId is defined as a u64 in mock.rs, so we need to convert an AccountId to u64 first
    let mut account_bytes: [u8; 8] = Default::default();
    account_bytes.copy_from_slice(&sender_account_id.encode()[0..8]);
    let account_id_as_u64: <mock::TestRuntime as system::Config>::AccountId =
        u64::from_ne_bytes(account_bytes);
    mock::set_mock_recovered_account_id(account_id_as_u64);
}

benchmarks_instance_pallet! {
    set_periods {
        let new_schedule_period: BlockNumberFor<T> = 200u32.into();
        let new_voting_period: BlockNumberFor<T> = 150u32.into();
    }: _(RawOrigin::Root, new_schedule_period, new_voting_period)
    verify {
        assert_eq!(SchedulePeriod::<T, I>::get(), new_schedule_period);
        assert_eq!(VotingPeriod::<T, I>::get(), new_voting_period);
    }

    record_summary_calculation {
        let v in 3 .. MAX_VALIDATOR_ACCOUNTS;
        let r in 1 .. MAX_NUMBER_OF_ROOT_DATA_PER_RANGE;

        let validators = setup_validators::<T, I>(v);
        let validator = validators[validators.len() - (1 as usize)].clone();
        let (new_block_number, root_hash, ingress_counter, signature) = setup_record_summary_calculation::<T, I>();
        setup_roots::<T, I>(r, validator.account_id.clone(), ingress_counter);
        let next_block_to_process = NextBlockToProcess::<T, I>::get();
    }: _(RawOrigin::None, new_block_number, root_hash, ingress_counter, validator.clone(), signature)
    verify {
        let range = RootRange::new(next_block_to_process, new_block_number);
        let root = Roots::<T, I>::get(range, ingress_counter);

        assert_eq!(<TotalIngresses<T, I>>::get(), ingress_counter);
        assert!(PendingApproval::<T, I>::contains_key(range));
        assert_eq!(true, VotesRepository::<T, I>::contains_key(RootId::new(range, ingress_counter)));
        assert_last_event::<T, I>(Event::<T, I>::SummaryCalculated {
            from: next_block_to_process,
            to: new_block_number,
            root_hash: root_hash,
            submitter: validator.account_id
        }.into());
    }

    approve_root_with_end_voting {
        let v in 3 .. MAX_VALIDATOR_ACCOUNTS;
        let o in 1 .. MAX_OFFENDERS;

        let mut validators = setup_validators::<T, I>(v);
        let (sender, root_id,  signature, quorum) = setup_publish_root_voting::<T, I>(validators.clone());
        validators.remove(validators.len() - (1 as usize)); // Avoid setting up sender to approve vote automatically

        setup_roots::<T, I>(1, sender.account_id.clone(), root_id.ingress_counter);

        // Setup votes more than quorum to trigger end voting period
        let number_of_votes = quorum;
        setup_approval_votes::<T, I>(&validators, number_of_votes, &root_id);

        let mut reject_voters = validators.clone();
        reject_voters.reverse();
        setup_reject_votes::<T, I>(&reject_voters, o, &root_id);

        CurrentSlot::<T, I>::put::<BlockNumberFor<T>>(3u32.into());

        //In test mode, we want to set the recovered account (when verifying ECDSA signature) as a validator
        #[cfg(test)]
        set_recovered_account_for_tests::<T, I>(&sender.account_id);

    }: approve_root(RawOrigin::None, root_id, sender.clone(),  signature)
    verify {
        let vote = VotesRepository::<T, I>::get(&root_id);
        assert_eq!(true, vote.ayes.contains(&sender.account_id));

        assert_eq!(true, NextBlockToProcess::<T, I>::get() == root_id.range.to_block + 1u32.into());
        assert_eq!(true, Roots::<T, I>::get(root_id.range, root_id.ingress_counter).is_validated);
        assert_eq!(true, SlotOfLastPublishedSummary::<T, I>::get() == CurrentSlot::<T, I>::get());
        assert_eq!(false, PendingApproval::<T, I>::contains_key(&root_id.range));

        let vote = VotesRepository::<T, I>::get(&root_id);


        assert_last_nth_event::<T, I>(Event::<T, I>::SummaryOffenceReported {
                offence_type: SummaryOffenceType::RejectedValidRoot,
                offenders: create_offenders_identification::<T, I>(&vote.nays)
            }.into(),
            4
        );
        let root_data = Roots::<T, I>::get(root_id.range, root_id.ingress_counter);
        assert_last_nth_event::<T, I>(
            Event::<T, I>::SummaryRootValidated {
                root_hash: root_data.root_hash,
                ingress_counter: root_id.ingress_counter,
                block_range: root_id.range
            }.into(),
            3
        );

        assert_last_nth_event::<T, I>(
            Event::<T, I>::VotingEnded {
                root_id: root_id.clone(),
                vote_approved: true
            }.into(),
            2
        );

        assert_last_event::<T, I>(Event::<T, I>::VoteAdded {
                voter: sender.account_id.clone(),
                root_id: root_id,
                agree_vote: true
            }.into()
        );
    }

    approve_root_without_end_voting {
        let v in 4 .. MAX_VALIDATOR_ACCOUNTS;
        let validators = setup_validators::<T, I>(v);
        let (sender, root_id,  signature, quorum) = setup_publish_root_voting::<T, I>(validators.clone());
        setup_roots::<T, I>(1, sender.account_id.clone(), root_id.ingress_counter - 1);

        CurrentSlot::<T, I>::put::<BlockNumberFor<T>>(3u32.into());
    }: approve_root(RawOrigin::None, root_id, sender.clone(), signature)
    verify {
        let vote = VotesRepository::<T, I>::get(&root_id);
        assert_eq!(true, vote.ayes.contains(&sender.account_id));

        assert_eq!(false, NextBlockToProcess::<T, I>::get() == root_id.range.to_block + 1u32.into());
        assert_eq!(false, Roots::<T, I>::get(root_id.range, root_id.ingress_counter).is_validated);
        assert_eq!(false, SlotOfLastPublishedSummary::<T, I>::get() == CurrentSlot::<T, I>::get());
        assert_eq!(true, PendingApproval::<T, I>::contains_key(&root_id.range));

        assert_last_event::<T, I>(Event::<T, I>::VoteAdded {
            voter: sender.account_id,
            root_id: root_id.clone(),
            agree_vote: true
        }.into());
    }

    reject_root_with_end_voting {
        let v in 7 .. MAX_VALIDATOR_ACCOUNTS;
        let o in 1 .. MAX_OFFENDERS;

        let mut validators = setup_validators::<T, I>(v);
        let (sender, root_id, signature, quorum) = setup_publish_root_voting::<T, I>(validators.clone());
        validators.remove(validators.len() - (1 as usize)); // Avoid setting up sender to reject vote automatically

        setup_roots::<T, I>(1, sender.account_id.clone(), root_id.ingress_counter);

        // Setup votes more than quorum to trigger end voting period
        let reject_voters = quorum;
        setup_reject_votes::<T, I>(&validators, reject_voters, &root_id);

        let mut approve_voters = validators.clone();
        approve_voters.reverse();
        setup_approval_votes::<T, I>(&approve_voters, o, &root_id);
    }: reject_root(RawOrigin::None, root_id.clone(), sender.clone(), signature)
    verify {
        assert_eq!(false, NextBlockToProcess::<T, I>::get() == root_id.range.to_block + 1u32.into());
        assert_eq!(false, Roots::<T, I>::get(root_id.range, root_id.ingress_counter).is_validated);
        assert_eq!(false, SlotOfLastPublishedSummary::<T, I>::get() == CurrentSlot::<T, I>::get() + 1u32.into());

        assert_eq!(false, PendingApproval::<T, I>::contains_key(&root_id.range));

        let root_data = Roots::<T, I>::get(root_id.range, root_id.ingress_counter);
        assert_event_exists::<T, I>(Event::<T, I>::SummaryOffenceReported {
                offence_type: SummaryOffenceType::CreatedInvalidRoot,
                offenders: create_offenders_identification::<T, I>(&vec![root_data.added_by.unwrap()])
            }.into()
        );

        let vote = VotesRepository::<T, I>::get(&root_id);
        assert_event_exists::<T, I>(Event::<T, I>::SummaryOffenceReported {
                offence_type: SummaryOffenceType::ApprovedInvalidRoot,
                offenders: create_offenders_identification::<T, I>(&vote.ayes)
            }.into()
        );

        assert_event_exists::<T, I>(
            Event::<T, I>::VotingEnded {
                root_id: root_id.clone(),
                vote_approved: false
            }.into()
        );

        assert_last_event::<T, I>(Event::<T, I>::VoteAdded {
            voter: sender.account_id,
            root_id: root_id.clone(),
            agree_vote: false
        }.into());
    }

    reject_root_without_end_voting {
        let v in 4 .. MAX_VALIDATOR_ACCOUNTS;
        let mut validators = setup_validators::<T, I>(v);
        let (sender, root_id,  signature, quorum) = setup_publish_root_voting::<T, I>(validators.clone());
        validators.remove(validators.len() - (1 as usize)); // Avoid setting up sender to reject vote automatically

        setup_roots::<T, I>(1, sender.account_id.clone(), root_id.ingress_counter);
    }: reject_root(RawOrigin::None, root_id.clone(), sender.clone(), signature)
    verify {
        assert_eq!(false, NextBlockToProcess::<T, I>::get() == root_id.range.to_block + 1u32.into());
        assert_eq!(false, Roots::<T, I>::get(root_id.range, root_id.ingress_counter).is_validated);
        assert_eq!(false, SlotOfLastPublishedSummary::<T, I>::get() == CurrentSlot::<T, I>::get() + 1u32.into());

        assert_eq!(true, PendingApproval::<T, I>::contains_key(&root_id.range));

        assert_last_event::<T, I>(Event::<T, I>::VoteAdded {
            voter: sender.account_id,
            root_id: root_id.clone(),
            agree_vote: false
        }.into());
    }

    end_voting_period_with_rejected_valid_votes {
        let v in 7 .. MAX_VALIDATOR_ACCOUNTS;
        let o in 1 .. MAX_OFFENDERS;
        let validators = setup_validators::<T, I>(v);
        let (sender, root_id,  signature, quorum) = setup_publish_root_voting::<T, I>(validators.clone());
        setup_roots::<T, I>(1, sender.account_id.clone(), root_id.ingress_counter);

        let current_slot_number: BlockNumberFor<T> = 3u32.into();
        CurrentSlot::<T, I>::put(current_slot_number);

        // Setup votes more than quorum to trigger end voting period
        let number_of_approval_votes = quorum;
        setup_approval_votes::<T, I>(&validators, number_of_approval_votes, &root_id);

        // setup offenders votes
        let (_, offenders) = validators.split_at(quorum as usize);
        setup_reject_votes::<T, I>(&offenders.to_vec(), o, &root_id);
    }: end_voting_period(RawOrigin::None, root_id.clone(), sender.clone(), signature)
    verify {
        assert_eq!(true, NextBlockToProcess::<T, I>::get() == root_id.range.to_block + 1u32.into());
        assert_eq!(true, Roots::<T, I>::get(root_id.range, root_id.ingress_counter).is_validated);
        assert_eq!(true, SlotOfLastPublishedSummary::<T, I>::get() == CurrentSlot::<T, I>::get());
        assert_eq!(false, PendingApproval::<T, I>::contains_key(&root_id.range));

        let vote = VotesRepository::<T, I>::get(&root_id);
        assert_event_exists::<T, I>(Event::<T, I>::SummaryOffenceReported {
                offence_type: SummaryOffenceType::RejectedValidRoot,
                offenders: create_offenders_identification::<T, I>(&vote.nays)
            }.into()
        );

        assert_last_event::<T, I>(
            Event::<T, I>::VotingEnded {
                root_id: root_id.clone(),
                vote_approved: true,
            }.into());
    }

    end_voting_period_with_approved_invalid_votes {
        let v in 7 .. MAX_VALIDATOR_ACCOUNTS;
        let o in 1 .. MAX_OFFENDERS;
        let validators = setup_validators::<T, I>(v);
        let (sender, root_id,  signature, quorum) = setup_publish_root_voting::<T, I>(validators.clone());
        setup_roots::<T, I>(1, sender.account_id.clone(), root_id.ingress_counter);

        let current_slot_number: BlockNumberFor<T> = 3u32.into();
        CurrentSlot::<T, I>::put(current_slot_number);

        // Setup votes more than quorum to trigger end voting period
        let number_of_reject_votes = quorum;
        setup_reject_votes::<T, I>(&validators, number_of_reject_votes, &root_id);

        // setup offenders votes
        let (_, offenders) = validators.split_at(quorum as usize);
        setup_approval_votes::<T, I>(&offenders.to_vec(), o, &root_id);
    }: end_voting_period(RawOrigin::None, root_id.clone(), sender.clone(), signature)
    verify {
        assert_eq!(false, NextBlockToProcess::<T, I>::get() == root_id.range.to_block + 1u32.into());
        assert_eq!(false, Roots::<T, I>::get(root_id.range, root_id.ingress_counter).is_validated);
        assert_eq!(false, SlotOfLastPublishedSummary::<T, I>::get() == CurrentSlot::<T, I>::get());
        assert_eq!(false, PendingApproval::<T, I>::contains_key(&root_id.range));

        let vote = VotesRepository::<T, I>::get(&root_id);
        assert_event_exists::<T, I>(Event::<T, I>::SummaryOffenceReported {
                offence_type: SummaryOffenceType::ApprovedInvalidRoot,
                offenders: create_offenders_identification::<T, I>(&vote.ayes)
            }.into()
        );

        assert_last_event::<T, I>(
            Event::<T, I>::VotingEnded {
                root_id: root_id.clone(),
                vote_approved: false
            }.into()
        );
    }

    advance_slot_with_offence {
        // There can only be 1 offender here (the validator that failed to create a summary) so skip using MAX_OFFENDERS
        let v in 5 .. MAX_VALIDATOR_ACCOUNTS;
        let validators = setup_validators::<T, I>(v);
        let (sender, _, signature, quorum) = setup_publish_root_voting::<T, I>(validators);

        advance_block::<T, I>(SchedulePeriod::<T, I>::get());
        CurrentSlotsValidator::<T, I>::put(sender.account_id.clone());

        // Create an offence: last published summary slot number < current slot number
        let old_slot_number: BlockNumberFor<T> = 2u32.into();
        CurrentSlot::<T, I>::put(old_slot_number);

        let last_summary_slot: BlockNumberFor<T> = 1u32.into();
        SlotOfLastPublishedSummary::<T, I>::put(last_summary_slot);

        let old_new_slot_start = NextSlotAtBlock::<T, I>::get();
    }: advance_slot(RawOrigin::None, sender.clone(), signature)
    verify {
        let new_slot_number = CurrentSlot::<T, I>::get();
        let new_validator = CurrentSlotsValidator::<T, I>::get();
        let new_slot_start = NextSlotAtBlock::<T, I>::get();

        assert_eq!(new_slot_number, old_slot_number + 1u32.into());
        assert_eq!(false, new_validator == Some(sender.account_id.clone()));
        assert_last_event::<T, I>(Event::<T, I>::SlotAdvanced {
            advanced_by: sender.account_id.clone(),
            new_slot: new_slot_number,
            slot_validator: new_validator.unwrap(),
            slot_end: new_slot_start
        }.into());

        assert_event_exists::<T, I>(
            Event::<T, I>::SummaryNotPublishedOffence {
                challengee: sender.account_id.clone(),
                void_slot: old_slot_number,
                last_published: last_summary_slot,
                end_vote: old_new_slot_start
            }.into()
        );

        // TODO: assert_emitted_event_for_offence_of_type(SummaryOffenceType::SlotNotAdvanced);
    }

    advance_slot_without_offence {
        // No offence committed, so skip using MAX_OFFENDERS
        let v in 3 .. MAX_VALIDATOR_ACCOUNTS;
        let validators = setup_validators::<T, I>(v);
        let (sender, _, signature, _) = setup_publish_root_voting::<T, I>(validators.clone());

        advance_block::<T, I>(SchedulePeriod::<T, I>::get());
        CurrentSlotsValidator::<T, I>::put(sender.account_id.clone());

        let old_slot_number = CurrentSlot::<T, I>::get();
    }: advance_slot(RawOrigin::None, sender.clone(), signature)
    verify {
        let new_slot_number = CurrentSlot::<T, I>::get();
        let new_validator = CurrentSlotsValidator::<T, I>::get();
        let new_slot_start = NextSlotAtBlock::<T, I>::get();

        assert_eq!(new_slot_number, old_slot_number + 1u32.into());
        assert_eq!(false, new_validator == Some(sender.account_id.clone()));
        assert_last_event::<T, I>(Event::<T, I>::SlotAdvanced {
            advanced_by: sender.account_id,
            new_slot: new_slot_number,
            slot_validator: new_validator.unwrap(),
            slot_end: new_slot_start
        }.into());
    }

    add_challenge {
        // There can only be 1 offender here (the validator that failed to advance the slot) so skip using MAX_OFFENDERS
        let v in 3 .. MAX_VALIDATOR_ACCOUNTS;
        let validators = setup_validators::<T, I>(v);
        let (sender, _,  signature, _) = setup_publish_root_voting::<T, I>(validators.clone());

        let current_block_number = SchedulePeriod::<T, I>::get() + T::MinBlockAge::get();
        let next_slot_at_block: BlockNumberFor<T> = current_block_number - T::AdvanceSlotGracePeriod::get() - 1u32.into();
        let current_slot_number: BlockNumberFor<T> = 3u32.into();
        let slot_number_to_challenge_as_u32: u32 = AVN::<T>::convert_block_number_to_u32(current_slot_number).expect("valid u32 value");

        advance_block::<T, I>(current_block_number);
        NextSlotAtBlock::<T, I>::put(next_slot_at_block);
        CurrentSlot::<T, I>::put(current_slot_number);
        SlotOfLastPublishedSummary::<T, I>::put(current_slot_number - 1u32.into());
        CurrentSlotsValidator::<T, I>::put(validators[1].account_id.clone());

        let challenge: SummaryChallenge<T::AccountId> = SummaryChallenge {
            challenge_reason: SummaryChallengeReason::SlotNotAdvanced(slot_number_to_challenge_as_u32),
            challenger: sender.account_id.clone(),
            challengee: validators[1].account_id.clone()
        };
    }: _(RawOrigin::None, challenge.clone(), sender.clone(), signature)
    verify {
        let new_slot_number = CurrentSlot::<T, I>::get();
        let new_validator = CurrentSlotsValidator::<T, I>::get();
        let new_slot_start = NextSlotAtBlock::<T, I>::get();

        assert_eq!(new_slot_number, current_slot_number + 1u32.into());

        assert_event_exists::<T, I>(Event::<T, I>::SummaryOffenceReported {
                offence_type: SummaryOffenceType::SlotNotAdvanced,
                offenders: create_offenders_identification::<T, I>(&vec![validators[1].account_id.clone()])
            }.into()
        );

        assert_event_exists::<T, I>(Event::<T, I>::SummaryNotPublishedOffence {
                challengee: validators[1].account_id.clone(),
                void_slot: current_slot_number,
                last_published: current_slot_number - 1u32.into(),
                end_vote: next_slot_at_block
            }.into()
        );

        assert_event_exists::<T, I>(Event::<T, I>::SlotAdvanced {
            advanced_by: sender.account_id,
            new_slot: new_slot_number,
            slot_validator: new_validator.unwrap(),
            slot_end: new_slot_start
            }.into()
        );

        assert_last_event::<T, I>(
            Event::<T, I>::ChallengeAdded {
                challenge_reason: challenge.challenge_reason.clone(),
                challenger: challenge.challenger,
                challengee: challenge.challengee
            }.into()
        );
    }

    admin_resolve_challenge_accepted {
        let validators = setup_validators::<T, I>(3u32);
        let (sender, root_id,  signature, quorum) = setup_publish_root_voting::<T, I>(validators.clone());
        setup_roots::<T, I>(1, sender.account_id.clone(), root_id.ingress_counter);
        let passed = true;

        let external_validation_status = ExternalValidationEnum::PendingAdminReview;
        let external_validation_data = ExternalValidationData {
            proposal_id: ProposalId::from_slice(&[5u8; 32]),
            external_ref: H256::from_slice(&[2u8; 32]),
            proposal_status: ProposalStatusEnum::Resolved { passed },
        };

        PendingAdminReviews::<T, I>::insert(root_id, external_validation_data);
        ExternalValidationStatus::<T, I>::insert(root_id, external_validation_status);

    }: admin_resolve_challenge(RawOrigin::Root, root_id, passed)
    verify {
        assert_eq!(false, PendingAdminReviews::<T, I>::contains_key(&root_id));
        assert_eq!(false, ExternalValidationStatus::<T, I>::contains_key(&root_id));

        let root_data = Roots::<T, I>::get(root_id.range, root_id.ingress_counter);
        assert_event_exists::<T, I>(
            Event::<T, I>::RootPassedValidation {root_id, root_hash: root_data.root_hash}.into()
        );

        assert_last_event::<T, I>(
            Event::<T, I>::RootChallengeResolved {root_id, accepted: passed}.into()
        );
    }

    admin_resolve_challenge_rejected {
        let validators = setup_validators::<T, I>(3u32);
        let (sender, root_id,  signature, quorum) = setup_publish_root_voting::<T, I>(validators.clone());
        setup_roots::<T, I>(1, sender.account_id.clone(), root_id.ingress_counter);
        let passed = false;

        let external_validation_status = ExternalValidationEnum::PendingAdminReview;
        let external_validation_data = ExternalValidationData {
            proposal_id: ProposalId::from_slice(&[5u8; 32]),
            external_ref: H256::from_slice(&[2u8; 32]),
            proposal_status: ProposalStatusEnum::Resolved { passed },
        };

        PendingAdminReviews::<T, I>::insert(root_id, external_validation_data);
        ExternalValidationStatus::<T, I>::insert(root_id, external_validation_status);

    }: admin_resolve_challenge(RawOrigin::Root, root_id, passed)
    verify {
        assert_eq!(false, PendingAdminReviews::<T, I>::contains_key(&root_id));
        assert_eq!(false, ExternalValidationStatus::<T, I>::contains_key(&root_id));

        let root_data = Roots::<T, I>::get(root_id.range, root_id.ingress_counter);
        assert_event_not_emitted::<T, I>(
            Event::<T, I>::RootPassedValidation {root_id, root_hash: root_data.root_hash}.into()
        );

        assert_last_event::<T, I>(
            Event::<T, I>::RootChallengeResolved {root_id, accepted: passed}.into()
        );
    }

    set_external_validation_threshold {
        let new_threshold = 51u32;
        let config = AdminConfig::ExternalValidationThreshold(new_threshold);
    }: set_admin_config(RawOrigin::Root, config)
    verify {
        assert!(<ExternalValidationThreshold<T, I>>::get() == Some(new_threshold));
    }

    set_schedule_period {
        let new_period: BlockNumberFor<T> = 106u32.into();
        let config = AdminConfig::SchedulePeriod(new_period);
    }: set_admin_config(RawOrigin::Root, config)
    verify {
        assert!(<SchedulePeriod<T, I>>::get() == new_period);
    }

    set_voting_period {
        let new_period: BlockNumberFor<T> = 101u32.into();
        let config = AdminConfig::VotingPeriod(new_period);
    }: set_admin_config(RawOrigin::Root, config)
    verify {
        assert!(<VotingPeriod<T, I>>::get() == new_period);
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default()
        .with_validators()
        .with_genesis_config()
        .as_externality(),
    crate::mock::TestRuntime,
);
