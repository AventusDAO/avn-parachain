// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_core::H160;
use sp_runtime::{
    traits::ValidateUnsigned,
    transaction_validity::{InvalidTransaction, TransactionSource},
};
use sp_watchtower::VoteChoice;

const SEED_BLOCK: u32 = 40_000_000;
const CHECKPOINT_MAX_AGE: u64 = 10_000;
/// Parachain time, in seconds, the venue tests start at.
const CLOCK_START: u64 = 1_750_000_000;
/// Target of a round activated at `CLOCK_START`.
const TARGET_TS: u64 = CLOCK_START - VENUE_TARGET_LAG_SECS;
/// Block timestamp of the mocked observations: the latest venue block at or before `TARGET_TS`.
/// A finalised observation is fresh until `MAX_VENUE_AGE_SECS` seconds after it.
const PINNED_TS: u64 = TARGET_TS - 1;

fn advance_secs(secs: u64) {
    Timestamp::set_timestamp(Timestamp::get() + secs * 1_000);
}

/// Sets the market and starts the clock at `CLOCK_START`, so targets are real timestamps.
fn set_market() {
    Timestamp::set_timestamp(CLOCK_START * 1_000);
    assert_ok!(FloorOracle::set_venue_market(RuntimeOrigin::root(), Some(test_market())));
}

/// Requests as the privileged account, setting it first if needed.
fn request() -> DispatchResult {
    if FloorOracle::oracle_requester().is_none() {
        assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
    }
    FloorOracle::request_venue_state(RuntimeOrigin::signed(requester()))
}

/// Configures the market, requests and activates the proposal. Returns the activated round.
fn start_active_round() -> VenueRound<BlockNumber> {
    set_market();
    assert_ok!(request());
    let round = FloorOracle::active_venue_round().expect("round started");
    assert_eq!(activate_head(), round.proposal_id);
    let round = FloorOracle::active_venue_round().expect("round still open");
    assert_eq!(round.target_external_block_time_sec, Some(TARGET_TS));
    round
}

fn observation(spot_e18: u128) -> VenueObservation {
    VenueObservation {
        external_block: SEED_BLOCK as u64,
        external_block_time_sec: PINNED_TS,
        spot_e18,
        oracle_e18: spot_e18 - 1,
        oi_notional_e18: 1_000,
    }
}

fn report(round_id: u64, spot_e18: u128, watchtower_index: u8) -> DispatchResult {
    VenueReportContext::new(round_id, observation(spot_e18), watchtower_index).submit()
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
fn assert_round_cleared(round: &VenueRound<BlockNumber>) {
    assert!(FloorOracle::active_venue_round().is_none());
    let key = (OracleChannel::Venue, round.round_id);
    let stored = VenueReports::<TestRuntime>::iter_prefix(round.round_id).count();
    assert_eq!(RoundsToClean::<TestRuntime>::contains_key(key), stored > 0);

    run_idle_cleanup();
    assert!(!RoundsToClean::<TestRuntime>::contains_key(key));
    assert_eq!(VenueReports::<TestRuntime>::iter_prefix(round.round_id).count(), 0);
    assert_eq!(VenueObservations::<TestRuntime>::iter_prefix(round.round_id).count(), 0);
}

mod requests {
    use super::*;

    #[test]
    fn only_root_or_the_privileged_requester_may_request() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            assert_noop!(
                FloorOracle::request_venue_state(RuntimeOrigin::none()),
                sp_runtime::DispatchError::BadOrigin
            );
            assert_noop!(
                FloorOracle::request_venue_state(RuntimeOrigin::signed(requester())),
                Error::<TestRuntime>::UnauthorizedOracleRequest
            );
            assert_ok!(FloorOracle::request_venue_state(RuntimeOrigin::root()));
            assert_eq!(FloorOracle::last_venue_round_id(), 1);

            assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
            assert_ok!(FloorOracle::request_venue_state(RuntimeOrigin::signed(requester())));
            assert_noop!(
                FloorOracle::request_venue_state(RuntimeOrigin::signed(watchtower(1))),
                Error::<TestRuntime>::UnauthorizedOracleRequest
            );
        });
    }

    #[test]
    fn requires_a_market() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
            assert_noop!(request(), Error::<TestRuntime>::VenueMarketNotSet);

            assert_noop!(
                FloorOracle::set_venue_market(
                    RuntimeOrigin::signed(requester()),
                    Some(test_market())
                ),
                sp_runtime::DispatchError::BadOrigin
            );
            set_market();
            assert_event(Event::VenueMarketSet { market: Some(test_market()) });
            assert_eq!(FloorOracle::venue_market(), Some(test_market()));
            assert_ok!(request());
            assert!(FloorOracle::active_venue_round().is_some());

            let max_tolerance = MAX_VENUE_AGE_SECS - VENUE_TARGET_LAG_SECS;
            for tolerance in [0, max_tolerance + 1] {
                assert_noop!(
                    FloorOracle::set_venue_market(
                        RuntimeOrigin::root(),
                        Some(VenueMarket { target_tolerance_secs: tolerance, ..test_market() })
                    ),
                    Error::<TestRuntime>::InvalidVenueTargetTolerance
                );
            }
            assert_ok!(FloorOracle::set_venue_market(
                RuntimeOrigin::root(),
                Some(VenueMarket { target_tolerance_secs: max_tolerance, ..test_market() })
            ));

            assert_ok!(FloorOracle::set_venue_market(RuntimeOrigin::root(), None));
            assert_eq!(FloorOracle::venue_market(), None);
        });
    }

    /// The venue chain has no checkpoint at all: the round starts anyway, and no checkpoint
    /// round is opened for it.
    #[test]
    fn a_request_starts_a_choice_proposal_without_a_checkpoint() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            assert_ok!(request());

            let round = FloorOracle::active_venue_round().expect("round started");
            assert!(FloorOracle::checkpoint_round(VENUE_CHAIN).is_none());
            assert_eq!(round.round_id, 1);
            assert_eq!(round.target_external_block_time_sec, None, "set on activation");
            assert_eq!(round.market, test_market());
            assert_eq!(round.reports, 0);
            assert_eq!(round.started_at, System::block_number());
            assert_eq!(FloorOracle::last_venue_round_id(), 1);

            let proposal = Watchtower::proposals(round.proposal_id).expect("proposal exists");
            assert_eq!(
                proposal.source,
                sp_watchtower::ProposalSource::Internal(sp_watchtower::ProposalType::VenueOracle)
            );
            assert_eq!(proposal.decision_rule, sp_watchtower::DecisionRule::RejectOnExpiry);
            assert_eq!(proposal.external_ref, FloorOracle::venue_round_external_ref(1));
            assert_ne!(proposal.external_ref, FloorOracle::floor_round_external_ref(1));
            assert_eq!(proposal.vote_duration, ORACLE_VOTE_DURATION);
            assert_eq!(
                pallet_watchtower::VotingKindOf::<TestRuntime>::get(round.proposal_id),
                sp_watchtower::VotingKind::Choice
            );
            assert_eq!(Watchtower::proposal_status(round.proposal_id), ProposalStatusEnum::Queued);
            assert_event(Event::VenueRoundStarted {
                round_id: 1,
                proposal_id: round.proposal_id,
                market: test_market(),
            });
        });
    }

    /// The target is taken when the proposal activates, so time in the queue does not age it.
    #[test]
    fn activation_sets_the_target_timestamp() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            assert_ok!(request());
            advance_secs(30);
            let round = FloorOracle::active_venue_round().unwrap();
            assert_eq!(activate_head(), round.proposal_id);

            let target_ts = CLOCK_START + 30 - VENUE_TARGET_LAG_SECS;
            assert_eq!(
                FloorOracle::active_venue_round().unwrap().target_external_block_time_sec,
                Some(target_ts)
            );
            assert_event(Event::VenueRoundActivated {
                round_id: round.round_id,
                target_external_block_time_sec: target_ts,
            });
        });
    }

    #[test]
    fn requests_join_the_running_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            assert_ok!(request());
            assert_ok!(request());
            assert_eq!(FloorOracle::last_venue_round_id(), 1);
            assert_event(Event::VenueRoundInProgress { round_id: 1 });
        });
    }

    #[test]
    fn a_market_change_only_affects_the_next_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            let other = VenueMarket { amm: H160::repeat_byte(0xB2), ..test_market() };
            assert_ok!(FloorOracle::set_venue_market(RuntimeOrigin::root(), Some(other)));

            // The running round keeps its snapshot and its reports still validate.
            assert_eq!(FloorOracle::active_venue_round().unwrap().market, test_market());
            for i in 1..=6 {
                assert_ok!(report(round.round_id, 50, i));
            }
            let venue = FloorOracle::last_finalised_venue().expect("finalised");
            assert_eq!(venue.market, test_market());

            // The next round reads the new market.
            advance_secs(MAX_VENUE_AGE_SECS + 1);
            assert_ok!(request());
            let next = FloorOracle::active_venue_round().expect("round started");
            assert_eq!(next.market, other);
            assert_event(Event::VenueRoundStarted {
                round_id: next.round_id,
                proposal_id: next.proposal_id,
                market: other,
            });
        });
    }
}

mod reports {
    use super::*;

    #[test]
    fn rejected_until_the_proposal_is_active() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            assert_ok!(request());
            let report = VenueReportContext::new(1, observation(50), 1);
            assert_eq!(
                validate(&report.call()),
                Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID))
            );
            assert_noop!(report.submit(), Error::<TestRuntime>::VenueProposalNotActive);
        });
    }

    #[test]
    fn reports_are_counted_as_answers() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            let (fifty, fifty_one) = (observation(50).hash(), observation(51).hash());

            let first = VenueReportContext::new(round.round_id, observation(50), 1);
            assert_ok!(validate(&first.call()));
            assert_ok!(first.submit());
            assert_event(Event::VenueReportSubmitted {
                round_id: 1,
                watchtower: watchtower(1),
                hash: fifty,
                country: TEST_COUNTRY,
            });
            assert_ok!(report(round.round_id, 51, 2));
            assert_ok!(report(round.round_id, 50, 3));

            let round = FloorOracle::active_venue_round().unwrap();
            assert_eq!(round.reports, 3);
            assert_eq!(FloorOracle::venue_reports(1, watchtower(1)), Some(fifty));
            assert_eq!(FloorOracle::venue_reports(1, watchtower(2)), Some(fifty_one));
            assert_eq!(FloorOracle::venue_observations(1, fifty), Some(observation(50)));
            assert_eq!(FloorOracle::venue_observations(1, fifty_one), Some(observation(51)));
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

            let cases: Vec<(VenueReportContext, Error<TestRuntime>)> = vec![
                (
                    VenueReportContext::new(round.round_id, observation(50), 1),
                    Error::DuplicateVenueReport,
                ),
                (
                    VenueReportContext::new(round.round_id + 1, observation(50), 2),
                    Error::VenueRoundMismatch,
                ),
                (
                    VenueReportContext::new(
                        round.round_id,
                        VenueObservation { spot_e18: 0, ..observation(50) },
                        2,
                    ),
                    Error::ImplausibleVenueObservation,
                ),
                (
                    VenueReportContext::new(
                        round.round_id,
                        VenueObservation { external_block_time_sec: 0, ..observation(50) },
                        2,
                    ),
                    Error::ImplausibleVenueObservation,
                ),
                (
                    VenueReportContext::new(
                        round.round_id,
                        VenueObservation {
                            external_block_time_sec: TARGET_TS + 1,
                            ..observation(50)
                        },
                        2,
                    ),
                    Error::VenueObservationOffTarget,
                ),
                (
                    VenueReportContext::new(
                        round.round_id,
                        VenueObservation {
                            external_block_time_sec: TARGET_TS - VENUE_TARGET_TOLERANCE_SECS,
                            ..observation(50)
                        },
                        2,
                    ),
                    Error::VenueObservationOffTarget,
                ),
            ];
            for (report, error) in cases {
                assert_eq!(
                    validate(&report.call()),
                    Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID)),
                    "{:?}",
                    error
                );
                assert_noop!(report.submit(), error);
            }

            // Both ends of the target window are accepted.
            let at_target =
                VenueObservation { external_block_time_sec: TARGET_TS, ..observation(50) };
            assert_ok!(validate(&VenueReportContext::new(round.round_id, at_target, 2).call()));
            let oldest = VenueObservation {
                external_block_time_sec: TARGET_TS - VENUE_TARGET_TOLERANCE_SECS + 1,
                ..observation(50)
            };
            assert_ok!(validate(&VenueReportContext::new(round.round_id, oldest, 2).call()));

            // Zero oracle price and zero open interest are raw values, not errors.
            let zeros = VenueObservation { oracle_e18: 0, oi_notional_e18: 0, ..observation(50) };
            assert_ok!(VenueReportContext::new(round.round_id, zeros, 2).submit());

            set_authorized_watchtowers(vec![watchtower(1)]);
            let outsider = VenueReportContext::new(round.round_id, observation(50), 3);
            assert_eq!(
                validate(&outsider.call()),
                Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID))
            );
            assert_noop!(outsider.submit(), Error::<TestRuntime>::NotInVenueCommittee);
            set_authorized_watchtowers(all_watchtowers());

            let forged = Call::<TestRuntime>::submit_venue_report {
                round_id: round.round_id,
                observation: observation(50),
                country: TEST_COUNTRY,
                watchtower: watchtower(3),
                signature: VenueReportContext::new(round.round_id, observation(50), 4).signature(),
            };
            assert_eq!(validate(&forged), Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID)));

            // A floor report signature does not validate a venue report either.
            let floor_signed = Call::<TestRuntime>::submit_venue_report {
                round_id: round.round_id,
                observation: observation(50),
                country: TEST_COUNTRY,
                watchtower: watchtower(3),
                signature: ReportContext::new(
                    round.round_id,
                    FloorObservation { external_block: 1, floor_wei: 50, ..Default::default() },
                    3,
                )
                .signature(),
            };
            assert_eq!(
                validate(&floor_signed),
                Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID))
            );

            // The chain and the round's market are signed too: a report made on another
            // deployment, or for another market, does not verify.
            let signed = VenueReportContext::new(round.round_id, observation(50), 3).call();
            assert_eq!(validate(&signed), Ok(()));
            let genesis = frame_system::BlockHash::<TestRuntime>::get(0);
            frame_system::BlockHash::<TestRuntime>::insert(0, H256::repeat_byte(9));
            assert_eq!(validate(&signed), Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID)));
            frame_system::BlockHash::<TestRuntime>::insert(0, genesis);
            ActiveVenueRound::<TestRuntime>::mutate(|round| {
                round.as_mut().expect("round running").market.chain_id += 1
            });
            assert_eq!(validate(&signed), Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID)));
        });
    }
}

mod tolerance {
    use super::*;

    /// A chain with slower blocks needs a wider window: the round uses the tolerance of the
    /// market it snapshotted.
    #[test]
    fn the_market_sets_how_far_before_the_target_a_block_may_be() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            Timestamp::set_timestamp(CLOCK_START * 1_000);
            let slow = VenueMarket { target_tolerance_secs: 15, ..test_market() };
            assert_ok!(FloorOracle::set_venue_market(RuntimeOrigin::root(), Some(slow)));
            assert_ok!(request());
            let round = FloorOracle::active_venue_round().unwrap();
            assert_eq!(activate_head(), round.proposal_id);

            // 12s before the target: outside the default 6s, inside this market's 15s.
            let older =
                VenueObservation { external_block_time_sec: TARGET_TS - 12, ..observation(50) };
            assert_ok!(validate(&VenueReportContext::new(round.round_id, older, 1).call()));
            let too_old =
                VenueObservation { external_block_time_sec: TARGET_TS - 15, ..observation(50) };
            assert_noop!(
                VenueReportContext::new(round.round_id, too_old, 1).submit(),
                Error::<TestRuntime>::VenueObservationOffTarget
            );
        });
    }
}

mod committee {
    use super::*;

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

            let outsider = VenueReportContext::new(round.round_id, observation(50), outsiders[0]);
            assert_eq!(
                validate(&outsider.call()),
                Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID))
            );
            assert_noop!(outsider.submit(), Error::<TestRuntime>::NotInVenueCommittee);
            assert_noop!(
                <FloorOracle as VoteFilter<AccountId>>::ensure_can_vote(
                    round.proposal_id,
                    &watchtower(outsiders[0]),
                    &VoteChoice::Choice(observation(50).hash())
                ),
                Error::<TestRuntime>::NotInVenueCommittee
            );

            // The mock threshold is 60% of 3 = 2 matching reports.
            assert_ok!(report(round.round_id, 50, members[0]));
            assert_ok!(report(round.round_id, 50, members[1]));
            let venue = FloorOracle::last_finalised_venue().expect("finalised");
            assert_eq!((venue.agreeing, venue.reports, venue.required), (2, 2, 2));
        });
    }
}

mod voting {
    use super::*;

    #[test]
    fn yes_no_votes_are_refused_on_venue_proposals() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            assert_ok!(report(round.round_id, 50, 1));

            assert!(!validate_watchtower(&vote_call(round.proposal_id, true, 1)));
            assert_noop!(
                submit_vote(round.proposal_id, true, 2),
                pallet_watchtower::Error::<TestRuntime>::WrongVoteKind
            );
            assert_noop!(
                <FloorOracle as VoteFilter<AccountId>>::ensure_can_vote(
                    round.proposal_id,
                    &watchtower(1),
                    &VoteChoice::Binary(true)
                ),
                Error::<TestRuntime>::VenueVotesAreReports
            );
        });
    }

    #[test]
    fn a_direct_answer_must_be_the_reported_one() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            let other = observation(99).hash();

            assert!(!validate_watchtower(&choice_vote_call(round.proposal_id, other, 1)));
            assert_noop!(
                submit_choice_vote(round.proposal_id, other, 1),
                Error::<TestRuntime>::NoVenueReport
            );

            assert_ok!(report(round.round_id, 50, 1));
            assert!(!validate_watchtower(&choice_vote_call(round.proposal_id, other, 1)));
            assert_noop!(
                submit_choice_vote(round.proposal_id, other, 1),
                Error::<TestRuntime>::VenueVoteContradictsReport
            );
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

            assert_ok!(report(round.round_id, 52, 7));
            for i in 1..=6 {
                assert!(FloorOracle::active_venue_round().is_some(), "not finalised before quorum");
                assert_ok!(report(round.round_id, 51, i));
            }

            let venue = FloorOracle::last_finalised_venue().expect("finalised");
            assert_eq!(venue.round_id, round.round_id);
            assert_eq!(venue.proposal_id, round.proposal_id);
            assert_eq!(venue.market, test_market());
            assert_eq!(venue.observation, observation(51));
            assert_eq!((venue.agreeing, venue.reports, venue.required), (6, 7, 6));
            assert_eq!(venue.finalised_at_block, System::block_number() as u32);
            assert_event(Event::VenueRoundFinalised {
                round_id: round.round_id,
                external_block: SEED_BLOCK as u64,
                external_block_time_sec: observation(51).external_block_time_sec,
                spot_e18: 51,
                oracle_e18: 50,
                oi_notional_e18: 1_000,
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

            // A request now is answered from the finalised state, until it ages out.
            assert_ok!(request());
            assert_event(Event::VenueStateServed {
                round_id: round.round_id,
                external_block: SEED_BLOCK as u64,
            });
            assert!(FloorOracle::active_venue_round().is_none());
            advance_secs(MAX_VENUE_AGE_SECS + 1);
            assert_ok!(request());
            assert_eq!(FloorOracle::active_venue_round().unwrap().round_id, round.round_id + 1);
        });
    }

    #[test]
    fn a_split_committee_fails_early() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            for i in 1..=5 {
                assert_ok!(report(round.round_id, 50 + i as u128, i));
                assert!(FloorOracle::active_venue_round().is_some(), "report {}", i);
            }
            assert_ok!(report(round.round_id, 56, 6));

            assert!(FloorOracle::last_finalised_venue().is_none());
            assert_round_cleared(&round);
            assert_event(Event::VenueRoundFailed {
                round_id: round.round_id,
                result: ProposalStatusEnum::Resolved { passed: false },
            });

            assert_ok!(request());
            assert_eq!(FloorOracle::active_venue_round().unwrap().round_id, round.round_id + 1);
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

            assert!(FloorOracle::last_finalised_venue().is_none());
            assert_round_cleared(&round);
            assert_event(Event::VenueRoundFailed {
                round_id: round.round_id,
                result: ProposalStatusEnum::Resolved { passed: false },
            });
        });
    }

    /// After the voting period a report would only finalise the proposal without being
    /// counted, so it is refused and nothing is stored.
    #[test]
    fn reports_after_the_voting_period_are_refused() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round = start_active_round();
            let end = Watchtower::proposals(round.proposal_id).unwrap().end_at.unwrap();
            System::set_block_number(end);

            let late = VenueReportContext::new(round.round_id, observation(50), 1);
            assert_eq!(
                validate(&late.call()),
                Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID))
            );
            assert_noop!(late.submit(), Error::<TestRuntime>::VenueProposalNotActive);
            assert_eq!(VenueReports::<TestRuntime>::iter_prefix(round.round_id).count(), 0);
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
            assert_event(Event::VenueRoundFailed {
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
            assert!(FloorOracle::active_venue_round().is_some());
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
}

mod cross_channel {
    use super::*;

    fn seed_floor_chain() {
        assert_ok!(FloorOracle::set_floor_market(RuntimeOrigin::root(), Some(test_floor_market())));
        assert_ok!(FloorOracle::seed_chain(
            RuntimeOrigin::root(),
            FLOOR_CHAIN,
            SEED_BLOCK,
            CHECKPOINT_MAX_AGE,
            None
        ));
    }

    pub(super) fn floor_obs(floor_wei: u128) -> FloorObservation {
        FloorObservation {
            external_block: SEED_BLOCK as u64,
            external_block_time_sec: pinned_floor_block_time(FLOOR_CHAIN),
            floor_wei,
            last_sale_ts: 1_700_000_000,
            last_sale_wei: floor_wei,
            flags: 0,
        }
    }

    #[test]
    fn rounds_on_both_channels_queue_through_the_watchtower() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            seed_floor_chain();
            assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));

            assert_ok!(FloorOracle::request_floor_price(RuntimeOrigin::signed(requester())));
            let floor_round = FloorOracle::active_floor_round().expect("floor round");
            assert_eq!(activate_head(), floor_round.proposal_id);

            // The venue round opens, but its proposal waits behind the floor one.
            assert_ok!(request());
            let venue_round = FloorOracle::active_venue_round().expect("venue round");
            assert_eq!(
                Watchtower::proposal_status(venue_round.proposal_id),
                ProposalStatusEnum::Queued
            );
            assert_noop!(
                report(venue_round.round_id, 50, 1),
                Error::<TestRuntime>::VenueProposalNotActive
            );
            // A venue report hash is not a valid answer on the floor proposal.
            assert_noop!(
                <FloorOracle as VoteFilter<AccountId>>::ensure_can_vote(
                    floor_round.proposal_id,
                    &watchtower(1),
                    &VoteChoice::Choice(observation(50).hash())
                ),
                Error::<TestRuntime>::NoFloorReport
            );

            for i in 1..=6 {
                assert_ok!(ReportContext::new(floor_round.round_id, floor_obs(50), i).submit());
            }
            assert!(FloorOracle::last_finalised_floor().is_some());
            assert!(FloorOracle::active_floor_round().is_none());
            assert_eq!(FloorOracle::active_venue_round(), Some(venue_round.clone()));
            assert_eq!(
                venue_round.target_external_block_time_sec, None,
                "the floor activation set no venue target"
            );

            assert_eq!(activate_head(), venue_round.proposal_id);
            assert_eq!(
                FloorOracle::active_venue_round().unwrap().target_external_block_time_sec,
                Some(TARGET_TS)
            );
            for i in 1..=6 {
                assert_ok!(report(venue_round.round_id, 50, i));
            }
            let venue = FloorOracle::last_finalised_venue().expect("finalised");
            assert_eq!(venue.observation, observation(50));
            assert_round_cleared(&venue_round);
        });
    }

    /// Both markets on one chain with a stale checkpoint: the floor round waits at activation
    /// for a checkpoint round, the venue round does not, and the agreement leaves the venue
    /// round alone.
    #[test]
    fn a_stale_checkpoint_only_holds_back_the_floor() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            let market = VenueMarket { chain_id: FLOOR_CHAIN, ..test_market() };
            assert_ok!(FloorOracle::set_venue_market(RuntimeOrigin::root(), Some(market)));
            seed_floor_chain();
            advance_secs(CHECKPOINT_MAX_AGE + 1);

            assert_ok!(FloorOracle::request_floor_price(RuntimeOrigin::root()));
            assert_ok!(request());
            let venue_round = FloorOracle::active_venue_round().expect("venue round");
            assert_eq!(venue_round.market, market);
            let floor_round = FloorOracle::active_floor_round().expect("floor round");
            assert_eq!(activate_head(), floor_round.proposal_id);
            assert_eq!(FloorOracle::active_floor_round().unwrap().pin, None);
            let checkpoint_round = FloorOracle::checkpoint_round(FLOOR_CHAIN).expect("started");

            for i in 1..=4 {
                assert_ok!(VoteContext::new(
                    FLOOR_CHAIN,
                    checkpoint_round.round_id,
                    i,
                    SEED_BLOCK + 50
                )
                .submit());
            }

            let floor_round = FloorOracle::active_floor_round().expect("floor round");
            assert_eq!(floor_round.pin.map(|pin| pin.external_block), Some(SEED_BLOCK + 50));
            assert_eq!(FloorOracle::active_venue_round(), Some(venue_round));
        });
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
            mock_venue_service(
                &mut offchain.write(),
                &round.market,
                TARGET_TS,
                Some(encoded_venue_observation(&obs)),
            );
            mock_country_service(&mut offchain.write(), Some(encoded_country(&TEST_COUNTRY)));

            FloorOracle::offchain_worker(System::block_number());
            let call = pop_call(&pool);
            match &call {
                RuntimeCall::FloorOracle(
                    inner @ Call::submit_venue_report {
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

            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty());
            System::set_block_number(System::block_number() + 1);
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty());
        });
    }

    /// The country is asked of the service once and then reused for an hour, so votes and
    /// reports do not each make the call. The mocked service fails the test on any request it
    /// does not expect.
    #[test]
    fn the_node_country_is_cached_for_an_hour() {
        let (mut ext, _pool, offchain) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            offchain.write().timestamp = sp_core::offchain::Timestamp::from_unix_millis(1_000);
            mock_country_service(&mut offchain.write(), Some(encoded_country(&TEST_COUNTRY)));
            assert_eq!(pallet_watchtower::fetch_node_country::<TestRuntime>(), TEST_COUNTRY);
            assert_eq!(pallet_watchtower::fetch_node_country::<TestRuntime>(), TEST_COUNTRY);

            // After an hour it is asked again. A failed refresh keeps the last known country.
            offchain.write().timestamp =
                sp_core::offchain::Timestamp::from_unix_millis(1_000 + 60 * 60 * 1000);
            mock_country_service(&mut offchain.write(), Some(encoded_country(&UNKNOWN_COUNTRY)));
            assert_eq!(pallet_watchtower::fetch_node_country::<TestRuntime>(), TEST_COUNTRY);
            // ...for another hour, without asking again on every call.
            assert_eq!(pallet_watchtower::fetch_node_country::<TestRuntime>(), TEST_COUNTRY);

            offchain.write().timestamp =
                sp_core::offchain::Timestamp::from_unix_millis(1_000 + 2 * 60 * 60 * 1000);
            mock_country_service(&mut offchain.write(), Some(encoded_country(b"DE")));
            assert_eq!(pallet_watchtower::fetch_node_country::<TestRuntime>(), *b"DE");
        });
    }

    /// Unknown is not cached: the next call asks again and picks the country up.
    #[test]
    fn an_unknown_country_is_asked_again() {
        let (mut ext, _pool, offchain) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            mock_country_service(&mut offchain.write(), Some(b"not a country".to_vec()));
            assert_eq!(pallet_watchtower::fetch_node_country::<TestRuntime>(), UNKNOWN_COUNTRY);
            mock_country_service(&mut offchain.write(), Some(encoded_country(&TEST_COUNTRY)));
            assert_eq!(pallet_watchtower::fetch_node_country::<TestRuntime>(), TEST_COUNTRY);
        });
    }

    #[test]
    fn the_request_path_names_the_market() {
        let market = test_market();
        assert_eq!(
            FloorOracle::venue_service_path(&market, 1_750_000_000),
            format!("venue/exotic/8453/1750000000/{}/{}", "56".repeat(20), "a1".repeat(20))
        );
    }

    #[test]
    fn does_nothing_without_an_active_proposal_or_with_a_bad_service_response() {
        let (mut ext, pool, offchain) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            set_market();
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "no round");

            assert_ok!(request());
            System::set_block_number(2);
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "proposal not active yet");

            let round = FloorOracle::active_venue_round().unwrap();
            activate_head();
            System::set_block_number(3);
            mock_venue_service(
                &mut offchain.write(),
                &round.market,
                TARGET_TS,
                Some(b"not an observation".to_vec()),
            );
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "undecodable response");

            System::set_block_number(System::block_number() + 1);
            mock_venue_service(
                &mut offchain.write(),
                &round.market,
                TARGET_TS,
                Some(encoded_venue_not_ready()),
            );
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "venue chain not past the target yet");

            System::set_block_number(5);
            let after_target =
                VenueObservation { external_block_time_sec: TARGET_TS + 1, ..observation(50) };
            mock_venue_service(
                &mut offchain.write(),
                &round.market,
                TARGET_TS,
                Some(encoded_venue_observation(&after_target)),
            );
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "observation off the target");
        });
    }

    /// The floor worker running in a block must not silence the venue worker in the same
    /// block: each channel has its own per-block lock and its own submission records, even
    /// when both rounds carry the same round id.
    #[test]
    fn the_two_workers_do_not_share_locks() {
        let (mut ext, pool, offchain) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            cross_channel_setup();
            let floor_round = FloorOracle::active_floor_round().expect("floor round");
            assert_eq!(activate_head(), floor_round.proposal_id);
            let floor_round = FloorOracle::active_floor_round().expect("floor round");
            mock_floor_service(
                &mut offchain.write(),
                &floor_round.market,
                floor_round.pin.expect("pinned at activation").external_block,
                Some(encoded_observation(&cross_channel::floor_obs(50))),
            );
            // An unusable country answer never blocks the report; it is reported as unknown.
            mock_country_service(&mut offchain.write(), Some(b"not a country".to_vec()));
            FloorOracle::offchain_worker(System::block_number());
            let call = pop_call(&pool);
            match &call {
                RuntimeCall::FloorOracle(Call::submit_floor_report { country, .. }) =>
                    assert_eq!(*country, UNKNOWN_COUNTRY),
                other => panic!("unexpected call {:?}", other),
            }
            assert_ok!(call.dispatch(RawOrigin::None.into()));
            for i in 2..=6 {
                assert_ok!(ReportContext::new(
                    floor_round.round_id,
                    cross_channel::floor_obs(50),
                    i
                )
                .submit());
            }
            assert!(FloorOracle::active_floor_round().is_none(), "floor finalised");

            // Same block: the venue proposal activates and the venue worker still reports.
            let venue_round = FloorOracle::active_venue_round().expect("venue round");
            assert_eq!(venue_round.round_id, floor_round.round_id, "same round id on purpose");
            assert_eq!(activate_head(), venue_round.proposal_id);
            mock_venue_service(
                &mut offchain.write(),
                &venue_round.market,
                TARGET_TS,
                Some(encoded_venue_observation(&observation(50))),
            );
            mock_country_service(&mut offchain.write(), Some(encoded_country(&TEST_COUNTRY)));
            FloorOracle::offchain_worker(System::block_number());
            let call = pop_call(&pool);
            match &call {
                RuntimeCall::FloorOracle(Call::submit_venue_report { country, .. }) =>
                    assert_eq!(*country, TEST_COUNTRY),
                other => panic!("unexpected call {:?}", other),
            }
            assert_ok!(call.dispatch(RawOrigin::None.into()));
            assert_eq!(
                Watchtower::choice_tally(venue_round.proposal_id, observation(50).hash()),
                1
            );
        });
    }

    /// Floor chain and venue chain seeded, both rounds requested (floor first).
    fn cross_channel_setup() {
        set_market();
        assert_ok!(FloorOracle::set_floor_market(RuntimeOrigin::root(), Some(test_floor_market())));
        assert_ok!(FloorOracle::seed_chain(
            RuntimeOrigin::root(),
            FLOOR_CHAIN,
            SEED_BLOCK,
            CHECKPOINT_MAX_AGE,
            None
        ));
        assert_ok!(FloorOracle::request_floor_price(RuntimeOrigin::root()));
        assert_ok!(FloorOracle::request_venue_state(RuntimeOrigin::root()));
    }
}
