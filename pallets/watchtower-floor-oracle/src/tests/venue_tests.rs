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
/// Block timestamp of the mocked observations. The test clock starts at second 1, so a
/// finalised observation is fresh until `MAX_VENUE_AGE_SECS` seconds have passed.
const PINNED_TS: u64 = 1;

fn advance_secs(secs: u64) {
    Timestamp::set_timestamp(Timestamp::get() + secs * 1_000);
}

fn seed_venue_chain() -> Checkpoint {
    assert_ok!(FloorOracle::seed_chain(
        RuntimeOrigin::root(),
        VENUE_CHAIN,
        SEED_BLOCK,
        CHECKPOINT_MAX_AGE
    ));
    FloorOracle::checkpoints(VENUE_CHAIN).unwrap()
}

fn set_market() {
    assert_ok!(FloorOracle::set_venue_market(RuntimeOrigin::root(), Some(test_market())));
}

/// Requests as the privileged account, setting it first if needed.
fn request() -> DispatchResult {
    if FloorOracle::oracle_requester().is_none() {
        assert_ok!(FloorOracle::set_oracle_requester(RuntimeOrigin::root(), Some(requester())));
    }
    FloorOracle::request_venue_state(RuntimeOrigin::signed(requester()))
}

/// Configures the market, seeds its chain, requests and activates the proposal.
fn start_active_round() -> VenueRound<BlockNumber> {
    set_market();
    seed_venue_chain();
    assert_ok!(request());
    let round = FloorOracle::active_venue_round().expect("round started");
    assert_eq!(activate_head(), round.proposal_id);
    round
}

fn observation(spot_e18: u128) -> VenueObservation {
    VenueObservation {
        pinned_block: SEED_BLOCK as u64,
        pinned_ts: PINNED_TS,
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

fn assert_round_cleared(round: &VenueRound<BlockNumber>) {
    assert!(FloorOracle::active_venue_round().is_none());
    assert_eq!(VenueReports::<TestRuntime>::iter_prefix(round.round_id).count(), 0);
    assert_eq!(VenueObservations::<TestRuntime>::iter_prefix(round.round_id).count(), 0);
}

mod requests {
    use super::*;

    #[test]
    fn only_root_or_the_privileged_requester_may_request() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            seed_venue_chain();
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
    fn requires_a_market_and_a_seeded_chain() {
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
            assert_noop!(request(), Error::<TestRuntime>::ChainNotRegistered);

            seed_venue_chain();
            assert_ok!(request());
            assert!(FloorOracle::active_venue_round().is_some());

            assert_ok!(FloorOracle::set_venue_market(RuntimeOrigin::root(), None));
            assert_eq!(FloorOracle::venue_market(), None);
        });
    }

    #[test]
    fn fresh_checkpoint_starts_a_choice_proposal() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            seed_venue_chain();
            assert_ok!(request());

            let round = FloorOracle::active_venue_round().expect("round started");
            assert_eq!(round.round_id, 1);
            assert_eq!(round.pinned_block, SEED_BLOCK);
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
                pinned_block: SEED_BLOCK,
                market: test_market(),
            });
        });
    }

    #[test]
    fn requests_join_the_running_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            seed_venue_chain();
            assert_ok!(request());
            assert_ok!(request());
            assert_eq!(FloorOracle::last_venue_round_id(), 1);
            assert_event(Event::VenueRoundInProgress { round_id: 1 });
        });
    }

    #[test]
    fn stale_checkpoint_waits_for_the_checkpoint_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            seed_venue_chain();
            advance_secs(CHECKPOINT_MAX_AGE + 1);

            assert_ok!(request());
            assert!(FloorOracle::pending_venue_request());
            assert!(FloorOracle::active_venue_round().is_none());
            let checkpoint_round = FloorOracle::checkpoint_round(VENUE_CHAIN).expect("started");
            assert_event(Event::VenueRequestWaitingForCheckpoint {
                checkpoint_round_id: checkpoint_round.round_id,
            });

            for i in 1..=4 {
                assert_ok!(VoteContext::new(
                    VENUE_CHAIN,
                    checkpoint_round.round_id,
                    i,
                    SEED_BLOCK + 50
                )
                .submit());
            }
            assert!(!FloorOracle::pending_venue_request());
            let round = FloorOracle::active_venue_round().expect("round started");
            assert_eq!(round.pinned_block, SEED_BLOCK + 50);
            assert_eq!(round.market, test_market());
        });
    }

    #[test]
    fn checkpoints_of_other_chains_do_not_start_rounds() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_market();
            seed_venue_chain();
            assert_ok!(FloorOracle::seed_chain(
                RuntimeOrigin::root(),
                FLOOR_CHAIN,
                SEED_BLOCK,
                CHECKPOINT_MAX_AGE
            ));
            advance_secs(CHECKPOINT_MAX_AGE + 1);
            assert_ok!(request());
            assert!(FloorOracle::pending_venue_request());

            // The floor chain settles a checkpoint round; the venue request keeps waiting.
            assert_ok!(FloorOracle::request_checkpoint(RuntimeOrigin::root(), FLOOR_CHAIN));
            let floor_round = FloorOracle::checkpoint_round(FLOOR_CHAIN).expect("started");
            for i in 1..=4 {
                assert_ok!(
                    VoteContext::new(FLOOR_CHAIN, floor_round.round_id, i, SEED_BLOCK + 5).submit()
                );
            }
            assert!(FloorOracle::checkpoint_round(FLOOR_CHAIN).is_none(), "agreed");
            assert!(FloorOracle::pending_venue_request());
            assert!(FloorOracle::active_venue_round().is_none());
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
                pinned_block: SEED_BLOCK,
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
            seed_venue_chain();
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
                        VenueObservation { pinned_ts: 0, ..observation(50) },
                        2,
                    ),
                    Error::ImplausibleVenueObservation,
                ),
                (
                    VenueReportContext::new(
                        round.round_id,
                        VenueObservation { pinned_block: SEED_BLOCK as u64 - 1, ..observation(50) },
                        2,
                    ),
                    Error::VenueObservationBlockMismatch,
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
                    FloorObservation { observed_eth_block: 1, floor_wei: 50, ..Default::default() },
                    3,
                )
                .signature(),
            };
            assert_eq!(
                validate(&floor_signed),
                Err(InvalidTransaction::Custom(VENUE_REPORT_NOT_VALID))
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
                pinned_block: SEED_BLOCK as u64,
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
                pinned_block: SEED_BLOCK as u64,
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
}

mod cross_channel {
    use super::*;

    fn seed_floor_chain() {
        assert_ok!(FloorOracle::set_floor_market(RuntimeOrigin::root(), Some(test_floor_market())));
        assert_ok!(FloorOracle::seed_chain(
            RuntimeOrigin::root(),
            FLOOR_CHAIN,
            SEED_BLOCK,
            CHECKPOINT_MAX_AGE
        ));
    }

    pub(super) fn floor_obs(floor_wei: u128) -> FloorObservation {
        FloorObservation {
            observed_eth_block: SEED_BLOCK as u64,
            floor_wei,
            last_sale_ts: 1_700_000_000,
            last_sale_wei: floor_wei,
            flags: 0,
        }
    }

    #[test]
    fn rounds_on_both_channels_queue_through_the_watchtower() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed_floor_chain();
            set_market();
            seed_venue_chain();
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

            assert_eq!(activate_head(), venue_round.proposal_id);
            for i in 1..=6 {
                assert_ok!(report(venue_round.round_id, 50, i));
            }
            let venue = FloorOracle::last_finalised_venue().expect("finalised");
            assert_eq!(venue.observation, observation(50));
            assert_round_cleared(&venue_round);
        });
    }

    #[test]
    fn one_checkpoint_agreement_starts_both_pending_rounds() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            // A market on the floor chain, so both channels wait for the same checkpoint.
            let market = VenueMarket { chain_id: FLOOR_CHAIN, ..test_market() };
            assert_ok!(FloorOracle::set_venue_market(RuntimeOrigin::root(), Some(market)));
            seed_floor_chain();
            advance_secs(CHECKPOINT_MAX_AGE + 1);

            assert_ok!(FloorOracle::request_floor_price(RuntimeOrigin::root()));
            assert_ok!(request());
            assert!(FloorOracle::pending_floor_request());
            assert!(FloorOracle::pending_venue_request());
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

            assert!(!FloorOracle::pending_floor_request());
            assert!(!FloorOracle::pending_venue_request());
            let floor_round = FloorOracle::active_floor_round().expect("floor round");
            let venue_round = FloorOracle::active_venue_round().expect("venue round");
            assert_eq!(floor_round.pinned_block, SEED_BLOCK + 50);
            assert_eq!(venue_round.pinned_block, SEED_BLOCK + 50);
            assert_eq!(venue_round.market, market);
            assert_ne!(floor_round.proposal_id, venue_round.proposal_id);
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
                round.pinned_block,
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

    #[test]
    fn the_request_path_names_the_market() {
        let market = test_market();
        assert_eq!(
            FloorOracle::venue_service_path(&market, 123),
            format!("venue/exotic/8453/123/{}/{}", "56".repeat(20), "a1".repeat(20))
        );
    }

    #[test]
    fn does_nothing_without_an_active_proposal_or_with_a_bad_service_response() {
        let (mut ext, pool, offchain) =
            ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
        ext.execute_with(|| {
            set_market();
            seed_venue_chain();
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
                round.pinned_block,
                Some(b"not an observation".to_vec()),
            );
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "undecodable response");

            System::set_block_number(4);
            let wrong_block = VenueObservation { pinned_block: 1, ..observation(50) };
            mock_venue_service(
                &mut offchain.write(),
                &round.market,
                round.pinned_block,
                Some(encoded_venue_observation(&wrong_block)),
            );
            FloorOracle::offchain_worker(System::block_number());
            assert!(pool.read().transactions.is_empty(), "observation at the wrong block");
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
            mock_floor_service(
                &mut offchain.write(),
                &floor_round.market,
                floor_round.pinned_block,
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
                venue_round.pinned_block,
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
        assert_ok!(FloorOracle::set_floor_market(RuntimeOrigin::root(), Some(test_floor_market())));
        assert_ok!(FloorOracle::seed_chain(
            RuntimeOrigin::root(),
            FLOOR_CHAIN,
            SEED_BLOCK,
            CHECKPOINT_MAX_AGE
        ));
        set_market();
        seed_venue_chain();
        assert_ok!(FloorOracle::request_floor_price(RuntimeOrigin::root()));
        assert_ok!(FloorOracle::request_venue_state(RuntimeOrigin::root()));
    }
}
