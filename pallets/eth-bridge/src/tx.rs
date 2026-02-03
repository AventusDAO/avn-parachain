use super::*;
use crate::{offence::create_and_report_bridge_offence, Config};
use frame_support::BoundedVec;
use sp_avn_common::eth::{create_function_confirmation_hash, EthereumId};

pub fn is_active_request<T: Config<I>, I: 'static>(id: EthereumId) -> bool {
    ActiveRequest::<T, I>::get().map_or(false, |r| r.request.id_matches(&id))
}

fn complete_transaction<T: Config<I>, I: 'static>(
    mut tx: ActiveTransactionData<T::AccountId>,
    success: bool,
) -> Result<(), Error<T, I>> {
    // Alert the originating pallet:
    T::BridgeInterfaceNotification::process_result(
        tx.request.tx_id,
        tx.request.caller_id.into(),
        success,
    )
    .map_err(|_| Error::<T, I>::HandlePublishingResultFailed)?;

    tx.data.tx_succeeded = success;

    // Check for offences:
    if success {
        if !tx.data.failure_corroborations.is_empty() {
            create_and_report_bridge_offence::<T, I>(
                &tx.data.sender,
                &tx.data.failure_corroborations,
                offence::EthBridgeOffenceType::ChallengeAttemptedOnSuccessfulTransaction,
            )
        }

        // if the transaction is a success but the eth tx hash is wrong remove it
        if util::has_enough_corroborations::<T, I>(tx.data.invalid_tx_hash_corroborations.len()) {
            tx.data.eth_tx_hash = H256::zero();
        }
    } else {
        if !tx.data.success_corroborations.is_empty() {
            create_and_report_bridge_offence::<T, I>(
                &tx.data.sender,
                &tx.data.success_corroborations,
                offence::EthBridgeOffenceType::ChallengeAttemptedOnUnsuccessfulTransaction,
            )
        }
    }

    // Write the tx data to permanent storage:
    SettledTransactions::<T, I>::insert(
        tx.request.tx_id,
        TransactionData {
            function_name: tx.request.function_name,
            params: tx.data.eth_tx_params,
            sender: tx.data.sender,
            eth_tx_hash: tx.data.eth_tx_hash,
            tx_succeeded: tx.data.tx_succeeded,
        },
    );

    // Process any new request from the queue
    request::process_next_request::<T, I>();

    Ok(())
}

pub fn finalize_state<T: Config<I>, I: 'static>(
    tx: ActiveTransactionData<T::AccountId>,
    success: bool,
) -> Result<(), Error<T, I>> {
    // if the transaction failed and the tx hash is missing or pointing to a different transaction,
    // replay transaction
    if !success &&
        util::has_enough_corroborations::<T, I>(tx.data.invalid_tx_hash_corroborations.len())
    {
        // raise an offence on the "sender" because the tx_hash they provided was invalid
        return Ok(replay_send_request(tx)?)
    }

    Ok(complete_transaction::<T, I>(tx, success)?)
}

pub fn set_up_active_tx<T: Config<I>, I: 'static>(
    req: SendRequestData,
    replay_maybe: Option<u16>,
) -> Result<(), Error<T, I>> {
    let expiry = util::time_now::<T, I>() + EthTxLifetimeSecs::<T, I>::get();
    let extended_params: BoundedVec<
        (BoundedVec<u8, ConstU32<7>>, BoundedVec<u8, ConstU32<130>>),
        ConstU32<5>,
    > = req.extend_params(expiry)?;
    let params_vec: Vec<(Vec<u8>, Vec<u8>)> = extended_params
        .iter()
        .map(|(t, v)| (t.clone().into_inner(), v.clone().into_inner()))
        .collect();

    let msg_hash = create_function_confirmation_hash(
        req.function_name.clone().into_inner(),
        params_vec,
        Instance::<T, I>::get().into(),
    )
    .map_err(|_| Error::<T, I>::MsgHashError)?;

    let replay_attempt = replay_maybe.unwrap_or(0);
    ActiveRequest::<T, I>::put(ActiveRequestData {
        request: Request::Send(req.clone()),
        confirmation: ActiveConfirmation { msg_hash, confirmations: BoundedVec::default() },
        tx_data: Some(ActiveEthTransaction {
            function_name: req.function_name,
            eth_tx_params: extended_params,
            expiry,
            eth_tx_hash: H256::zero(),
            sender: assign_sender()?,
            success_corroborations: BoundedVec::default(),
            failure_corroborations: BoundedVec::default(),
            valid_tx_hash_corroborations: BoundedVec::default(),
            invalid_tx_hash_corroborations: BoundedVec::default(),
            tx_succeeded: false,
            replay_attempt,
        }),
        last_updated: <frame_system::Pallet<T>>::block_number(),
    });

    return Ok(())
}

pub fn replay_send_request<T: Config<I>, I: 'static>(
    tx: ActiveTransactionData<T::AccountId>,
) -> Result<(), Error<T, I>> {
    <crate::Pallet<T, I>>::deposit_event(Event::<T, I>::ActiveRequestRetried {
        function_name: tx.request.function_name.clone(),
        params: tx.request.params.clone(),
        caller_id: tx.request.caller_id.clone(),
    });

    let replay_attempt = Some(tx.replay_attempt.saturating_plus_one());
    return Ok(set_up_active_tx(tx.request, replay_attempt)?)
}

pub fn use_next_tx_id<T: Config<I>, I: 'static>() -> u32 {
    let tx_id = NextTxId::<T, I>::get();
    NextTxId::<T, I>::put(tx_id + 1);
    tx_id
}

fn assign_sender<T: Config<I>, I: 'static>() -> Result<T::AccountId, Error<T, I>> {
    match AVN::<T>::advance_primary_validator_for_sending() {
        Ok(primary_validator) => {
            let sender = primary_validator;
            Ok(sender)
        },
        Err(_) => Err(Error::<T, I>::ErrorAssigningSender),
    }
}
