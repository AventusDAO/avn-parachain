// Copyright 2026 Aventus DAO Ltd

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_support::WeakBoundedVec;
use frame_system::RawOrigin;
#[cfg(feature = "runtime-benchmarks")]
use pallet_watchtower::BenchmarkHelper;
use pallet_watchtower::NodesInterface;

const CHAIN_ID: ExternalChainId = 1;
const VENUE_CHAIN_ID: ExternalChainId = 8453;
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
    LastCheckpointRoundId::<T>::put(round_id);
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

/// Checkpoint agreement time benchmarks give floor rounds, since the benchmark clock may be at
/// zero.
const FLOOR_AGREED_AT: u64 = 1_750_000_000;

fn plausible_observation(block: u32) -> FloorObservation {
    FloorObservation {
        external_block: block as u64,
        // Ten Ethereum blocks before the agreement.
        external_block_time_sec: FLOOR_AGREED_AT - 120,
        floor_wei: 50_000_000_000_000_000_000,
        last_sale_ts: 1_700_000_000,
        last_sale_wei: 49_000_000_000_000_000_000,
        flags: 0,
    }
}

fn floor_market() -> FloorMarket {
    FloorMarket { chain_id: CHAIN_ID, punks: sp_core::H160::repeat_byte(0xB4) }
}

/// Configures the market, seeds its chain, opens a floor round and activates its proposal. The
/// round's checkpoint time is replaced by [`FLOOR_AGREED_AT`].
fn start_active_floor_round<T: Config>() -> FloorRound<BlockNumberFor<T>> {
    let market = floor_market();
    assert!(Pallet::<T>::set_floor_market(RawOrigin::Root.into(), Some(market)).is_ok());
    assert!(Pallet::<T>::seed_chain(
        RawOrigin::Root.into(),
        market.chain_id,
        SEED_BLOCK,
        MAX_AGE_SECS
    )
    .is_ok());
    assert!(Pallet::<T>::do_request_floor_price().is_ok());
    let round = ActiveFloorRound::<T>::get().expect("round started");
    assert!(pallet_watchtower::Pallet::<T>::activate_next_proposal(
        RawOrigin::None.into(),
        round.proposal_id
    )
    .is_ok());
    ActiveFloorRound::<T>::mutate(|round| {
        round.as_mut().expect("round started").checkpoint_agreed_at_sec = FLOOR_AGREED_AT
    });
    ActiveFloorRound::<T>::get().expect("round started")
}

fn venue_market() -> VenueMarket {
    VenueMarket {
        chain_id: VENUE_CHAIN_ID,
        clearing_house: sp_core::H160::repeat_byte(0x56),
        amm: sp_core::H160::repeat_byte(0xA1),
        target_tolerance_secs: 6,
    }
}

/// Target timestamp benchmarks give venue rounds, since the benchmark clock may be at zero.
const VENUE_TARGET_TS: u64 = 1_750_000_000;

fn plausible_venue_observation() -> VenueObservation {
    VenueObservation {
        external_block: SEED_BLOCK as u64,
        external_block_time_sec: VENUE_TARGET_TS,
        spot_e18: 51_000_000_000_000_000_000,
        oracle_e18: 50_000_000_000_000_000_000,
        oi_notional_e18: 1_000_000_000_000_000_000_000,
    }
}

/// Configures the market, opens a venue round and activates its proposal. The target set on
/// activation is replaced by [`VENUE_TARGET_TS`].
fn start_active_venue_round<T: Config>() -> VenueRound<BlockNumberFor<T>> {
    let market = venue_market();
    assert!(Pallet::<T>::set_venue_market(RawOrigin::Root.into(), Some(market)).is_ok());
    assert!(Pallet::<T>::do_request_venue_state().is_ok());
    let round = ActiveVenueRound::<T>::get().expect("round started");
    assert!(pallet_watchtower::Pallet::<T>::activate_next_proposal(
        RawOrigin::None.into(),
        round.proposal_id
    )
    .is_ok());
    ActiveVenueRound::<T>::mutate(|round| {
        round.as_mut().expect("round started").target_external_block_time_sec =
            Some(VENUE_TARGET_TS)
    });
    ActiveVenueRound::<T>::get().expect("round started")
}

/// Counts the votes an answer needs, minus one, on the round's proposal, so the benchmarked
/// report is the one that decides it. That is the heaviest report: it also finalises the
/// proposal and closes the round.
fn preload_votes_for<T: Config>(proposal_id: ProposalId, answer: H256) {
    let proposal = pallet_watchtower::Proposals::<T>::get(proposal_id).expect("proposal exists");
    let needed = pallet_watchtower::Pallet::<T>::votes_needed(proposal_id, &proposal)
        .expect("the committee is selected");
    let earlier = needed.saturating_sub(1);
    pallet_watchtower::ChoiceTally::<T>::insert(proposal_id, answer, earlier);
    pallet_watchtower::ChoiceStandings::<T>::insert(
        proposal_id,
        pallet_watchtower::ChoiceStanding {
            cast: earlier,
            leader: (earlier > 0).then_some(answer),
            leader_tally: earlier,
            tied: false,
        },
    );
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
    // Worst path: no fresh floor and a fresh checkpoint, so a watchtower proposal is created.
    request_floor_price {
        let market = floor_market();
        assert!(Pallet::<T>::set_floor_market(RawOrigin::Root.into(), Some(market)).is_ok());
        assert!(Pallet::<T>::seed_chain(
            RawOrigin::Root.into(),
            market.chain_id,
            SEED_BLOCK,
            MAX_AGE_SECS
        )
        .is_ok());
    }: _(RawOrigin::Root)
    verify {
        let round = ActiveFloorRound::<T>::get().expect("round started");
        assert_eq!(round.round_id, 1);
        assert_eq!(round.market, market);
        assert!(pallet_watchtower::Proposals::<T>::contains_key(round.proposal_id));
    }

    // Same worst path as `request_floor_price`: a round is started at once.
    force_floor_round {
        let market = floor_market();
        assert!(Pallet::<T>::set_floor_market(RawOrigin::Root.into(), Some(market)).is_ok());
        assert!(Pallet::<T>::seed_chain(
            RawOrigin::Root.into(),
            market.chain_id,
            SEED_BLOCK,
            MAX_AGE_SECS
        )
        .is_ok());
    }: _(RawOrigin::Root)
    verify {
        let round = ActiveFloorRound::<T>::get().expect("round started");
        assert_eq!(round.round_id, 1);
        assert_eq!(round.market, market);
        assert!(pallet_watchtower::Proposals::<T>::contains_key(round.proposal_id));
    }

    submit_floor_report {
        // Watchtowers come from the runtime's benchmark helper, as in pallet-watchtower.
        <T as pallet_watchtower::Config>::BenchmarkHelper::setup_nodes(
            T::MinCommitteeSize::get().max(1)
        );
        let round = start_active_floor_round::<T>();
        let watchtower = <T as pallet_watchtower::Config>::Watchtowers::get_node_at_index(0)
            .expect("a node is registered");
        let observation = plausible_observation(round.external_block);
        preload_votes_for::<T>(round.proposal_id, observation.hash());
        let signature = <T as pallet_watchtower::Config>::SignerId::generate_pair(None)
            .sign(&("DummyProof").encode())
            .expect("Error signing proof");
    }: _(RawOrigin::None, round.round_id, observation, UNKNOWN_COUNTRY, watchtower.clone(), signature)
    verify {
        assert!(ActiveFloorRound::<T>::get().is_none(), "the report closed the round");
        let floor = LastFinalisedFloor::<T>::get().expect("floor finalised");
        assert_eq!(floor.round_id, round.round_id);
        assert_eq!(floor.observation, observation);
        assert!(RoundsToClean::<T>::contains_key((OracleChannel::Floor, round.round_id)));
    }

    set_oracle_requester {
        let who: T::AccountId = account("requester", 0, 0);
    }: _(RawOrigin::Root, Some(who.clone()))
    verify {
        assert_eq!(OracleRequester::<T>::get(), Some(who));
    }

    // Worst path: no fresh state, so a watchtower proposal is created.
    request_venue_state {
        let market = venue_market();
        assert!(Pallet::<T>::set_venue_market(RawOrigin::Root.into(), Some(market)).is_ok());
    }: _(RawOrigin::Root)
    verify {
        let round = ActiveVenueRound::<T>::get().expect("round started");
        assert_eq!(round.round_id, 1);
        assert_eq!(round.market, market);
        assert!(pallet_watchtower::Proposals::<T>::contains_key(round.proposal_id));
    }

    submit_venue_report {
        <T as pallet_watchtower::Config>::BenchmarkHelper::setup_nodes(
            T::MinCommitteeSize::get().max(1)
        );
        let round = start_active_venue_round::<T>();
        let watchtower = <T as pallet_watchtower::Config>::Watchtowers::get_node_at_index(0)
            .expect("a node is registered");
        let observation = plausible_venue_observation();
        preload_votes_for::<T>(round.proposal_id, observation.hash());
        let signature = <T as pallet_watchtower::Config>::SignerId::generate_pair(None)
            .sign(&("DummyProof").encode())
            .expect("Error signing proof");
    }: _(RawOrigin::None, round.round_id, observation, UNKNOWN_COUNTRY, watchtower.clone(), signature)
    verify {
        assert!(ActiveVenueRound::<T>::get().is_none(), "the report closed the round");
        let venue = LastFinalisedVenue::<T>::get().expect("venue state finalised");
        assert_eq!(venue.round_id, round.round_id);
        assert_eq!(venue.observation, observation);
        assert!(RoundsToClean::<T>::contains_key((OracleChannel::Venue, round.round_id)));
    }

    set_venue_market {
        let market = venue_market();
    }: _(RawOrigin::Root, Some(market))
    verify {
        assert_eq!(VenueMarketConfig::<T>::get(), Some(market));
    }

    set_floor_market {
        let market = floor_market();
    }: _(RawOrigin::Root, Some(market))
    verify {
        assert_eq!(FloorMarketConfig::<T>::get(), Some(market));
    }

    set_oracle_committee_size {
        let size = T::MinCommitteeSize::get().max(1);
        <T as pallet_watchtower::Config>::BenchmarkHelper::setup_nodes(size);
    }: _(RawOrigin::Root, Some(size))
    verify {
        assert_eq!(OracleCommitteeSize::<T>::get(), Some(size));
    }

    // One `on_idle` step: find a finished round and delete `n` reports and `n` observations of
    // it. Venue rows are used because a venue observation is larger than a floor one.
    cleanup_round_page {
        let n in 0 .. ROUND_CLEANUP_PAGE_SIZE;
        let round_id = 1u64;
        for i in 0..n {
            let reporter: T::AccountId = account("reporter", i, 0);
            let mut observation = plausible_venue_observation();
            observation.external_block = i as u64 + 1;
            VenueReports::<T>::insert(round_id, &reporter, observation.hash());
            VenueObservations::<T>::insert(round_id, observation.hash(), observation);
        }
        RoundsToClean::<T>::insert((OracleChannel::Venue, round_id), ());
        let mut removed = 0u32;
    }: {
        let (channel, round_id) = Pallet::<T>::next_round_to_clean().expect("a round to clean");
        removed = Pallet::<T>::cleanup_round_page(channel, round_id, ROUND_CLEANUP_PAGE_SIZE);
    }
    verify {
        assert_eq!(removed, n);
        assert!(VenueReports::<T>::iter_prefix(round_id).next().is_none());
        assert!(VenueObservations::<T>::iter_prefix(round_id).next().is_none());
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().as_externality(),
    crate::mock::TestRuntime,
);
