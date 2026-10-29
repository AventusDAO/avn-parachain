// Copyright 2026 Aventus DAO Ltd

//! Venue rounds: the raw state of the Exotic perpetuals market for PUNKS, read at a block of the
//! venue chain (Base) picked by timestamp. The channel mirrors the floor channel in [`floor`],
//! except that it does not wait for a checkpoint: venue state must be as fresh as possible, and
//! a checkpoint round plus its block rounding would add minutes.
//!
//! 1. `request_venue_state` finds no fresh state and no running round. It needs a configured
//!    [`VenueMarket`] and starts a round at once.
//! 2. Starting a round snapshots the market into the round and submits an internal watchtower
//!    proposal of type `VenueOracle` with `VotingKind::Choice`.
//! 3. When the proposal activates, the round gets its target timestamp: the parachain time minus
//!    `VenueTargetLag`. Setting it at activation keeps time spent in the queue from aging the
//!    state.
//! 4. Watchtower nodes fetch `venue/exotic/{chain_id}/{target_ts}/{clearing_house}/{amm}` from
//!    their local external service, which reads at the latest venue block whose timestamp is at
//!    most `target_ts`. Every honest node resolves the same block, so their observations match
//!    without voting on a block first. They submit `submit_venue_report`; the report is the vote,
//!    exactly as on the floor channel.
//! 5. A passed proposal stores the winning observation as `LastFinalisedVenue`; anything else fails
//!    the round.
//!
//! Observations carry raw contract return values only. Nothing here interprets them.

use crate::{ocw::OcwChannel, *};
use codec::DecodeWithMemTracking;
use pallet_watchtower::NodesInterface;
use sp_avn_common::http_data_codec::decode_from_http_data;
use sp_runtime::SaturatedConversion;
use sp_watchtower::{
    DecisionRule, ProposalRequest, ProposalSource, ProposalType, RawPayload, VoteChoice,
    VotingKind, WatchtowerInterface,
};

const VENUE_OCW: OcwChannel = OcwChannel {
    name: "Venue oracle",
    id: b"venue_oracle::ocw",
    db_prefix: b"venue_oracle::ocw::",
};
const VENUE_ROUND_REF_CONTEXT: &[u8] = b"wt_venue_oracle_round";
const VENUE_PROPOSAL_TITLE: &[u8] = b"Punks venue oracle round";

/// A venue round collecting reports.
#[derive(
    Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub struct VenueRound<BlockNumber> {
    pub round_id: u64,
    /// The watchtower proposal deciding this round.
    pub proposal_id: ProposalId,
    /// Watchtowers read the venue chain at the latest block whose timestamp is at most this, in
    /// unix seconds. `None` until the proposal activates.
    pub target_external_block_time_sec: Option<u64>,
    /// The market as configured when the round started. Reports are validated against this
    /// snapshot, so a later `set_venue_market` only affects the next round.
    pub market: VenueMarket,
    pub started_at: BlockNumber,
    /// Reports received so far.
    pub reports: u32,
}

impl<T: Config> Pallet<T> {
    // ------------------------------------------------------------------ requests

    pub fn do_request_venue_state() -> DispatchResult {
        if let Some(round) = ActiveVenueRound::<T>::get() {
            Self::deposit_event(Event::<T>::VenueRoundInProgress { round_id: round.round_id });
            return Ok(())
        }

        if let Some(venue) = LastFinalisedVenue::<T>::get() {
            if Self::now_secs().saturating_sub(venue.observation.external_block_time_sec) <=
                T::MaxVenueAge::get()
            {
                Self::deposit_event(Event::<T>::VenueStateServed {
                    round_id: venue.round_id,
                    external_block: venue.observation.external_block,
                });
                return Ok(())
            }
        }

        let market = VenueMarketConfig::<T>::get().ok_or(Error::<T>::VenueMarketNotSet)?;
        Self::start_venue_round(market)
    }

    fn start_venue_round(market: VenueMarket) -> DispatchResult {
        let round_id = LastVenueRoundId::<T>::mutate(|id| {
            *id = id.saturating_add(1);
            *id
        });
        let now_block = <frame_system::Pallet<T>>::block_number();
        let external_ref = Self::venue_round_external_ref(round_id);

        let request = ProposalRequest {
            title: VENUE_PROPOSAL_TITLE.to_vec(),
            payload: RawPayload::Inline((round_id, market).encode()),
            threshold: T::OracleRoundThreshold::get(),
            source: ProposalSource::Internal(ProposalType::VenueOracle),
            decision_rule: DecisionRule::RejectOnExpiry,
            external_ref,
            created_at: now_block.saturated_into::<u32>(),
            vote_duration: Some(T::OracleVoteDuration::get()),
            committee_size: OracleCommitteeSize::<T>::get(),
            voting: VotingKind::Choice,
        };
        <pallet_watchtower::Pallet<T> as WatchtowerInterface>::submit_proposal(None, request)?;
        let proposal_id = pallet_watchtower::Pallet::<T>::id_by_external_ref(external_ref);

        ActiveVenueRound::<T>::put(VenueRound {
            round_id,
            proposal_id,
            target_external_block_time_sec: None,
            market,
            started_at: now_block,
            reports: 0,
        });
        Self::deposit_event(Event::<T>::VenueRoundStarted { round_id, proposal_id, market });
        Ok(())
    }

    /// Sets the running round's target timestamp when its proposal activates. Other proposals
    /// are ignored.
    /// Runs on every proposal activation, so it only writes when the proposal is the venue one.
    pub(crate) fn on_venue_proposal_activated(proposal_id: ProposalId) {
        let Some(mut round) = ActiveVenueRound::<T>::get() else { return };
        if round.proposal_id != proposal_id {
            return
        }
        let target_ts = Self::now_secs().saturating_sub(T::VenueTargetLag::get());
        round.target_external_block_time_sec = Some(target_ts);
        let round_id = round.round_id;
        ActiveVenueRound::<T>::put(round);
        Self::deposit_event(Event::<T>::VenueRoundActivated {
            round_id,
            target_external_block_time_sec: target_ts,
        });
    }

    /// True when `observation` was taken at the latest venue block at or before `target_ts`, as
    /// far as the pallet can tell: its timestamp is at most the target and within the market's
    /// `target_tolerance_secs` of it. Agreement on the exact block comes from matching reports.
    pub fn venue_observation_on_target(
        observation: &VenueObservation,
        target_ts: u64,
        market: &VenueMarket,
    ) -> bool {
        observation.external_block_time_sec <= target_ts &&
            target_ts - observation.external_block_time_sec < market.target_tolerance_secs
    }

    pub fn venue_round_external_ref(round_id: u64) -> H256 {
        H256(sp_io::hashing::blake2_256(&(VENUE_ROUND_REF_CONTEXT, round_id).encode()))
    }

    // ------------------------------------------------------------------ reports

    /// Bytes a watchtower signs for a venue report. The round's market is included because the
    /// observation does not name the contracts it was read from.
    pub fn venue_report_signing_payload(
        round_id: u64,
        market: &VenueMarket,
        observation: &VenueObservation,
        country: &CountryCode,
        watchtower: &T::AccountId,
    ) -> Vec<u8> {
        (
            VENUE_ORACLE_REPORT_CONTEXT,
            Self::genesis_hash(),
            round_id,
            market,
            observation,
            country,
            watchtower,
        )
            .encode()
    }

    /// Everything a venue report must satisfy, including the signature. Used by the pool.
    pub fn validate_venue_report(
        round_id: u64,
        observation: &VenueObservation,
        country: &CountryCode,
        watchtower: &T::AccountId,
        signature: &<T::SignerId as RuntimeAppPublic>::Signature,
    ) -> DispatchResult {
        let round = Self::ensure_venue_report_accepted(round_id, observation, watchtower)?;

        let key = T::Watchtowers::get_node_signing_key(watchtower)
            .ok_or(Error::<T>::VenueReportBadSignature)?;
        // Checked against the bytes the offchain worker signs, so the two cannot drift apart.
        let payload = Self::venue_report_signing_payload(
            round_id,
            &round.market,
            observation,
            country,
            watchtower,
        );
        ensure!(key.verify(&payload, signature), Error::<T>::VenueReportBadSignature);
        Ok(())
    }

    fn ensure_venue_report_accepted(
        round_id: u64,
        observation: &VenueObservation,
        watchtower: &T::AccountId,
    ) -> Result<VenueRound<BlockNumberFor<T>>, DispatchError> {
        let round = ActiveVenueRound::<T>::get().ok_or(Error::<T>::NoActiveVenueRound)?;
        ensure!(round.round_id == round_id, Error::<T>::VenueRoundMismatch);
        ensure!(Self::oracle_proposal_open(round.proposal_id), Error::<T>::VenueProposalNotActive);
        ensure!(
            Self::is_oracle_committee_member(round.proposal_id, watchtower),
            Error::<T>::NotInVenueCommittee
        );
        ensure!(
            !VenueReports::<T>::contains_key(round_id, watchtower),
            Error::<T>::DuplicateVenueReport
        );
        // Set when the proposal activates, which the check above guarantees.
        let target_ts =
            round.target_external_block_time_sec.ok_or(Error::<T>::VenueProposalNotActive)?;
        ensure!(observation.is_plausible(), Error::<T>::ImplausibleVenueObservation);
        ensure!(
            Self::venue_observation_on_target(observation, target_ts, &round.market),
            Error::<T>::VenueObservationOffTarget
        );
        Ok(round)
    }

    /// Stores the report and counts it as the watchtower's answer on the proposal. All round
    /// state is written before the vote is cast, since the vote may finalise the proposal and
    /// its hook clears the round.
    pub(crate) fn do_submit_venue_report(
        round_id: u64,
        observation: VenueObservation,
        country: CountryCode,
        watchtower: T::AccountId,
    ) -> DispatchResult {
        let mut round = Self::ensure_venue_report_accepted(round_id, &observation, &watchtower)?;
        let hash = observation.hash();

        VenueReports::<T>::insert(round_id, &watchtower, hash);
        if !VenueObservations::<T>::contains_key(round_id, hash) {
            VenueObservations::<T>::insert(round_id, hash, observation);
        }
        round.reports = round.reports.saturating_add(1);
        let proposal_id = round.proposal_id;
        ActiveVenueRound::<T>::put(round);
        Self::deposit_event(Event::<T>::VenueReportSubmitted {
            round_id,
            watchtower: watchtower.clone(),
            hash,
            country: sanitise_country(country),
        });

        pallet_watchtower::Pallet::<T>::vote_choice_for(&watchtower, proposal_id, hash, country)?;
        Ok(())
    }

    // ------------------------------------------------------------------ finalisation

    fn finish_venue_round(round: VenueRound<BlockNumberFor<T>>, result: &ProposalStatusEnum) {
        let winner =
            pallet_watchtower::Pallet::<T>::winning_choice(round.proposal_id).and_then(|hash| {
                VenueObservations::<T>::get(round.round_id, hash).map(|obs| (hash, obs))
            });

        match (result, winner) {
            (ProposalStatusEnum::Resolved { passed: true }, Some((hash, observation))) => {
                let eligible = pallet_watchtower::Pallet::<T>::eligible_voters(round.proposal_id);
                let required = T::OracleRoundThreshold::get().mul_ceil(eligible);
                let agreeing =
                    pallet_watchtower::Pallet::<T>::choice_tally(round.proposal_id, hash);
                LastFinalisedVenue::<T>::put(FinalisedVenueRound {
                    round_id: round.round_id,
                    proposal_id: round.proposal_id,
                    market: round.market,
                    observation,
                    agreeing,
                    reports: round.reports,
                    required,
                    finalised_at_block: <frame_system::Pallet<T>>::block_number()
                        .saturated_into::<u32>(),
                });
                Self::deposit_event(Event::<T>::VenueRoundFinalised {
                    round_id: round.round_id,
                    external_block: observation.external_block,
                    external_block_time_sec: observation.external_block_time_sec,
                    spot_e18: observation.spot_e18,
                    oracle_e18: observation.oracle_e18,
                    oi_notional_e18: observation.oi_notional_e18,
                    agreeing,
                    reports: round.reports,
                    required,
                });
            },
            _ => {
                Self::deposit_event(Event::<T>::VenueRoundFailed {
                    round_id: round.round_id,
                    result: result.clone(),
                });
            },
        }

        ActiveVenueRound::<T>::kill();
        // The round's reports and observations are deleted later by `on_idle`, so ending a
        // round costs the same however many watchtowers reported.
        if round.reports > 0 {
            RoundsToClean::<T>::insert((OracleChannel::Venue, round.round_id), ());
        }
    }

    // ------------------------------------------------------------------ offchain worker

    /// Reports once per round. Only nodes holding a registered watchtower signing key take
    /// part; collators have none and return early.
    pub fn venue_offchain_worker(now: BlockNumberFor<T>) {
        let round = match ActiveVenueRound::<T>::get() {
            Some(round) => round,
            None => return,
        };
        if !Self::oracle_proposal_open(round.proposal_id) {
            return
        }
        let Some(target_ts) = round.target_external_block_time_sec else { return };
        if Self::ocw_already_run(&VENUE_OCW, now).is_err() {
            return
        }

        let (watchtower, signing_key) = match T::Watchtowers::get_node_from_local_signing_keys() {
            Some(info) => info,
            None => return,
        };
        if !pallet_watchtower::Pallet::<T>::is_committee_member(round.proposal_id, &watchtower) {
            return
        }
        if VenueReports::<T>::contains_key(round.round_id, &watchtower) {
            return
        }

        let key = (b"report", round.round_id, &watchtower).encode();
        if Self::submission_in_progress(&VENUE_OCW, &key, now) {
            return
        }

        let observation = match Self::fetch_venue_observation(&round.market, target_ts) {
            Ok(Some(observation)) => observation,
            // Normal on a round's first attempts: the venue chain has not passed the target.
            Ok(None) => {
                log::debug!("Venue oracle: venue chain not past target {} yet", target_ts);
                return
            },
            Err(e) => {
                log::error!("💔 Venue oracle: cannot fetch observation: {:?}", e);
                return
            },
        };
        if !observation.is_plausible() ||
            !Self::venue_observation_on_target(&observation, target_ts, &round.market)
        {
            log::error!(
                "💔 Venue oracle: service returned an unusable observation {:?}",
                observation
            );
            return
        }

        let country = pallet_watchtower::fetch_node_country::<T>();
        let payload = Self::venue_report_signing_payload(
            round.round_id,
            &round.market,
            &observation,
            &country,
            &watchtower,
        );
        let signature = match signing_key.sign(&payload) {
            Some(signature) => signature,
            None => {
                log::error!("💔 Venue oracle: cannot sign report");
                return
            },
        };
        let call = <T as CreateBare<Call<T>>>::create_bare(
            Call::<T>::submit_venue_report {
                round_id: round.round_id,
                observation,
                country,
                watchtower: watchtower.clone(),
                signature,
            }
            .into(),
        );
        match SubmitTransaction::<T, Call<T>>::submit_transaction(call) {
            Ok(()) => Self::record_submission(&VENUE_OCW, &key, now),
            Err(e) => log::error!("💔 Venue oracle: cannot submit report: {:?}", e),
        }
    }

    /// Path of the service request for `market` at `target_ts`:
    /// `venue/exotic/{chain_id}/{target_ts}/{clearing_house}/{amm}`, addresses as 40
    /// lower-case hex characters without `0x`. Shared with the tests so they cannot drift.
    pub fn venue_service_path(market: &VenueMarket, target_ts: u64) -> String {
        format!(
            "{}/{}/{}/{:x}/{:x}",
            VENUE_SERVICE_PATH, market.chain_id, target_ts, market.clearing_house, market.amm
        )
    }

    /// `GET` of [`Pallet::venue_service_path`] on the local external service. The body is the
    /// http-data encoding of an `Option<VenueObservation>`; `None` means the venue chain has no
    /// block after the target yet.
    pub fn fetch_venue_observation(
        market: &VenueMarket,
        target_ts: u64,
    ) -> Result<Option<VenueObservation>, DispatchError> {
        let body = AVN::<T>::get_data_from_service(Self::venue_service_path(market, target_ts))?;
        let text = core::str::from_utf8(&body)
            .map_err(|_| DispatchError::Other("venue observation is not utf8"))?;
        decode_from_http_data::<Option<VenueObservation>>(text)
            .map_err(|_| DispatchError::Other("venue observation does not decode"))
    }

    // ------------------------------------------------------------------ watchtower traits

    /// The venue channel's part of `VoteFilter`: `None` when `proposal_id` is not the active
    /// venue proposal. On it, a vote must be the hash the voter reported; yes/no votes are
    /// refused.
    pub(crate) fn venue_vote_filter(
        proposal_id: ProposalId,
        voter: &T::AccountId,
        vote: &VoteChoice,
    ) -> Option<DispatchResult> {
        let round = match ActiveVenueRound::<T>::get() {
            Some(round) if round.proposal_id == proposal_id => round,
            _ => return None,
        };
        Some((|| {
            ensure!(
                Self::is_oracle_committee_member(proposal_id, voter),
                Error::<T>::NotInVenueCommittee
            );
            let report =
                VenueReports::<T>::get(round.round_id, voter).ok_or(Error::<T>::NoVenueReport)?;
            match vote {
                VoteChoice::Binary(_) => Err(Error::<T>::VenueVotesAreReports.into()),
                VoteChoice::Choice(hash) => {
                    ensure!(*hash == report, Error::<T>::VenueVoteContradictsReport);
                    Ok(())
                },
            }
        })())
    }

    /// Settles the venue round decided by `proposal_id`, if there is one. Returns whether the
    /// proposal belonged to this channel.
    pub(crate) fn finish_venue_if_matches(
        proposal_id: ProposalId,
        result: &ProposalStatusEnum,
    ) -> bool {
        match ActiveVenueRound::<T>::get() {
            Some(round) if round.proposal_id == proposal_id => {
                Self::finish_venue_round(round, result);
                true
            },
            _ => false,
        }
    }
}
