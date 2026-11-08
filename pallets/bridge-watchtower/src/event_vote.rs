//! The offchain worker that lets watchtower nodes vote on event checks.

#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use crate::*;
use frame_system::offchain::{CreateBare, SubmitTransaction};
use pallet_avn::BridgeInterface;
use pallet_watchtower::{NodesInterface, VoteChoice};
use sp_avn_common::{
    eth_receipt::decode_receipt_response,
    event_types::{LiftedData, LowerRevertedData, TotalSupplyUpdatedData},
    ocw_lock::{self as OcwLock, OcwStorageError},
    EthQueryRequest, EthQueryResponseType, EthTransaction,
};
use sp_runtime::{
    offchain::storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
    RuntimeAppPublic, Saturating,
};

const OC_DB_PREFIX: &[u8] = b"bridge_wt::event_vote::";
/// Blocks a submitted vote has to be included before it is submitted again.
const VOTE_INCLUSION_PERIOD: u32 = 5;

impl<T: Config> Pallet<T> {
    /// Votes on the event being checked, if this node is in its committee and has not voted yet.
    pub(crate) fn vote_on_event_check(now: BlockNumberFor<T>) {
        let Some((proposal_id, event)) = ActiveEventCheck::<T>::get() else { return };
        let Some((watchtower, signing_key)) = T::Watchtowers::get_node_from_local_signing_keys()
        else {
            return
        };

        // A counted vote is final, and the pool refuses votes from nodes without a seat: outside
        // the committee and not a trusted owner's node in the open trusted voting window
        if pallet_watchtower::Voters::<T>::contains_key(proposal_id, &watchtower) ||
            !pallet_watchtower::Pallet::<T>::can_vote(proposal_id, &watchtower)
        {
            return
        }

        if Self::vote_in_progress(proposal_id, &watchtower, now) ||
            Self::ocw_already_run(now).is_err()
        {
            return
        }

        // Without an answer from T1 the node does not vote, and tries again next block
        let Ok(in_favor) = Self::event_matches_t1(&event) else { return };

        if let Err(e) = Self::submit_vote(proposal_id, in_favor, signing_key, watchtower, now) {
            log::error!("❌ Failed to vote on event check {:?}: {:?}", proposal_id, e);
        }
    }

    /// Whether the event T2 credited happened on T1: its transaction succeeded and the bridge
    /// emitted the same event. Errors if T1 could not be read, or if the transaction is not yet
    /// deep enough to vote on.
    pub(crate) fn event_matches_t1(event: &EthEvent) -> Result<bool, ()> {
        let eth_tx_hash = event.event_id.transaction_hash;
        let Some(parse) = Self::event_parser(&event.event_data) else {
            // e.g. growth lifts, which are no longer used, so one being credited cannot be verified
            log::warn!("❌ Event {:?} cannot be verified: {:?}", eth_tx_hash, event.event_data);
            return Ok(false)
        };
        // Has a type, since it has a parser
        let Some(event_type) = event.event_data.event_type() else { return Ok(false) };

        let response = Self::fetch_receipt(eth_tx_hash).map_err(|e| {
            log::error!("❌ Failed to fetch the receipt of event {:?}: {:?}", eth_tx_hash, e);
        })?;
        let receipt = match decode_receipt_response(&response) {
            Ok(Some((receipt, confirmations))) => {
                // A shallow receipt could still be reorged away, so the node waits to vote
                let required = T::MinEthBlockConfirmation::get();
                if confirmations < required {
                    log::info!(
                        "⏳ Event {:?} has {} of {} confirmations, waiting to vote",
                        eth_tx_hash,
                        confirmations,
                        required
                    );
                    return Err(())
                }
                receipt
            },
            Ok(None) => {
                log::warn!("❌ Event {:?} has no transaction on T1", eth_tx_hash);
                return Ok(false)
            },
            Err(e) => {
                log::error!("❌ Invalid receipt for event {:?}: {:?}", eth_tx_hash, e);
                return Err(())
            },
        };

        if !receipt.succeeded {
            log::warn!("❌ Event {:?} failed on T1", eth_tx_hash);
            return Ok(false)
        }

        let bridge = T::BridgeInterface::bridge_contract_address();
        let signature = event_type.signature();
        let matches = receipt.logs_from(&bridge, &signature).any(|log| {
            parse(log.data(), log.topics.clone()).is_ok_and(|t1| t1 == event.event_data)
        });
        if !matches {
            log::warn!("❌ Event {:?} does not match any {:?} on T1", eth_tx_hash, event_type);
        }

        Ok(matches)
    }

    /// Parses a T1 log into the same kind of event as `credited`, if it can be verified.
    fn event_parser(
        credited: &EventData,
    ) -> Option<fn(Option<Vec<u8>>, Vec<Vec<u8>>) -> Result<EventData, ()>> {
        Some(match credited {
            EventData::LogLifted(_) => |data, topics| {
                LiftedData::parse_bytes(data, topics).map(EventData::LogLifted).map_err(|_| ())
            },
            EventData::LogLowerReverted(_) => |data, topics| {
                LowerRevertedData::parse_bytes(data, topics)
                    .map(EventData::LogLowerReverted)
                    .map_err(|_| ())
            },
            EventData::LogRewardsMinted(_) => |data, topics| {
                TotalSupplyUpdatedData::parse_bytes(data, topics)
                    .map(EventData::LogRewardsMinted)
                    .map_err(|_| ())
            },
            _ => return None,
        })
    }

    fn fetch_receipt(eth_tx_hash: EthTransactionId) -> Result<Vec<u8>, DispatchError> {
        let query = EthQueryRequest::new(eth_tx_hash, EthQueryResponseType::TransactionReceipt);
        let call = EthTransaction::new(
            [0; 32],
            T::BridgeInterface::bridge_contract_address(),
            query.encode(),
        );

        pallet_avn::Pallet::<T>::post_data_to_service("/eth/query".to_string(), call.encode(), None)
    }

    fn submit_vote(
        proposal_id: ProposalId,
        in_favor: bool,
        signing_key: T::SignerId,
        watchtower: T::AccountId,
        now: BlockNumberFor<T>,
    ) -> Result<(), &'static str> {
        let country = pallet_watchtower::fetch_node_country::<T>();
        let data_to_sign = pallet_watchtower::Pallet::<T>::unsigned_vote_signing_payload(
            proposal_id,
            &VoteChoice::Binary(in_favor),
            &country,
            &watchtower,
        );
        let signature = signing_key.sign(&data_to_sign).ok_or("Failed to sign the vote")?;

        let call = <T as CreateBare<pallet_watchtower::Call<T>>>::create_bare(
            pallet_watchtower::Call::unsigned_vote {
                proposal_id,
                in_favor,
                country,
                watchtower: watchtower.clone(),
                signature,
            }
            .into(),
        );
        SubmitTransaction::<T, pallet_watchtower::Call<T>>::submit_transaction(call)
            .map_err(|_| "Failed to submit the vote")?;

        Self::record_vote_submission(proposal_id, &watchtower, now)
    }

    fn vote_key(proposal_id: ProposalId, watchtower: &T::AccountId) -> Vec<u8> {
        let mut key = OC_DB_PREFIX.to_vec();
        key.extend((proposal_id, watchtower).encode());
        key
    }

    fn record_vote_submission(
        proposal_id: ProposalId,
        watchtower: &T::AccountId,
        now: BlockNumberFor<T>,
    ) -> Result<(), &'static str> {
        let key = Self::vote_key(proposal_id, watchtower);
        let storage = StorageValueRef::persistent(&key);
        storage
            .mutate(|_: Result<Option<BlockNumberFor<T>>, StorageRetrievalError>| Ok::<_, ()>(now))
            .map(|_| ())
            .map_err(|e| match e {
                MutateStorageError::ConcurrentModification(_) =>
                    "Failed to record the vote: concurrent modification",
                MutateStorageError::ValueFunctionFailed(_) => "Failed to record the vote",
            })
    }

    /// Whether this node submitted a vote recently enough that it may still be included.
    pub(crate) fn vote_in_progress(
        proposal_id: ProposalId,
        watchtower: &T::AccountId,
        now: BlockNumberFor<T>,
    ) -> bool {
        StorageValueRef::persistent(&Self::vote_key(proposal_id, watchtower))
            .get::<BlockNumberFor<T>>()
            .ok()
            .flatten()
            .is_some_and(|submitted_at| {
                now <= submitted_at.saturating_add(VOTE_INCLUSION_PERIOD.into())
            })
    }

    fn ocw_already_run(now: BlockNumberFor<T>) -> Result<(), ()> {
        // Offchain workers can run more than once for the same block, e.g. on re-orgs
        OcwLock::record_block_run(now, PALLET_ID.to_vec()).map_err(|e| match e {
            OcwStorageError::OffchainWorkerAlreadyRun => {
                log::warn!("❌ Bridge watchtower OCW has already run for block {:?}", now);
            },
            OcwStorageError::ErrorRecordingOffchainWorkerRun => {
                log::error!("❌ Unable to record the OCW run for block {:?}, skipping", now);
            },
        })
    }
}
