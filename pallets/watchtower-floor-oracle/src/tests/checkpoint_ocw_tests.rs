// Copyright 2026 Aventus DAO Ltd

//! The collator offchain worker that produces checkpoint votes.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::assert_ok;
use frame_system::RawOrigin;
use sp_runtime::{
    traits::{Dispatchable, ValidateUnsigned},
    transaction_validity::TransactionSource,
};

const ETHEREUM: ExternalChainId = 1;
const BASE: ExternalChainId = 8453;
const SEED_BLOCK: u32 = 100;
const MAX_AGE_SECS: u64 = 600;
const VOTED_BLOCK: u32 = 150;

fn advance_secs(secs: u64) {
    Timestamp::set_timestamp(Timestamp::get() + secs * 1_000);
}

fn next_block() {
    System::set_block_number(System::block_number() + 1);
}

/// Seeds the chain, makes the seed stale and opens a round. Returns the round id.
fn open_round(chain_id: ExternalChainId) -> RoundId {
    assert_ok!(FloorOracle::seed_chain(
        RuntimeOrigin::root(),
        chain_id,
        SEED_BLOCK,
        MAX_AGE_SECS,
        None
    ));
    advance_secs(MAX_AGE_SECS + 1);
    match FloorOracle::do_request_checkpoint(chain_id).expect("chain registered") {
        CheckpointRequest::Pending { round_id, .. } => round_id,
        other => panic!("expected a round to start, got {:?}", other),
    }
}

fn pop_calls(pool: &Arc<RwLock<PoolState>>) -> Vec<RuntimeCall> {
    let txs: Vec<Vec<u8>> = pool.write().transactions.drain(..).collect();
    txs.iter()
        .map(|tx| Extrinsic::decode(&mut &tx[..]).expect("decodes").function)
        .collect()
}

fn run_ocw() {
    FloorOracle::offchain_worker(System::block_number());
}

fn assert_vote(call: &RuntimeCall, chain: ExternalChainId, round: RoundId, block: u32) {
    match call {
        RuntimeCall::FloorOracle(
            inner @ Call::submit_checkpoint_vote {
                chain_id, round_id, author, block_number, ..
            },
        ) => {
            assert_eq!((*chain_id, *round_id, *block_number), (chain, round, block));
            assert_eq!(author.account_id, acc(1));
            assert_ok!(FloorOracle::validate_unsigned(TransactionSource::External, inner));
        },
        other => panic!("unexpected call {:?}", other),
    }
}

#[test]
fn a_validator_votes_once_per_open_round() {
    let (mut ext, pool, offchain) =
        ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
    ext.execute_with(|| {
        set_local_validator(1);
        let eth_round = open_round(ETHEREUM);
        let base_round = open_round(BASE);

        // Requests are served in the order the worker walks the rounds.
        for (chain_id, _) in FloorOracle::active_rounds() {
            mock_checkpoint_service(
                &mut offchain.write(),
                chain_id,
                Some(encoded_block(VOTED_BLOCK)),
            );
        }
        run_ocw();

        let calls = pop_calls(&pool);
        assert_eq!(calls.len(), 2, "one vote per round");
        for call in &calls {
            let (chain, round) = match call {
                RuntimeCall::FloorOracle(Call::submit_checkpoint_vote {
                    chain_id,
                    round_id,
                    ..
                }) => (*chain_id, *round_id),
                other => panic!("unexpected call {:?}", other),
            };
            let expected_round = if chain == ETHEREUM { eth_round } else { base_round };
            assert_vote(call, chain, expected_round, VOTED_BLOCK);
            assert_ok!(call.clone().dispatch(RawOrigin::None.into()));
        }
        assert!(FloorOracle::author_has_voted(ETHEREUM, eth_round, &acc(1)));
        assert!(FloorOracle::author_has_voted(BASE, base_round, &acc(1)));

        // Same block again: the per-block lock stops it. Next block: already voted.
        run_ocw();
        assert!(pool.read().transactions.is_empty());
        next_block();
        run_ocw();
        assert!(pool.read().transactions.is_empty());
    });
}

#[test]
fn a_service_block_past_the_vote_bound_is_voted_at_the_bound() {
    let (mut ext, pool, offchain) =
        ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
    ext.execute_with(|| {
        set_local_validator(1);
        assert_ok!(FloorOracle::seed_chain(
            RuntimeOrigin::root(),
            ETHEREUM,
            SEED_BLOCK,
            MAX_AGE_SECS,
            Some(12_000)
        ));
        advance_secs(MAX_AGE_SECS + 1);
        let round = match FloorOracle::do_request_checkpoint(ETHEREUM).expect("chain registered") {
            CheckpointRequest::Pending { round_id, .. } => round_id,
            other => panic!("expected a round to start, got {:?}", other),
        };
        let bound = FloorOracle::max_checkpoint_vote(ETHEREUM).expect("block time set");
        assert!(bound < 1_000_000);

        mock_checkpoint_service(&mut offchain.write(), ETHEREUM, Some(encoded_block(1_000_000)));
        run_ocw();

        let calls = pop_calls(&pool);
        assert_eq!(calls.len(), 1);
        assert_vote(&calls[0], ETHEREUM, round, bound);
        assert_ok!(calls[0].clone().dispatch(RawOrigin::None.into()));
    });
}

#[test]
fn a_node_without_a_validator_key_does_nothing() {
    let (mut ext, pool, _offchain) =
        ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
    ext.execute_with(|| {
        clear_local_validator();
        open_round(ETHEREUM);
        run_ocw();
        assert!(pool.read().transactions.is_empty());
    });
}

#[test]
fn no_second_submission_while_the_first_is_pending() {
    let (mut ext, pool, offchain) =
        ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
    ext.execute_with(|| {
        set_local_validator(1);
        open_round(ETHEREUM);
        mock_checkpoint_service(&mut offchain.write(), ETHEREUM, Some(encoded_block(VOTED_BLOCK)));
        run_ocw();
        assert_eq!(pop_calls(&pool).len(), 1);

        // The vote was not included yet; the next block makes no request and no submission.
        next_block();
        run_ocw();
        assert!(pool.read().transactions.is_empty());
    });
}

#[test]
fn unusable_service_blocks_are_skipped() {
    let (mut ext, pool, offchain) =
        ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
    ext.execute_with(|| {
        set_local_validator(1);
        open_round(ETHEREUM);

        mock_checkpoint_service(
            &mut offchain.write(),
            ETHEREUM,
            Some(encoded_block(SEED_BLOCK - 1)),
        );
        run_ocw();
        assert!(pool.read().transactions.is_empty(), "below the current checkpoint");

        next_block();
        mock_checkpoint_service(&mut offchain.write(), ETHEREUM, Some(encoded_block(0)));
        run_ocw();
        assert!(pool.read().transactions.is_empty(), "zero");

        next_block();
        mock_checkpoint_service(&mut offchain.write(), ETHEREUM, Some(b"garbage".to_vec()));
        run_ocw();
        assert!(pool.read().transactions.is_empty(), "undecodable");

        // Reconfirming the current checkpoint height is allowed.
        next_block();
        mock_checkpoint_service(&mut offchain.write(), ETHEREUM, Some(encoded_block(SEED_BLOCK)));
        run_ocw();
        assert_eq!(pop_calls(&pool).len(), 1);
    });
}

#[test]
fn identical_votes_settle_the_round() {
    let (mut ext, pool, offchain) =
        ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
    ext.execute_with(|| {
        set_local_validator(1);
        let round_id = open_round(ETHEREUM);
        mock_checkpoint_service(&mut offchain.write(), ETHEREUM, Some(encoded_block(VOTED_BLOCK)));
        run_ocw();
        let calls = pop_calls(&pool);
        assert_eq!(calls.len(), 1);
        assert_ok!(calls[0].clone().dispatch(RawOrigin::None.into()));

        // Other collators vote the same number, as their own workers would.
        for i in 2..=4 {
            assert_ok!(VoteContext::new(ETHEREUM, round_id, i, VOTED_BLOCK).submit());
        }
        let checkpoint = FloorOracle::checkpoints(ETHEREUM).expect("agreed");
        assert_eq!((checkpoint.block_number, checkpoint.round_id), (VOTED_BLOCK, round_id));
        assert!(FloorOracle::checkpoint_round(ETHEREUM).is_none());
        assert!(FloorOracle::active_rounds().is_empty());

        // Nothing left to vote on.
        next_block();
        run_ocw();
        assert!(pool.read().transactions.is_empty());
    });
}

#[test]
fn a_restarted_round_gets_a_fresh_vote() {
    let (mut ext, pool, offchain) =
        ExtBuilder::build_default().for_offchain_worker().as_externality_with_state();
    ext.execute_with(|| {
        set_local_validator(1);
        let first = open_round(ETHEREUM);
        mock_checkpoint_service(&mut offchain.write(), ETHEREUM, Some(encoded_block(VOTED_BLOCK)));
        run_ocw();
        let calls = pop_calls(&pool);
        assert_vote(&calls[0], ETHEREUM, first, VOTED_BLOCK);
        assert_ok!(calls[0].clone().dispatch(RawOrigin::None.into()));

        // Nobody else votes; the round times out and the next request restarts it.
        System::set_block_number(System::block_number() + ROUND_TIMEOUT_BLOCKS + 1);
        let second = match FloorOracle::do_request_checkpoint(ETHEREUM).unwrap() {
            CheckpointRequest::Pending { round_id, .. } => round_id,
            other => panic!("expected a restarted round, got {:?}", other),
        };
        assert_eq!(second, first + 1);

        mock_checkpoint_service(&mut offchain.write(), ETHEREUM, Some(encoded_block(VOTED_BLOCK)));
        run_ocw();
        let calls = pop_calls(&pool);
        assert_eq!(calls.len(), 1);
        assert_vote(&calls[0], ETHEREUM, second, VOTED_BLOCK);
    });
}

#[test]
fn the_request_path_names_the_chain() {
    assert_eq!(FloorOracle::checkpoint_service_path(8453), "checkpoint/block/8453");
}
