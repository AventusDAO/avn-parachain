// Copyright 2026 Aventus DAO Ltd

//! Venue rounds: the raw state of the Exotic perpetuals market for PUNKS, read at a pinned
//! block of the venue chain (Base). The channel mirrors the floor channel in [`floor`]:
//!
//! 1. `request_venue_state` finds no fresh state and no running round. It needs a configured
//!    [`VenueMarket`] and asks the checkpoint side for that market's chain. With a fresh checkpoint
//!    the round starts at once; otherwise the request is parked in `PendingVenueRequest` and the
//!    round starts when the checkpoint round settles.
//! 2. Starting a round snapshots the market into the round and submits an internal watchtower
//!    proposal of type `VenueOracle` with `VotingKind::Choice`.
//! 3. Watchtower nodes fetch `venue/exotic/{chain_id}/{pinned_block}/{clearing_house}/{amm}` from
//!    their local external service and submit `submit_venue_report`. The report is the vote,
//!    exactly as on the floor channel.
//! 4. A passed proposal stores the winning observation as `LastFinalisedVenue`; anything else fails
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
    /// Venue chain block every watchtower reads the contracts at.
    pub pinned_block: u32,
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
            if Self::now_secs().saturating_sub(venue.observation.pinned_ts) <= T::MaxVenueAge::get()
            {
                Self::deposit_event(Event::<T>::VenueStateServed {
                    round_id: venue.round_id,
                    pinned_block: venue.observation.pinned_block,
                });
                return Ok(())
            }
        }

        let market = VenueMarketConfig::<T>::get().ok_or(Error::<T>::VenueMarketNotSet)?;
        match Self::do_request_checkpoint(market.chain_id)? {
            CheckpointRequest::Ready(checkpoint) => Self::start_venue_round(&checkpoint, market),
            CheckpointRequest::Pending { round_id, .. } => {
                PendingVenueRequest::<T>::put(true);
                Self::deposit_event(Event::<T>::VenueRequestWaitingForCheckpoint {
                    checkpoint_round_id: round_id,
                });
                Ok(())
            },
        }
    }

    /// Called by the checkpoint side when a round on `chain_id` settles.
    pub(crate) fn on_checkpoint_agreed_venue(chain_id: ExternalChainId, checkpoint: &Checkpoint) {
        let market = match VenueMarketConfig::<T>::get() {
            Some(market) => market,
            None => return,
        };
        if chain_id != market.chain_id || !PendingVenueRequest::<T>::take() {
            return
        }
        if ActiveVenueRound::<T>::exists() {
            return
        }
        if let Err(error) = Self::start_venue_round(checkpoint, market) {
            Self::deposit_event(Event::<T>::VenueRoundStartFailed { error });
        }
    }

    fn start_venue_round(checkpoint: &Checkpoint, market: VenueMarket) -> DispatchResult {
        let round_id = LastVenueRoundId::<T>::mutate(|id| {
            *id = id.saturating_add(1);
            *id
        });
        let now_block = <frame_system::Pallet<T>>::block_number();
        let external_ref = Self::venue_round_external_ref(round_id);

        let request = ProposalRequest {
            title: VENUE_PROPOSAL_TITLE.to_vec(),
            payload: RawPayload::Inline((round_id, checkpoint.block_number, market).encode()),
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
            pinned_block: checkpoint.block_number,
            market,
            started_at: now_block,
            reports: 0,
        });
        Self::deposit_event(Event::<T>::VenueRoundStarted {
            round_id,
            proposal_id,
            pinned_block: checkpoint.block_number,
            market,
        });
        Ok(())
    }

    pub fn venue_round_external_ref(round_id: u64) -> H256 {
        H256(sp_io::hashing::blake2_256(&(VENUE_ROUND_REF_CONTEXT, round_id).encode()))
    }

    // ------------------------------------------------------------------ reports

    /// Bytes a watchtower signs for a venue report.
    pub fn venue_report_signing_payload(
        round_id: u64,
        observation: &VenueObservation,
        country: &CountryCode,
        watchtower: &T::AccountId,
    ) -> Vec<u8> {
        (VENUE_ORACLE_REPORT_CONTEXT, round_id, observation, country, watchtower).encode()
    }

    /// Everything a venue report must satisfy, including the signature. Used by the pool.
    pub fn validate_venue_report(
        round_id: u64,
        observation: &VenueObservation,
        country: &CountryCode,
        watchtower: &T::AccountId,
        signature: &<T::SignerId as RuntimeAppPublic>::Signature,
    ) -> DispatchResult {
        Self::ensure_venue_report_accepted(round_id, observation, watchtower)?;

        let key = T::Watchtowers::get_node_signing_key(watchtower)
            .ok_or(Error::<T>::VenueReportBadSignature)?;
        // The tuple is encoded inside the check; it must match `venue_report_signing_payload`.
        ensure!(
            pallet_watchtower::Pallet::<T>::offchain_signature_is_valid(
                &(VENUE_ORACLE_REPORT_CONTEXT, round_id, observation, country, watchtower),
                &key,
                signature,
            ),
            Error::<T>::VenueReportBadSignature
        );
        Ok(())
    }

    fn ensure_venue_report_accepted(
        round_id: u64,
        observation: &VenueObservation,
        watchtower: &T::AccountId,
    ) -> Result<VenueRound<BlockNumberFor<T>>, DispatchError> {
        let round = ActiveVenueRound::<T>::get().ok_or(Error::<T>::NoActiveVenueRound)?;
        ensure!(round.round_id == round_id, Error::<T>::VenueRoundMismatch);
        ensure!(
            pallet_watchtower::ActiveInternalProposal::<T>::get() == Some(round.proposal_id),
            Error::<T>::VenueProposalNotActive
        );
        ensure!(
            Self::is_oracle_committee_member(round.proposal_id, watchtower),
            Error::<T>::NotInVenueCommittee
        );
        ensure!(
            !VenueReports::<T>::contains_key(round_id, watchtower),
            Error::<T>::DuplicateVenueReport
        );
        ensure!(observation.is_plausible(), Error::<T>::ImplausibleVenueObservation);
        ensure!(
            observation.pinned_block == round.pinned_block as u64,
            Error::<T>::VenueObservationBlockMismatch
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
            country,
        });

        pallet_watchtower::Pallet::<T>::vote_choice_for(&watchtower, proposal_id, hash)?;
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
                    pinned_block: observation.pinned_block,
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
        let limit =
            T::MaxCommitteeSize::get().max(T::Watchtowers::get_authorized_watchtowers_count());
        let _ = VenueReports::<T>::clear_prefix(round.round_id, limit, None);
        let _ = VenueObservations::<T>::clear_prefix(round.round_id, limit, None);
    }

    // ------------------------------------------------------------------ offchain worker

    /// Reports once per round. Only nodes holding a registered watchtower signing key take
    /// part; collators have none and return early.
    pub fn venue_offchain_worker(now: BlockNumberFor<T>) {
        let round = match ActiveVenueRound::<T>::get() {
            Some(round) => round,
            None => return,
        };
        if pallet_watchtower::ActiveInternalProposal::<T>::get() != Some(round.proposal_id) {
            return
        }
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

        let observation = match Self::fetch_venue_observation(&round.market, round.pinned_block) {
            Ok(observation) => observation,
            Err(e) => {
                log::error!("💔 Venue oracle: cannot fetch observation: {:?}", e);
                return
            },
        };
        if !observation.is_plausible() || observation.pinned_block != round.pinned_block as u64 {
            log::error!(
                "💔 Venue oracle: service returned an unusable observation {:?}",
                observation
            );
            return
        }

        let country = Self::fetch_node_country();
        let payload =
            Self::venue_report_signing_payload(round.round_id, &observation, &country, &watchtower);
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

    /// Path of the service request for `market` at `pinned_block`:
    /// `venue/exotic/{chain_id}/{pinned_block}/{clearing_house}/{amm}`, addresses as 40
    /// lower-case hex characters without `0x`. Shared with the tests so they cannot drift.
    pub fn venue_service_path(market: &VenueMarket, pinned_block: u32) -> String {
        format!(
            "{}/{}/{}/{:x}/{:x}",
            VENUE_SERVICE_PATH, market.chain_id, pinned_block, market.clearing_house, market.amm
        )
    }

    /// `GET` of [`Pallet::venue_service_path`] on the local external service. The body is the
    /// http-data encoding of a `VenueObservation`.
    pub fn fetch_venue_observation(
        market: &VenueMarket,
        pinned_block: u32,
    ) -> Result<VenueObservation, DispatchError> {
        let body = AVN::<T>::get_data_from_service(Self::venue_service_path(market, pinned_block))?;
        let text = core::str::from_utf8(&body)
            .map_err(|_| DispatchError::Other("venue observation is not utf8"))?;
        decode_from_http_data::<VenueObservation>(text)
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
