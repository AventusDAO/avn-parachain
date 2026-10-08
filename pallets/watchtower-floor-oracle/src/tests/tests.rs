// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::{
    testing::UintAuthorityId,
    traits::ValidateUnsigned,
    transaction_validity::{InvalidTransaction, TransactionSource},
};

const ETHEREUM: ExternalChainId = 1;
const BASE: ExternalChainId = 8453;
const SEED_BLOCK: u32 = 100;
const MAX_AGE_SECS: u64 = 600;

fn seed(chain_id: ExternalChainId) {
    assert_ok!(FloorOracle::seed_chain(RuntimeOrigin::root(), chain_id, SEED_BLOCK, MAX_AGE_SECS));
}

fn now_secs() -> u64 {
    Timestamp::get() / 1_000
}

fn advance_secs(secs: u64) {
    Timestamp::set_timestamp(Timestamp::get() + secs * 1_000);
}

fn advance_blocks(blocks: BlockNumber) {
    System::set_block_number(System::block_number() + blocks);
}

/// Seed the chain, make the seed stale and start a round. Returns the round id.
fn seed_and_start_round(chain_id: ExternalChainId) -> RoundId {
    seed(chain_id);
    advance_secs(MAX_AGE_SECS + 1);
    match FloorOracle::do_request_checkpoint(chain_id).expect("chain registered") {
        CheckpointRequest::Pending { round_id, .. } => round_id,
        other => panic!("expected a round to start, got {:?}", other),
    }
}

fn last_event() -> RuntimeEvent {
    System::events().pop().expect("at least one event").event
}

fn assert_event(event: Event<TestRuntime>) {
    assert!(
        System::events()
            .iter()
            .any(|record| record.event == RuntimeEvent::FloorOracle(event.clone())),
        "event {:?} not found in {:?}",
        event,
        System::events()
    );
}

fn validate(call: &Call<TestRuntime>) -> Result<(), InvalidTransaction> {
    FloorOracle::validate_unsigned(TransactionSource::External, call)
        .map(|_| ())
        .map_err(|e| match e {
            sp_runtime::transaction_validity::TransactionValidityError::Invalid(i) => i,
            other => panic!("unexpected validity error {:?}", other),
        })
}

mod seed_chain {
    use super::*;

    #[test]
    fn requires_root() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_noop!(
                FloorOracle::seed_chain(
                    RuntimeOrigin::signed(1),
                    ETHEREUM,
                    SEED_BLOCK,
                    MAX_AGE_SECS
                ),
                sp_runtime::DispatchError::BadOrigin
            );
            assert_noop!(
                FloorOracle::seed_chain(RuntimeOrigin::none(), ETHEREUM, SEED_BLOCK, MAX_AGE_SECS),
                sp_runtime::DispatchError::BadOrigin
            );
        });
    }

    #[test]
    fn rejects_zero_freshness_limit() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_noop!(
                FloorOracle::seed_chain(RuntimeOrigin::root(), ETHEREUM, SEED_BLOCK, 0),
                Error::<TestRuntime>::InvalidFreshnessLimit
            );
        });
    }

    #[test]
    fn seeds_checkpoint_once() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed(ETHEREUM);

            assert_eq!(FloorOracle::max_checkpoint_age(ETHEREUM), Some(MAX_AGE_SECS));
            assert_eq!(
                FloorOracle::checkpoints(ETHEREUM),
                Some(Checkpoint { block_number: SEED_BLOCK, agreed_at: now_secs(), round_id: 0 })
            );
            assert_event(Event::FreshnessLimitSet {
                chain_id: ETHEREUM,
                max_checkpoint_age_secs: MAX_AGE_SECS,
            });
            assert_event(Event::CheckpointSeeded {
                chain_id: ETHEREUM,
                block_number: SEED_BLOCK,
                agreed_at: now_secs(),
            });

            // A second call changes the limit but keeps the checkpoint.
            advance_secs(5);
            assert_ok!(FloorOracle::seed_chain(RuntimeOrigin::root(), ETHEREUM, 999, 30));
            assert_eq!(FloorOracle::max_checkpoint_age(ETHEREUM), Some(30));
            assert_eq!(FloorOracle::checkpoints(ETHEREUM).unwrap().block_number, SEED_BLOCK);
        });
    }

    #[test]
    fn chains_are_independent() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed(ETHEREUM);
            assert_ok!(FloorOracle::seed_chain(RuntimeOrigin::root(), BASE, 7, 60));

            assert_eq!(FloorOracle::checkpoints(ETHEREUM).unwrap().block_number, SEED_BLOCK);
            assert_eq!(FloorOracle::checkpoints(BASE).unwrap().block_number, 7);
            assert_eq!(FloorOracle::max_checkpoint_age(BASE), Some(60));
        });
    }
}

mod request_checkpoint {
    use super::*;

    #[test]
    fn unregistered_chain_fails() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_noop!(
                FloorOracle::do_request_checkpoint(ETHEREUM),
                Error::<TestRuntime>::ChainNotRegistered
            );
            assert_noop!(
                FloorOracle::request_checkpoint(RuntimeOrigin::root(), ETHEREUM),
                Error::<TestRuntime>::ChainNotRegistered
            );
        });
    }

    #[test]
    fn fresh_checkpoint_is_reused() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed(ETHEREUM);
            let seeded = FloorOracle::checkpoints(ETHEREUM).unwrap();

            advance_secs(MAX_AGE_SECS); // exactly at the limit is still fresh
            assert_eq!(
                FloorOracle::do_request_checkpoint(ETHEREUM),
                Ok(CheckpointRequest::Ready(seeded.clone()))
            );
            assert_eq!(FloorOracle::next_round_id(), 0);
            assert!(FloorOracle::checkpoint_round(ETHEREUM).is_none());

            assert_ok!(FloorOracle::request_checkpoint(RuntimeOrigin::root(), ETHEREUM));
            assert_eq!(
                last_event(),
                RuntimeEvent::FloorOracle(Event::CheckpointRequestServed {
                    chain_id: ETHEREUM,
                    ready: true,
                    round_id: 0
                })
            );
        });
    }

    #[test]
    fn stale_checkpoint_starts_a_round() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            seed(ETHEREUM);
            let seeded = FloorOracle::checkpoints(ETHEREUM).unwrap();
            advance_secs(MAX_AGE_SECS + 1);

            assert_eq!(
                FloorOracle::do_request_checkpoint(ETHEREUM),
                Ok(CheckpointRequest::Pending {
                    round_id: 1,
                    started_at: System::block_number(),
                    stale: Some(seeded.clone())
                })
            );
            assert_event(Event::CheckpointRoundStarted { chain_id: ETHEREUM, round_id: 1 });

            let round = FloorOracle::checkpoint_round(ETHEREUM).expect("round open");
            assert_eq!(round.round_id, 1);
            assert_eq!(round.started_at, System::block_number());
            assert!(round.votes.is_empty());
            assert_eq!(FloorOracle::next_round_id(), 1);
            // The stale checkpoint stays in place until the round agrees.
            assert_eq!(FloorOracle::checkpoints(ETHEREUM), Some(seeded));
        });
    }

    #[test]
    fn requests_during_a_round_share_it() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round_id = seed_and_start_round(ETHEREUM);
            let started_at = System::block_number();

            advance_blocks(ROUND_TIMEOUT_BLOCKS); // exactly at the timeout is still the same round
            let result = FloorOracle::do_request_checkpoint(ETHEREUM).unwrap();
            assert_eq!(
                result,
                CheckpointRequest::Pending {
                    round_id,
                    started_at,
                    stale: FloorOracle::checkpoints(ETHEREUM)
                }
            );
            assert_eq!(FloorOracle::next_round_id(), round_id);

            assert_ok!(FloorOracle::request_checkpoint(RuntimeOrigin::root(), ETHEREUM));
            assert_eq!(
                last_event(),
                RuntimeEvent::FloorOracle(Event::CheckpointRequestServed {
                    chain_id: ETHEREUM,
                    ready: false,
                    round_id
                })
            );
        });
    }

    #[test]
    fn timed_out_round_is_restarted() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let old_round = seed_and_start_round(ETHEREUM);
            assert_ok!(VoteContext::new(ETHEREUM, old_round, 1, 500).submit());

            advance_blocks(ROUND_TIMEOUT_BLOCKS + 1);
            let result = FloorOracle::do_request_checkpoint(ETHEREUM).unwrap();
            let new_round = old_round + 1;
            assert_eq!(
                result,
                CheckpointRequest::Pending {
                    round_id: new_round,
                    started_at: System::block_number(),
                    stale: FloorOracle::checkpoints(ETHEREUM)
                }
            );
            assert_event(Event::CheckpointRoundRestarted {
                chain_id: ETHEREUM,
                old_round_id: old_round,
                new_round_id: new_round,
            });

            let round = FloorOracle::checkpoint_round(ETHEREUM).unwrap();
            assert_eq!(round.round_id, new_round);
            assert!(round.votes.is_empty(), "votes of the old round are dropped");

            // A vote for the old round is now rejected, in the pool and in a block.
            let stale_vote = VoteContext::new(ETHEREUM, old_round, 2, 500);
            assert_eq!(
                validate(&stale_vote.call()),
                Err(InvalidTransaction::Custom(VOTE_NO_MATCHING_ROUND))
            );
            assert_noop!(stale_vote.submit(), Error::<TestRuntime>::RoundMismatch);
        });
    }

    #[test]
    fn rounds_of_different_chains_do_not_interfere() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let eth_round = seed_and_start_round(ETHEREUM);
            assert_ok!(FloorOracle::seed_chain(RuntimeOrigin::root(), BASE, 7, 60));
            advance_secs(61);
            let base_round = match FloorOracle::do_request_checkpoint(BASE).unwrap() {
                CheckpointRequest::Pending { round_id, .. } => round_id,
                other => panic!("expected pending, got {:?}", other),
            };

            assert_ne!(eth_round, base_round);
            assert_eq!(FloorOracle::checkpoint_round(ETHEREUM).unwrap().round_id, eth_round);
            assert_eq!(FloorOracle::checkpoint_round(BASE).unwrap().round_id, base_round);
            let mut active = FloorOracle::active_rounds();
            active.sort();
            assert_eq!(active, vec![(ETHEREUM, eth_round), (BASE, base_round)]);
        });
    }
}

mod submit_checkpoint_vote {
    use super::*;

    #[test]
    fn records_a_vote_below_threshold() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round_id = seed_and_start_round(ETHEREUM);
            let vote = VoteContext::new(ETHEREUM, round_id, 1, 500);

            assert_ok!(validate(&vote.call()));
            assert_ok!(vote.submit());

            let round = FloorOracle::checkpoint_round(ETHEREUM).unwrap();
            assert_eq!(round.votes.get(&1), Some(&500));
            assert!(FloorOracle::author_has_voted(ETHEREUM, round_id, &1));
            assert!(!FloorOracle::author_has_voted(ETHEREUM, round_id, &2));
            assert_eq!(FloorOracle::checkpoints(ETHEREUM).unwrap().round_id, 0, "unchanged");
        });
    }

    #[test]
    fn agrees_with_eth_bridge_selection_on_raw_heights() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round_id = seed_and_start_round(ETHEREUM);
            advance_secs(10);
            let supermajority = <TestRuntime as Config>::Quorum::get_supermajority_quorum();
            assert_eq!(supermajority, 4);

            // Validators 1..=4 vote 100, 200, 300, 400. No rounding is applied.
            for account in 1..=supermajority as AccountId {
                let vote = VoteContext::new(ETHEREUM, round_id, account, account as u32 * 100);
                assert_ok!(vote.submit());
            }

            let agreed_at = now_secs();
            assert_eq!(
                FloorOracle::checkpoints(ETHEREUM),
                Some(Checkpoint { block_number: 300, agreed_at, round_id })
            );
            assert!(FloorOracle::checkpoint_round(ETHEREUM).is_none());
            assert!(FloorOracle::active_rounds().is_empty());
            assert_event(Event::CheckpointAgreed {
                chain_id: ETHEREUM,
                round_id,
                block_number: 300,
                agreed_at,
            });

            // Agreement makes the checkpoint fresh again.
            assert_eq!(
                FloorOracle::do_request_checkpoint(ETHEREUM),
                Ok(CheckpointRequest::Ready(Checkpoint { block_number: 300, agreed_at, round_id }))
            );

            // Late votes are rejected both at the pool and in a block.
            let late = VoteContext::new(ETHEREUM, round_id, 5, 500);
            assert_eq!(
                validate(&late.call()),
                Err(InvalidTransaction::Custom(VOTE_NO_MATCHING_ROUND))
            );
            assert_noop!(late.submit(), Error::<TestRuntime>::NoActiveRound);
        });
    }

    #[test]
    fn checkpoint_of_a_round_is_never_changed() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let first_round = seed_and_start_round(ETHEREUM);
            for account in 1..=4 {
                assert_ok!(VoteContext::new(ETHEREUM, first_round, account, 1_000).submit());
            }
            let first = FloorOracle::checkpoints(ETHEREUM).unwrap();
            assert_eq!(
                first,
                Checkpoint { block_number: 1_000, agreed_at: now_secs(), round_id: first_round }
            );

            // Make it stale and run a second round with different values.
            advance_secs(MAX_AGE_SECS + 1);
            let second_round = match FloorOracle::do_request_checkpoint(ETHEREUM).unwrap() {
                CheckpointRequest::Pending { round_id, stale, .. } => {
                    assert_eq!(stale, Some(first.clone()));
                    round_id
                },
                other => panic!("expected pending, got {:?}", other),
            };
            assert_eq!(second_round, first_round + 1);
            for account in 1..=4 {
                assert_ok!(VoteContext::new(ETHEREUM, second_round, account, 2_000).submit());
            }

            let second = FloorOracle::checkpoints(ETHEREUM).unwrap();
            assert_eq!(
                second,
                Checkpoint { block_number: 2_000, agreed_at: now_secs(), round_id: second_round }
            );
            assert_ne!(second.agreed_at, first.agreed_at);
        });
    }

    #[test]
    fn single_validator_resolves_on_first_vote() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_validators(&[1]);
            let round_id = seed_and_start_round(ETHEREUM);

            assert_ok!(VoteContext::new(ETHEREUM, round_id, 1, 777).submit());

            assert_eq!(FloorOracle::checkpoints(ETHEREUM).unwrap().block_number, 777);
            assert!(FloorOracle::checkpoint_round(ETHEREUM).is_none());
        });
    }

    #[test]
    fn votes_of_removed_validators_are_pruned() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let round_id = seed_and_start_round(ETHEREUM);
            for account in 1..=2 {
                assert_ok!(VoteContext::new(ETHEREUM, round_id, account, 500).submit());
            }

            // Validator 1 leaves. With 5 validators the supermajority is 3. Validator 1's vote
            // no longer counts, so after validator 3 votes only 2 votes exist.
            set_validators(&[2, 3, 4, 5, 6]);
            assert_eq!(<TestRuntime as Config>::Quorum::get_supermajority_quorum(), 3);
            assert_ok!(VoteContext::new(ETHEREUM, round_id, 3, 500).submit());

            let round = FloorOracle::checkpoint_round(ETHEREUM).expect("still open");
            assert!(!round.votes.contains_key(&1));
            assert_eq!(round.votes.len(), 2);
            assert_eq!(FloorOracle::checkpoints(ETHEREUM).unwrap().round_id, 0);

            // The removed validator cannot vote again either.
            assert_noop!(
                VoteContext::new(ETHEREUM, round_id, 1, 500).submit(),
                Error::<TestRuntime>::NotAValidator
            );

            // The third live vote meets the supermajority.
            assert_ok!(VoteContext::new(ETHEREUM, round_id, 4, 500).submit());
            assert_eq!(FloorOracle::checkpoints(ETHEREUM).unwrap().block_number, 500);
            assert!(FloorOracle::checkpoint_round(ETHEREUM).is_none());
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn no_round_is_open() {
            ExtBuilder::build_default().as_externality().execute_with(|| {
                seed(ETHEREUM);
                let vote = VoteContext::new(ETHEREUM, 1, 1, 500);
                assert_eq!(
                    validate(&vote.call()),
                    Err(InvalidTransaction::Custom(VOTE_NO_MATCHING_ROUND))
                );
                assert_noop!(vote.submit(), Error::<TestRuntime>::NoActiveRound);
            });
        }

        #[test]
        fn round_id_does_not_match() {
            ExtBuilder::build_default().as_externality().execute_with(|| {
                let round_id = seed_and_start_round(ETHEREUM);
                let vote = VoteContext::new(ETHEREUM, round_id + 1, 1, 500);
                assert_eq!(
                    validate(&vote.call()),
                    Err(InvalidTransaction::Custom(VOTE_NO_MATCHING_ROUND))
                );
                assert_noop!(vote.submit(), Error::<TestRuntime>::RoundMismatch);
            });
        }

        #[test]
        fn author_votes_twice() {
            ExtBuilder::build_default().as_externality().execute_with(|| {
                let round_id = seed_and_start_round(ETHEREUM);
                assert_ok!(VoteContext::new(ETHEREUM, round_id, 1, 500).submit());

                let again = VoteContext::new(ETHEREUM, round_id, 1, 600);
                assert_eq!(
                    validate(&again.call()),
                    Err(InvalidTransaction::Custom(VOTE_ALREADY_CAST))
                );
                assert_noop!(again.submit(), Error::<TestRuntime>::DuplicateVote);
            });
        }

        #[test]
        fn author_is_not_a_validator() {
            ExtBuilder::build_default().as_externality().execute_with(|| {
                let round_id = seed_and_start_round(ETHEREUM);
                let vote = VoteContext::new(ETHEREUM, round_id, 99, 500);
                assert_eq!(
                    validate(&vote.call()),
                    Err(InvalidTransaction::Custom(VOTE_BAD_SIGNATURE))
                );
                assert_noop!(vote.submit(), Error::<TestRuntime>::NotAValidator);
            });
        }

        #[test]
        fn signature_is_invalid() {
            ExtBuilder::build_default().as_externality().execute_with(|| {
                let round_id = seed_and_start_round(ETHEREUM);
                let vote = VoteContext::new(ETHEREUM, round_id, 1, 500);

                // Signed by another validator's key.
                let forged = Call::<TestRuntime>::submit_checkpoint_vote {
                    chain_id: ETHEREUM,
                    round_id,
                    author: vote.author.clone(),
                    block_number: 500,
                    signature: VoteContext::new(ETHEREUM, round_id, 2, 500).signature(),
                };
                assert_eq!(validate(&forged), Err(InvalidTransaction::Custom(VOTE_BAD_SIGNATURE)));

                // Signed over a different block number.
                let tampered = Call::<TestRuntime>::submit_checkpoint_vote {
                    chain_id: ETHEREUM,
                    round_id,
                    author: vote.author.clone(),
                    block_number: 501,
                    signature: vote.signature(),
                };
                assert_eq!(
                    validate(&tampered),
                    Err(InvalidTransaction::Custom(VOTE_BAD_SIGNATURE))
                );

                // The key sent in the call is ignored; the registered key is used.
                let wrong_key = Call::<TestRuntime>::submit_checkpoint_vote {
                    chain_id: ETHEREUM,
                    round_id,
                    author: Author::<TestRuntime> { account_id: 1, key: UintAuthorityId(2) },
                    block_number: 500,
                    signature: vote.signature(),
                };
                assert_ok!(validate(&wrong_key));
            });
        }

        #[test]
        fn origin_is_signed() {
            ExtBuilder::build_default().as_externality().execute_with(|| {
                let round_id = seed_and_start_round(ETHEREUM);
                let vote = VoteContext::new(ETHEREUM, round_id, 1, 500);
                assert_noop!(
                    FloorOracle::submit_checkpoint_vote(
                        RuntimeOrigin::signed(1),
                        ETHEREUM,
                        round_id,
                        vote.author.clone(),
                        500,
                        vote.signature()
                    ),
                    sp_runtime::DispatchError::BadOrigin
                );
            });
        }
    }
}

#[test]
fn other_calls_are_not_valid_unsigned() {
    ExtBuilder::build_default().as_externality().execute_with(|| {
        let call = Call::<TestRuntime>::request_checkpoint { chain_id: ETHEREUM };
        assert_eq!(validate(&call), Err(InvalidTransaction::Call));
    });
}
