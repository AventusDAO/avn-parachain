// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::{
    traits::ValidateUnsigned,
    transaction_validity::{InvalidTransaction, TransactionSource},
};
use sp_watchtower::VoteChoice;

const SEED_BLOCK: u32 = 20_000_000;
const CHECKPOINT_MAX_AGE: u64 = 10_000;

fn now_secs() -> u64 {
    Timestamp::get() / 1_000
}

fn advance_secs(secs: u64) {
    Timestamp::set_timestamp(Timestamp::get() + secs * 1_000);
}

/// Configures the market and seeds its chain.
fn seed_floor_chain() -> Checkpoint {
    assert_ok!(FloorOracle::set_floor_market(RuntimeOrigin::root(), Some(test_floor_market())));
    assert_ok!(FloorOracle::seed_chain(
        RuntimeOrigin::root(),
        FLOOR_CHAIN,
        SEED_BLOCK,
        CHECKPOINT_MAX_AGE,
        None
    ));
    FloorOracle::checkpoints(FLOOR_CHAIN).unwrap()
}

/// Requests as the privileged account, setting it first if needed.
fn request() -> DispatchResult {
    if FloorOracle::oracle_requester().is_none() {
        assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
    }
    FloorOracle::request_floor_price(RuntimeOrigin::signed(requester()))
}

/// Seeds the chain, requests a floor and activates the proposal. Returns the round.
fn start_active_round() -> FloorRound<BlockNumber> {
    seed_floor_chain();
    assert_ok!(request());
    let round = FloorOracle::active_floor_round().expect("round started");
    assert_eq!(activate_head(), round.proposal_id);
    round
}

/// Requests once the checkpoint is too old for a floor, then agrees the checkpoint round the
/// request opened, as the collators would, so the floor round starts.
fn request_with_new_checkpoint() {
    assert_ok!(request());
    agree_checkpoint_round();
}

/// Agrees the running checkpoint round 50 blocks past the current checkpoint.
fn agree_checkpoint_round() {
    let round = FloorOracle::checkpoint_round(FLOOR_CHAIN).expect("checkpoint round started");
    let block = FloorOracle::checkpoints(FLOOR_CHAIN).unwrap().block_number + 50;
    for i in 1..=4 {
        assert_ok!(VoteContext::new(FLOOR_CHAIN, round.round_id, i, block).submit());
    }
}

fn observation(floor_wei: u128) -> FloorObservation {
    FloorObservation {
        external_block: SEED_BLOCK as u64,
        external_block_time_sec: pinned_floor_block_time(FLOOR_CHAIN),
        floor_wei,
        last_sale_ts: 1_700_000_000,
        last_sale_wei: floor_wei - 1,
        flags: 0,
    }
}

fn report(round_id: u64, floor_wei: u128, watchtower_index: u8) -> DispatchResult {
    ReportContext::new(round_id, observation(floor_wei), watchtower_index).submit()
}

fn assert_event(event: Event<TestRuntime>) {
    assert!(
        System::events()
            .iter()
            .any(|r| r.event == RuntimeEvent::FloorOracle(event.clone())),
        "event {:?} not found in {:?}",
        event,
        System::events()
    );
}

fn validate(call: &Call<TestRuntime>) -> Result<(), InvalidTransaction> {
    FloorOracle::validate_unsigned(TransactionSource::External, call)
        .map(|_| ())
        .map_err(|e| match e {
            sp_runtime::transaction_validity::TransactionValidityError::Invalid(i) => i,
            other => panic!("unexpected validity error {:?}", other),
        })
}

fn validate_watchtower(call: &pallet_watchtower::Call<TestRuntime>) -> bool {
    <Watchtower as ValidateUnsigned>::validate_unsigned(TransactionSource::External, call).is_ok()
}

/// The round is closed, its rows are queued for cleanup if it has any, and `on_idle` then
/// deletes them.
fn assert_round_cleared(round: &FloorRound<BlockNumber>) {
    assert!(FloorOracle::active_floor_round().is_none());
    let key = (OracleChannel::Floor, round.round_id);
    let stored = FloorReports::<TestRuntime>::iter_prefix(round.round_id).count();
    assert_eq!(RoundsToClean::<TestRuntime>::contains_key(key), stored > 0);

    run_idle_cleanup();
    assert!(!RoundsToClean::<TestRuntime>::contains_key(key));
    assert_eq!(FloorReports::<TestRuntime>::iter_prefix(round.round_id).count(), 0);
    assert_eq!(FloorObservations::<TestRuntime>::iter_prefix(round.round_id).count(), 0);
}

mod requests {
    use super::*;

    #[test]
    fn only_root_or_the_privileged_requester_may_request() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            assert_noop!(
                FloorOracle::request_floor_price(RuntimeOrigin::none()),
                sp_runtime::DispatchError::BadOrigin
            );
            assert_noop!(
                FloorOracle::request_floor_price(RuntimeOrigin::signed(requester())),
                Error::<TestRuntime>::UnauthorizedOracleRequest
            );
            assert_ok!(FloorOracle::request_floor_price(RuntimeOrigin::root()));
            assert_eq!(FloorOracle::last_floor_round_id(), 1);

            assert_noop!(
                FloorOracle::set_oracle_requester(
                    RuntimeOrigin::signed(requester()),
                    Some(requester())
                ),
                sp_runtime::DispatchError::BadOrigin
            );
            assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
            assert_event(Event::OracleRequesterSet { who: Some(requester()) });
            assert_ok!(FloorOracle::request_floor_price(RuntimeOrigin::signed(requester())));
            assert_noop!(
                FloorOracle::request_floor_price(RuntimeOrigin::signed(watchtower(1))),
                Error::<TestRuntime>::UnauthorizedOracleRequest
            );

            assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), None));
            assert_eq!(FloorOracle::oracle_requester(), None);
            assert_noop!(
                FloorOracle::request_floor_price(RuntimeOrigin::signed(requester())),
                Error::<TestRuntime>::UnauthorizedOracleRequest
            );
        });
    }

    #[test]
    fn requires_a_market_and_a_seeded_chain() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
            assert_noop!(request(), Error::<TestRuntime>::FloorMarketNotSet);

            assert_noop!(
                FloorOracle::set_floor_market(
                    RuntimeOrigin::signed(requester()),
                    Some(test_floor_market())
                ),
                sp_runtime::DispatchError::BadOrigin
            );
            assert_ok!(FloorOracle::set_floor_market(
                RuntimeOrigin::root(),
                Some(test_floor_market())
            ));
            assert_event(Event::FloorMarketSet { market: Some(test_floor_market()) });
            assert_eq!(FloorOracle::floor_market(), Some(test_floor_market()));
            assert_noop!(request(), Error::<TestRuntime>::ChainNotRegistered);

            assert_ok!(FloorOracle::set_floor_market(RuntimeOrigin::root(), None));
            assert_eq!(FloorOracle::floor_market(), None);
        });
    }

    #[test]
    fn a_market_change_only_affects_the_next_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            let other =
                FloorMarket { punks: sp_core::H160::repeat_byte(0xC5), ..test_floor_market() };
            assert_ok!(FloorOracle::set_floor_market(RuntimeOrigin::root(), Some(other)));

            assert_eq!(FloorOracle::active_floor_round().unwrap().market, test_floor_market());
            for i in 1..=6 {
                assert_ok!(report(round.round_id, 50, i));
            }
            assert_eq!(FloorOracle::last_finalised_floor().unwrap().market, test_floor_market());

            advance_secs(MAX_FLOOR_AGE_SECS + 1);
            request_with_new_checkpoint();
            assert_eq!(FloorOracle::active_floor_round().unwrap().market, other);
        });
    }

    #[test]
    fn fresh_checkpoint_starts_a_choice_proposal() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let checkpoint = seed_floor_chain();
            assert_ok!(request());

            let round = FloorOracle::active_floor_round().expect("round started");
            assert_eq!(round.round_id, 1);
            assert_eq!(round.checkpoint_agreed_at_sec, checkpoint.agreed_at);
            assert_eq!(round.external_block, SEED_BLOCK);
            assert_eq!(round.market, test_floor_market());
            assert_eq!(round.reports, 0);
            assert_eq!(round.started_at, System::block_number());
            assert_eq!(FloorOracle::last_floor_round_id(), 1);

            let proposal = Watchtower::proposals(round.proposal_id).expect("proposal exists");
            assert_eq!(
                proposal.source,
                sp_watchtower::ProposalSource::Internal(sp_watchtower::ProposalType::FloorOracle)
            );
            assert_eq!(proposal.decision_rule, sp_watchtower::DecisionRule::RejectOnExpiry);
            assert_eq!(proposal.external_ref, FloorOracle::floor_round_external_ref(1));
            assert_eq!(proposal.vote_duration, ORACLE_VOTE_DURATION);
            assert_eq!(
                pallet_watchtower::VotingKindOf::<TestRuntime>::get(round.proposal_id),
                sp_watchtower::VotingKind::Choice
            );
            assert_eq!(Watchtower::proposal_status(round.proposal_id), ProposalStatusEnum::Queued);
            assert_event(Event::FloorRoundStarted {
                round_id: 1,
                proposal_id: round.proposal_id,
                checkpoint_agreed_at_sec: checkpoint.agreed_at,
                external_block: SEED_BLOCK,
                market: test_floor_market(),
            });
        });
    }

    #[test]
    fn requests_join_the_running_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            assert_ok!(request());
            assert_ok!(request());
            assert_eq!(FloorOracle::last_floor_round_id(), 1);
            assert_event(Event::FloorRoundInProgress { round_id: 1 });
        });
    }

    #[test]
    fn stale_checkpoint_waits_for_the_checkpoint_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            advance_secs(CHECKPOINT_MAX_AGE + 1);

            assert_ok!(request());
            assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Requested));
            assert!(FloorOracle::active_floor_round().is_none());
            let checkpoint_round = FloorOracle::checkpoint_round(FLOOR_CHAIN).expect("started");
            assert_event(Event::FloorRequestWaitingForCheckpoint {
                checkpoint_round_id: checkpoint_round.round_id,
            });

            for i in 1..=4 {
                assert_ok!(VoteContext::new(
                    FLOOR_CHAIN,
                    checkpoint_round.round_id,
                    i,
                    SEED_BLOCK + 50
                )
                .submit());
            }
            let checkpoint = FloorOracle::checkpoints(FLOOR_CHAIN).unwrap();
            assert_eq!(checkpoint.block_number, SEED_BLOCK + 50);

            assert_eq!(FloorOracle::pending_floor_request(), None);
            let round = FloorOracle::active_floor_round().expect("round started");
            assert_eq!(round.external_block, SEED_BLOCK + 50);
            assert_eq!(round.checkpoint_agreed_at_sec, checkpoint.agreed_at);
        });
    }

    /// A request waiting on a checkpoint round that root drops is served by the checkpoint
    /// root sets, as it would be by an agreed one, rather than left waiting.
    #[test]
    fn a_forced_checkpoint_serves_the_waiting_request() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            advance_secs(CHECKPOINT_MAX_AGE + 1);
            assert_ok!(request());
            assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Requested));

            assert_ok!(FloorOracle::force_set_checkpoint(
                RuntimeOrigin::root(),
                FLOOR_CHAIN,
                SEED_BLOCK + 50
            ));
            assert!(FloorOracle::checkpoint_round(FLOOR_CHAIN).is_none());
            assert_eq!(FloorOracle::pending_floor_request(), None);
            let round = FloorOracle::active_floor_round().expect("round started");
            assert_eq!(round.external_block, SEED_BLOCK + 50);
            assert_eq!(round.checkpoint_agreed_at_sec, now_secs());
        });
    }

    /// The chain still accepts the checkpoint, but it is too old for a floor: a floor pinned to
    /// it would be close to `MaxFloorAge` before it even finalised. A new checkpoint round runs.
    #[test]
    fn a_checkpoint_too_old_for_a_floor_starts_a_checkpoint_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            advance_secs(MAX_FLOOR_CHECKPOINT_AGE_SECS);
            assert_ok!(request());
            assert!(FloorOracle::active_floor_round().is_some(), "exactly at the limit is fine");
        });

        ExtBuilder::build_default().as_externality().execute_with(|| {
            let seeded = seed_floor_chain();
            advance_secs(MAX_FLOOR_CHECKPOINT_AGE_SECS + 1);
            assert!(MAX_FLOOR_CHECKPOINT_AGE_SECS + 1 < CHECKPOINT_MAX_AGE);
            assert!(matches!(
                FloorOracle::do_request_checkpoint(FLOOR_CHAIN),
                Ok(CheckpointRequest::Ready(checkpoint)) if checkpoint == seeded
            ));

            assert_ok!(request());
            assert!(FloorOracle::active_floor_round().is_none());
            assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Requested));
            let checkpoint_round = FloorOracle::checkpoint_round(FLOOR_CHAIN).expect("started");
            assert_event(Event::FloorRequestWaitingForCheckpoint {
                checkpoint_round_id: checkpoint_round.round_id,
            });
        });
    }

    #[test]
    fn checkpoints_of_other_chains_do_not_start_rounds() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            advance_secs(CHECKPOINT_MAX_AGE + 1);
            assert_ok!(request());
            assert_ok!(FloorOracle::seed_chain(
                RuntimeOrigin::root(),
                8453,
                5,
                CHECKPOINT_MAX_AGE,
                None
            ));
            assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Requested));
            assert!(FloorOracle::active_floor_round().is_none());
        });
    }

    /// A floor is as old as the Ethereum block it was read at, which was mined before the
    /// checkpoint was agreed. Its cache life runs from the block, not from the agreement.
    #[test]
    fn a_floor_ages_from_its_ethereum_block() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            advance_secs(MAX_FLOOR_AGE_SECS);
            let round = start_active_round();
            let block_age = MAX_FLOOR_AGE_SECS - 10;
            let observation = FloorObservation {
                external_block_time_sec: round.checkpoint_agreed_at_sec - block_age,
                ..observation(50)
            };
            for i in 1..=6 {
                assert_ok!(ReportContext::new(round.round_id, observation, i).submit());
            }
            let floor = FloorOracle::last_finalised_floor().expect("finalised");
            assert_eq!(floor.observation, observation);

            assert_ok!(request());
            assert_event(Event::FloorPriceServed { round_id: round.round_id, floor_wei: 50 });

            // Ten seconds after the agreement the checkpoint is young, but the block is past
            // `MaxFloorAge`.
            advance_secs(11);
            assert_ok!(request());
            let next = FloorOracle::active_floor_round().expect("a new round started");
            assert_eq!(next.round_id, round.round_id + 1);
        });
    }
}

mod forced_rounds {
    use super::*;

    /// Forces as the privileged account, setting it first if needed.
    fn force() -> DispatchResult {
        if FloorOracle::oracle_requester().is_none() {
            assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
        }
        FloorOracle::force_floor_round(RuntimeOrigin::signed(requester()))
    }

    /// Watchtower `i` reports a floor of `floor_wei` at the round's external block.
    fn report_at(round: &FloorRound<BlockNumber>, floor_wei: u128, i: u8) -> DispatchResult {
        let observation = FloorObservation {
            external_block: round.external_block as u64,
            ..observation(floor_wei)
        };
        ReportContext::new(round.round_id, observation, i).submit()
    }

    /// Runs a requested round to agreement on a floor of 50. Returns the finalised floor.
    fn finalise_floor() -> FinalisedFloorRound {
        let round = start_active_round();
        for i in 1..=6 {
            assert_ok!(report(round.round_id, 50, i));
        }
        FloorOracle::last_finalised_floor().expect("finalised")
    }

    /// Activates the running round and lets it finalise on `floor_wei`.
    fn finalise_running_round(floor_wei: u128) -> FloorRound<BlockNumber> {
        let round = FloorOracle::active_floor_round().expect("round started");
        assert_eq!(activate_head(), round.proposal_id);
        for i in 1..=6 {
            assert_ok!(report_at(&round, floor_wei, i));
        }
        assert_eq!(FloorOracle::last_finalised_floor().unwrap().round_id, round.round_id);
        round
    }

    /// Activates the running round and splits the committee so it fails.
    fn fail_running_round() -> FloorRound<BlockNumber> {
        let round = FloorOracle::active_floor_round().expect("round started");
        assert_eq!(activate_head(), round.proposal_id);
        for i in 1..=6 {
            assert_ok!(report_at(&round, 50 + i as u128, i));
        }
        assert!(FloorOracle::active_floor_round().is_none());
        assert_event(Event::FloorRoundFailed {
            round_id: round.round_id,
            result: ProposalStatusEnum::Resolved { passed: false },
        });
        round
    }

    fn forced_events() -> usize {
        System::events()
            .iter()
            .filter(|r| {
                matches!(r.event, RuntimeEvent::FloorOracle(Event::FloorRoundForced { .. }))
            })
            .count()
    }

    #[test]
    fn only_root_or_the_privileged_requester_may_force() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            assert_noop!(
                FloorOracle::force_floor_round(RuntimeOrigin::none()),
                sp_runtime::DispatchError::BadOrigin
            );
            assert_noop!(
                FloorOracle::force_floor_round(RuntimeOrigin::signed(requester())),
                Error::<TestRuntime>::UnauthorizedOracleRequest
            );
            assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
            assert_noop!(
                FloorOracle::force_floor_round(RuntimeOrigin::signed(watchtower(1))),
                Error::<TestRuntime>::UnauthorizedOracleRequest
            );

            assert_ok!(FloorOracle::force_floor_round(RuntimeOrigin::root()));
            let round = FloorOracle::active_floor_round().expect("round started");
            assert_eq!(round.round_id, 1);
            assert_eq!(round.kind, FloorRoundKind::Forced);
            assert_event(Event::FloorRoundForced { round_id: 1 });
        });
    }

    /// `request_floor_price` serves the cached floor, but a force asks for a new one. The
    /// checkpoint the floor was read at is still young, yet a round on it would read the same
    /// floor, so the force waits for a newer checkpoint.
    #[test]
    fn a_force_ignores_a_fresh_floor_and_its_checkpoint() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let floor = finalise_floor();
            assert_ok!(request());
            assert_event(Event::FloorPriceServed { round_id: floor.round_id, floor_wei: 50 });

            assert_ok!(force());
            assert!(FloorOracle::active_floor_round().is_none());
            assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Forced));
            let checkpoint_round = FloorOracle::checkpoint_round(FLOOR_CHAIN).expect("started");
            assert_event(Event::FloorRequestWaitingForCheckpoint {
                checkpoint_round_id: checkpoint_round.round_id,
            });

            agree_checkpoint_round();
            let round = FloorOracle::active_floor_round().expect("round started");
            assert_eq!(round.round_id, floor.round_id + 1);
            assert_eq!(round.kind, FloorRoundKind::Forced);
            assert_eq!(round.external_block, SEED_BLOCK + 50);
            assert_event(Event::FloorRoundForced { round_id: round.round_id });
        });
    }

    /// A checkpoint round that settles on the block the last floor was read at, because Ethereum
    /// has not moved past it, answers the force with the last floor and starts the cooldown.
    /// Nothing is left waiting, so a stalled chain cannot hold the request forever.
    #[test]
    fn a_force_without_a_newer_block_is_answered_with_the_last_floor() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let floor = finalise_floor();
            let last_block = floor.observation.external_block as u32;
            assert_ok!(force());
            assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Forced));

            let round = FloorOracle::checkpoint_round(FLOOR_CHAIN).expect("round started");
            for i in 1..=4 {
                assert_ok!(VoteContext::new(FLOOR_CHAIN, round.round_id, i, last_block).submit());
            }
            assert_eq!(FloorOracle::checkpoints(FLOOR_CHAIN).unwrap().block_number, last_block);
            assert!(FloorOracle::active_floor_round().is_none());
            assert!(FloorOracle::checkpoint_round(FLOOR_CHAIN).is_none());
            assert_eq!(FloorOracle::pending_floor_request(), None);
            assert_event(Event::FloorPriceServed {
                round_id: floor.round_id,
                floor_wei: floor.observation.floor_wei,
            });
            assert_eq!(FloorOracle::last_forced_round_ended_at(), Some(now_secs()));
            assert_eq!(forced_events(), 0);
        });
    }

    /// A checkpoint newer than the last floor's, and young enough, is pinned at once. A failed
    /// requested round leaves no cooldown.
    #[test]
    fn a_newer_checkpoint_starts_the_forced_round_at_once() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let floor = finalise_floor();
            advance_secs(MAX_FLOOR_AGE_SECS + 1);
            request_with_new_checkpoint();
            let failed = fail_running_round();
            assert_eq!(failed.kind, FloorRoundKind::Requested);
            assert_eq!(FloorOracle::last_forced_round_ended_at(), None);
            assert_eq!(FloorOracle::last_finalised_floor(), Some(floor));

            assert_ok!(force());
            let round = FloorOracle::active_floor_round().expect("round started");
            assert_eq!(round.kind, FloorRoundKind::Forced);
            assert_eq!(round.external_block, failed.external_block);
            assert_eq!(FloorOracle::pending_floor_request(), None);
            assert_event(Event::FloorRoundForced { round_id: round.round_id });
        });
    }

    #[test]
    fn a_running_round_is_joined() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            assert_ok!(force());
            assert_event(Event::FloorRoundInProgress { round_id: round.round_id });
            assert_eq!(FloorOracle::last_floor_round_id(), round.round_id);
            assert_eq!(forced_events(), 0);
        });
    }

    /// After a forced round finalises, a force is a plain request for `MinForcedFloorAge`: it
    /// gets the new floor, although its checkpoint is already older than that.
    #[test]
    fn a_finalised_forced_round_is_served_until_the_cooldown_ends() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            finalise_floor();
            assert_ok!(force());
            agree_checkpoint_round();
            advance_secs(20);
            let round = finalise_running_round(60);
            assert_eq!(FloorOracle::last_forced_round_ended_at(), Some(now_secs()));

            advance_secs(MIN_FORCED_FLOOR_AGE_SECS);
            let floor = FloorOracle::last_finalised_floor().unwrap();
            assert!(now_secs() - floor.checkpoint_agreed_at_sec > MIN_FORCED_FLOOR_AGE_SECS);
            assert_ok!(force());
            assert_event(Event::FloorPriceServed { round_id: round.round_id, floor_wei: 60 });
            assert_eq!(FloorOracle::pending_floor_request(), None);

            advance_secs(1);
            assert_ok!(force());
            assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Forced));
        });
    }

    /// A forced round that fails starts the cooldown too, so the requester cannot retry at once.
    #[test]
    fn a_failed_forced_round_starts_the_cooldown() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let floor = finalise_floor();
            assert_ok!(force());
            agree_checkpoint_round();
            let failed = fail_running_round();
            assert_eq!(failed.kind, FloorRoundKind::Forced);
            assert_eq!(FloorOracle::last_forced_round_ended_at(), Some(now_secs()));

            assert_ok!(force());
            assert_event(Event::FloorPriceServed { round_id: floor.round_id, floor_wei: 50 });
            assert!(FloorOracle::active_floor_round().is_none());
            assert_eq!(FloorOracle::pending_floor_request(), None);

            advance_secs(MIN_FORCED_FLOOR_AGE_SECS + 1);
            assert_ok!(force());
            assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Forced));
        });
    }

    /// In the cooldown a force is a plain request: with no fresh floor it starts a requested
    /// round, not a forced one.
    #[test]
    fn a_force_in_the_cooldown_starts_a_requested_round_when_the_floor_is_stale() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            finalise_floor();
            advance_secs(MAX_FLOOR_AGE_SECS + 1);
            assert_ok!(force());
            agree_checkpoint_round();
            fail_running_round();
            let forced = forced_events();

            assert_ok!(force());
            let round = FloorOracle::active_floor_round().expect("round started");
            assert_eq!(round.kind, FloorRoundKind::Requested);
            assert_eq!(forced_events(), forced);
        });
    }

    /// A forced and a requested request waiting for the same checkpoint start a forced round,
    /// whichever came first.
    #[test]
    fn a_waiting_forced_request_wins_over_a_requested_one() {
        for force_first in [false, true] {
            ExtBuilder::build_default().as_externality().execute_with(|| {
                finalise_floor();
                advance_secs(MAX_FLOOR_AGE_SECS + 1);
                if force_first {
                    assert_ok!(force());
                    assert_ok!(request());
                } else {
                    assert_ok!(request());
                    assert_eq!(
                        FloorOracle::pending_floor_request(),
                        Some(FloorRoundKind::Requested)
                    );
                    assert_ok!(force());
                }
                assert_eq!(FloorOracle::pending_floor_request(), Some(FloorRoundKind::Forced));

                agree_checkpoint_round();
                let round = FloorOracle::active_floor_round().expect("round started");
                assert_eq!(round.kind, FloorRoundKind::Forced, "force_first: {}", force_first);
                assert_event(Event::FloorRoundForced { round_id: round.round_id });
            });
        }
    }
}

mod reports {
    use super::*;

    #[test]
    fn rejected_until_the_proposal_is_active() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            assert_ok!(request());
            let report = ReportContext::new(1, observation(50), 1);
            assert_eq!(
                validate(&report.call()),
                Err(InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID))
            );
            assert_noop!(report.submit(), Error::<TestRuntime>::FloorProposalNotActive);
        });
    }

    #[test]
    fn reports_are_counted_as_answers() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            let (fifty, fifty_one) = (observation(50).hash(), observation(51).hash());

            let first = ReportContext::new(round.round_id, observation(50), 1);
            assert_ok!(validate(&first.call()));
            assert_ok!(first.submit());
            assert_event(Event::FloorReportSubmitted {
                round_id: 1,
                watchtower: watchtower(1),
                hash: fifty,
                country: TEST_COUNTRY,
            });
            assert_ok!(report(round.round_id, 51, 2));
            assert_ok!(report(round.round_id, 50, 3));

            let round = FloorOracle::active_floor_round().unwrap();
            assert_eq!(round.reports, 3);
            assert_eq!(FloorOracle::floor_reports(1, watchtower(1)), Some(fifty));
            assert_eq!(FloorOracle::floor_reports(1, watchtower(2)), Some(fifty_one));
            assert_eq!(FloorOracle::floor_observations(1, fifty), Some(observation(50)));
            assert_eq!(FloorOracle::floor_observations(1, fifty_one), Some(observation(51)));
            assert_eq!(Watchtower::choice_tally(round.proposal_id, fifty), 2);
            assert_eq!(Watchtower::choice_tally(round.proposal_id, fifty_one), 1);
            for i in 1..=3 {
                assert!(Watchtower::voters(round.proposal_id, watchtower(i)), "report is the vote");
            }
        });
    }

    #[test]
    fn invalid_reports_are_rejected() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            assert_ok!(report(round.round_id, 50, 1));

            let cases: Vec<(ReportContext, Error<TestRuntime>)> = vec![
                (
                    ReportContext::new(round.round_id, observation(50), 1),
                    Error::DuplicateFloorReport,
                ),
                (
                    ReportContext::new(round.round_id + 1, observation(50), 2),
                    Error::FloorRoundMismatch,
                ),
                (
                    ReportContext::new(
                        round.round_id,
                        FloorObservation { floor_wei: 0, ..observation(50) },
                        2,
                    ),
                    Error::ImplausibleObservation,
                ),
                (
                    // The block was deep before the checkpoint was agreed; a later time would
                    // make the floor look fresher than it is.
                    ReportContext::new(
                        round.round_id,
                        FloorObservation {
                            external_block_time_sec: round.checkpoint_agreed_at_sec + 1,
                            ..observation(50)
                        },
                        2,
                    ),
                    Error::ImplausibleObservation,
                ),
                (
                    ReportContext::new(
                        round.round_id,
                        FloorObservation {
                            external_block: SEED_BLOCK as u64 - 1,
                            ..observation(50)
                        },
                        2,
                    ),
                    Error::ObservationBlockMismatch,
                ),
            ];
            for (report, error) in cases {
                assert_eq!(
                    validate(&report.call()),
                    Err(InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID)),
                    "{:?}",
                    error
                );
                assert_noop!(report.submit(), error);
            }

            set_authorized_watchtowers(vec![watchtower(1)]);
            let outsider = ReportContext::new(round.round_id, observation(50), 2);
            assert_eq!(
                validate(&outsider.call()),
                Err(InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID))
            );
            assert_noop!(outsider.submit(), Error::<TestRuntime>::NotInFloorCommittee);
            set_authorized_watchtowers(all_watchtowers());

            let forged = Call::<TestRuntime>::submit_floor_report {
                round_id: round.round_id,
                observation: observation(50),
                country: TEST_COUNTRY,
                watchtower: watchtower(2),
                signature: ReportContext::new(round.round_id, observation(50), 3).signature(),
            };
            assert_eq!(validate(&forged), Err(InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID)));

            // The country is signed too: changing it after signing invalidates the report.
            let relabelled = Call::<TestRuntime>::submit_floor_report {
                round_id: round.round_id,
                observation: observation(50),
                country: *b"FR",
                watchtower: watchtower(2),
                signature: ReportContext::new(round.round_id, observation(50), 2).signature(),
            };
            assert_eq!(
                validate(&relabelled),
                Err(InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID))
            );

            // The chain and the round's market are signed too: a report made on another
            // deployment, or for another market, does not verify.
            let signed = ReportContext::new(round.round_id, observation(50), 2).call();
            assert_eq!(validate(&signed), Ok(()));
            let genesis = frame_system::BlockHash::<TestRuntime>::get(0);
            frame_system::BlockHash::<TestRuntime>::insert(0, H256::repeat_byte(9));
            assert_eq!(validate(&signed), Err(InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID)));
            frame_system::BlockHash::<TestRuntime>::insert(0, genesis);
            ActiveFloorRound::<TestRuntime>::mutate(|round| {
                round.as_mut().expect("round running").market.chain_id += 1
            });
            assert_eq!(validate(&signed), Err(InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID)));
        });
    }
}

mod committee {
    use super::*;

    #[test]
    fn root_sets_a_bounded_committee_size_for_later_rounds() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_noop!(
                FloorOracle::set_oracle_committee_size(RuntimeOrigin::signed(requester()), Some(3)),
                sp_runtime::DispatchError::BadOrigin
            );
            // Mock limits: min 2, max 5, ten authorized watchtowers.
            assert_noop!(
                FloorOracle::set_oracle_committee_size(RuntimeOrigin::root(), Some(1)),
                Error::<TestRuntime>::CommitteeSizeOutOfBounds
            );
            assert_noop!(
                FloorOracle::set_oracle_committee_size(RuntimeOrigin::root(), Some(6)),
                Error::<TestRuntime>::CommitteeSizeOutOfBounds
            );
            set_authorized_watchtowers(vec![watchtower(1), watchtower(2)]);
            assert_noop!(
                FloorOracle::set_oracle_committee_size(RuntimeOrigin::root(), Some(3)),
                Error::<TestRuntime>::CommitteeSizeExceedsWatchtowers
            );
            set_authorized_watchtowers(all_watchtowers());

            // While the node index is backfilling no committee can be drawn, so the size is
            // refused rather than left to block the watchtower queue.
            set_indexed_nodes(Some(7));
            assert_noop!(
                FloorOracle::set_oracle_committee_size(RuntimeOrigin::root(), Some(3)),
                pallet_watchtower::Error::<TestRuntime>::NodeIndexNotReady
            );
            set_indexed_nodes(None);

            // A running round keeps the committee it started with.
            let round = start_active_round();
            assert_ok!(FloorOracle::set_oracle_committee_size(RuntimeOrigin::root(), Some(3)));
            assert_event(Event::OracleCommitteeSizeSet { size: Some(3) });
            assert_eq!(FloorOracle::oracle_committee_size(), Some(3));
            assert_eq!(
                pallet_watchtower::ProposalCommitteeSize::<TestRuntime>::get(round.proposal_id),
                None,
                "everyone was eligible when the round started"
            );
            for i in 1..=6 {
                assert_ok!(report(round.round_id, 50, i));
            }
            assert!(FloorOracle::last_finalised_floor().is_some());

            advance_secs(MAX_FLOOR_AGE_SECS + 1);
            request_with_new_checkpoint();
            let next = FloorOracle::active_floor_round().unwrap();
            // The committee is drawn when the proposal activates.
            assert_eq!(activate_head(), next.proposal_id);
            assert_eq!(
                pallet_watchtower::ProposalCommitteeSize::<TestRuntime>::get(next.proposal_id),
                Some(3)
            );

            assert_ok!(FloorOracle::set_oracle_committee_size(RuntimeOrigin::root(), None));
            assert_eq!(FloorOracle::oracle_committee_size(), None);
        });
    }

    #[test]
    fn only_committee_members_may_report() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_ok!(FloorOracle::set_oracle_committee_size(RuntimeOrigin::root(), Some(3)));
            let round = start_active_round();

            assert_eq!(
                pallet_watchtower::ProposalCommitteeSize::<TestRuntime>::get(round.proposal_id),
                Some(3)
            );
            let members: Vec<u8> = (1..=10)
                .filter(|i| Watchtower::is_committee_member(round.proposal_id, &watchtower(*i)))
                .collect();
            let outsiders: Vec<u8> = (1..=10).filter(|i| !members.contains(i)).collect();
            assert_eq!(members.len(), 3);

            let outsider = ReportContext::new(round.round_id, observation(50), outsiders[0]);
            assert_eq!(
                validate(&outsider.call()),
                Err(InvalidTransaction::Custom(FLOOR_REPORT_NOT_VALID))
            );
            assert_noop!(outsider.submit(), Error::<TestRuntime>::NotInFloorCommittee);
            assert_noop!(
                <FloorOracle as VoteFilter<AccountId>>::ensure_can_vote(
                    round.proposal_id,
                    &watchtower(outsiders[0]),
                    &VoteChoice::Choice(observation(50).hash())
                ),
                Error::<TestRuntime>::NotInFloorCommittee
            );

            // 67%? No: the mock threshold is 60% of 3 = 2 matching reports.
            assert_ok!(report(round.round_id, 50, members[0]));
            assert_ok!(report(round.round_id, 50, members[1]));
            let floor = FloorOracle::last_finalised_floor().expect("finalised");
            assert_eq!((floor.agreeing, floor.reports, floor.required), (2, 2, 2));
        });
    }
}

mod voting {
    use super::*;

    #[test]
    fn yes_no_votes_are_refused_on_floor_proposals() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            assert_ok!(report(round.round_id, 50, 1));

            // The watchtower refuses the kind before the oracle's filter is reached...
            assert!(!validate_watchtower(&vote_call(round.proposal_id, true, 1)));
            assert_noop!(
                submit_vote(round.proposal_id, true, 2),
                pallet_watchtower::Error::<TestRuntime>::WrongVoteKind
            );
            // ...and the filter refuses it on its own too.
            assert_noop!(
                <FloorOracle as VoteFilter<AccountId>>::ensure_can_vote(
                    round.proposal_id,
                    &watchtower(1),
                    &VoteChoice::Binary(true)
                ),
                Error::<TestRuntime>::FloorVotesAreReports
            );
        });
    }

    #[test]
    fn a_direct_answer_must_be_the_reported_one() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            let other = observation(99).hash();

            // No report: no vote.
            assert!(!validate_watchtower(&choice_vote_call(round.proposal_id, other, 1)));
            assert_noop!(
                submit_choice_vote(round.proposal_id, other, 1),
                Error::<TestRuntime>::NoFloorReport
            );

            assert_ok!(report(round.round_id, 50, 1));
            // A different answer than the report: refused in the pool and in a block.
            assert!(!validate_watchtower(&choice_vote_call(round.proposal_id, other, 1)));
            assert_noop!(
                submit_choice_vote(round.proposal_id, other, 1),
                Error::<TestRuntime>::VoteContradictsReport
            );
            // The reported answer was already counted by the report itself.
            assert_noop!(
                submit_choice_vote(round.proposal_id, observation(50).hash(), 1),
                pallet_watchtower::Error::<TestRuntime>::AlreadyVoted
            );
            assert_eq!(Watchtower::choice_tally(round.proposal_id, observation(50).hash()), 1);
        });
    }

    #[test]
    fn a_bad_first_reporter_cannot_block_the_majority() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            advance_secs(5);

            // Watchtower 7 reports garbage first; 60% of 10 = 6 matching reports still win.
            assert_ok!(report(round.round_id, 52, 7));
            for i in 1..=6 {
                assert!(FloorOracle::active_floor_round().is_some(), "not finalised before quorum");
                assert_ok!(report(round.round_id, 51, i));
            }

            let floor = FloorOracle::last_finalised_floor().expect("finalised");
            assert_eq!(floor.round_id, round.round_id);
            assert_eq!(floor.proposal_id, round.proposal_id);
            assert_eq!(floor.checkpoint_agreed_at_sec, round.checkpoint_agreed_at_sec);
            assert_eq!(floor.observation, observation(51));
            assert_eq!((floor.agreeing, floor.reports, floor.required), (6, 7, 6));
            assert_eq!(floor.finalised_at_block, System::block_number() as u32);
            assert_eq!(floor.finalised_at_sec, now_secs());
            assert_event(Event::FloorRoundFinalised {
                round_id: round.round_id,
                external_block_time_sec: observation(51).external_block_time_sec,
                floor_wei: 51,
                agreeing: 6,
                reports: 7,
                required: 6,
            });
            assert_round_cleared(&round);
            assert_eq!(
                Watchtower::proposal_status(round.proposal_id),
                ProposalStatusEnum::Resolved { passed: true }
            );
            assert_eq!(Watchtower::winning_choice(round.proposal_id), Some(observation(51).hash()));

            // A request now is answered from the finalised floor, until it ages out.
            assert_ok!(request());
            assert_event(Event::FloorPriceServed { round_id: round.round_id, floor_wei: 51 });
            assert!(FloorOracle::active_floor_round().is_none());
            advance_secs(MAX_FLOOR_AGE_SECS + 1);
            request_with_new_checkpoint();
            assert_eq!(FloorOracle::active_floor_round().unwrap().round_id, round.round_id + 1);
        });
    }

    #[test]
    fn arrival_order_does_not_matter() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            for (i, wei) in [51u128, 51, 52, 51, 53, 51, 51].iter().enumerate() {
                assert!(FloorOracle::active_floor_round().is_some());
                assert_ok!(report(round.round_id, *wei, i as u8 + 1));
            }
            // Only five 51s so far: still open.
            assert!(FloorOracle::active_floor_round().is_some());
            assert_ok!(report(round.round_id, 51, 8));
            let floor = FloorOracle::last_finalised_floor().expect("finalised");
            assert_eq!(floor.observation, observation(51));
            assert_eq!((floor.agreeing, floor.reports), (6, 8));
        });
    }

    #[test]
    fn a_split_committee_fails_early() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            // 6 needed of 10. After 5 distinct answers the leader (1) plus the 5 remaining
            // voters could still reach 6; after the 6th distinct answer they cannot.
            for i in 1..=5 {
                assert_ok!(report(round.round_id, 50 + i as u128, i));
                assert!(FloorOracle::active_floor_round().is_some(), "report {}", i);
            }
            assert_ok!(report(round.round_id, 56, 6));

            assert!(FloorOracle::last_finalised_floor().is_none());
            assert_round_cleared(&round);
            assert_event(Event::FloorRoundFailed {
                round_id: round.round_id,
                result: ProposalStatusEnum::Resolved { passed: false },
            });

            assert_ok!(request());
            assert_eq!(FloorOracle::active_floor_round().unwrap().round_id, round.round_id + 1);
        });
    }

    #[test]
    fn expiry_fails_the_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            assert_ok!(report(round.round_id, 50, 1));
            assert_ok!(report(round.round_id, 50, 2));

            let end = Watchtower::proposals(round.proposal_id).unwrap().end_at.unwrap();
            System::set_block_number(end);
            assert_ok!(Watchtower::finalise_proposal(
                RawOrigin::Signed(requester()).into(),
                round.proposal_id
            ));

            assert!(FloorOracle::last_finalised_floor().is_none());
            assert_round_cleared(&round);
            assert_event(Event::FloorRoundFailed {
                round_id: round.round_id,
                result: ProposalStatusEnum::Resolved { passed: false },
            });
        });
    }

    #[test]
    fn cancelled_proposal_fails_the_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            assert_ok!(report(round.round_id, 50, 1));

            <FloorOracle as WatchtowerHooks<pallet_watchtower::Proposal<TestRuntime>>>::on_cancelled(
                round.proposal_id,
                &H256::zero(),
            );

            assert_round_cleared(&round);
            assert_event(Event::FloorRoundFailed {
                round_id: round.round_id,
                result: ProposalStatusEnum::Cancelled,
            });
        });
    }

    /// Nodes that reported and then deregistered must not leave rows behind: the cleanup is
    /// bounded by the reports the round stored, not by who is registered when it ends.
    #[test]
    fn rows_of_deregistered_reporters_are_cleared() {
        let mut ext = ExtBuilder::build_default().as_externality();
        let round = ext.execute_with(|| {
            let round = start_active_round();
            // Two answers of three reports each: still undecided.
            for i in 1..=6 {
                assert_ok!(report(round.round_id, if i <= 3 { 50 } else { 51 }, i));
            }
            assert!(FloorOracle::active_floor_round().is_some());
            round
        });
        // Reports land in earlier blocks on a real chain; commit them so the cleanup deletes
        // committed keys, as it does there.
        ext.commit_all().unwrap();

        ext.execute_with(|| {
            // Three nodes left: below the 6 stored reports and the mock's max committee of 5.
            set_authorized_watchtowers(vec![watchtower(1), watchtower(2), watchtower(3)]);
            <FloorOracle as WatchtowerHooks<pallet_watchtower::Proposal<TestRuntime>>>::on_cancelled(
                round.proposal_id,
                &H256::zero(),
            );

            assert_round_cleared(&round);
        });
    }

    /// `on_idle` deletes only the page the weight left in the block pays for, and keeps the
    /// round queued until every row is gone.
    #[test]
    fn cleanup_is_paged_by_the_idle_weight() {
        let mut ext = ExtBuilder::build_default().as_externality();
        let round = ext.execute_with(|| {
            let round = start_active_round();
            // Two answers of two reports each: still undecided.
            for i in 1..=4 {
                assert_ok!(report(round.round_id, if i <= 2 { 50 } else { 51 }, i));
            }
            <FloorOracle as WatchtowerHooks<pallet_watchtower::Proposal<TestRuntime>>>::on_cancelled(
                round.proposal_id,
                &H256::zero(),
            );
            round
        });
        let key = (OracleChannel::Floor, round.round_id);
        let page = |n| <TestRuntime as crate::Config>::WeightInfo::cleanup_round_page(n);
        let rows = || {
            (
                FloorReports::<TestRuntime>::iter_prefix(round.round_id).count(),
                FloorObservations::<TestRuntime>::iter_prefix(round.round_id).count(),
            )
        };
        // Each step is its own block, with the previous blocks' deletions committed.
        let mut idle_block = |weight: Weight| {
            ext.commit_all().unwrap();
            ext.execute_with(|| {
                let used = FloorOracle::cleanup_finished_rounds(weight);
                (used, rows(), RoundsToClean::<TestRuntime>::contains_key(key))
            })
        };

        // Not enough for a single row: nothing is touched.
        assert_eq!(idle_block(page(0)), (Weight::zero(), (4, 2), true));
        // Two rows of each map per block. The observations fit in one page.
        assert_eq!(idle_block(page(2)), (page(2), (2, 0), true));
        // The last two reports fill the page exactly, and the round is known to be empty.
        assert_eq!(idle_block(page(2)), (page(2), (0, 0), false));
        // Nothing queued: one read.
        let one_read = <<TestRuntime as frame_system::Config>::DbWeight as Get<
            frame_support::weights::RuntimeDbWeight,
        >>::get()
        .reads(1);
        assert_eq!(idle_block(page(2)), (one_read, (0, 0), false));
    }
}

mod offchain_worker {
    use super::*;
    use sp_runtime::traits::Dispatchable;

    fn pop_call(pool: &Arc<RwLock<PoolState>>) -> RuntimeCall {
        let tx = pool.write().transactions.pop().expect("a transaction was submitted");
        assert!(pool.read().transactions.is_empty(), "exactly one transaction expected");
        Extrinsic::decode(&mut &tx[..]).expect("decodes").function
    }

    #[test]
    fn reports_once() {
        let (mut ext, pool, offchain) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            let round = start_active_round();
            let obs = observation(50);
            mock_floor_service(
                &mut offchain.write(),
                &round.market,
                round.external_block,
                Some(encoded_observation(&obs)),
            );
            mock_country_service(&mut offchain.write(), Some(encoded_country(&TEST_COUNTRY)));

            FloorOracle::offchain_worker(System::block_number());
            let call = pop_call(&pool);
            match &call {
                RuntimeCall::FloorOracle(
                    inner @ Call::submit_floor_report {
                        round_id,
                        observation,
                        country,
                        watchtower,
                        ..
                    },
                ) => {
                    assert_eq!(
                        (*round_id, *observation, *country, *watchtower),
                        (round.round_id, obs, TEST_COUNTRY, mock::watchtower(1))
                    );
                    assert_ok!(validate(inner));
                },
                other => panic!("unexpected call {:?}", other),
            }
            assert_ok!(call.dispatch(RawOrigin::None.into()));
            assert_eq!(Watchtower::choice_tally(round.proposal_id, obs.hash()), 1);

            // Same block again: nothing (already ran). Next block: nothing (already reported).
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty());
            System::set_block_number(System::block_number() + 1);
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty());
        });
    }

    #[test]
    fn does_nothing_without_an_active_proposal_or_with_a_bad_service_response() {
        let (mut ext, pool, offchain) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            seed_floor_chain();
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "no round");

            assert_ok!(request());
            System::set_block_number(2);
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "proposal not active yet");

            let round = FloorOracle::active_floor_round().unwrap();
            activate_head();
            System::set_block_number(3);
            mock_floor_service(
                &mut offchain.write(),
                &round.market,
                round.external_block,
                Some(b"not an observation".to_vec()),
            );
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "undecodable response");

            System::set_block_number(4);
            let wrong_block = FloorObservation { external_block: 1, ..observation(50) };
            mock_floor_service(
                &mut offchain.write(),
                &round.market,
                round.external_block,
                Some(encoded_observation(&wrong_block)),
            );
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "observation at the wrong block");
        });
    }
}
