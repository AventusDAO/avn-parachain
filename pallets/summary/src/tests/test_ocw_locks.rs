// Copyright 2025 Aventus Network Services (UK) Ltd.

#![cfg(test)]

use crate::mock::*;
use codec::alloc::sync::Arc;
use frame_support::traits::Hooks;
use parking_lot::RwLock;
use sp_core::offchain::testing::PoolState;
use sp_runtime::{offchain::storage::StorageValueRef, testing::UintAuthorityId};

type MockValidator = Validator<UintAuthorityId, u64>;

pub struct LocalContext {
    pub current_block: BlockNumber,
    pub block_number_for_next_slot: BlockNumber,
    pub slot_validator: MockValidator,
    pub other_validator: MockValidator,
    pub slot_number: BlockNumber,
    pub grace_period: BlockNumber,
    pub summary_last_block_in_range: BlockNumber,
    pub block_after_grace_period: BlockNumber,
    pub challenge_reason: SummaryChallengeReason,
    pub finalised_block_vec: Option<Vec<u8>>,
}

pub fn setup_success_preconditions() -> LocalContext {
    let schedule_period = 2;
    let voting_period = 2;
    let min_block_age = <TestRuntime as Config>::MinBlockAge::get();
    let grace_period = <TestRuntime as Config>::AdvanceSlotGracePeriod::get();
    let arbitrary_margin = 3;
    let next_block_to_process = 2;
    let summary_last_block_in_range = next_block_to_process + schedule_period - 1;
    let current_block = summary_last_block_in_range + min_block_age + arbitrary_margin;
    let slot_number = 3;
    let block_number_for_next_slot = current_block;
    let slot_validator = get_validator(SIXTH_VALIDATOR_INDEX);
    let other_validator = get_validator(FIRST_VALIDATOR_INDEX);
    let challenge_reason = SummaryChallengeReason::SlotNotAdvanced(slot_number.try_into().unwrap());
    let block_after_grace_period = block_number_for_next_slot + grace_period + 5;
    let finalised_block_vec = Some(hex::encode(0u32.encode()).into());

    assert!(slot_validator != other_validator);

    System::set_block_number(current_block);
    Summary::set_schedule_and_voting_periods(schedule_period, voting_period);
    Summary::set_next_block_to_process(next_block_to_process);
    Summary::set_next_slot_block_number(block_number_for_next_slot);
    Summary::set_current_slot(slot_number);
    Summary::set_current_slot_validator(slot_validator.account_id.clone());

    UintAuthorityId::set_all_keys(vec![UintAuthorityId(slot_validator.account_id)]);

    return LocalContext {
        current_block,
        slot_number,
        slot_validator,
        other_validator,
        block_number_for_next_slot,
        grace_period,
        summary_last_block_in_range,
        block_after_grace_period,
        challenge_reason,
        finalised_block_vec,
    }
}

mod advance_slot_locks {
    use super::*;

    fn expire_advance_slot_lock() {
        let lock_name = Summary::get_advance_slot_lock_name(Summary::current_slot());
        let mut guard = StorageValueRef::persistent(&lock_name);
        guard.clear();
    }

    #[test]
    fn lock_prevents_multiple_calls_for_same_block() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_success_preconditions();
            assert_eq!(true, pool_state.read().transactions.is_empty());

            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            Summary::offchain_worker(context.block_number_for_next_slot);

            // First run succeeds and lock is in place
            assert_eq!(false, pool_state.read().transactions.is_empty());
            let _ = pool_state.write().transactions.pop();
            assert_eq!(true, pool_state.read().transactions.is_empty());

            // We should prevent multiple ocw's from running for the same block
            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            Summary::offchain_worker(context.block_number_for_next_slot);
            assert_eq!(true, pool_state.read().transactions.is_empty());
        });
    }

    #[test]
    fn lock_prevents_multiple_advance_slot_calls_for_same_slot() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_success_preconditions();
            assert_eq!(true, pool_state.read().transactions.is_empty());

            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            Summary::offchain_worker(context.block_number_for_next_slot);

            //First run succeeds and lock is in place
            assert_eq!(false, pool_state.read().transactions.is_empty());
            let _ = pool_state.write().transactions.pop();
            assert_eq!(true, pool_state.read().transactions.is_empty());

            // the lock should prevent duplicate slot advancement calls even for different block
            // numbers
            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            Summary::offchain_worker(context.block_number_for_next_slot + 1);
            assert_eq!(true, pool_state.read().transactions.is_empty());

            expire_advance_slot_lock();

            // Although this is logically wrong, we can see that advance_slot is called
            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            Summary::offchain_worker(context.block_number_for_next_slot + 2);
            assert_eq!(false, pool_state.read().transactions.is_empty());
        });
    }
}

mod record_summary_locks {
    use super::*;

    fn expire_process_summary_lock(last_block_in_range: u64) {
        let lock_name = Summary::create_root_lock_name(last_block_in_range);
        let mut guard = StorageValueRef::persistent(&lock_name);
        guard.clear();
    }

    fn get_call_from_mem_pool(pool_state: &Arc<RwLock<PoolState>>) -> crate::Call<TestRuntime> {
        let tx = pool_state.write().transactions.pop().unwrap();
        let tx = Extrinsic::decode(&mut &*tx).unwrap();
        assert_eq!(tx.signature, None);
        match tx.call {
            mock::RuntimeCall::Summary(inner_tx) => inner_tx,
            _ => unreachable!(),
        }
    }

    fn expected_record_summary_call(context: &Context) -> crate::Call<TestRuntime> {
        let signature = context
            .validator
            .key
            .sign(
                &(
                    &Summary::update_block_number_context(),
                    context.root_hash_h256,
                    context.root_id.ingress_counter,
                    context.last_block_in_range,
                )
                    .encode(),
            )
            .expect("Signature is signed");

        return crate::Call::record_summary_calculation {
            new_block_number: context.last_block_in_range,
            root_hash: context.root_hash_h256,
            ingress_counter: context.root_id.ingress_counter,
            validator: context.validator.clone(),
            signature,
        }
    }

    #[test]
    fn lock_prevents_multiple_process_summary_calls_for_same_range() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();
            UintAuthorityId::set_all_keys(vec![UintAuthorityId(context.validator.account_id)]);

            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            mock_response_of_get_roothash(
                &mut offchain_state.write(),
                context.url_param.clone(),
                Some(context.root_hash_vec.clone()),
            );

            setup_blocks(&context);
            setup_total_ingresses(&context);
            assert!(pool_state.read().transactions.is_empty());

            Summary::offchain_worker(context.current_block_number);

            assert_eq!(false, pool_state.read().transactions.is_empty());

            let record_summary_call = get_call_from_mem_pool(&pool_state);
            let expected_call = expected_record_summary_call(&context);
            assert_eq!(record_summary_call, expected_call);

            let _ = pool_state.write().transactions.pop();
            assert_eq!(true, pool_state.read().transactions.is_empty());

            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            Summary::offchain_worker(context.current_block_number + 1);
            // Lock prevents a duplicate 'process summary' call
            assert_eq!(true, pool_state.read().transactions.is_empty());

            expire_process_summary_lock(context.last_block_in_range);

            // Although this is logically wrong, we can see that process summary is called
            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            Summary::offchain_worker(context.current_block_number + 2);
            assert_eq!(false, pool_state.read().transactions.is_empty());
        });
    }

    #[test]
    fn lock_releases_when_call_fails() {
        let (mut ext, pool_state, offchain_state) = ExtBuilder::build_default()
            .with_validators()
            .for_offchain_worker()
            .as_externality_with_state();

        ext.execute_with(|| {
            let context = setup_context();
            UintAuthorityId::set_all_keys(vec![UintAuthorityId(context.validator.account_id)]);

            setup_blocks(&context);
            setup_total_ingresses(&context);

            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );

            // Fails at the default current block
            let bad_failure_response = b"0".to_vec();
            mock_response_of_get_roothash(
                &mut offchain_state.write(),
                context.url_param.clone(),
                Some(bad_failure_response),
            );
            assert!(pool_state.read().transactions.is_empty());

            Summary::offchain_worker(context.current_block_number);

            // Due to the error caused by "bad_failure_response", there is no transaction
            assert_eq!(true, pool_state.read().transactions.is_empty());

            // Fix the error and try again without reseting the lock
            mock_response_of_get_finalised_block(
                &mut offchain_state.write(),
                &context.finalised_block_vec,
            );
            mock_response_of_get_roothash(
                &mut offchain_state.write(),
                context.url_param.clone(),
                Some(context.root_hash_vec.clone()),
            );

            Summary::offchain_worker(context.current_block_number + 1);

            // This time it works, because the guard is unlocked on error
            assert_eq!(false, pool_state.read().transactions.is_empty());
        });
    }
}
