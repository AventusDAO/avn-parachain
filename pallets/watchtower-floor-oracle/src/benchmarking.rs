// Copyright 2026 Aventus DAO Ltd

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_support::WeakBoundedVec;
use frame_system::RawOrigin;

const CHAIN_ID: ExternalChainId = 1;
const MAX_AGE_SECS: u64 = 600;
const SEED_BLOCK: u32 = 100;
const VOTED_BLOCK: u32 = 1_000;

fn setup_validators<T: Config>(count: u32) -> Vec<Author<T>> {
    let mnemonic: &str =
        "basic anxiety marine match castle rival moral whisper insane away avoid bike";
    let validators: Vec<Author<T>> = (0..count)
        .map(|i| {
            let account = account("validator", i, i);
            let key =
                <T as avn::Config>::AuthorityId::generate_pair(Some(mnemonic.as_bytes().to_vec()));
            Validator::new(account, key)
        })
        .collect();

    avn::Validators::<T>::put(WeakBoundedVec::force_from(
        validators.clone(),
        Some("Too many validators for session"),
    ));

    validators
}

fn seed_default_chain<T: Config>() {
    assert!(
        Pallet::<T>::seed_chain(RawOrigin::Root.into(), CHAIN_ID, SEED_BLOCK, MAX_AGE_SECS).is_ok()
    );
}

/// Open a round directly in storage, without touching the timestamp.
fn open_round<T: Config>(round_id: RoundId, votes: Vec<(T::AccountId, u32)>) {
    let mut bounded = BoundedBTreeMap::new();
    for (account, block) in votes {
        bounded.try_insert(account, block).expect("fits in validator bound");
    }
    NextRoundId::<T>::put(round_id);
    CheckpointRound::<T>::insert(
        CHAIN_ID,
        CheckpointRoundState {
            round_id,
            started_at: <frame_system::Pallet<T>>::block_number(),
            votes: bounded,
        },
    );
}

fn dummy_signature<T: Config>(
    author: &Author<T>,
) -> <T::AuthorityId as RuntimeAppPublic>::Signature {
    author.key.sign(&("DummyProof").encode()).expect("Error signing proof")
}

benchmarks! {
    seed_chain {
    }: _(RawOrigin::Root, CHAIN_ID, SEED_BLOCK, MAX_AGE_SECS)
    verify {
        assert_eq!(MaxCheckpointAge::<T>::get(CHAIN_ID), Some(MAX_AGE_SECS));
        let checkpoint = Checkpoints::<T>::get(CHAIN_ID).expect("seeded");
        assert_eq!(checkpoint.block_number, SEED_BLOCK);
        assert_eq!(checkpoint.round_id, 0);
    }

    // Worst path: a timed out round is replaced by a new one.
    request_checkpoint {
        seed_default_chain::<T>();
        open_round::<T>(1, vec![]);
        let now = <frame_system::Pallet<T>>::block_number();
        <frame_system::Pallet<T>>::set_block_number(
            now + T::RoundTimeoutBlocks::get() + 1u32.into()
        );
    }: _(RawOrigin::Root, CHAIN_ID)
    verify {
        let round = CheckpointRound::<T>::get(CHAIN_ID).expect("round restarted");
        assert_eq!(round.round_id, 2);
        assert!(round.votes.is_empty());
    }

    submit_checkpoint_vote {
        let v in 3 .. MAX_VALIDATOR_ACCOUNTS;
        let validators = setup_validators::<T>(v);
        seed_default_chain::<T>();
        open_round::<T>(1, vec![]);
        let author = validators[0].clone();
        let signature = dummy_signature::<T>(&author);
    }: _(RawOrigin::None, CHAIN_ID, 1, author.clone(), VOTED_BLOCK, signature)
    verify {
        let round = CheckpointRound::<T>::get(CHAIN_ID).expect("round still open");
        assert_eq!(round.votes.get(&author.account_id), Some(&VOTED_BLOCK));
        assert_eq!(Checkpoints::<T>::get(CHAIN_ID).expect("seeded").round_id, 0);
    }

    submit_checkpoint_vote_with_quorum {
        let v in 3 .. MAX_VALIDATOR_ACCOUNTS;
        let validators = setup_validators::<T>(v);
        seed_default_chain::<T>();
        let existing_votes = (T::Quorum::get_supermajority_quorum() as usize).saturating_sub(1);
        let votes = validators.iter().skip(1).take(existing_votes)
            .map(|validator| (validator.account_id.clone(), VOTED_BLOCK))
            .collect();
        open_round::<T>(1, votes);
        let author = validators[0].clone();
        let signature = dummy_signature::<T>(&author);
    }: submit_checkpoint_vote(RawOrigin::None, CHAIN_ID, 1, author.clone(), VOTED_BLOCK, signature)
    verify {
        assert!(CheckpointRound::<T>::get(CHAIN_ID).is_none());
        let checkpoint = Checkpoints::<T>::get(CHAIN_ID).expect("agreed");
        assert_eq!(checkpoint.round_id, 1);
        assert_eq!(checkpoint.block_number, VOTED_BLOCK);
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().as_externality(),
    crate::mock::TestRuntime,
);
