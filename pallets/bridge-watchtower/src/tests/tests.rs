#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok, BoundedVec};
use pallet_watchtower::Payload;
use sp_avn_common::{
    eth::concat_lower_data,
    event_types::{AvtGrowthLiftedData, AvtLowerClaimedData, EthEventId, LiftedData, ValidEvents},
};
use sp_runtime::{traits::BadOrigin, Perbill};

fn make_proposal(source: ProposalSource, external_ref: H256) -> Proposal<TestRuntime> {
    Proposal {
        title: BoundedVec::try_from(b"Title".to_vec()).unwrap(),
        payload: Payload::Inline(BoundedVec::try_from(b"payload".to_vec()).unwrap()),
        threshold: Perbill::from_percent(50),
        source,
        decision_rule: DecisionRule::ExpireUnresolved,
        external_ref,
        proposer: None,
        created_at: 1,
        vote_duration: 10,
        end_at: None,
        committee_size: None,
    }
}

#[test]
fn hooks_ignore_other_proposals() {
    ExtBuilder::build_default().as_externality().execute_with(|| {
        let proposal_id = H256::repeat_byte(0x01);
        let external_ref = H256::repeat_byte(0xaa);

        for source in [
            ProposalSource::Internal(ProposalType::Summary),
            ProposalSource::Internal(ProposalType::Lift),
            ProposalSource::External,
        ] {
            assert_ok!(BridgeWatchtower::on_proposal_submitted(
                proposal_id,
                make_proposal(source, external_ref)
            ));
        }
        assert_eq!(ActiveLiftCheck::<TestRuntime>::get(), None);

        BridgeWatchtower::on_voting_completed(
            proposal_id,
            &external_ref,
            &ProposalStatusEnum::Resolved { passed: false },
        );
        BridgeWatchtower::on_cancelled(proposal_id, &external_ref);
        assert!(!lowering_disabled());
        assert!(System::events().is_empty());
    });
}

const LOWER_ID: LowerId = 7;
const ETH_TX_HASH: H256 = H256::repeat_byte(0xee);

fn locked_lower(lower_id: LowerId) -> LowerLockedData {
    LowerLockedData {
        lower_id,
        t1_recipient: H160::repeat_byte(0x22),
        token_contract: H160::repeat_byte(0x11),
        amount: U256::from(1_000u32),
        t2_sender: H256::repeat_byte(0xaa),
        unlock_time: 1_700_000_000,
    }
}

/// Records on T2 an unclaimed lower with the same values as `lock`
fn issue_lower(lock: &LowerLockedData) {
    let params = concat_lower_data(
        lock.lower_id,
        lock.token_contract,
        &lock.amount.as_u128(),
        &lock.t1_recipient,
        lock.t2_sender,
        1_600_000_000,
    );
    add_unclaimed_lower(lock.lower_id, params);
}

fn lock_event(lock: LowerLockedData) -> EthEvent {
    EthEvent {
        event_id: EthEventId {
            signature: ValidEvents::LowerLocked.signature(),
            transaction_hash: ETH_TX_HASH,
        },
        event_data: EventData::LogLowerLocked(lock),
    }
}

fn freeze_tx(lower_id: LowerId) -> PublishedTx {
    (
        b"freezeLower".to_vec(),
        vec![(b"uint32".to_vec(), lower_id.to_string().into_bytes())],
        PALLET_ID.to_vec(),
    )
}

fn mismatch_event(lock: &LowerLockedData) -> RuntimeEvent {
    RuntimeEvent::BridgeWatchtower(crate::Event::LowerLockMismatch {
        lower_id: lock.lower_id,
        t1_recipient: lock.t1_recipient,
        token_contract: lock.token_contract,
        amount: lock.amount,
        t2_sender: lock.t2_sender,
        unlock_time: lock.unlock_time,
        eth_tx_hash: ETH_TX_HASH,
    })
}

/// Processes a mismatched lock of `LOWER_ID`, which requests a freeze in tx 1
fn request_freeze_on_mismatch() {
    assert_ok!(BridgeWatchtower::on_incoming_event_processed(&lock_event(locked_lower(LOWER_ID))));
    assert_eq!(LowerIdToFreezeTxId::<TestRuntime>::get(LOWER_ID), Some(1));
}

mod lower_locked {
    use super::*;

    #[test]
    fn matching_lock_is_verified() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let lock = locked_lower(LOWER_ID);
            issue_lower(&lock);

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&lock_event(lock)));

            System::assert_last_event(RuntimeEvent::BridgeWatchtower(
                crate::Event::LowerLockVerified { lower_id: LOWER_ID, eth_tx_hash: ETH_TX_HASH },
            ));
            assert!(published().is_empty());
            assert!(!LowerIdToFreezeTxId::<TestRuntime>::contains_key(LOWER_ID));
        });
    }

    fn assert_mismatch_is_frozen(issued: LowerLockedData, locked: LowerLockedData) {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            issue_lower(&issued);

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&lock_event(locked.clone())));

            System::assert_has_event(mismatch_event(&locked));
            System::assert_last_event(RuntimeEvent::BridgeWatchtower(
                crate::Event::LowerFreezeRequested { lower_id: locked.lower_id, tx_id: 1 },
            ));
            assert_eq!(published(), vec![freeze_tx(locked.lower_id)]);
            assert_eq!(LowerIdToFreezeTxId::<TestRuntime>::get(locked.lower_id), Some(1));
            assert_eq!(FreezeTxIdToLowerId::<TestRuntime>::get(1), Some(locked.lower_id));
            assert!(!FailedFreezes::<TestRuntime>::contains_key(locked.lower_id));
        });
    }

    #[test]
    fn lock_of_an_unknown_lower_is_frozen() {
        let lock = locked_lower(LOWER_ID);
        assert_mismatch_is_frozen(locked_lower(LOWER_ID + 1), lock);
    }

    #[test]
    fn lock_with_another_token_is_frozen() {
        let issued = locked_lower(LOWER_ID);
        let locked = LowerLockedData { token_contract: H160::repeat_byte(0x99), ..issued.clone() };
        assert_mismatch_is_frozen(issued, locked);
    }

    #[test]
    fn lock_with_another_amount_is_frozen() {
        let issued = locked_lower(LOWER_ID);
        let locked = LowerLockedData { amount: issued.amount + 1, ..issued.clone() };
        assert_mismatch_is_frozen(issued, locked);
    }

    #[test]
    fn lock_with_an_amount_above_u128_is_frozen() {
        let issued = LowerLockedData { amount: U256::from(u128::MAX), ..locked_lower(LOWER_ID) };
        let locked = LowerLockedData { amount: U256::from(u128::MAX) + 1, ..issued.clone() };
        assert_mismatch_is_frozen(issued, locked);
    }

    #[test]
    fn lock_with_another_recipient_is_frozen() {
        let issued = locked_lower(LOWER_ID);
        let locked = LowerLockedData { t1_recipient: H160::repeat_byte(0x99), ..issued.clone() };
        assert_mismatch_is_frozen(issued, locked);
    }

    #[test]
    fn lock_with_another_t2_sender_is_frozen() {
        let issued = locked_lower(LOWER_ID);
        let locked = LowerLockedData { t2_sender: H256::repeat_byte(0x99), ..issued.clone() };
        assert_mismatch_is_frozen(issued, locked);
    }

    #[test]
    fn failed_publish_is_flagged_and_reverted() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_publish_fails(true);
            let lock = locked_lower(LOWER_ID);

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&lock_event(lock.clone())));

            System::assert_has_event(mismatch_event(&lock));
            System::assert_last_event(RuntimeEvent::BridgeWatchtower(
                crate::Event::LowerFreezePublishFailed { lower_id: LOWER_ID },
            ));
            assert!(FailedFreezes::<TestRuntime>::contains_key(LOWER_ID));
            assert!(!LowerIdToFreezeTxId::<TestRuntime>::contains_key(LOWER_ID));
            assert!(frame_support::storage::unhashed::get::<bool>(PARTIAL_PUBLISH_KEY).is_none());
        });
    }

    #[test]
    fn lock_of_a_lower_already_being_frozen_is_not_frozen_again() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_ok!(BridgeWatchtower::freeze_lower(RuntimeOrigin::root(), LOWER_ID));

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&lock_event(locked_lower(
                LOWER_ID
            ))));

            System::assert_last_event(mismatch_event(&locked_lower(LOWER_ID)));
            assert_eq!(published(), vec![freeze_tx(LOWER_ID)]);
        });
    }

    #[test]
    fn other_events_are_ignored() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let event = EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::AvtLowerClaimed.signature(),
                    transaction_hash: ETH_TX_HASH,
                },
                event_data: EventData::LogLowerClaimed(AvtLowerClaimedData { lower_id: LOWER_ID }),
            };

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&event));

            assert!(System::events().is_empty());
            assert!(published().is_empty());
        });
    }
}

mod process_result {
    use super::*;

    #[test]
    fn successful_freeze_is_recorded() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            FailedFreezes::<TestRuntime>::insert(LOWER_ID, ());
            request_freeze_on_mismatch();

            assert_ok!(BridgeWatchtower::process_result(1, PALLET_ID.to_vec(), true));

            System::assert_last_event(RuntimeEvent::BridgeWatchtower(crate::Event::LowerFrozen {
                lower_id: LOWER_ID,
                tx_id: 1,
            }));
            assert!(!LowerIdToFreezeTxId::<TestRuntime>::contains_key(LOWER_ID));
            assert!(!FreezeTxIdToLowerId::<TestRuntime>::contains_key(1));
            assert!(!FailedFreezes::<TestRuntime>::contains_key(LOWER_ID));
        });
    }

    #[test]
    fn failed_freeze_is_flagged() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            request_freeze_on_mismatch();

            assert_ok!(BridgeWatchtower::process_result(1, PALLET_ID.to_vec(), false));

            System::assert_last_event(RuntimeEvent::BridgeWatchtower(
                crate::Event::LowerFreezeFailed { lower_id: LOWER_ID, tx_id: 1 },
            ));
            assert!(!LowerIdToFreezeTxId::<TestRuntime>::contains_key(LOWER_ID));
            assert!(!FreezeTxIdToLowerId::<TestRuntime>::contains_key(1));
            assert!(FailedFreezes::<TestRuntime>::contains_key(LOWER_ID));
        });
    }

    #[test]
    fn results_for_other_callers_are_ignored() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            request_freeze_on_mismatch();
            let events_before = System::events().len();

            assert_ok!(BridgeWatchtower::process_result(1, b"other_pallet".to_vec(), true));

            assert_eq!(System::events().len(), events_before);
            assert_eq!(LowerIdToFreezeTxId::<TestRuntime>::get(LOWER_ID), Some(1));
            assert_eq!(FreezeTxIdToLowerId::<TestRuntime>::get(1), Some(LOWER_ID));
        });
    }

    #[test]
    fn results_for_unknown_transactions_are_ignored() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_ok!(BridgeWatchtower::process_result(1, PALLET_ID.to_vec(), true));
            assert!(System::events().is_empty());
        });
    }
}

mod freeze_lower {
    use super::*;

    #[test]
    fn root_can_freeze_a_lower() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_ok!(BridgeWatchtower::freeze_lower(RuntimeOrigin::root(), LOWER_ID));

            System::assert_last_event(RuntimeEvent::BridgeWatchtower(
                crate::Event::LowerFreezeRequested { lower_id: LOWER_ID, tx_id: 1 },
            ));
            assert_eq!(published(), vec![freeze_tx(LOWER_ID)]);
            assert_eq!(LowerIdToFreezeTxId::<TestRuntime>::get(LOWER_ID), Some(1));
            assert_eq!(FreezeTxIdToLowerId::<TestRuntime>::get(1), Some(LOWER_ID));
        });
    }

    #[test]
    fn root_can_retry_a_failed_freeze() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            request_freeze_on_mismatch();
            assert_ok!(BridgeWatchtower::process_result(1, PALLET_ID.to_vec(), false));

            assert_ok!(BridgeWatchtower::freeze_lower(RuntimeOrigin::root(), LOWER_ID));
            assert_eq!(LowerIdToFreezeTxId::<TestRuntime>::get(LOWER_ID), Some(2));
            // Still flagged until the freeze succeeds
            assert!(FailedFreezes::<TestRuntime>::contains_key(LOWER_ID));

            assert_ok!(BridgeWatchtower::process_result(2, PALLET_ID.to_vec(), true));
            assert!(!FailedFreezes::<TestRuntime>::contains_key(LOWER_ID));
        });
    }

    #[test]
    fn fails_for_non_root_origins() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let caller = sr25519::Public::from_raw([1u8; 32]);
            assert_noop!(
                BridgeWatchtower::freeze_lower(RuntimeOrigin::signed(caller), LOWER_ID),
                BadOrigin
            );
        });
    }

    #[test]
    fn fails_while_a_freeze_is_in_flight() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            assert_ok!(BridgeWatchtower::freeze_lower(RuntimeOrigin::root(), LOWER_ID));
            assert_noop!(
                BridgeWatchtower::freeze_lower(RuntimeOrigin::root(), LOWER_ID),
                Error::<TestRuntime>::FreezeAlreadyRequested
            );
        });
    }

    #[test]
    fn fails_when_publishing_fails() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_publish_fails(true);
            assert_noop!(
                BridgeWatchtower::freeze_lower(RuntimeOrigin::root(), LOWER_ID),
                DispatchError::Other("publish failed")
            );
        });
    }
}

mod lift_check {
    use super::*;

    const PROPOSAL_ID: H256 = H256::repeat_byte(0x01);

    fn lift_event() -> EthEvent {
        EthEvent {
            event_id: EthEventId {
                signature: ValidEvents::Lifted.signature(),
                transaction_hash: ETH_TX_HASH,
            },
            event_data: EventData::LogLifted(LiftedData::new(
                H160::repeat_byte(0x11),
                H256::repeat_byte(0x33),
                1_000,
            )),
        }
    }

    fn growth_event() -> EthEvent {
        EthEvent {
            event_id: EthEventId {
                signature: ValidEvents::AvtGrowthLifted.signature(),
                transaction_hash: ETH_TX_HASH,
            },
            event_data: EventData::LogAvtGrowthLifted(AvtGrowthLiftedData {
                amount: 1_000,
                period: 3,
            }),
        }
    }

    fn external_ref() -> H256 {
        BridgeWatchtower::lift_check_ref(&ETH_TX_HASH)
    }

    /// Processes `event` and starts the vote on its check
    fn start_check(event: &EthEvent) {
        assert_ok!(BridgeWatchtower::on_incoming_event_processed(event));
        assert_ok!(BridgeWatchtower::on_proposal_submitted(
            PROPOSAL_ID,
            make_proposal(ProposalSource::Internal(ProposalType::Lift), external_ref())
        ));
    }

    fn failed_verification_event(reason: LiftCheckFailure) -> RuntimeEvent {
        RuntimeEvent::BridgeWatchtower(crate::Event::LiftFailedVerification {
            eth_tx_hash: ETH_TX_HASH,
            reason,
        })
    }

    #[test]
    fn lift_requests_a_check() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let event = lift_event();

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&event));

            let requests = proposals();
            assert_eq!(requests.len(), 1);
            let request = &requests[0];
            assert_eq!(request.external_ref, external_ref());
            assert_eq!(request.payload, RawPayload::Inline(event.encode()));
            assert_eq!(request.source, ProposalSource::Internal(ProposalType::Lift));
            assert_eq!(request.decision_rule, DecisionRule::RejectOnExpiry);
            assert_eq!(request.threshold, Perbill::from_percent(51));
            assert_eq!(request.committee_size, Some(20));
            assert_eq!(request.voting, VotingKind::Binary);
            assert_eq!(request.created_at, 1);

            assert_eq!(LiftChecks::<TestRuntime>::get(external_ref()), Some(event));
            System::assert_last_event(RuntimeEvent::BridgeWatchtower(
                crate::Event::LiftCheckRequested {
                    eth_tx_hash: ETH_TX_HASH,
                    external_ref: external_ref(),
                },
            ));
            assert!(!lowering_disabled());
        });
    }

    #[test]
    fn growth_lift_requests_a_check() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let event = growth_event();

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&event));

            assert_eq!(proposals().len(), 1);
            assert_eq!(LiftChecks::<TestRuntime>::get(external_ref()), Some(event));
        });
    }

    #[test]
    fn other_events_are_ignored() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let event = EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::AvtLowerClaimed.signature(),
                    transaction_hash: ETH_TX_HASH,
                },
                event_data: EventData::LogLowerClaimed(AvtLowerClaimedData { lower_id: 1 }),
            };

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&event));

            assert!(proposals().is_empty());
        });
    }

    #[test]
    fn failed_request_disables_lowering_and_is_reverted() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            set_proposal_fails(true);

            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&lift_event()));

            assert!(lowering_disabled());
            assert_eq!(LiftChecks::<TestRuntime>::get(external_ref()), None);
            assert!(!frame_support::storage::unhashed::exists(PARTIAL_PROPOSAL_KEY));
            System::assert_last_event(failed_verification_event(LiftCheckFailure::RequestFailed));
        });
    }

    #[test]
    fn started_vote_becomes_the_active_check() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let event = lift_event();

            start_check(&event);

            assert_eq!(ActiveLiftCheck::<TestRuntime>::get(), Some((PROPOSAL_ID, event)));
        });
    }

    #[test]
    fn passed_vote_verifies_the_lift() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            start_check(&lift_event());

            BridgeWatchtower::on_voting_completed(
                PROPOSAL_ID,
                &external_ref(),
                &ProposalStatusEnum::Resolved { passed: true },
            );

            assert!(!lowering_disabled());
            assert_eq!(LiftChecks::<TestRuntime>::get(external_ref()), None);
            assert_eq!(ActiveLiftCheck::<TestRuntime>::get(), None);
            System::assert_last_event(RuntimeEvent::BridgeWatchtower(crate::Event::LiftVerified {
                eth_tx_hash: ETH_TX_HASH,
            }));
        });
    }

    #[test]
    fn unpassed_vote_disables_lowering() {
        for (status, reason) in [
            (ProposalStatusEnum::Resolved { passed: false }, LiftCheckFailure::Rejected),
            (ProposalStatusEnum::Expired, LiftCheckFailure::Expired),
            (ProposalStatusEnum::Cancelled, LiftCheckFailure::Cancelled),
            (ProposalStatusEnum::Queued, LiftCheckFailure::UnexpectedResult),
            (ProposalStatusEnum::Active, LiftCheckFailure::UnexpectedResult),
            (ProposalStatusEnum::Unknown, LiftCheckFailure::UnexpectedResult),
        ] {
            ExtBuilder::build_default().as_externality().execute_with(|| {
                start_check(&lift_event());

                BridgeWatchtower::on_voting_completed(PROPOSAL_ID, &external_ref(), &status);

                assert!(lowering_disabled());
                assert_eq!(LiftChecks::<TestRuntime>::get(external_ref()), None);
                assert_eq!(ActiveLiftCheck::<TestRuntime>::get(), None);
                System::assert_last_event(failed_verification_event(reason));
            });
        }
    }

    #[test]
    fn cancelled_check_disables_lowering() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            start_check(&lift_event());

            BridgeWatchtower::on_cancelled(PROPOSAL_ID, &external_ref());

            assert!(lowering_disabled());
            System::assert_last_event(failed_verification_event(LiftCheckFailure::Cancelled));
        });
    }

    #[test]
    fn queued_check_completing_keeps_the_active_check() {
        ExtBuilder::build_default().as_externality().execute_with(|| {
            let active = (H256::repeat_byte(0x02), growth_event());
            ActiveLiftCheck::<TestRuntime>::put(active.clone());
            assert_ok!(BridgeWatchtower::on_incoming_event_processed(&lift_event()));

            // e.g. cancelled by root while queued
            BridgeWatchtower::on_voting_completed(
                PROPOSAL_ID,
                &external_ref(),
                &ProposalStatusEnum::Cancelled,
            );

            assert_eq!(ActiveLiftCheck::<TestRuntime>::get(), Some(active));
        });
    }
}
