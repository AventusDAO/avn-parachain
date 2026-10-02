use crate::*;

impl<T: Config> Pallet<T> {
    pub fn threshold_achieved(proposal_id: ProposalId, threshold: Perbill) -> Option<bool> {
        let vote = Votes::<T>::get(proposal_id);
        let total_voters = T::Watchtowers::get_authorized_watchtowers_count();
        if total_voters == 0 {
            return None
        }

        let min_votes = threshold.mul_ceil(total_voters);
        if vote.in_favors >= min_votes {
            Some(true)
        } else if vote.againsts >= min_votes {
            Some(false)
        } else {
            None
        }
    }

    pub fn get_proposal_status(result: bool) -> ProposalStatusEnum {
        if result {
            ProposalStatusEnum::Resolved { passed: true }
        } else {
            ProposalStatusEnum::Resolved { passed: false }
        }
    }

    pub fn get_vote_result_on_expiry(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
    ) -> ProposalStatusEnum {
        match proposal.source {
            ProposalSource::Internal(_) => ProposalStatusEnum::Expired,
            ProposalSource::External => {
                let votes = Votes::<T>::get(proposal_id);
                if proposal.decision_rule == DecisionRule::SimpleMajority &&
                    votes.in_favors > votes.againsts
                {
                    ProposalStatusEnum::Resolved { passed: true }
                } else {
                    ProposalStatusEnum::Resolved { passed: false }
                }
            },
        }
    }

    pub fn finalise_expired_voting(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
    ) -> DispatchResult {
        let consensus_result = Self::get_vote_result_on_expiry(proposal_id, proposal);
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

        if let ProposalSource::Internal(_) = proposal.source {
            ActiveInternalProposal::<T>::kill();
        }

        ProposalsToRemove::<T>::insert(proposal_id, ());

        Ok(())
    }

    /// Single place where a proposal is marked as active. Any future per-activation logic
    /// (e.g. committee selection) must be added here so that both the immediate activation in
    /// `add_proposal` and the deferred activation in `activate_next_proposal` share it.
    pub(crate) fn activate_proposal(
        _proposal_id: ProposalId,
        proposal: &mut Proposal<T>,
        now: BlockNumberFor<T>,
    ) {
        proposal.end_at = Some(now.saturating_add(proposal.vote_duration.into()));
    }

    /// Dequeues the head of the internal proposal queue and activates it.
    /// Callers must have verified there is no active internal proposal.
    pub(crate) fn activate_next_proposal_inner() -> DispatchResult {
        ensure!(ActiveInternalProposal::<T>::get().is_none(), Error::<T>::ProposalAlreadyActive);

        let proposal_id = Self::dequeue()?;

        // Only reachable if storage was tampered with. Advance the head rather than leaving a
        // dangling id at the front of the queue forever.
        if !Proposals::<T>::contains_key(proposal_id) {
            ProposalStatus::<T>::insert(proposal_id, ProposalStatusEnum::Unknown);
            Self::deposit_event(Event::ProposalActivationSkipped { proposal_id });
            return Ok(())
        }

        let now = frame_system::Pallet::<T>::block_number();
        let proposal = Proposals::<T>::mutate(proposal_id, |p_opt| {
            // Safe: checked above.
            let p = p_opt.as_mut().expect("proposal exists; qed");
            Self::activate_proposal(proposal_id, p, now);
            p.clone()
        });

        ProposalStatus::<T>::insert(proposal_id, ProposalStatusEnum::Active);
        ActiveInternalProposal::<T>::put(proposal_id);

        if let Err(e) = T::WatchtowerHooks::on_proposal_submitted(proposal_id, proposal.clone()) {
            // The extrinsic is transactional: propagating the error would roll back the
            // dequeue, leave this proposal at the head and make the OCW resubmit a failing tx
            // every block. Cancel it instead so the queue can move on.
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

    pub fn get_finalised_consensus_result(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
        current_block: BlockNumberFor<T>,
    ) -> Option<ProposalStatusEnum> {
        if let Some(result) = Self::threshold_achieved(proposal_id, proposal.threshold) {
            Some(Self::get_proposal_status(result))
        } else if Self::proposal_expired(current_block, proposal) {
            Some(Self::get_vote_result_on_expiry(proposal_id, proposal))
        } else {
            None
        }
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
