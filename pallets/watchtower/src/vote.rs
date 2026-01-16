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
        // - finally we add a new active proposal if needed
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

        // If this was an internal proposal, activate the next one in the queue
        if let ProposalSource::Internal(_) = proposal.source {
            ActiveInternalProposal::<T>::kill();
            if let Ok(next_proposal_id) = Self::dequeue() {
                ActiveInternalProposal::<T>::put(next_proposal_id);
                ProposalStatus::<T>::insert(next_proposal_id, ProposalStatusEnum::Active);
                // Try to mutate and fetch the proposal in one storage access
                let updated_proposal = Proposals::<T>::try_mutate(next_proposal_id, |p_opt| {
                    let p = p_opt.as_mut().ok_or(Error::<T>::ProposalNotFound)?;
                    p.end_at =
                        Some(frame_system::Pallet::<T>::block_number() + p.vote_duration.into());
                    Ok::<_, Error<T>>(p.clone())
                })?;

                T::WatchtowerHooks::on_proposal_submitted(next_proposal_id, updated_proposal)?;
            }
        }

        ProposalsToRemove::<T>::insert(proposal_id, ());

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
