#![cfg(test)]

use crate::{mock::*, *};
use parking_lot::RwLock;
use sp_avn_common::{
    event_types::{AvtGrowthLiftedData, EthEventId, LiftedData, ValidEvents},
    EthQueryRequest, EthQueryResponse, EthQueryResponseType, EthTransaction,
};
use sp_watchtower::UNKNOWN_COUNTRY;
use std::sync::Arc;

const PROPOSAL_ID: H256 = H256::repeat_byte(0x01);
const ETH_TX_HASH: H256 = H256::repeat_byte(0xee);
const TOKEN: H160 = H160::repeat_byte(0x11);
const RECEIVER: H256 = H256::repeat_byte(0x33);
const AMOUNT: u128 = 1_000;

fn watchtower() -> AccountId {
    AccountId::from_raw([1u8; 32])
}

fn credited_lift() -> LiftedData {
    LiftedData::new(TOKEN, RECEIVER, AMOUNT)
}

fn lift_event(lift: LiftedData) -> EthEvent {
    EthEvent {
        event_id: EthEventId {
            signature: ValidEvents::Lifted.signature(),
            transaction_hash: ETH_TX_HASH,
        },
        event_data: EventData::LogLifted(lift),
    }
}

/// A lift on T1 as `eth_getTransactionReceipt` shows it.
struct T1Lift {
    status: &'static str,
    contract: H160,
    signature: H256,
    token: H160,
    receiver: H256,
    amount: u128,
}

impl Default for T1Lift {
    fn default() -> Self {
        T1Lift {
            status: "0x1",
            contract: BRIDGE_CONTRACT,
            signature: ValidEvents::Lifted.signature(),
            token: TOKEN,
            receiver: RECEIVER,
            amount: AMOUNT,
        }
    }
}

impl T1Lift {
    fn log(&self) -> String {
        let mut amount = [0u8; 32];
        amount[16..].copy_from_slice(&self.amount.to_be_bytes());
        format!(
            r#"{{
                "address": "{:?}",
                "topics": ["{:?}", "{:?}", "{:?}"],
                "data": "0x{}"
            }}"#,
            self.contract,
            self.signature,
            H256::from(self.token),
            self.receiver,
            hex::encode(amount),
        )
    }

    fn response(&self) -> Vec<u8> {
        query_response(&receipt(self.status, &[self.log()]))
    }
}

fn receipt(status: &str, logs: &[String]) -> String {
    format!(
        r#"{{"transactionHash": "{:?}", "status": "{}", "logs": [{}]}}"#,
        ETH_TX_HASH,
        status,
        logs.join(",")
    )
}

fn query_response(receipt: &str) -> Vec<u8> {
    hex::encode(
        EthQueryResponse { data: receipt.as_bytes().to_vec().encode(), num_confirmations: 20 }
            .encode(),
    )
    .into_bytes()
}

fn expect_receipt_request(state: &mut OffchainState, response: Vec<u8>) {
    let query = EthQueryRequest::new(ETH_TX_HASH, EthQueryResponseType::TransactionReceipt);
    state.expect_request(PendingRequest {
        method: "POST".into(),
        uri: "http://127.0.0.1:2020/eth/query".into(),
        body: EthTransaction::new([0; 32], BRIDGE_CONTRACT, query.encode()).encode(),
        response: Some(response),
        sent: true,
        ..Default::default()
    });
}

/// The node cannot tell its country, so its votes report it as unknown.
fn expect_country_request(state: &mut OffchainState) {
    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: "http://127.0.0.1:2020/node/country".into(),
        response: Some(vec![]),
        sent: true,
        ..Default::default()
    });
}

/// Runs the node as a watchtower with a key it can sign with.
fn run_as_watchtower() -> SignerId {
    let key = SignerId::generate_pair(None);
    set_local_watchtower(Some((watchtower(), key.clone())));
    key
}

fn start_check(event: EthEvent) {
    ActiveLiftCheck::<TestRuntime>::put((PROPOSAL_ID, event));
}

/// The votes the node submitted, checking each is signed by `key`.
fn submitted_votes(pool: &Arc<RwLock<PoolState>>, key: &SignerId) -> Vec<bool> {
    pool.read()
        .transactions
        .iter()
        .map(|tx| match Extrinsic::decode(&mut &**tx).unwrap().function {
            RuntimeCall::Watchtower(pallet_watchtower::Call::unsigned_vote {
                proposal_id,
                in_favor,
                country,
                watchtower: voter,
                signature,
            }) => {
                assert_eq!(proposal_id, PROPOSAL_ID);
                assert_eq!(voter, watchtower());
                assert_eq!(country, UNKNOWN_COUNTRY);
                let payload = Watchtower::unsigned_vote_signing_payload(
                    proposal_id,
                    &pallet_watchtower::VoteChoice::Binary(in_favor),
                    &country,
                    &voter,
                );
                assert!(key.verify(&payload, &signature));
                in_favor
            },
            call => panic!("Unexpected call {:?}", call),
        })
        .collect()
}

mod votes {
    use super::*;

    fn assert_votes(t1: Vec<u8>, credited: EthEvent, expected_vote: bool) {
        let (mut ext, offchain, pool) = ExtBuilder::build_default().for_offchain_worker();
        ext.execute_with(|| {
            let key = run_as_watchtower();
            start_check(credited);
            expect_receipt_request(&mut offchain.write(), t1);
            expect_country_request(&mut offchain.write());

            BridgeWatchtower::vote_on_lift_check(1);

            assert_eq!(submitted_votes(&pool, &key), vec![expected_vote]);
        });
    }

    #[test]
    fn in_favour_of_a_lift_that_matches_t1() {
        assert_votes(T1Lift::default().response(), lift_event(credited_lift()), true);
    }

    #[test]
    fn in_favour_when_any_bridge_lift_in_the_transaction_matches() {
        let logs = [
            T1Lift { contract: H160::repeat_byte(0x99), ..Default::default() }.log(),
            T1Lift { amount: 1, ..Default::default() }.log(),
            T1Lift::default().log(),
        ];

        assert_votes(query_response(&receipt("0x1", &logs)), lift_event(credited_lift()), true);
    }

    #[test]
    fn against_a_transaction_missing_from_t1() {
        assert_votes(vec![], lift_event(credited_lift()), false);
    }

    #[test]
    fn against_a_transaction_that_failed_on_t1() {
        let t1 = T1Lift { status: "0x0", ..Default::default() };

        assert_votes(t1.response(), lift_event(credited_lift()), false);
    }

    #[test]
    fn against_a_lift_from_another_contract() {
        let t1 = T1Lift { contract: H160::repeat_byte(0x99), ..Default::default() };

        assert_votes(t1.response(), lift_event(credited_lift()), false);
    }

    #[test]
    fn against_a_transaction_without_a_lift() {
        let t1 = T1Lift { signature: ValidEvents::LowerLocked.signature(), ..Default::default() };

        assert_votes(t1.response(), lift_event(credited_lift()), false);
    }

    #[test]
    fn against_a_lift_with_another_token() {
        let t1 = T1Lift { token: H160::repeat_byte(0x22), ..Default::default() };

        assert_votes(t1.response(), lift_event(credited_lift()), false);
    }

    #[test]
    fn against_a_lift_to_another_receiver() {
        let t1 = T1Lift { receiver: H256::repeat_byte(0x44), ..Default::default() };

        assert_votes(t1.response(), lift_event(credited_lift()), false);
    }

    #[test]
    fn against_a_lift_with_another_amount() {
        for amount in [AMOUNT - 1, AMOUNT + 1] {
            let t1 = T1Lift { amount, ..Default::default() };

            assert_votes(t1.response(), lift_event(credited_lift()), false);
        }
    }

    #[test]
    fn against_a_growth_lift_without_reading_t1() {
        let (mut ext, offchain, pool) = ExtBuilder::build_default().for_offchain_worker();
        ext.execute_with(|| {
            let key = run_as_watchtower();
            start_check(EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::AvtGrowthLifted.signature(),
                    transaction_hash: ETH_TX_HASH,
                },
                event_data: EventData::LogAvtGrowthLifted(AvtGrowthLiftedData {
                    amount: AMOUNT,
                    period: 3,
                }),
            });
            expect_country_request(&mut offchain.write());

            BridgeWatchtower::vote_on_lift_check(1);

            assert_eq!(submitted_votes(&pool, &key), vec![false]);
        });
    }
}

mod does_not_vote {
    use super::*;

    /// Runs the offchain worker as a watchtower with a lift being checked. Any HTTP request it
    /// makes that the test did not expect fails the test.
    fn assert_no_vote(setup: impl FnOnce(&mut OffchainState)) {
        let (mut ext, offchain, pool) = ExtBuilder::build_default().for_offchain_worker();
        ext.execute_with(|| {
            run_as_watchtower();
            start_check(lift_event(credited_lift()));
            setup(&mut offchain.write());

            BridgeWatchtower::vote_on_lift_check(1);

            assert!(pool.read().transactions.is_empty());
        });
    }

    #[test]
    fn when_t1_cannot_be_read() {
        for response in [b"not hex".to_vec(), b"00".to_vec(), query_response("bad")] {
            assert_no_vote(|state| expect_receipt_request(state, response));
        }
    }

    #[test]
    fn when_no_lift_is_being_checked() {
        assert_no_vote(|_| ActiveLiftCheck::<TestRuntime>::kill());
    }

    #[test]
    fn when_the_node_is_not_a_watchtower() {
        assert_no_vote(|_| set_local_watchtower(None));
    }

    #[test]
    fn when_the_node_already_voted() {
        assert_no_vote(|_| {
            pallet_watchtower::Voters::<TestRuntime>::insert(PROPOSAL_ID, watchtower(), true)
        });
    }

    #[test]
    fn when_the_node_is_not_in_the_committee() {
        assert_no_vote(|_| {
            pallet_watchtower::ProposalCommitteeSize::<TestRuntime>::insert(PROPOSAL_ID, 1);
            pallet_watchtower::ProposalCommittee::<TestRuntime>::insert(
                PROPOSAL_ID,
                AccountId::from_raw([2u8; 32]),
                (),
            );
        });
    }
}

#[test]
fn committee_member_votes() {
    let (mut ext, offchain, pool) = ExtBuilder::build_default().for_offchain_worker();
    ext.execute_with(|| {
        let key = run_as_watchtower();
        start_check(lift_event(credited_lift()));
        pallet_watchtower::ProposalCommitteeSize::<TestRuntime>::insert(PROPOSAL_ID, 1);
        pallet_watchtower::ProposalCommittee::<TestRuntime>::insert(PROPOSAL_ID, watchtower(), ());
        expect_receipt_request(&mut offchain.write(), T1Lift::default().response());
        expect_country_request(&mut offchain.write());

        BridgeWatchtower::vote_on_lift_check(1);

        assert_eq!(submitted_votes(&pool, &key), vec![true]);
    });
}

#[test]
fn requested_check_is_voted_on_after_lift_checks_stop() {
    let (mut ext, offchain, pool) = ExtBuilder::build_default().for_offchain_worker();
    ext.execute_with(|| {
        let key = run_as_watchtower();
        start_check(lift_event(credited_lift()));
        LiftCheckingEnabled::<TestRuntime>::put(false);
        expect_receipt_request(&mut offchain.write(), T1Lift::default().response());
        expect_country_request(&mut offchain.write());

        BridgeWatchtower::vote_on_lift_check(1);

        assert_eq!(submitted_votes(&pool, &key), vec![true]);
    });
}

#[test]
fn vote_is_submitted_again_only_after_the_inclusion_period() {
    let (mut ext, offchain, pool) = ExtBuilder::build_default().for_offchain_worker();
    ext.execute_with(|| {
        let key = run_as_watchtower();
        start_check(lift_event(credited_lift()));
        expect_receipt_request(&mut offchain.write(), T1Lift::default().response());
        expect_country_request(&mut offchain.write());
        BridgeWatchtower::vote_on_lift_check(1);

        // Any request would fail the test
        for block in 2..=6 {
            BridgeWatchtower::vote_on_lift_check(block);
        }
        assert_eq!(submitted_votes(&pool, &key), vec![true]);

        expect_receipt_request(&mut offchain.write(), T1Lift::default().response());
        expect_country_request(&mut offchain.write());
        BridgeWatchtower::vote_on_lift_check(7);
        assert_eq!(submitted_votes(&pool, &key), vec![true, true]);
    });
}

#[test]
fn reading_t1_is_retried_on_the_next_block_only() {
    let (mut ext, offchain, pool) = ExtBuilder::build_default().for_offchain_worker();
    ext.execute_with(|| {
        let key = run_as_watchtower();
        start_check(lift_event(credited_lift()));
        expect_receipt_request(&mut offchain.write(), b"not hex".to_vec());
        BridgeWatchtower::vote_on_lift_check(1);

        // The offchain worker already ran for this block
        BridgeWatchtower::vote_on_lift_check(1);
        assert!(pool.read().transactions.is_empty());

        expect_receipt_request(&mut offchain.write(), T1Lift::default().response());
        expect_country_request(&mut offchain.write());
        BridgeWatchtower::vote_on_lift_check(2);
        assert_eq!(submitted_votes(&pool, &key), vec![true]);
    });
}
