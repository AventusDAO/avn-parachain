// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{
    mock::{AnchorSummary, *},
    system,
};
use codec::alloc::sync::Arc;
use frame_support::assert_ok;
use parking_lot::RwLock;
use sp_core::offchain::testing::PoolState;
use sp_runtime::testing::UintAuthorityId;
use system::RawOrigin;

fn record_summary_calculation_is_called(
    current_block_number: BlockNumber,
    this_validator: &Validator<UintAuthorityId, AccountId>,
    pool_state: &Arc<RwLock<PoolState>>,
) -> bool {
    AnchorSummary::process_summary_if_required(current_block_number, this_validator);

    return !pool_state.read().transactions.is_empty()
}

fn record_summary_calculation_is_ok(context: &Context) -> bool {
    assert_eq!(
        Ok(()),
        AnchorSummary::record_summary_calculation(
            RawOrigin::None.into(),
            context.last_block_in_range,
            context.root_hash_h256,
            context.root_id.ingress_counter,
            context.validator.clone(),
            context.record_summary_calculation_signature.clone(),
        )
    );
    return true
}

fn setup_block_numbers_and_slots(context: &Context) {
    let current_block_number: BlockNumber = 12;
    let next_block_to_process: BlockNumber = 3;
    let previous_slot_number: BlockNumber = 1;
    let current_slot_number: BlockNumber = 2;
    let block_number_for_next_slot: BlockNumber =
        current_block_number + AnchorSummary::schedule_period();

    System::set_block_number(current_block_number);
    AnchorSummary::set_next_block_to_process(next_block_to_process);
    AnchorSummary::set_previous_summary_slot(previous_slot_number);
    AnchorSummary::set_current_slot(current_slot_number);
    AnchorSummary::set_current_slot_validator(context.validator.account_id);
    AnchorSummary::set_next_slot_block_number(block_number_for_next_slot);
}

fn approve_summary(context: &Context) {
    vote_and_end_summary(context);

    assert!(AnchorSummary::get_root_data(&context.root_id).is_validated);
    assert!(!PendingApproval::<TestRuntime, Instance1>::contains_key(&context.root_id.range));
    assert_eq!(
        AnchorSummary::get_next_block_to_process(),
        context.next_block_to_process + AnchorSummary::schedule_period()
    );
    assert_eq!(AnchorSummary::last_summary_slot(), AnchorSummary::current_slot());

    assert!(System::events().iter().any(|a| a.event ==
        mock::RuntimeEvent::AnchorSummary(
            crate::Event::<TestRuntime, Instance1>::VotingEnded {
                root_id: context.root_id,
                vote_approved: true
            }
        )));
}

fn vote_and_end_summary(context: &Context) {
    AnchorSummary::insert_root_hash(
        &context.root_id,
        context.root_hash_h256,
        context.validator.account_id.clone(),
        context.tx_id,
    );
    AnchorSummary::insert_pending_approval(&context.root_id);
    AnchorSummary::register_root_for_voting(&context.root_id, QUORUM, VOTING_PERIOD_END);

    let validators = vec![
        get_validator(FIRST_VALIDATOR_INDEX),
        get_validator(SECOND_VALIDATOR_INDEX),
        get_validator(THIRD_VALIDATOR_INDEX),
    ];

    validators.iter().for_each(|validator| {
        AnchorSummary::record_approve_vote(&context.root_id, validator.account_id);
    });

    assert_ok!(AnchorSummary::end_voting_period(
        RawOrigin::None.into(),
        context.root_id,
        context.validator.clone(),
        context.record_summary_calculation_signature.clone(),
    ));
}

pub fn update_context_to_anchor_summary(context: &mut Context) {
    AnchorSummary::set_schedule_and_voting_periods(DEFAULT_SCHEDULE_PERIOD, DEFAULT_VOTING_PERIOD);
    let last_block_in_range = context.next_block_to_process + AnchorSummary::schedule_period() - 1;

    context.last_block_in_range = last_block_in_range;
    context.url_param =
        get_url_param(context.next_block_to_process, AnchorSummary::schedule_period());
    context.record_summary_calculation_signature = get_signature_for_record_summary_calculation(
        context.validator.clone(),
        &AnchorSummary::update_block_number_context(),
        context.root_hash_h256,
        context.root_id.ingress_counter,
        context.last_block_in_range,
    );

    ExternalValidationThreshold::<TestRuntime, Instance1>::set(Some(51));
}

pub fn setup_total_ingresses(context: &Context) {
    AnchorSummary::set_total_ingresses(context.root_id.ingress_counter - 1);
}

mod on_successful_summary_approval {
    use super::*;

    #[test]
    fn anchor_data_is_popualated() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let mut context = setup_context();
            update_context_to_anchor_summary(&mut context);

            setup_total_ingresses(&context);
            setup_block_numbers_and_slots(&context);

            assert!(pool_state.read().transactions.is_empty());

            // Mock a successful compute root hash response
            let successful_response = context.root_hash_vec.clone();
            mock_response_of_get_roothash(
                &mut offchain_state.write(),
                context.url_param.clone(),
                Some(successful_response),
            );

            // The first time process summary for root [from:3;to:4] is created successfully at
            // block#12
            assert!(record_summary_calculation_is_called(
                context.current_block_number,
                &context.validator,
                &pool_state
            ));

            // Simulate recording summary calculation
            assert_eq!(true, record_summary_calculation_is_ok(&context));

            // Approve the summary
            approve_summary(&context);

            // No root hash created yet because we are waiting for external validation
            let root_counter = AnchorRootsCounter::<TestRuntime, Instance1>::get();

            // Since external validation is enabled, root counter should not increase until we
            // complete external validation
            assert_eq!(
                root_counter, 0,
                "Root counter should not increase because external validation is enabled"
            );

            assert_ok!(AnchorSummary::process_accepted_root(
                &context.root_id,
                context.root_hash_h256
            ));

            // Root hash is recorded and ready for anchoring.
            // We -1 root_counter because it is incremented after a root is approved.
            let root_counter = AnchorRootsCounter::<TestRuntime, Instance1>::get();
            assert_eq!(
                AnchorRoots::<TestRuntime, Instance1>::get(root_counter - 1),
                context.root_hash_h256,
                "Root not ready for anchoring"
            );
        });
    }
}
