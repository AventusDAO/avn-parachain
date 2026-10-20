// Copyright 2026 Aventus DAO Ltd

//! Floor rounds: pin a checkpoint, open a watchtower `Choice` proposal, and let every
//! committee member report what it observed. The report is the vote.
//!
//! Life of a round:
//! 1. `request_floor_price` finds no fresh floor and no running round. It needs a configured
//!    [`FloorMarket`] and asks the checkpoint side for that market's chain. With a fresh checkpoint
//!    the round starts at once; otherwise the request is parked in `PendingFloorRequest` and the
//!    round starts when the checkpoint round settles.
//! 2. Starting a round submits an internal watchtower proposal of type `FloorOracle` with
//!    `VotingKind::Choice`. The proposal queues and the collator OCW activates it, which fixes the
//!    committee.
//! 3. Watchtower nodes (their OCW, `floor_offchain_worker`) fetch
//!    `floor/punks/{chain_id}/{pinned_block}/{punks}` from their local external service and submit
//!    `submit_floor_report`. The pallet stores the observation under its hash and counts the hash
//!    as the node's answer through `pallet_watchtower::vote_choice_for`. The first answer reaching
//!    the proposal's threshold wins; no single reporter can set a "candidate".
//! 4. The watchtower pallet decides the proposal and calls `on_voting_completed`. A passed proposal
//!    stores the winning observation as `LastFinalisedFloor`; anything else fails the round. Both
//!    clear the round, so the next request can open a new one.
//!
//! The pallet's `VoteFilter` keeps the two paths consistent: a watchtower vote on a floor
//! proposal must be the hash it reported, and yes/no votes are refused outright.

use crate::{ocw::OcwChannel, *};
use codec::DecodeWithMemTracking;
use pallet_watchtower::NodesInterface;
use sp_avn_common::http_data_codec::decode_from_http_data;
use sp_runtime::SaturatedConversion;
use sp_watchtower::{
    DecisionRule, ProposalRequest, ProposalSource, ProposalType, RawPayload, VoteChoice,
    VotingKind, WatchtowerInterface,
};

const FLOOR_OCW: OcwChannel = OcwChannel {
    name: "Floor oracle",
    id: b"floor_oracle::ocw",
    db_prefix: b"floor_oracle::ocw::",
};
const FLOOR_ROUND_REF_CONTEXT: &[u8] = b"wt_floor_oracle_round";
const FLOOR_PROPOSAL_TITLE: &[u8] = b"Punks floor oracle round";

/// A floor round collecting reports.
#[derive(
    Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub struct FloorRound<BlockNumber> {
    pub round_id: u64,
    /// The watchtower proposal deciding this round.
    pub proposal_id: ProposalId,
    /// Parachain time, in seconds, every watchtower observes the floor at.
    pub pinned_ts: u64,
    /// External chain block every watchtower reads on-chain data at.
    pub pinned_block: u32,
    /// The market as configured when the round started. Reports are validated against this
    /// snapshot, so a later `set_floor_market` only affects the next round.
    pub market: FloorMarket,
    pub started_at: BlockNumber,
    /// Reports received so far.
    pub reports: u32,
}

impl<T: Config> Pallet<T> {
    // ------------------------------------------------------------------ requests

    pub fn do_request_floor_price() -> DispatchResult {
        Self::request_floor(T::MaxFloorAge::get(), T::MaxFloorCheckpointAge::get()).map(|_| ())
    }

    /// As [`Pallet::do_request_floor_price`] with `max_age_secs`, raised to `MinForcedFloorAge`
    /// and capped at `MaxFloorAge`, as the oldest floor that answers. A new round also pins a
    /// checkpoint no older than that: the suspect floor usually pins the current checkpoint, and
    /// a round on the same block would read the same floor again.
    pub fn do_force_floor_round(max_age_secs: u64) -> DispatchResult {
        let max_age = max_age_secs.max(T::MinForcedFloorAge::get()).min(T::MaxFloorAge::get());
        let max_checkpoint_age = max_age.min(T::MaxFloorCheckpointAge::get());
        if let Some(round_id) = Self::request_floor(max_age, max_checkpoint_age)? {
            Self::deposit_event(Event::<T>::FloorRoundForced {
                round_id,
                requested_max_age: max_age,
            });
        }
        Ok(())
    }

    /// Joins the running round, answers from a floor at most `max_age` old, or starts a round
    /// pinned to a checkpoint at most `max_checkpoint_age` old, waiting for one if needed.
    /// Returns the id of the round it started, if any.
    fn request_floor(max_age: u64, max_checkpoint_age: u64) -> Result<Option<u64>, DispatchError> {
        if let Some(round) = ActiveFloorRound::<T>::get() {
            Self::deposit_event(Event::<T>::FloorRoundInProgress { round_id: round.round_id });
            return Ok(None)
        }

        if let Some(floor) = LastFinalisedFloor::<T>::get() {
            if Self::now_secs().saturating_sub(floor.pinned_ts) <= max_age {
                Self::deposit_event(Event::<T>::FloorPriceServed {
                    round_id: floor.round_id,
                    floor_wei: floor.observation.floor_wei,
                });
                return Ok(None)
            }
        }

        let market = FloorMarketConfig::<T>::get().ok_or(Error::<T>::FloorMarketNotSet)?;
        // A floor is as old as the checkpoint it pins, so pin only a young one; otherwise the
        // new floor could be past `MaxFloorAge` by the time it finalises.
        match Self::do_request_checkpoint_within(market.chain_id, Some(max_checkpoint_age))? {
            CheckpointRequest::Ready(checkpoint) =>
                Self::start_floor_round(&checkpoint, market).map(Some),
            CheckpointRequest::Pending { round_id, .. } => {
                PendingFloorRequest::<T>::put(true);
                Self::deposit_event(Event::<T>::FloorRequestWaitingForCheckpoint {
                    checkpoint_round_id: round_id,
                });
                Ok(None)
            },
        }
    }

    /// Called by the checkpoint side when a round on `chain_id` settles.
    pub(crate) fn on_checkpoint_agreed_floor(chain_id: ExternalChainId, checkpoint: &Checkpoint) {
        let market = match FloorMarketConfig::<T>::get() {
            Some(market) => market,
            None => return,
        };
        if chain_id != market.chain_id || !PendingFloorRequest::<T>::take() {
            return
        }
        if ActiveFloorRound::<T>::exists() {
            return
        }
        if let Err(error) = Self::start_floor_round(checkpoint, market) {
            Self::deposit_event(Event::<T>::FloorRoundStartFailed { error });
        }
    }

    fn start_floor_round(
        checkpoint: &Checkpoint,
        market: FloorMarket,
    ) -> Result<u64, DispatchError> {
        let round_id = LastFloorRoundId::<T>::mutate(|id| {
            *id = id.saturating_add(1);
            *id
        });
        let now_block = <frame_system::Pallet<T>>::block_number();
        let external_ref = Self::floor_round_external_ref(round_id);

        let request = ProposalRequest {
            title: FLOOR_PROPOSAL_TITLE.to_vec(),
            payload: RawPayload::Inline(
                (round_id, checkpoint.agreed_at, checkpoint.block_number, market).encode(),
            ),
            threshold: T::OracleRoundThreshold::get(),
            source: ProposalSource::Internal(ProposalType::FloorOracle),
            decision_rule: DecisionRule::RejectOnExpiry,
            external_ref,
            created_at: now_block.saturated_into::<u32>(),
            vote_duration: Some(T::OracleVoteDuration::get()),
            committee_size: OracleCommitteeSize::<T>::get(),
            voting: VotingKind::Choice,
        };
        <pallet_watchtower::Pallet<T> as WatchtowerInterface>::submit_proposal(None, request)?;
        let proposal_id = pallet_watchtower::Pallet::<T>::id_by_external_ref(external_ref);

        ActiveFloorRound::<T>::put(FloorRound {
            round_id,
            proposal_id,
            pinned_ts: checkpoint.agreed_at,
            pinned_block: checkpoint.block_number,
            market,
            started_at: now_block,
            reports: 0,
        });
        Self::deposit_event(Event::<T>::FloorRoundStarted {
            round_id,
            proposal_id,
            pinned_ts: checkpoint.agreed_at,
            pinned_block: checkpoint.block_number,
            market,
        });
        Ok(round_id)
    }

    pub fn floor_round_external_ref(round_id: u64) -> H256 {
        H256(sp_io::hashing::blake2_256(&(FLOOR_ROUND_REF_CONTEXT, round_id).encode()))
    }

    // ------------------------------------------------------------------ reports

    /// Bytes a watchtower signs for a report.
    pub fn report_signing_payload(
        round_id: u64,
        observation: &FloorObservation,
        country: &CountryCode,
        watchtower: &T::AccountId,
    ) -> Vec<u8> {
        (FLOOR_ORACLE_REPORT_CONTEXT, round_id, observation, country, watchtower).encode()
    }

    /// Everything a report must satisfy, including the signature. Used by the pool.
    pub fn validate_floor_report(
        round_id: u64,
        observation: &FloorObservation,
        country: &CountryCode,
        watchtower: &T::AccountId,
        signature: &<T::SignerId as RuntimeAppPublic>::Signature,
    ) -> DispatchResult {
        Self::ensure_report_accepted(round_id, observation, watchtower)?;

        let key = T::Watchtowers::get_node_signing_key(watchtower)
            .ok_or(Error::<T>::FloorReportBadSignature)?;
        // Checked against the bytes the offchain worker signs, so the two cannot drift apart.
        let payload = Self::report_signing_payload(round_id, observation, country, watchtower);
        ensure!(key.verify(&payload, signature), Error::<T>::FloorReportBadSignature);
        Ok(())
    }

    fn ensure_report_accepted(
        round_id: u64,
        observation: &FloorObservation,
        watchtower: &T::AccountId,
    ) -> Result<FloorRound<BlockNumberFor<T>>, DispatchError> {
        let round = ActiveFloorRound::<T>::get().ok_or(Error::<T>::NoActiveFloorRound)?;
        ensure!(round.round_id == round_id, Error::<T>::FloorRoundMismatch);
        ensure!(Self::oracle_proposal_open(round.proposal_id), Error::<T>::FloorProposalNotActive);
        ensure!(
            Self::is_oracle_committee_member(round.proposal_id, watchtower),
            Error::<T>::NotInFloorCommittee
        );
        ensure!(
            !FloorReports::<T>::contains_key(round_id, watchtower),
            Error::<T>::DuplicateFloorReport
        );
        ensure!(observation.is_plausible(), Error::<T>::ImplausibleObservation);
        ensure!(
            observation.observed_eth_block == round.pinned_block as u64,
            Error::<T>::ObservationBlockMismatch
        );
        Ok(round)
    }

    /// An authorized watchtower that sits in the proposal's committee. Shared by both channels.
    pub(crate) fn is_oracle_committee_member(proposal_id: ProposalId, who: &T::AccountId) -> bool {
        T::Watchtowers::is_authorized_watchtower(who) &&
            pallet_watchtower::Pallet::<T>::is_committee_member(proposal_id, who)
    }

    /// Stores the report and counts it as the watchtower's answer on the proposal. The vote may
    /// finalise the proposal, whose hook re-enters this pallet and clears the round, so all
    /// round state is written before the vote is cast.
    pub(crate) fn do_submit_floor_report(
        round_id: u64,
        observation: FloorObservation,
        country: CountryCode,
        watchtower: T::AccountId,
    ) -> DispatchResult {
        let mut round = Self::ensure_report_accepted(round_id, &observation, &watchtower)?;
        let hash = observation.hash();

        FloorReports::<T>::insert(round_id, &watchtower, hash);
        if !FloorObservations::<T>::contains_key(round_id, hash) {
            FloorObservations::<T>::insert(round_id, hash, observation);
        }
        round.reports = round.reports.saturating_add(1);
        let proposal_id = round.proposal_id;
        ActiveFloorRound::<T>::put(round);
        Self::deposit_event(Event::<T>::FloorReportSubmitted {
            round_id,
            watchtower: watchtower.clone(),
            hash,
            country: sanitise_country(country),
        });

        pallet_watchtower::Pallet::<T>::vote_choice_for(&watchtower, proposal_id, hash, country)?;
        Ok(())
    }

    // ------------------------------------------------------------------ finalisation

    fn finish_round(round: FloorRound<BlockNumberFor<T>>, result: &ProposalStatusEnum) {
        let winner =
            pallet_watchtower::Pallet::<T>::winning_choice(round.proposal_id).and_then(|hash| {
                FloorObservations::<T>::get(round.round_id, hash).map(|obs| (hash, obs))
            });

        match (result, winner) {
            (ProposalStatusEnum::Resolved { passed: true }, Some((hash, observation))) => {
                let eligible = pallet_watchtower::Pallet::<T>::eligible_voters(round.proposal_id);
                let required = T::OracleRoundThreshold::get().mul_ceil(eligible);
                let agreeing =
                    pallet_watchtower::Pallet::<T>::choice_tally(round.proposal_id, hash);
                LastFinalisedFloor::<T>::put(FinalisedFloorRound {
                    round_id: round.round_id,
                    proposal_id: round.proposal_id,
                    pinned_ts: round.pinned_ts,
                    market: round.market,
                    observation,
                    agreeing,
                    reports: round.reports,
                    required,
                    finalised_at_block: <frame_system::Pallet<T>>::block_number()
                        .saturated_into::<u32>(),
                });
                Self::deposit_event(Event::<T>::FloorRoundFinalised {
                    round_id: round.round_id,
                    floor_wei: observation.floor_wei,
                    agreeing,
                    reports: round.reports,
                    required,
                });
            },
            _ => {
                Self::deposit_event(Event::<T>::FloorRoundFailed {
                    round_id: round.round_id,
                    result: result.clone(),
                });
            },
        }

        ActiveFloorRound::<T>::kill();
        // `reports` counts the rows this round wrote, and there is at most one observation per
        // report, so this removes every row of the round however many nodes are registered now.
        let _ = FloorReports::<T>::clear_prefix(round.round_id, round.reports, None);
        let _ = FloorObservations::<T>::clear_prefix(round.round_id, round.reports, None);
    }

    // ------------------------------------------------------------------ offchain worker

    /// Reports once per round. Only nodes holding a registered watchtower signing key take
    /// part; collators have none and return early.
    pub fn floor_offchain_worker(now: BlockNumberFor<T>) {
        let round = match ActiveFloorRound::<T>::get() {
            Some(round) => round,
            None => return,
        };
        if !Self::oracle_proposal_open(round.proposal_id) {
            return
        }
        if Self::ocw_already_run(&FLOOR_OCW, now).is_err() {
            return
        }

        let (watchtower, signing_key) = match T::Watchtowers::get_node_from_local_signing_keys() {
            Some(info) => info,
            None => return,
        };
        if !pallet_watchtower::Pallet::<T>::is_committee_member(round.proposal_id, &watchtower) {
            return
        }
        if FloorReports::<T>::contains_key(round.round_id, &watchtower) {
            return
        }

        let key = (b"report", round.round_id, &watchtower).encode();
        if Self::submission_in_progress(&FLOOR_OCW, &key, now) {
            return
        }

        let observation = match Self::fetch_observation(&round.market, round.pinned_block) {
            Ok(observation) => observation,
            Err(e) => {
                log::error!("💔 Floor oracle: cannot fetch observation: {:?}", e);
                return
            },
        };
        if !observation.is_plausible() ||
            observation.observed_eth_block != round.pinned_block as u64
        {
            log::error!(
                "💔 Floor oracle: service returned an unusable observation {:?}",
                observation
            );
            return
        }

        let country = pallet_watchtower::fetch_node_country::<T>();
        let payload =
            Self::report_signing_payload(round.round_id, &observation, &country, &watchtower);
        let signature = match signing_key.sign(&payload) {
            Some(signature) => signature,
            None => {
                log::error!("💔 Floor oracle: cannot sign report");
                return
            },
        };
        let call = <T as CreateBare<Call<T>>>::create_bare(
            Call::<T>::submit_floor_report {
                round_id: round.round_id,
                observation,
                country,
                watchtower: watchtower.clone(),
                signature,
            }
            .into(),
        );
        match SubmitTransaction::<T, Call<T>>::submit_transaction(call) {
            Ok(()) => Self::record_submission(&FLOOR_OCW, &key, now),
            Err(e) => log::error!("💔 Floor oracle: cannot submit report: {:?}", e),
        }
    }

    /// Path of the service request for `market` at `pinned_block`:
    /// `floor/punks/{chain_id}/{pinned_block}/{punks}`, the address as 40 lower-case hex
    /// characters without `0x`. Shared with the tests so they cannot drift.
    pub fn floor_service_path(market: &FloorMarket, pinned_block: u32) -> String {
        format!("{}/{}/{}/{:x}", FLOOR_SERVICE_PATH, market.chain_id, pinned_block, market.punks)
    }

    /// `GET` of [`Pallet::floor_service_path`] on the local external service. The body is the
    /// http-data encoding of a `FloorObservation`.
    pub fn fetch_observation(
        market: &FloorMarket,
        pinned_block: u32,
    ) -> Result<FloorObservation, DispatchError> {
        let body = AVN::<T>::get_data_from_service(Self::floor_service_path(market, pinned_block))?;
        let text = core::str::from_utf8(&body)
            .map_err(|_| DispatchError::Other("floor observation is not utf8"))?;
        decode_from_http_data::<FloorObservation>(text)
            .map_err(|_| DispatchError::Other("floor observation does not decode"))
    }
}

// ---------------------------------------------------------------------- watchtower traits

impl<T: Config> Pallet<T> {
    /// The floor channel's part of `VoteFilter`: `None` when `proposal_id` is not the active
    /// floor proposal. On it, a vote must be the hash the voter reported; yes/no votes are
    /// refused.
    pub(crate) fn floor_vote_filter(
        proposal_id: ProposalId,
        voter: &T::AccountId,
        vote: &VoteChoice,
    ) -> Option<DispatchResult> {
        let round = match ActiveFloorRound::<T>::get() {
            Some(round) if round.proposal_id == proposal_id => round,
            _ => return None,
        };
        Some((|| {
            ensure!(
                Self::is_oracle_committee_member(proposal_id, voter),
                Error::<T>::NotInFloorCommittee
            );
            let report =
                FloorReports::<T>::get(round.round_id, voter).ok_or(Error::<T>::NoFloorReport)?;
            match vote {
                VoteChoice::Binary(_) => Err(Error::<T>::FloorVotesAreReports.into()),
                VoteChoice::Choice(hash) => {
                    ensure!(*hash == report, Error::<T>::VoteContradictsReport);
                    Ok(())
                },
            }
        })())
    }

    /// Settles the floor round decided by `proposal_id`, if there is one. Returns whether the
    /// proposal belonged to this channel.
    pub(crate) fn finish_floor_if_matches(
        proposal_id: ProposalId,
        result: &ProposalStatusEnum,
    ) -> bool {
        match ActiveFloorRound::<T>::get() {
            Some(round) if round.proposal_id == proposal_id => {
                Self::finish_round(round, result);
                true
            },
            _ => false,
        }
    }
}
