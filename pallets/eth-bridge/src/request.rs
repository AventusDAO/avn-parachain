use super::*;
use crate::{util::bound_params, Config};
use frame_support::BoundedVec;
use sp_avn_common::eth::{create_lower_proof_hash, EthereumId};
use sp_core::Get;

pub fn add_new_send_request<T: Config<I>, I: 'static>(
    function_name: &[u8],
    params: &[(Vec<u8>, Vec<u8>)],
    caller_id: &Vec<u8>,
) -> Result<EthereumId, Error<T, I>> {
    let name_bytes = function_name.to_vec();
    let function_name_string =
        String::from_utf8(name_bytes.clone()).map_err(|_| Error::<T, I>::FunctionNameError)?;
    if function_name_string.is_empty() {
        return Err(Error::<T, I>::EmptyFunctionName)
    }

    let tx_id = tx::use_next_tx_id::<T, I>();

    let send_req = SendRequestData {
        tx_id,
        function_name: BoundedVec::<u8, FunctionLimit>::try_from(name_bytes)
            .map_err(|_| Error::<T, I>::ExceedsFunctionNameLimit)?,
        params: bound_params(&params.to_vec())?,
        caller_id: BoundedVec::<_, CallerIdLimit>::try_from(caller_id.clone())
            .map_err(|_| Error::<T, I>::CallerIdLengthExceeded)?,
    };

    if ActiveRequest::<T, I>::get().is_some() {
        queue_request(Request::Send(send_req))?;
    } else {
        tx::set_up_active_tx(send_req, None)?;
    }

    Ok(tx_id)
}

pub fn add_new_lower_proof_request<T: Config<I>, I: 'static>(
    lower_id: LowerId,
    params: &LowerParams,
    caller_id: &Vec<u8>,
) -> Result<(), Error<T, I>> {
    let proof_req = LowerProofRequestData {
        lower_id,
        params: *params,
        caller_id: BoundedVec::<_, CallerIdLimit>::try_from(caller_id.clone())
            .map_err(|_| Error::<T, I>::CallerIdLengthExceeded)?,
    };

    if ActiveRequest::<T, I>::get().is_some() {
        queue_request(Request::LowerProof(proof_req))?;
    } else {
        set_up_active_lower_proof(proof_req)?;
    }

    Ok(())
}

// This function cannot error. Otherwise we need a way to resume processing queued requests.
pub fn process_next_request<T: Config<I>, I: 'static>() {
    ActiveRequest::<T, I>::kill();

    if let Some(req) = request::dequeue_request::<T, I>() {
        match req {
            Request::Send(send_req) => {
                if let Err(e) = tx::set_up_active_tx::<T, I>(send_req.clone(), None) {
                    // If we failed to setup the next request, notify caller
                    log::error!(target: "runtime::eth-bridge", "Error processing send request from queue: {:?}", e);
                    let _ = T::BridgeInterfaceNotification::process_result(
                        send_req.tx_id,
                        send_req.caller_id.clone().into(),
                        false,
                    );
                    process_next_request::<T, I>();
                }
            },
            Request::LowerProof(lower_req) => {
                if let Err(e) = set_up_active_lower_proof::<T, I>(lower_req.clone()) {
                    // If we failed to setup the next request, notify caller
                    log::error!(target: "runtime::eth-bridge", "Error processing lower proof request from queue: {:?}", e);
                    let _ = T::BridgeInterfaceNotification::process_lower_proof_result(
                        lower_req.lower_id,
                        lower_req.caller_id.clone().into(),
                        Err(()),
                    );
                    process_next_request::<T, I>();
                }
            },
        };
    };
}

pub fn has_enough_confirmations<T: Config<I>, I: 'static>(
    req: &ActiveRequestData<BlockNumberFor<T>, T::AccountId>,
) -> bool {
    let confirmations = req.confirmation.confirmations.len() as u32;
    match req.request {
        // The sender's confirmation is implicit so we only collect them from other authors:
        Request::Send(_) => util::has_enough_confirmations::<T, I>(confirmations),
        Request::LowerProof(_) => util::has_supermajority_confirmations::<T, I>(confirmations),
    }
}

pub fn complete_lower_proof_request<T: Config<I>, I: 'static>(
    lower_req: &LowerProofRequestData,
    confirmations: BoundedVec<ecdsa::Signature, ConfirmationsLimit>,
) -> Result<(), Error<T, I>> {
    let lower_proof = eth::generate_encoded_lower_proof::<T, I>(lower_req, confirmations);
    let result = T::BridgeInterfaceNotification::process_lower_proof_result(
        lower_req.lower_id,
        lower_req.caller_id.clone().into(),
        Ok(lower_proof),
    );

    if let Err(e) = result {
        log::error!(target: "runtime::eth-bridge", "Lower proof notification failed: {:?}", e);
    }

    // Process any new request from the queue
    request::process_next_request::<T, I>();

    Ok(())
}

fn set_up_active_lower_proof<T: Config<I>, I: 'static>(
    req: LowerProofRequestData,
) -> Result<(), Error<T, I>> {
    let msg_hash = create_lower_proof_hash(req.params.clone(), Instance::<T, I>::get().into())
        .map_err(|_| Error::<T, I>::LowerParamsError)?;

    ActiveRequest::<T, I>::put(ActiveRequestData::<BlockNumberFor<T>, T::AccountId> {
        request: Request::LowerProof(req),
        confirmation: ActiveConfirmation { msg_hash, confirmations: BoundedVec::default() },
        tx_data: None,
        last_updated: <frame_system::Pallet<T>>::block_number(),
    });

    return Ok(())
}

fn queue_request<T: Config<I>, I: 'static>(request: Request) -> Result<(), Error<T, I>> {
    RequestQueue::<T, I>::mutate(|maybe_queue| {
        let mut queue: Vec<_> = maybe_queue.clone().unwrap_or_else(Default::default).into();

        if queue.len() < T::MaxQueuedTxRequests::get() as usize {
            queue.push(request);
            let bounded_queue =
                BoundedVec::try_from(queue).expect("Size known to be in bounds here");
            *maybe_queue = Some(bounded_queue);
            Ok(())
        } else {
            Err(Error::<T, I>::TxRequestQueueFull)
        }
    })
}

fn dequeue_request<T: Config<I>, I: 'static>() -> Option<Request> {
    let mut queue = <RequestQueue<T, I>>::take();

    let next_tx_request = match &mut queue {
        Some(q) if !q.is_empty() => Some(q.remove(0)),
        _ => None,
    };

    if let Some(q) = &queue {
        if !q.is_empty() {
            RequestQueue::<T, I>::put(q);
        }
    }

    next_tx_request
}
