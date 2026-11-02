use crate::{AccountId, AvnId, Runtime, RuntimeCall};
use codec::Decode;
use polkadot_sdk::{
    frame_support::{dispatch::GetDispatchInfo, traits::Get},
    sp_runtime::RuntimeAppPublic,
};
use sp_avn_common::event_discovery::{
    EthBlockRange, EthereumEventsPartition, MAX_INCOMING_EVENTS_BATCH_SIZE,
};

/// Authors could never submit a vote that does not fit in a block, and the bridge would stop
/// processing Ethereum events.
#[test]
fn vote_processing_a_full_batch_of_ethereum_events_fits_in_a_block() {
    let key = AvnId::decode(&mut &[0u8; 32][..]).unwrap();
    let signature = <AvnId as RuntimeAppPublic>::Signature::decode(&mut &[0u8; 64][..]).unwrap();
    let call = RuntimeCall::EthBridge(pallet_eth_bridge::Call::submit_ethereum_events {
        author: pallet_eth_bridge::Author::<Runtime>::new(AccountId::new([0u8; 32]), key),
        events_partition: EthereumEventsPartition::new(
            EthBlockRange::default(),
            0,
            false,
            Default::default(),
        ),
        signature,
    });
    let info = call.get_dispatch_info();

    // Covers a mix of events too: a full batch of the costliest kind costs at least as much
    let full_batch =
        pallet_eth_bridge::Pallet::<Runtime>::process_batch_weight(MAX_INCOMING_EVENTS_BATCH_SIZE);
    assert!(full_batch.all_lte(info.call_weight), "the vote is charged less than a full batch");

    let max_extrinsic = <Runtime as polkadot_sdk::frame_system::Config>::BlockWeights::get()
        .get(info.class)
        .max_extrinsic
        .expect("the dispatch class has a limit");
    assert!(
        info.total_weight().all_lte(max_extrinsic),
        "the vote weighs {:?}, above the {:?} a transaction can use",
        info.total_weight(),
        max_extrinsic
    );
}
