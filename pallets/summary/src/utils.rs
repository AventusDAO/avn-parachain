use crate::*;

use frame_system::pallet_prelude::*;
pub use sp_runtime::{
    traits::{AtLeast32Bit, Hash},
    Perbill, SaturatedConversion,
};

impl<T: Config<I>, I: 'static> Pallet<T, I> {
    pub fn set_summary_status(root_id: &RootId<BlockNumberFor<T>>, status: ExternalValidationEnum) {
        <ExternalValidationStatus<T, I>>::insert(root_id, status);
    }

    pub fn process_accepted_root(
        root_id: &RootId<BlockNumberFor<T>>,
        root_hash: H256,
    ) -> DispatchResult {
        let root_data = Self::try_get_root_data(root_id)?;
        if root_data.root_hash != Self::empty_root() {
            if T::AutoSubmitSummaries::get() {
                Self::send_root_to_ethereum(root_id, &root_data)?;
            } else {
                let approved_root_id = Self::get_next_approved_root_id()?;
                <AnchorRoots<T, I>>::insert(approved_root_id, root_data.root_hash);
            }

            Self::deposit_event(Event::<T, I>::RootPassedValidation {
                root_id: *root_id,
                root_hash,
            });
        }

        Ok(())
    }

    pub fn cleanup_external_validation_data(
        root_id: &RootId<BlockNumberFor<T>>,
        external_ref: &H256,
    ) {
        <ExternalValidationStatus<T, I>>::remove(root_id);
        <ExternalValidationRef<T, I>>::remove(external_ref);
        <PendingAdminReviews<T, I>>::remove(root_id);
    }

    pub fn submit_root_for_external_validation(
        root_id: &RootId<BlockNumberFor<T>>,
        root_hash: H256,
    ) -> DispatchResult {
        let current_block = <frame_system::Pallet<T>>::block_number();
        let inner_payload = (root_id, root_hash);
        let external_ref = H256::from_slice(&T::Hashing::hash_of(&inner_payload).as_ref());
        let title;
        let proposal_type;

        if T::AutoSubmitSummaries::get() {
            title = "Summary Root";
            proposal_type = ProposalType::Summary;
        } else {
            title = "Anchor Root";
            proposal_type = ProposalType::Anchor;
        }

        let threshold_val = <ExternalValidationThreshold<T, I>>::get()
            .ok_or(Error::<T, I>::ExternalValidationThresholdNotSet)?;

        let request = ProposalRequest {
            title: title.as_bytes().to_vec(),
            external_ref,
            threshold: Perbill::from_percent(threshold_val),
            payload: RawPayload::Inline(inner_payload.encode()),
            source: ProposalSource::Internal(proposal_type),
            decision_rule: <ExternalValidationDecisionRule<T, I>>::get(),
            created_at: current_block.saturated_into::<u32>(),
            vote_duration: None,
            committee_size: <ExternalValidationCommitteeSize<T, I>>::get(),
        };

        T::ExternalValidator::submit_proposal(None, request)?;

        ExternalValidationRef::<T, I>::insert(external_ref, root_id);
        Self::set_summary_status(root_id, ExternalValidationEnum::ValidationInProgress);

        Ok(())
    }

    /// Benchmark setup: records `external_ref` as the in-progress external validation of the
    /// root in `payload`, the `(RootId, H256)` a summary proposal carries, so that the
    /// watchtower completion hooks do their real work for that proposal.
    #[cfg(feature = "runtime-benchmarks")]
    pub fn benchmark_setup_external_validation(external_ref: H256, payload: Vec<u8>) {
        let (root_id, root_hash): (RootId<BlockNumberFor<T>>, H256) =
            Decode::decode(&mut &payload[..]).expect("benchmark payload is a summary root");
        let validator: T::AccountId = frame_benchmarking::account("summary_validator", 0, 0);
        <Roots<T, I>>::insert(
            root_id.range,
            root_id.ingress_counter,
            RootData::new(root_hash, validator, None),
        );
        ExternalValidationRef::<T, I>::insert(external_ref, root_id);
        Self::set_summary_status(&root_id, ExternalValidationEnum::ValidationInProgress);
    }

    /// Benchmark check: true if the root behind `external_ref` was sent to admin review.
    #[cfg(feature = "runtime-benchmarks")]
    pub fn benchmark_is_pending_admin_review(external_ref: H256) -> bool {
        ExternalValidationRef::<T, I>::get(external_ref).map_or(false, |root_id| {
            <PendingAdminReviews<T, I>>::contains_key(root_id) &&
                <ExternalValidationStatus<T, I>>::get(root_id) ==
                    Some(ExternalValidationEnum::PendingAdminReview)
        })
    }

    pub fn get_root_id_by_external_ref(
        external_ref: &H256,
    ) -> Result<RootId<BlockNumberFor<T>>, DispatchError> {
        if let Some(root_id) = ExternalValidationRef::<T, I>::get(external_ref) {
            Ok(root_id)
        } else {
            Err(Error::<T, I>::ExternalRefNotFound.into())
        }
    }

    pub fn setup_root_for_admin_review(
        root_id: RootId<BlockNumberFor<T>>,
        proposal_id: ProposalId,
        external_ref: H256,
        status: ProposalStatusEnum,
    ) {
        Self::set_summary_status(&root_id, ExternalValidationEnum::PendingAdminReview);
        <PendingAdminReviews<T, I>>::insert(
            root_id,
            ExternalValidationData::new(proposal_id, external_ref, status.clone()),
        );
        Self::deposit_event(Event::<T, I>::AdminReviewRequested {
            root_id,
            proposal_id,
            external_ref,
            status,
        });
    }
}
