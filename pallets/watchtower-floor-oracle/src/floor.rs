// Copyright 2026 Aventus DAO Ltd

//! Floor rounds: pin a checkpoint, open a watchtower `Choice` proposal, and let every
//! committee member report what it observed. The report is the vote.
//!
//! Life of a round:
//! 1. `request_floor_price` finds no fresh floor and no running round. It asks the checkpoint side
//!    for `FloorChainId`. With a fresh checkpoint the round starts at once; otherwise the request
//!    is parked in `PendingFloorRequest` and the round starts when the checkpoint round settles
//!    (`on_checkpoint_agreed`).
//! 2. Starting a round submits an internal watchtower proposal of type `FloorOracle` with
//!    `VotingKind::Choice`. The proposal queues and the collator OCW activates it, which fixes the
//!    committee.
//! 3. Watchtower nodes (their OCW, `floor_offchain_worker`) fetch an observation for the pinned
//!    timestamp from their local external service and submit `submit_floor_report`. The pallet
//!    stores the observation under its hash and counts the hash as the node's answer through
//!    `pallet_watchtower::vote_choice_for`. The first answer reaching the proposal's threshold
//!    wins; no single reporter can set a "candidate".
//! 4. The watchtower pallet decides the proposal and calls `on_voting_completed`. A passed proposal
//!    stores the winning observation as `LastFinalisedFloor`; anything else fails the round. Both
//!    clear the round, so the next request can open a new one.
//!
//! `VoteFilter` keeps the two paths consistent: a watchtower vote on a floor proposal must be
//! the hash it reported, and yes/no votes are refused outright.

use crate::*;
use codec::DecodeWithMemTracking;
use pallet_watchtower::NodesInterface;
use sp_avn_common::{
    http_data_codec::decode_from_http_data,
    ocw_lock::{self as OcwLock, OcwStorageError},
};
use sp_runtime::{
    offchain::storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
    SaturatedConversion,
};
use sp_watchtower::{
    DecisionRule, ProposalRequest, ProposalSource, ProposalType, RawPayload, VoteChoice,
    VotingKind, WatchtowerInterface,
};

const OCW_ID: &[u8] = b"floor_oracle::ocw";
const OCW_DB_PREFIX: &[u8] = b"floor_oracle::ocw::";
/// Blocks an OCW submission is given to land before the node retries.
const BLOCK_INCLUSION_PERIOD: u32 = 5;
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
    pub started_at: BlockNumber,
    /// Reports received so far.
    pub reports: u32,
}

impl<T: Config> Pallet<T> {
    // ------------------------------------------------------------------ requests

    pub fn do_request_floor_price() -> DispatchResult {
        if let Some(round) = ActiveFloorRound::<T>::get() {
            Self::deposit_event(Event::<T>::FloorRoundInProgress { round_id: round.round_id });
            return Ok(())
        }

        if let Some(floor) = LastFinalisedFloor::<T>::get() {
            if Self::now_secs().saturating_sub(floor.pinned_ts) <= T::MaxFloorAge::get() {
                Self::deposit_event(Event::<T>::FloorPriceServed {
                    round_id: floor.round_id,
                    floor_wei: floor.observation.floor_wei,
                });
                return Ok(())
            }
        }

        match Self::do_request_checkpoint(T::FloorChainId::get())? {
            CheckpointRequest::Ready(checkpoint) => Self::start_floor_round(&checkpoint),
            CheckpointRequest::Pending { round_id, .. } => {
                PendingFloorRequest::<T>::put(true);
                Self::deposit_event(Event::<T>::FloorRequestWaitingForCheckpoint {
                    checkpoint_round_id: round_id,
                });
                Ok(())
            },
        }
    }

    /// Called by the checkpoint side when a round on `chain_id` settles.
    pub(crate) fn on_checkpoint_agreed(chain_id: ExternalChainId, checkpoint: &Checkpoint) {
        if chain_id != T::FloorChainId::get() || !PendingFloorRequest::<T>::take() {
            return
        }
        if ActiveFloorRound::<T>::exists() {
            return
        }
        if let Err(error) = Self::start_floor_round(checkpoint) {
            Self::deposit_event(Event::<T>::FloorRoundStartFailed { error });
        }
    }

    fn start_floor_round(checkpoint: &Checkpoint) -> DispatchResult {
        let round_id = LastFloorRoundId::<T>::mutate(|id| {
            *id = id.saturating_add(1);
            *id
        });
        let now_block = <frame_system::Pallet<T>>::block_number();
        let external_ref = Self::floor_round_external_ref(round_id);

        let request = ProposalRequest {
            title: FLOOR_PROPOSAL_TITLE.to_vec(),
            payload: RawPayload::Inline(
                (round_id, checkpoint.agreed_at, checkpoint.block_number).encode(),
            ),
            threshold: T::FloorRoundThreshold::get(),
            source: ProposalSource::Internal(ProposalType::FloorOracle),
            decision_rule: DecisionRule::RejectOnExpiry,
            external_ref,
            created_at: now_block.saturated_into::<u32>(),
            vote_duration: Some(T::FloorVoteDuration::get()),
            committee_size: T::FloorCommitteeSize::get(),
            voting: VotingKind::Choice,
        };
        <pallet_watchtower::Pallet<T> as WatchtowerInterface>::submit_proposal(None, request)?;
        let proposal_id = pallet_watchtower::Pallet::<T>::id_by_external_ref(external_ref);

        ActiveFloorRound::<T>::put(FloorRound {
            round_id,
            proposal_id,
            pinned_ts: checkpoint.agreed_at,
            pinned_block: checkpoint.block_number,
            started_at: now_block,
            reports: 0,
        });
        Self::deposit_event(Event::<T>::FloorRoundStarted {
            round_id,
            proposal_id,
            pinned_ts: checkpoint.agreed_at,
            pinned_block: checkpoint.block_number,
        });
        Ok(())
    }

    pub fn floor_round_external_ref(round_id: u64) -> H256 {
        H256(sp_io::hashing::blake2_256(&(FLOOR_ROUND_REF_CONTEXT, round_id).encode()))
    }

    // ------------------------------------------------------------------ reports

    /// Bytes a watchtower signs for a report.
    pub fn report_signing_payload(
        round_id: u64,
        observation: &FloorObservation,
        watchtower: &T::AccountId,
    ) -> Vec<u8> {
        (FLOOR_ORACLE_REPORT_CONTEXT, round_id, observation, watchtower).encode()
    }

    /// Everything a report must satisfy, including the signature. Used by the pool.
    pub fn validate_floor_report(
        round_id: u64,
        observation: &FloorObservation,
        watchtower: &T::AccountId,
        signature: &<T::SignerId as RuntimeAppPublic>::Signature,
    ) -> DispatchResult {
        Self::ensure_report_accepted(round_id, observation, watchtower)?;

        let key = T::Watchtowers::get_node_signing_key(watchtower)
            .ok_or(Error::<T>::FloorReportBadSignature)?;
        // The tuple is encoded inside the check; it must match `report_signing_payload`.
        ensure!(
            pallet_watchtower::Pallet::<T>::offchain_signature_is_valid(
                &(FLOOR_ORACLE_REPORT_CONTEXT, round_id, observation, watchtower),
                &key,
                signature,
            ),
            Error::<T>::FloorReportBadSignature
        );
        Ok(())
    }

    fn ensure_report_accepted(
        round_id: u64,
        observation: &FloorObservation,
        watchtower: &T::AccountId,
    ) -> Result<FloorRound<BlockNumberFor<T>>, DispatchError> {
        let round = ActiveFloorRound::<T>::get().ok_or(Error::<T>::NoActiveFloorRound)?;
        ensure!(round.round_id == round_id, Error::<T>::FloorRoundMismatch);
        ensure!(
            pallet_watchtower::ActiveInternalProposal::<T>::get() == Some(round.proposal_id),
            Error::<T>::FloorProposalNotActive
        );
        ensure!(
            Self::is_floor_committee_member(round.proposal_id, watchtower),
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

    fn is_floor_committee_member(proposal_id: ProposalId, who: &T::AccountId) -> bool {
        T::Watchtowers::is_authorized_watchtower(who) &&
            pallet_watchtower::Pallet::<T>::is_committee_member(proposal_id, who)
    }

    /// Stores the report and counts it as the watchtower's answer on the proposal. The vote may
    /// finalise the proposal, whose hook re-enters this pallet and clears the round, so all
    /// round state is written before the vote is cast.
    pub(crate) fn do_submit_floor_report(
        round_id: u64,
        observation: FloorObservation,
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
        });

        pallet_watchtower::Pallet::<T>::vote_choice_for(&watchtower, proposal_id, hash)?;
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
                let required = T::FloorRoundThreshold::get().mul_ceil(eligible);
                let agreeing =
                    pallet_watchtower::Pallet::<T>::choice_tally(round.proposal_id, hash);
                LastFinalisedFloor::<T>::put(FinalisedFloorRound {
                    round_id: round.round_id,
                    proposal_id: round.proposal_id,
                    pinned_ts: round.pinned_ts,
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
        let limit =
            T::MaxCommitteeSize::get().max(T::Watchtowers::get_authorized_watchtowers_count());
        let _ = FloorReports::<T>::clear_prefix(round.round_id, limit, None);
        let _ = FloorObservations::<T>::clear_prefix(round.round_id, limit, None);
    }

    // ------------------------------------------------------------------ offchain worker

    /// Reports once per round. Only nodes holding a registered watchtower signing key take
    /// part; collators have none and return early.
    pub fn floor_offchain_worker(now: BlockNumberFor<T>) {
        let round = match ActiveFloorRound::<T>::get() {
            Some(round) => round,
            None => return,
        };
        if pallet_watchtower::ActiveInternalProposal::<T>::get() != Some(round.proposal_id) {
            return
        }
        if Self::ocw_already_run(now).is_err() {
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
        if Self::submission_in_progress(&key, now) {
            return
        }

        let observation = match Self::fetch_observation(round.pinned_ts, round.pinned_block) {
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

        let payload = Self::report_signing_payload(round.round_id, &observation, &watchtower);
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
                watchtower: watchtower.clone(),
                signature,
            }
            .into(),
        );
        match SubmitTransaction::<T, Call<T>>::submit_transaction(call) {
            Ok(()) => Self::record_submission(&key, now),
            Err(e) => log::error!("💔 Floor oracle: cannot submit report: {:?}", e),
        }
    }

    /// `GET floor/punks/{pinned_ts}/{pinned_block}` on the local external service. The body is
    /// the http-data encoding of a `FloorObservation`.
    pub fn fetch_observation(
        pinned_ts: u64,
        pinned_block: u32,
    ) -> Result<FloorObservation, DispatchError> {
        let path = format!("{}/{}/{}", FLOOR_SERVICE_PATH, pinned_ts, pinned_block);
        let body = AVN::<T>::get_data_from_service(path)?;
        let text = core::str::from_utf8(&body)
            .map_err(|_| DispatchError::Other("floor observation is not utf8"))?;
        decode_from_http_data::<FloorObservation>(text)
            .map_err(|_| DispatchError::Other("floor observation does not decode"))
    }

    fn ocw_already_run(now: BlockNumberFor<T>) -> Result<(), ()> {
        OcwLock::record_block_run(now, OCW_ID.to_vec()).map_err(|e| match e {
            OcwStorageError::OffchainWorkerAlreadyRun => {
                log::debug!("Floor oracle OCW already ran for block {:?}", now)
            },
            OcwStorageError::ErrorRecordingOffchainWorkerRun => {
                log::error!("💔 Floor oracle OCW cannot record its run for block {:?}", now)
            },
        })
    }

    fn ocw_storage_key(suffix: &[u8]) -> Vec<u8> {
        let mut key = OCW_DB_PREFIX.to_vec();
        key.extend_from_slice(suffix);
        key
    }

    fn submission_in_progress(suffix: &[u8], now: BlockNumberFor<T>) -> bool {
        match StorageValueRef::persistent(&Self::ocw_storage_key(suffix))
            .get::<BlockNumberFor<T>>()
            .ok()
            .flatten()
        {
            Some(submitted_at) => now <= submitted_at.saturating_add(BLOCK_INCLUSION_PERIOD.into()),
            None => false,
        }
    }

    fn record_submission(suffix: &[u8], now: BlockNumberFor<T>) {
        let result = StorageValueRef::persistent(&Self::ocw_storage_key(suffix))
            .mutate(|_: Result<Option<BlockNumberFor<T>>, StorageRetrievalError>| Ok::<_, ()>(now));
        if let Err(MutateStorageError::ConcurrentModification(_)) = result {
            log::warn!("Floor oracle OCW: concurrent modification of submission record");
        }
    }
}

// ---------------------------------------------------------------------- watchtower traits

impl<T: Config> VoteFilter<T::AccountId> for Pallet<T> {
    /// On the active floor proposal a vote must be the hash the voter reported; yes/no votes
    /// are refused. Other proposals are not filtered.
    fn ensure_can_vote(
        proposal_id: ProposalId,
        voter: &T::AccountId,
        vote: &VoteChoice,
    ) -> DispatchResult {
        let round = match ActiveFloorRound::<T>::get() {
            Some(round) if round.proposal_id == proposal_id => round,
            _ => return Ok(()),
        };
        ensure!(
            Self::is_floor_committee_member(proposal_id, voter),
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
    }
}

impl<T: Config> WatchtowerHooks<pallet_watchtower::Proposal<T>> for Pallet<T> {
    fn on_proposal_submitted(
        _proposal_id: ProposalId,
        _proposal: pallet_watchtower::Proposal<T>,
    ) -> DispatchResult {
        // Floor proposals are created by this pallet; nothing to prepare on activation.
        Ok(())
    }

    fn on_voting_completed(
        proposal_id: ProposalId,
        _external_ref: &H256,
        result: &ProposalStatusEnum,
    ) {
        if let Some(round) = ActiveFloorRound::<T>::get() {
            if round.proposal_id == proposal_id {
                Self::finish_round(round, result);
            }
        }
    }

    fn on_cancelled(proposal_id: ProposalId, _external_ref: &H256) {
        if let Some(round) = ActiveFloorRound::<T>::get() {
            if round.proposal_id == proposal_id {
                Self::finish_round(round, &ProposalStatusEnum::Cancelled);
            }
        }
    }
}
