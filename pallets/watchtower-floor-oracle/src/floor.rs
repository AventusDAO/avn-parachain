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

/// Why a floor round runs.
#[derive(
    Encode,
    Decode,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
pub enum FloorRoundKind {
    /// `request_floor_price` found no fresh floor.
    Requested,
    /// `force_floor_round` asked for a new floor, whatever the cache holds.
    Forced,
}

/// A floor round collecting reports.
#[derive(
    Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub struct FloorRound<BlockNumber> {
    pub round_id: u64,
    /// The watchtower proposal deciding this round.
    pub proposal_id: ProposalId,
    /// Unix time, in seconds, of the checkpoint every watchtower observes the floor at.
    pub checkpoint_agreed_at_sec: u64,
    /// External chain block of the checkpoint; every watchtower reads on-chain data at it.
    pub external_block: u32,
    /// The market as configured when the round started. Reports are validated against this
    /// snapshot, so a later `set_floor_market` only affects the next round.
    pub market: FloorMarket,
    pub started_at: BlockNumber,
    /// Reports received so far.
    pub reports: u32,
    /// A forced round starts the `MinForcedFloorAge` cooldown when it ends.
    pub kind: FloorRoundKind,
}

impl<T: Config> Pallet<T> {
    // ------------------------------------------------------------------ requests

    pub fn do_request_floor_price() -> DispatchResult {
        Self::request_floor(FloorRoundKind::Requested)
    }

    /// Starts a forced round, unless one is running. Within `MinForcedFloorAge` of the last
    /// forced round ending, finalised or failed, or of a force answered with the last floor
    /// because Ethereum had not moved past it, it behaves as [`Pallet::do_request_floor_price`]
    /// instead, so the requester cannot open forced rounds back to back.
    pub fn do_force_floor_round() -> DispatchResult {
        let cooling_down = LastForcedRoundEndedAt::<T>::get().map_or(false, |ended_at| {
            Self::now_secs().saturating_sub(ended_at) <= T::MinForcedFloorAge::get()
        });
        let kind = if cooling_down { FloorRoundKind::Requested } else { FloorRoundKind::Forced };
        Self::request_floor(kind)
    }

    /// Joins the running round, answers a `Requested` request from a floor within `MaxFloorAge`,
    /// or starts a round of `kind`, waiting for a checkpoint first if needed.
    fn request_floor(kind: FloorRoundKind) -> DispatchResult {
        if let Some(round) = ActiveFloorRound::<T>::get() {
            Self::deposit_event(Event::<T>::FloorRoundInProgress { round_id: round.round_id });
            return Ok(())
        }

        if kind == FloorRoundKind::Requested {
            if let Some(floor) = LastFinalisedFloor::<T>::get() {
                // Measured from the Ethereum block the floor was read at, not from when the
                // parachain agreed on it: the block was already confirmations deep by then.
                let age =
                    Self::now_secs().saturating_sub(floor.observation.external_block_time_sec);
                if age <= T::MaxFloorAge::get() {
                    Self::deposit_event(Event::<T>::FloorPriceServed {
                        round_id: floor.round_id,
                        floor_wei: floor.observation.floor_wei,
                    });
                    return Ok(())
                }
            }
        }

        let market = FloorMarketConfig::<T>::get().ok_or(Error::<T>::FloorMarketNotSet)?;
        // A floor is at least as old as the checkpoint it pins, so pin only a young one;
        // otherwise the new floor could be past `MaxFloorAge` by the time it finalises. A forced
        // round pins an even younger one, past the block the last floor was read at: a round on
        // that block would read the same floor again.
        let (max_checkpoint_age, above_block) = match kind {
            FloorRoundKind::Requested => (T::MaxFloorCheckpointAge::get(), None),
            FloorRoundKind::Forced => (
                T::MinForcedFloorAge::get().min(T::MaxFloorCheckpointAge::get()),
                Self::last_floor_block(),
            ),
        };
        match Self::do_request_checkpoint_within(
            market.chain_id,
            Some(max_checkpoint_age),
            above_block,
        )? {
            CheckpointRequest::Ready(checkpoint) =>
                Self::start_floor_round(&checkpoint, market, kind),
            CheckpointRequest::Pending { round_id, .. } => {
                // A forced request waiting alongside a requested one makes the round forced.
                PendingFloorRequest::<T>::mutate(|pending| {
                    if *pending != Some(FloorRoundKind::Forced) {
                        *pending = Some(kind);
                    }
                });
                Self::deposit_event(Event::<T>::FloorRequestWaitingForCheckpoint {
                    checkpoint_round_id: round_id,
                });
                Ok(())
            },
        }
    }

    /// Called by the checkpoint side when a round on `chain_id` settles.
    pub(crate) fn on_checkpoint_agreed_floor(chain_id: ExternalChainId, checkpoint: &Checkpoint) {
        let market = match FloorMarketConfig::<T>::get() {
            Some(market) => market,
            None => return,
        };
        if chain_id != market.chain_id {
            return
        }
        let Some(kind) = PendingFloorRequest::<T>::take() else { return };
        if ActiveFloorRound::<T>::exists() {
            return
        }
        if kind == FloorRoundKind::Forced {
            // Checkpoints never go backwards, so one not above the last floor's block is at it:
            // the collators have not seen Ethereum move past it. The last floor is then the
            // newest there is, so answer with it rather than read it again or wait on a chain
            // that may have stalled. The force counts as ended, so the cooldown applies.
            if let Some(floor) = LastFinalisedFloor::<T>::get().filter(|floor| {
                checkpoint.block_number <= floor.observation.external_block.saturated_into::<u32>()
            }) {
                LastForcedRoundEndedAt::<T>::put(Self::now_secs());
                Self::deposit_event(Event::<T>::FloorPriceServed {
                    round_id: floor.round_id,
                    floor_wei: floor.observation.floor_wei,
                });
                return
            }
        }
        if let Err(error) = Self::start_floor_round(checkpoint, market, kind) {
            Self::deposit_event(Event::<T>::FloorRoundStartFailed { error });
        }
    }

    /// The Ethereum block the last finalised floor was read at, which a forced round must pass.
    fn last_floor_block() -> Option<u32> {
        LastFinalisedFloor::<T>::get()
            .map(|floor| floor.observation.external_block.saturated_into::<u32>())
    }

    fn start_floor_round(
        checkpoint: &Checkpoint,
        market: FloorMarket,
        kind: FloorRoundKind,
    ) -> DispatchResult {
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
            checkpoint_agreed_at_sec: checkpoint.agreed_at,
            external_block: checkpoint.block_number,
            market,
            started_at: now_block,
            reports: 0,
            kind,
        });
        Self::deposit_event(Event::<T>::FloorRoundStarted {
            round_id,
            proposal_id,
            checkpoint_agreed_at_sec: checkpoint.agreed_at,
            external_block: checkpoint.block_number,
            market,
        });
        if kind == FloorRoundKind::Forced {
            Self::deposit_event(Event::<T>::FloorRoundForced { round_id });
        }
        Ok(())
    }

    pub fn floor_round_external_ref(round_id: u64) -> H256 {
        H256(sp_io::hashing::blake2_256(&(FLOOR_ROUND_REF_CONTEXT, round_id).encode()))
    }

    // ------------------------------------------------------------------ reports

    /// Bytes a watchtower signs for a report. The round's market is included because the
    /// observation does not name the collection it was read for.
    pub fn report_signing_payload(
        round_id: u64,
        market: &FloorMarket,
        observation: &FloorObservation,
        country: &CountryCode,
        watchtower: &T::AccountId,
    ) -> Vec<u8> {
        (
            FLOOR_ORACLE_REPORT_CONTEXT,
            Self::genesis_hash(),
            round_id,
            market,
            observation,
            country,
            watchtower,
        )
            .encode()
    }

    /// Everything a report must satisfy, including the signature. Used by the pool.
    pub fn validate_floor_report(
        round_id: u64,
        observation: &FloorObservation,
        country: &CountryCode,
        watchtower: &T::AccountId,
        signature: &<T::SignerId as RuntimeAppPublic>::Signature,
    ) -> DispatchResult {
        let round = Self::ensure_report_accepted(round_id, observation, watchtower)?;

        let key = T::Watchtowers::get_node_signing_key(watchtower)
            .ok_or(Error::<T>::FloorReportBadSignature)?;
        // Checked against the bytes the offchain worker signs, so the two cannot drift apart.
        let payload =
            Self::report_signing_payload(round_id, &round.market, observation, country, watchtower);
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
        // The block was confirmations deep before the checkpoint was agreed, so it cannot be
        // newer than the agreement. A later time would make the floor look fresher than it is.
        ensure!(
            observation.is_plausible() &&
                observation.external_block_time_sec <= round.checkpoint_agreed_at_sec,
            Error::<T>::ImplausibleObservation
        );
        ensure!(
            observation.external_block == round.external_block as u64,
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
                    checkpoint_agreed_at_sec: round.checkpoint_agreed_at_sec,
                    market: round.market,
                    observation,
                    agreeing,
                    reports: round.reports,
                    required,
                    finalised_at_block: <frame_system::Pallet<T>>::block_number()
                        .saturated_into::<u32>(),
                    finalised_at_sec: Self::now_secs(),
                });
                Self::deposit_event(Event::<T>::FloorRoundFinalised {
                    round_id: round.round_id,
                    external_block_time_sec: observation.external_block_time_sec,
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
        if round.kind == FloorRoundKind::Forced {
            LastForcedRoundEndedAt::<T>::put(Self::now_secs());
        }
        // The round's reports and observations are deleted later by `on_idle`, so ending a
        // round costs the same however many watchtowers reported.
        if round.reports > 0 {
            RoundsToClean::<T>::insert((OracleChannel::Floor, round.round_id), ());
        }
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

        let observation = match Self::fetch_observation(&round.market, round.external_block) {
            Ok(observation) => observation,
            Err(e) => {
                log::error!("💔 Floor oracle: cannot fetch observation: {:?}", e);
                return
            },
        };
        if !observation.is_plausible() || observation.external_block != round.external_block as u64
        {
            log::error!(
                "💔 Floor oracle: service returned an unusable observation {:?}",
                observation
            );
            return
        }

        let country = pallet_watchtower::fetch_node_country::<T>();
        let payload = Self::report_signing_payload(
            round.round_id,
            &round.market,
            &observation,
            &country,
            &watchtower,
        );
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
