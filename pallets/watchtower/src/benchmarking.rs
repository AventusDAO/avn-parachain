// Copyright 2026 Aventus DAO.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_system::{EventRecord, RawOrigin};
use sp_avn_common::Proof;
use sp_core::{crypto::DEV_PHRASE, ByteArray};
use sp_runtime::{traits::Hash, SaturatedConversion};

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(1 as usize)];
    assert_eq!(event, &system_event);
}

fn create_proposal<T: Config>(
    external_ref_id: u32,
    created_at: BlockNumberFor<T>,
    end_at: Option<BlockNumberFor<T>>,
    is_internal: bool,
) -> Proposal<T> {
    let external_ref: T::Hash = T::Hashing::hash_of(&external_ref_id);
    let inner_payload = BoundedVec::try_from(external_ref_id.encode()).unwrap();
    let source: ProposalSource;
    let proposer: Option<T::AccountId>;

    if is_internal {
        source = ProposalSource::Internal(ProposalType::Governance);
        proposer = None;
    } else {
        source = ProposalSource::External;
        proposer = Some(account("proposer", 0, 0));
    };

    Proposal {
        title: BoundedVec::try_from("Bench proposal".as_bytes().to_vec()).unwrap(),
        external_ref: H256::from_slice(&external_ref.as_ref()),
        threshold: Perbill::from_percent(50),
        payload: Payload::Inline(inner_payload),
        source,
        proposer,
        decision_rule: DecisionRule::SimpleMajority,
        created_at,
        vote_duration: if let Some(end) = end_at {
            end.saturating_sub(created_at).saturated_into::<u32>()
        } else {
            MinVotingPeriod::<T>::get().saturated_into::<u32>()
        },
        end_at,
    }
}

fn create_proposal_request<T: Config>(
    external_ref_id: u32,
    created_at: u32,
    is_internal: bool,
) -> ProposalRequest {
    let external_ref: T::Hash = T::Hashing::hash_of(&external_ref_id);
    let source: ProposalSource;

    if is_internal {
        source = ProposalSource::Internal(ProposalType::Governance);
    } else {
        source = ProposalSource::External;
    };

    ProposalRequest {
        title: "Dummy Proposal".as_bytes().to_vec(),
        external_ref: H256::from_slice(&external_ref.as_ref()),
        threshold: Perbill::from_percent(50),
        payload: RawPayload::Uri(external_ref_id.encode()),
        source,
        decision_rule: DecisionRule::SimpleMajority,
        created_at,
        vote_duration: Some(MinVotingPeriod::<T>::get().saturated_into::<u32>() + 1u32),
    }
}

fn set_active_proposal<T: Config>(proposal_id: H256, created_at: u32, length: u32) -> Proposal<T> {
    let created_at: BlockNumberFor<T> = created_at.into();
    let active_proposal =
        create_proposal::<T>(1, created_at, Some(created_at + length.into()), true);
    Proposals::<T>::insert(proposal_id, &active_proposal);
    ActiveInternalProposal::<T>::put(proposal_id);
    ProposalStatus::<T>::insert(proposal_id, ProposalStatusEnum::Active);
    active_proposal
}

fn queue_proposal<T: Config>(proposal_id: H256, created_at: u32) -> Proposal<T> {
    let created_at: BlockNumberFor<T> = created_at.into();
    let queued_proposal = create_proposal::<T>(2, created_at, None, true);
    Proposals::<T>::insert(proposal_id, &queued_proposal);
    Pallet::<T>::enqueue(proposal_id).unwrap();
    ProposalStatus::<T>::insert(proposal_id, ProposalStatusEnum::Queued);
    queued_proposal
}

fn get_proof<T: Config>(
    relayer: &T::AccountId,
    signer: &T::AccountId,
    signature: &[u8],
) -> Proof<T::Signature, T::AccountId> {
    return Proof {
        signer: signer.clone(),
        relayer: relayer.clone(),
        signature: sp_core::sr25519::Signature::from_slice(signature).unwrap().into(),
    }
}

fn get_voter<T: Config>() -> (T::SignerId, T::AccountId) {
    let mnemonic: &str = DEV_PHRASE;
    let key_pair = T::SignerId::generate_pair(Some(mnemonic.as_bytes().to_vec()));
    let account_bytes = into_bytes::<T>(&key_pair);
    let account_id = T::AccountId::decode(&mut &account_bytes.encode()[..]).unwrap();
    return (key_pair, account_id)
}

fn into_bytes<T: Config>(account: &T::SignerId) -> [u8; 32] {
    let bytes = account.encode();
    let mut vector: [u8; 32] = Default::default();
    vector.copy_from_slice(&bytes[0..32]);
    return vector
}

fn setup_votes<T: Config>(proposal_id: ProposalId, vote_count: u32) {
    Votes::<T>::mutate(proposal_id, |v| {
        v.in_favors = vote_count;
        v.againsts = vote_count;
    });

    for i in 0..vote_count {
        let voter: T::AccountId = account("voter", i, 0);
        Voters::<T>::insert(proposal_id, &voter, true);
    }
}

benchmarks! {
    submit_external_proposal {
        let signer: T::AccountId = account("signer", 0, 0);
        let proposal_request = create_proposal_request::<T>(1, 1u32, false);
        let external_ref = proposal_request.external_ref;
    }: submit_external_proposal(RawOrigin::Signed(signer), proposal_request)
    verify {
        assert!(ExternalRef::<T>::contains_key(external_ref));

        let proposal_id = ExternalRef::<T>::get(external_ref);
        assert!(ProposalStatus::<T>::get(proposal_id) == ProposalStatusEnum::Active);
        assert_last_event::<T>(
            Event::ProposalSubmitted { proposal_id, external_ref, status: ProposalStatusEnum::Active }.into()
        );
    }

    signed_submit_external_proposal {
        let (signer_key, signer) = get_voter::<T>();
        let proposal_request = create_proposal_request::<T>(1, 1u32, false);
        let external_ref = proposal_request.external_ref;
        let relayer: T::AccountId = account("relayer", 11, 11);
        let now = frame_system::Pallet::<T>::block_number();
        let signed_payload = Pallet::<T>::encode_signed_submit_external_proposal_params(
            &relayer.clone(),
            &proposal_request,
            &now,
        );

        let signature = signer_key.sign(&signed_payload).unwrap().encode();
        let proof = get_proof::<T>(&relayer.clone(), &signer, &signature);
    }: signed_submit_external_proposal(RawOrigin::Signed(signer), proof, proposal_request, now)
    verify {
        assert!(ExternalRef::<T>::contains_key(external_ref));

        let proposal_id = ExternalRef::<T>::get(external_ref);
        assert!(ProposalStatus::<T>::get(proposal_id) == ProposalStatusEnum::Active);
        assert_last_event::<T>(
            Event::ProposalSubmitted { proposal_id, external_ref, status: ProposalStatusEnum::Active }.into()
        );
    }

    vote {
        let in_favor = true;
        let (_, voter) = get_voter::<T>();

        let proposal_id = H256::repeat_byte(3);
        let _ = set_active_proposal::<T>(proposal_id, 1u32, 50u32);
    }: vote(RawOrigin::Signed(voter.clone()), proposal_id, in_favor)
    verify {
        assert!(Votes::<T>::contains_key(proposal_id));
        assert!(Voters::<T>::contains_key(proposal_id, &voter));
        assert_last_event::<T>(
            Event::VoteSubmitted { proposal_id, voter, in_favor, vote_weight: 1 }.into()
        );
    }

    vote_end_proposal {
        let in_favor = true;
        let (_, voter) = get_voter::<T>();

        let proposal_id = H256::repeat_byte(3);
        let proposal = set_active_proposal::<T>(proposal_id, 1u32, 50u32);
        // Add some votes to be above the threshold
        setup_votes::<T>(proposal_id, 9u32);
    }: vote(RawOrigin::Signed(voter.clone()), proposal_id, in_favor)
    verify {
        assert!(Votes::<T>::contains_key(proposal_id));
        assert!(Voters::<T>::contains_key(proposal_id, &voter));
        assert_last_event::<T>(
            Event::VotingEnded {
                proposal_id,
                external_ref: proposal.external_ref,
                consensus_result: ProposalStatusEnum::Resolved { passed: true }}.into()
        );
    }

    signed_vote {
        let (voter_key, voter) = get_voter::<T>();
        let in_favor = true;
        let relayer: T::AccountId = account("relayer", 11, 11);
        let now = frame_system::Pallet::<T>::block_number();
        let proposal_id = H256::repeat_byte(3);
        let _ = set_active_proposal::<T>(proposal_id, 1u32, 50u32);

        let signed_payload = Pallet::<T>::encode_signed_submit_vote_params(
            &relayer.clone(),
            &proposal_id,
            &in_favor,
            &now,
        );

        let signature = voter_key.sign(&signed_payload).unwrap().encode();
        let proof = get_proof::<T>(&relayer.clone(), &voter, &signature);
    }: signed_vote(RawOrigin::Signed(voter.clone()), proof, proposal_id, in_favor, now)
    verify {
        assert!(Votes::<T>::contains_key(proposal_id));
        assert!(Voters::<T>::contains_key(proposal_id, &voter));
        assert_last_event::<T>(
            Event::VoteSubmitted { proposal_id, voter, in_favor, vote_weight: 1 }.into()
        );
    }

    signed_vote_end_proposal {
        let (voter_key, voter) = get_voter::<T>();
        let in_favor = true;
        let relayer: T::AccountId = account("relayer", 11, 11);
        let now = frame_system::Pallet::<T>::block_number();
        let proposal_id = H256::repeat_byte(3);
        let proposal = set_active_proposal::<T>(proposal_id, 1u32, 50u32);

        let signed_payload = Pallet::<T>::encode_signed_submit_vote_params(
            &relayer.clone(),
            &proposal_id,
            &in_favor,
            &now,
        );

        let signature = voter_key.sign(&signed_payload).unwrap().encode();
        let proof = get_proof::<T>(&relayer.clone(), &voter, &signature);

        // Add some votes to be above the threshold
        setup_votes::<T>(proposal_id, 9u32);
    }: signed_vote(RawOrigin::Signed(voter.clone()), proof, proposal_id, in_favor, now)
    verify {
        assert!(Votes::<T>::contains_key(proposal_id));
        assert!(Voters::<T>::contains_key(proposal_id, &voter));
        assert_last_event::<T>(
            Event::VotingEnded {
                proposal_id,
                external_ref: proposal.external_ref,
                consensus_result: ProposalStatusEnum::Resolved { passed: true }}.into()
        );
    }

    unsigned_vote {
        let (voter_key, voter) = get_voter::<T>();
        let in_favor = true;
        let proposal_id = H256::repeat_byte(3);
        let _ = set_active_proposal::<T>(proposal_id, 1u32, 50u32);

        let proof =  &(WATCHTOWER_UNSIGNED_VOTE_CONTEXT, proposal_id, in_favor, &voter).encode();
        let signature = voter_key.sign(&proof).unwrap();
    }: unsigned_vote(RawOrigin::None, proposal_id, in_favor, voter.clone(), signature.into())
    verify {
        assert!(Votes::<T>::contains_key(proposal_id));
        assert!(Voters::<T>::contains_key(proposal_id, &voter));
        assert_last_event::<T>(
            Event::VoteSubmitted { proposal_id, voter, in_favor, vote_weight: 1 }.into()
        );
    }

    unsigned_vote_end_proposal {
        let (voter_key, voter) = get_voter::<T>();
        let in_favor = true;
        let proposal_id = H256::repeat_byte(3);
        let proposal = set_active_proposal::<T>(proposal_id, 1u32, 50u32);

        let proof =  &(WATCHTOWER_UNSIGNED_VOTE_CONTEXT, proposal_id, in_favor, &voter).encode();
        let signature = voter_key.sign(&proof).unwrap();
        // Add some votes to be above the threshold
        setup_votes::<T>(proposal_id, 9u32);
    }: unsigned_vote(RawOrigin::None, proposal_id, in_favor, voter.clone(), signature.into())
    verify {
        assert!(Votes::<T>::contains_key(proposal_id));
        assert!(Voters::<T>::contains_key(proposal_id, &voter));
        assert_last_event::<T>(
            Event::VotingEnded {
                proposal_id,
                external_ref: proposal.external_ref,
                consensus_result: ProposalStatusEnum::Resolved { passed: true }}.into()
        );
    }

    finalise_proposal {
        let signer: T::AccountId = account("signer", 0, 0);
        let proposal_id = H256::repeat_byte(3);
        let queued_proposal_id = H256::repeat_byte(7);
        <frame_system::Pallet<T>>::set_block_number(100u32.into());
        let _ = set_active_proposal::<T>(proposal_id, 5u32, 50u32);
        let _ = queue_proposal::<T>(queued_proposal_id, 100u32);
    }: finalise_proposal(RawOrigin::Signed(signer), proposal_id)
    verify {
        assert!(ProposalStatus::<T>::get(proposal_id) == ProposalStatusEnum::Expired);
        assert!(ProposalStatus::<T>::get(queued_proposal_id) == ProposalStatusEnum::Active);
        assert!(ActiveInternalProposal::<T>::get() == Some(queued_proposal_id));
    }

    set_admin_config_voting {
        let new_period: BlockNumberFor<T> = 36u32.into();
        let config = AdminConfig::MinVotingPeriod(new_period);
    }: set_admin_config(RawOrigin::Root, config)
    verify {
        assert!(<MinVotingPeriod<T>>::get() == new_period);
    }

    set_admin_config_account {
        let new_account: Option<T::AccountId> = Some(account("new_account", 0, 0));
        let config = AdminConfig::AdminAccount(new_account.clone());
    }: set_admin_config(RawOrigin::Root, config)
    verify {
        assert!(<AdminAccount<T>>::get() == new_account);
    }

    active_proposal_expiry_status {
        <frame_system::Pallet<T>>::set_block_number(100u32.into());

        // Pick internal because it has more logic
        let proposal_id = H256::repeat_byte(3);
        let _ = set_active_proposal::<T>(proposal_id, 5u32, 50u32);
        let now = <frame_system::Pallet<T>>::block_number();
        let mut id = H256::zero();
        let mut expired = false;
    }: {
        let result = Pallet::<T>::active_proposal_expiry_status(now);
        let (p_id, _p, p_expired) = result.expect("expired proposal exists");
        id = p_id;
        expired = p_expired;
     }
    verify {
        assert!(expired == true);
        assert!(id == proposal_id);
    }

    finalise_expired_voting {
        <frame_system::Pallet<T>>::set_block_number(100u32.into());

        let proposal_id = H256::repeat_byte(12);
        let active_proposal = set_active_proposal::<T>(proposal_id, 5u32, 50u32);

        let queued_proposal_id = H256::repeat_byte(7);
        let _ = queue_proposal::<T>(queued_proposal_id, 100u32);
    }: { let _ = Pallet::<T>::finalise_expired_voting(proposal_id, &active_proposal); }
    verify {
        assert!(ProposalStatus::<T>::get(proposal_id) == ProposalStatusEnum::Expired);
        assert!(ProposalStatus::<T>::get(queued_proposal_id) == ProposalStatusEnum::Active);
        assert!(ActiveInternalProposal::<T>::get() == Some(queued_proposal_id));
    }

}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().as_externality(),
    crate::mock::TestRuntime,
);
