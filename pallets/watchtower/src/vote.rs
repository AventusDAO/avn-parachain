use crate::*;

impl<T: Config> Pallet<T> {
    /// The one place a proposal's outcome is decided.
    ///
    /// Either side holding `threshold` of the eligible voters resolves the proposal, before or
    /// after expiry: without a committee the eligible count is the live node count, which can
    /// drop after the last vote. Otherwise, once the voting period has ended (`expired`), the
    /// proposal's `DecisionRule` decides, so the result is always `Some` for an expired proposal.
    /// A committee proposal whose seats have all voted is decided the same way without waiting
    /// for the end of the period: it can get no more votes, so waiting would not change the
    /// outcome. Under `RejectUnresolved`, a committee proposal is also rejected as soon as it can
    /// no longer pass, since expiry would reject it anyway.
    pub fn decide(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
        expired: bool,
    ) -> Option<ProposalStatusEnum> {
        let expired = expired || Self::committee_full(proposal_id);
        if VotingKindOf::<T>::get(proposal_id) == VotingKind::Choice {
            return Self::decide_choice(proposal_id, proposal, expired)
        }

        let votes = Votes::<T>::get(proposal_id);

        // A committee fixes the denominator at activation; otherwise every node may vote and
        // the live node count is used.
        let needed = Self::votes_needed(proposal_id, proposal);
        if let Some(needed) = needed {
            if votes.in_favors >= needed {
                return Some(ProposalStatusEnum::Resolved { passed: true })
            }
            if votes.againsts >= needed {
                return Some(ProposalStatusEnum::Resolved { passed: false })
            }
        }
        if !expired {
            if proposal.decision_rule == DecisionRule::RejectUnresolved &&
                needed.is_some_and(|needed| {
                    Self::out_of_reach(proposal_id, votes.in_favors, needed)
                })
            {
                return Some(ProposalStatusEnum::Resolved { passed: false })
            }
            return None
        }

        Some(match proposal.decision_rule {
            DecisionRule::ReportUnresolved => ProposalStatusEnum::Unresolved,
            DecisionRule::SimpleMajority =>
                ProposalStatusEnum::Resolved { passed: votes.in_favors > votes.againsts },
            DecisionRule::RejectUnresolved => ProposalStatusEnum::Resolved { passed: false },
        })
    }

    /// Voters eligible for a proposal: the committee fixed at activation, else every node.
    pub fn eligible_voters(proposal_id: ProposalId) -> u32 {
        ProposalCommitteeSize::<T>::get(proposal_id)
            .unwrap_or_else(T::Watchtowers::get_authorized_watchtowers_count)
    }

    /// Votes one side (or one answer) needs to decide the proposal. `None` while nobody is
    /// eligible.
    pub fn votes_needed(proposal_id: ProposalId, proposal: &Proposal<T>) -> Option<u32> {
        let eligible = Self::eligible_voters(proposal_id);
        if eligible == 0 {
            return None
        }
        Some(proposal.threshold.mul_ceil(eligible))
    }

    /// True if a committee proposal can no longer reach `needed` from `tally`: even if every
    /// seat left voted for it, it would fall short. Always false without a committee, whose
    /// eligible voter count can still grow.
    fn out_of_reach(proposal_id: ProposalId, tally: u32, needed: u32) -> bool {
        let Some(size) = ProposalCommitteeSize::<T>::get(proposal_id) else { return false };
        let remaining = size.saturating_sub(Self::votes_cast(proposal_id));
        tally.saturating_add(remaining) < needed
    }

    /// Decision for `Choice` proposals. The first answer whose tally reaches the threshold
    /// wins and is stored in `WinningChoice`, before or after expiry. Under `RejectUnresolved`,
    /// a committee proposal where no answer can still get there with the seats left fails
    /// early, since expiry would reject it anyway. Otherwise once voting ends the decision rule
    /// applies, where "majority" means the single answer with the most votes. The other rules
    /// do not fail early: they can still pass a proposal that misses the threshold, or leave it
    /// unresolved.
    /// Reads only the running `ChoiceStandings`, so its cost does not grow with the number of
    /// distinct answers.
    fn decide_choice(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
        expired: bool,
    ) -> Option<ProposalStatusEnum> {
        let standing = ChoiceStandings::<T>::get(proposal_id);
        let best = standing.leader.map(|answer| (answer, standing.leader_tally));
        let tie_at_best = standing.tied;

        let needed = Self::votes_needed(proposal_id, proposal);
        if let (Some(needed), Some((answer, tally))) = (needed, best) {
            if tally >= needed {
                WinningChoice::<T>::insert(proposal_id, answer);
                return Some(ProposalStatusEnum::Resolved { passed: true })
            }
        }

        if !expired {
            // Even if every seat left backed the leader it would fall short.
            if proposal.decision_rule == DecisionRule::RejectUnresolved {
                if let (Some(needed), Some((_, tally))) = (needed, best) {
                    if Self::out_of_reach(proposal_id, tally, needed) {
                        return Some(ProposalStatusEnum::Resolved { passed: false })
                    }
                }
            }
            return None
        }

        Some(match proposal.decision_rule {
            DecisionRule::ReportUnresolved => ProposalStatusEnum::Unresolved,
            DecisionRule::RejectUnresolved => ProposalStatusEnum::Resolved { passed: false },
            DecisionRule::SimpleMajority => match best {
                Some((answer, _)) if !tie_at_best => {
                    WinningChoice::<T>::insert(proposal_id, answer);
                    ProposalStatusEnum::Resolved { passed: true }
                },
                _ => ProposalStatusEnum::Resolved { passed: false },
            },
        })
    }

    /// Outcome of a proposal whose voting period has ended.
    pub fn expiry_result(proposal_id: ProposalId, proposal: &Proposal<T>) -> ProposalStatusEnum {
        Self::decide(proposal_id, proposal, true).unwrap_or(ProposalStatusEnum::Unresolved)
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
            winning_choice: WinningChoice::<T>::get(proposal_id),
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
        // is the size checked by `committee_ready_for`, computed once by the caller.
        if let Some(size) = committee {
            Self::select_committee(proposal_id, size)?;
            ProposalCommitteeSize::<T>::insert(proposal_id, size);
            Self::deposit_event(Event::CommitteeSelected { proposal_id, size });
        }

        Ok(())
    }

    /// Cancels a dequeued proposal whose committee could not be built. Cancelling rather than
    /// failing keeps the queue moving: a failure would roll back the dequeue and the OCW would
    /// retry forever.
    fn cancel_without_committee(
        proposal_id: ProposalId,
        proposal: &Proposal<T>,
        error: DispatchError,
    ) -> DispatchResult {
        Self::deposit_event(Event::CommitteeSelectionFailed { proposal_id, error });
        Self::finalise_voting(proposal_id, proposal, ProposalStatusEnum::Cancelled)
    }

    /// Dequeues the head of the internal proposal queue and activates it.
    /// Callers must have verified there is no active internal proposal.
    ///
    /// Returns the committee size that selection ran for (0 when the proposal has no
    /// committee, was skipped or was cancelled before selection), so the caller can refund
    /// weight from it. The stored
    /// `ProposalCommitteeSize` is not a substitute: a failed selection rolls it back although
    /// the per-member work was done.
    pub(crate) fn activate_next_proposal_inner() -> Result<u32, DispatchError> {
        ensure!(ActiveInternalProposal::<T>::get().is_none(), Error::<T>::ProposalAlreadyActive);

        // The proposal is loaded once, before dequeuing, and threaded through activation.
        let proposal_id = Self::peek_front_id()?.ok_or(Error::<T>::QueueEmpty)?;

        // Only reachable if storage was tampered with. Advance the head rather than leaving a
        // dangling id at the front of the queue forever.
        let Some(mut proposal) = Proposals::<T>::get(proposal_id) else {
            Self::dequeue()?;
            ProposalStatus::<T>::insert(proposal_id, ProposalStatusEnum::Unknown);
            Self::deposit_event(Event::ProposalActivationSkipped { proposal_id });
            return Ok(0)
        };

        // Decided before dequeuing: a proposal whose committee cannot be sampled yet (index
        // backfilling) stays at the head and is retried later. The policy is never silently
        // changed to "everyone votes", nor the committee to a smaller one.
        let committee = match Self::committee_ready_for(&proposal) {
            Ok(committee) => committee,
            Err(error @ Error::<T>::NotEnoughSeatsForCommittee) => {
                // Owners have left since the proposal was submitted. Waiting for new ones could
                // hold the queue indefinitely.
                log::warn!(
                    "⚠️ Too few seats for the committee of proposal {:?}. Cancelling it.",
                    proposal_id
                );
                Self::dequeue()?;
                Self::cancel_without_committee(proposal_id, &proposal, error.into())?;
                return Ok(0)
            },
            Err(e) => return Err(e.into()),
        };
        let committee_size = committee.unwrap_or(0);

        Self::dequeue()?;

        let now = frame_system::Pallet::<T>::block_number();
        // Own storage layer: a corrupt node index must not leave a partial committee behind.
        let activated = frame_support::storage::with_storage_layer(|| {
            Self::activate_proposal(proposal_id, &mut proposal, now, committee)
                .map_err(DispatchError::from)
        });

        if let Err(error) = activated {
            // Readiness was checked above, so only index corruption gets here.
            log::error!(
                "🪲 Committee selection failed for proposal {:?}: {:?}. Cancelling it.",
                proposal_id,
                error
            );
            Self::cancel_without_committee(proposal_id, &proposal, error)?;
            return Ok(committee_size)
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
            return Ok(committee_size)
        }

        Self::deposit_event(Event::ProposalActivated { proposal_id });

        Ok(committee_size)
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
