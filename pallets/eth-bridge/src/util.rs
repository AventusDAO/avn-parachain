use super::*;
use crate::{Config, AVN};
use frame_support::{traits::UnixTime, BoundedVec};
use sp_avn_common::EthQueryResponse;

pub fn time_now<T: Config<I>, I: 'static>() -> u64 {
    <T as pallet::Config<I>>::TimeProvider::now().as_secs()
}

pub fn has_enough_corroborations<T: Config<I>, I: 'static>(corroborations: usize) -> bool {
    // the sender cannot corroborate their own transaction
    let num_authors_excluding_sender = AVN::<T>::validators().len() as u32 - 1;
    let quorum = T::Quorum::required_for(num_authors_excluding_sender);
    corroborations as u32 >= quorum
}

pub fn has_enough_confirmations<T: Config<I>, I: 'static>(confirmations: u32) -> bool {
    let num_confirmations_including_sender = confirmations + 1u32;
    num_confirmations_including_sender >= T::Quorum::get_quorum()
}

pub fn has_supermajority_confirmations<T: Config<I>, I: 'static>(confirmations: u32) -> bool {
    confirmations >= T::Quorum::get_supermajority_quorum()
}

pub fn requires_corroboration<T: Config<I>, I: 'static>(
    eth_tx: &ActiveEthTransaction<T::AccountId>,
    author: &Author<T>,
) -> bool {
    !eth_tx.success_corroborations.contains(&author.account_id) &&
        !eth_tx.failure_corroborations.contains(&author.account_id)
}

pub fn bound_params<T, I>(
    params: &[(Vec<u8>, Vec<u8>)],
) -> Result<
    BoundedVec<(BoundedVec<u8, TypeLimit>, BoundedVec<u8, ValueLimit>), ParamsLimit>,
    Error<T, I>,
> {
    let result: Result<Vec<_>, _> = params
        .iter()
        .map(|(type_vec, value_vec)| {
            let type_bounded = BoundedVec::try_from(type_vec.clone())
                .map_err(|_| Error::<T, I>::TypeNameLengthExceeded)?;
            let value_bounded = BoundedVec::try_from(value_vec.clone())
                .map_err(|_| Error::<T, I>::ValueLengthExceeded)?;
            Ok::<_, Error<T, I>>((type_bounded, value_bounded))
        })
        .collect();

    BoundedVec::<_, ParamsLimit>::try_from(result?).map_err(|_| Error::<T, I>::ParamsLimitExceeded)
}

pub fn unbound_params(
    params: &BoundedVec<(BoundedVec<u8, TypeLimit>, BoundedVec<u8, ValueLimit>), ParamsLimit>,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    params
        .iter()
        .map(|(type_bounded, value_bounded)| {
            (type_bounded.as_slice().to_vec(), value_bounded.as_slice().to_vec())
        })
        .collect()
}

pub fn try_process_query_result<R: Decode, T: Config<I>, I: 'static>(
    response_bytes: Vec<u8>,
) -> Result<(R, u64), Error<T, I>> {
    let eth_query_response: EthQueryResponse = EthQueryResponse::decode(&mut &response_bytes[..])
        .map_err(|e| {
        log::error!("❌ Error decoding eth query response {:?} - {:?}", response_bytes, e);
        Error::<T, I>::InvalidQueryResponseFromEthereum
    })?;

    let call_data: R = R::decode(&mut &eth_query_response.data[..]).map_err(|e| {
        log::error!(
            "❌ Error decoding eth query response data {:?} - {:?}",
            eth_query_response.data,
            e
        );
        Error::<T, I>::InvalidQueryResponseFromEthereum
    })?;

    return Ok((call_data, eth_query_response.num_confirmations))
}
