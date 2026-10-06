use crate::*;

impl<T: Config> Pallet<T> {
    /// The one place a proposal's outcome is decided.
    ///
    /// Either side holding `threshold` of the eligible voters resolves the proposal early. Once
    /// the voting period has ended (`expired`), the proposal's `DecisionRule` decides instead,
    /// so the result is always `Some` for an expired proposal.
    pub fn decide(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
        expired: bool,
    ) -> Option<ProposalStatusEnum> {
        let votes = Votes::<T>::get(proposal_id);

        if !expired {
            // A committee fixes the denominator at activation; otherwise every node may vote
            // and the live node count is used.
            let eligible = ProposalCommitteeSize::<T>::get(proposal_id)
                .unwrap_or_else(T::Watchtowers::get_authorized_watchtowers_count);
            if eligible == 0 {
                return None
            }

            let needed = proposal.threshold.mul_ceil(eligible);
            if votes.in_favors >= needed {
                return Some(ProposalStatusEnum::Resolved { passed: true })
            }
            if votes.againsts >= needed {
                return Some(ProposalStatusEnum::Resolved { passed: false })
            }
            return None
        }

        Some(match proposal.decision_rule {
            DecisionRule::ExpireUnresolved => ProposalStatusEnum::Expired,
            DecisionRule::SimpleMajorityOnExpiry =>
                ProposalStatusEnum::Resolved { passed: votes.in_favors > votes.againsts },
            DecisionRule::RejectOnExpiry => ProposalStatusEnum::Resolved { passed: false },
        })
    }

    /// Outcome of a proposal whose voting period has ended.
    pub fn expiry_result(proposal_id: ProposalId, proposal: &Proposal<T>) -> ProposalStatusEnum {
        Self::decide(proposal_id, proposal, true).unwrap_or(ProposalStatusEnum::Expired)
    }

    pub fn finalise_expired_voting(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
    ) -> DispatchResult {
        let consensus_result = Self::expiry_result(proposal_id, proposal);
        Self::finalise_voting(proposal_id, proposal, consensus_result)
    }

    pub fn finalise_voting(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
        consensus_result: ProposalStatusEnum,
    ) -> DispatchResult {
        ProposalStatus::<T>::insert(proposal_id, consensus_result.clone());

        // The order matters here:
        // - we first call the hook so other pallets cleanup their state
        // - then emit the event
        // - finally we clear the active slot. The next queued proposal is NOT activated here; that
        //   happens in `activate_next_proposal`, submitted by the offchain worker, so the
        //   (potentially expensive) activation never runs inside a vote or a block hook.
        T::WatchtowerHooks::on_voting_completed(
            proposal_id,
            &proposal.external_ref,
            &consensus_result,
        );

        Self::deposit_event(Event::VotingEnded {
            proposal_id,
            external_ref: proposal.external_ref,
            consensus_result,
        });

        // A queued proposal can be cancelled while another one is active; only clear the slot
        // if this proposal holds it.
        if ActiveInternalProposal::<T>::get() == Some(proposal_id) {
            ActiveInternalProposal::<T>::kill();
        }

        ProposalsToRemove::<T>::insert(proposal_id, ());

        Ok(())
    }

    /// Single place where a proposal is marked as active: sets the deadline and, for proposals
    /// that request one, selects and stores the committee. Shared by the activation of external
    /// proposals in `add_proposal` and of internal proposals in `activate_next_proposal`.
    ///
    /// Fails only for committee proposals (see `select_committee`). Callers must run it in a
    /// transactional context so a failed selection leaves no committee rows behind.
    pub(crate) fn activate_proposal(
        proposal_id: ProposalId,
        proposal: &mut Proposal<T>,
        now: BlockNumberFor<T>,
        committee: Option<u32>,
    ) -> Result<(), Error<T>> {
        proposal.end_at = Some(now.saturating_add(proposal.vote_duration.into()));

        // `None` is the explicit legacy mode: every node votes, live denominator. `committee`
        // is the effective size from `committee_ready_for`, computed once by the caller.
        if let Some(size) = committee {
            Self::select_committee(proposal_id, size)?;
            ProposalCommitteeSize::<T>::insert(proposal_id, size);
            Self::deposit_event(Event::CommitteeSelected { proposal_id, size });
        }

        Ok(())
    }

    /// Dequeues the head of the internal proposal queue and activates it.
    /// Callers must have verified there is no active internal proposal.
    pub(crate) fn activate_next_proposal_inner() -> DispatchResult {
        ensure!(ActiveInternalProposal::<T>::get().is_none(), Error::<T>::ProposalAlreadyActive);

        // The proposal is loaded once, before dequeuing, and threaded through activation.
        let proposal_id = Self::peek_front_id()?.ok_or(Error::<T>::QueueEmpty)?;

        // Only reachable if storage was tampered with. Advance the head rather than leaving a
        // dangling id at the front of the queue forever.
        let Some(mut proposal) = Proposals::<T>::get(proposal_id) else {
            Self::dequeue()?;
            ProposalStatus::<T>::insert(proposal_id, ProposalStatusEnum::Unknown);
            Self::deposit_event(Event::ProposalActivationSkipped { proposal_id });
            return Ok(())
        };

        // Decided before dequeuing: a proposal whose committee cannot be built yet (index
        // backfilling, too few nodes) stays at the head and is retried later. The policy is
        // never silently changed to "everyone votes".
        let committee = Self::committee_ready_for(&proposal)?;

        Self::dequeue()?;

        let now = frame_system::Pallet::<T>::block_number();
        // Own storage layer: a corrupt node index must not leave a partial committee behind.
        let activated = frame_support::storage::with_storage_layer(|| {
            Self::activate_proposal(proposal_id, &mut proposal, now, committee)
                .map_err(DispatchError::from)
        });

        if let Err(error) = activated {
            // Readiness was checked above, so only index corruption gets here. Cancel rather
            // than fail: failing would roll back the dequeue and the OCW would retry forever.
            log::error!(
                "🪲 Committee selection failed for proposal {:?}: {:?}. Cancelling it.",
                proposal_id,
                error
            );
            Self::deposit_event(Event::CommitteeSelectionFailed { proposal_id, error });
            Self::finalise_voting(proposal_id, &proposal, ProposalStatusEnum::Cancelled)?;
            return Ok(())
        }

        Proposals::<T>::insert(proposal_id, &proposal);
        ProposalStatus::<T>::insert(proposal_id, ProposalStatusEnum::Active);
        ActiveInternalProposal::<T>::put(proposal_id);

        if let Err(e) = T::WatchtowerHooks::on_proposal_submitted(proposal_id, proposal.clone()) {
            // Reached when a consumer (e.g. summary-watchtower) rejects the proposal; the tuple
            // hook impl propagates the first error. The extrinsic is transactional: propagating
            // the error would roll back the dequeue, leave this proposal at the head and make
            // the OCW resubmit a failing tx every block. Cancel it instead so the queue can move
            // on and the consumer is told via `on_voting_completed`.
            log::error!(
                "🪲 on_proposal_submitted failed for proposal {:?}: {:?}. Cancelling it.",
                proposal_id,
                e
            );
            Self::finalise_voting(proposal_id, &proposal, ProposalStatusEnum::Cancelled)?;
            return Ok(())
        }

        Self::deposit_event(Event::ProposalActivated { proposal_id });

        Ok(())
    }

    pub fn proposal_expired(current_block: BlockNumberFor<T>, proposal: &Proposal<T>) -> bool {
        current_block >= proposal.end_at.unwrap_or(0u32.into())
    }

    pub fn active_proposal_expiry_status(
        now: BlockNumberFor<T>,
    ) -> Option<(ProposalId, Proposal<T>, bool)> {
        let Some(proposal_id) = ActiveInternalProposal::<T>::get() else {
            return None;
        };

        let Some(active_proposal) = <Proposals<T>>::get(proposal_id) else {
            return None;
        };

        let expired = Self::proposal_expired(now, &active_proposal);
        Some((proposal_id, active_proposal, expired))
    }
}
