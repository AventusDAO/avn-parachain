#![cfg_attr(not(feature = "std"), no_std)]
use codec::Codec;
use sp_avn_common::{
    eth::EthBridgeInstance,
    event_discovery::{AdditionalEvents, EthBlockRange, EthereumEventsPartition},
};
use sp_core::H256;
use sp_std::{collections::btree_map::BTreeMap, vec::Vec};

pub type InstanceId = u8;

sp_api::decl_runtime_apis! {

    #[api_version(3)]
    pub trait EthEventHandlerApi<AccountId>
            where
        AccountId: Codec,
    {
        fn query_authors() -> Vec<([u8; 32], [u8; 32])>;
        fn query_active_block_range(instance_id: InstanceId)-> Option<(EthBlockRange, u16)>;
        #[changed_in(3)]
        fn query_active_block_range()-> Option<(EthBlockRange, u16)>;

        fn query_has_author_casted_vote(instance_id: InstanceId, account_id: AccountId) -> bool;
        #[changed_in(3)]
        fn query_has_author_casted_vote(account_id: AccountId) -> bool;

        fn query_signatures(instance_id: InstanceId) -> Vec<H256>;
        #[changed_in(3)]
        fn query_signatures() -> Vec<H256>;

        fn submit_vote(
            instance_id: InstanceId,
            author: AccountId,
            events_partition: EthereumEventsPartition,
            signature: sp_core::sr25519::Signature
        ) -> Option<()>;
        #[changed_in(3)]
        fn submit_vote(
            author: AccountId,
            events_partition: EthereumEventsPartition,
            signature: sp_core::sr25519::Signature
        ) -> Option<()>;

        fn submit_latest_ethereum_block(
            instance_id: InstanceId,
            author: AccountId,
            latest_seen_block: u32,
            signature: sp_core::sr25519::Signature
        ) -> Option<()>;
        #[changed_in(3)]
        fn submit_latest_ethereum_block(
            author: AccountId,
            latest_seen_block: u32,
            signature: sp_core::sr25519::Signature
        ) -> Option<()>;

        fn additional_transactions(instance_id: InstanceId) -> Option<AdditionalEvents>;
        #[changed_in(3)]
        fn additional_transactions() -> Option<AdditionalEvents>;

        fn instances() -> BTreeMap<InstanceId, EthBridgeInstance>;
    }
}
