use super::*;
use crate::{Author, Config};
use sp_avn_common::eth::EthereumId;
use sp_core::{ecdsa, H256};

pub fn add_confirmation<T: Config<I>, I: 'static>(
    request_id: EthereumId,
    confirmation: ecdsa::Signature,
    author: Author<T>,
) {
    let proof = add_confirmation_proof::<T, I>(request_id, &confirmation, &author.account_id);
    let signature = author.key.sign(&proof).expect("Error signing proof");
    let call = Call::<T, I>::add_confirmation { request_id, confirmation, author, signature };
    let xt = T::create_inherent(call.into());
    let _ = SubmitTransaction::<T, Call<T, I>>::submit_transaction(xt);
}

pub fn add_eth_tx_hash<T: Config<I>, I: 'static>(
    tx_id: EthereumId,
    eth_tx_hash: H256,
    author: Author<T>,
) {
    let proof = add_eth_tx_hash_proof::<T, I>(tx_id, &eth_tx_hash, &author.account_id);
    let signature = author.key.sign(&proof).expect("Error signing proof");
    let call = Call::<T, I>::add_eth_tx_hash { tx_id, eth_tx_hash, author, signature };
    let xt = T::create_inherent(call.into());
    let _ = SubmitTransaction::<T, Call<T, I>>::submit_transaction(xt);
}

pub fn add_corroboration<T: Config<I>, I: 'static>(
    tx_id: EthereumId,
    tx_succeeded: bool,
    tx_hash_is_valid: bool,
    author: Author<T>,
    replay_attempt: u16,
) {
    let proof = add_corroboration_proof::<T, I>(
        tx_id,
        tx_succeeded,
        tx_hash_is_valid,
        &author.account_id,
        replay_attempt,
    );
    let signature = author.key.sign(&proof).expect("Error signing proof");
    let call = Call::<T, I>::add_corroboration {
        tx_id,
        tx_succeeded,
        tx_hash_is_valid,
        author,
        replay_attempt,
        signature,
    };
    let xt = T::create_inherent(call.into());
    let _ = SubmitTransaction::<T, Call<T, I>>::submit_transaction(xt);
}

pub fn submit_ethereum_events<T: Config<I>, I: 'static>(
    author: Author<T>,
    events_partition: EthereumEventsPartition,
    signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
) -> Result<(), ()> {
    let call = Call::<T, I>::submit_ethereum_events { author, events_partition, signature };
    let xt = T::create_inherent(call.into());
    SubmitTransaction::<T, Call<T, I>>::submit_transaction(xt)
}

pub fn submit_latest_ethereum_block<T: Config<I>, I: 'static>(
    author: Author<T>,
    latest_seen_block: u32,
    signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
) -> Result<(), ()> {
    let call = Call::<T, I>::submit_latest_ethereum_block { author, latest_seen_block, signature };
    let xt = T::create_inherent(call.into());
    SubmitTransaction::<T, Call<T, I>>::submit_transaction(xt)
}

fn add_confirmation_proof<T: Config<I>, I: 'static>(
    tx_id: EthereumId,
    confirmation: &ecdsa::Signature,
    account_id: &T::AccountId,
) -> Vec<u8> {
    (Instance::<T, I>::get().hash(), ADD_CONFIRMATION_CONTEXT, tx_id, confirmation, &account_id)
        .encode()
}

fn add_eth_tx_hash_proof<T: Config<I>, I: 'static>(
    tx_id: EthereumId,
    eth_tx_hash: &H256,
    account_id: &T::AccountId,
) -> Vec<u8> {
    (Instance::<T, I>::get().hash(), ADD_ETH_TX_HASH_CONTEXT, tx_id, *eth_tx_hash, &account_id)
        .encode()
}

fn add_corroboration_proof<T: Config<I>, I: 'static>(
    tx_id: EthereumId,
    tx_succeeded: bool,
    tx_hash_is_valid: bool,
    account_id: &T::AccountId,
    replay_attempt: u16,
) -> Vec<u8> {
    (
        Instance::<T, I>::get().hash(),
        ADD_CORROBORATION_CONTEXT,
        tx_id,
        tx_succeeded,
        tx_hash_is_valid,
        &account_id,
        replay_attempt,
    )
        .encode()
}
