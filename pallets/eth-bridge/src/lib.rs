// Copyright 2025 Aventus Network Services (UK) Ltd.

//! This pallet implements the AvN pallet's **BridgeInterface** interface, providing a **publish**
//! and **generate_lower_proof** methods which other pallets, implementing the
//! **BridgeInterfaceNotification**, can use to execute any author function on the Ethereum-based
//! **avn-bridge** contract or request a proof to be generated to lower tokens on Ethereum.
//! To publish transactions, callers need to pass the name of the desired avn-bridge function,
//! along with an array of data type and value parameter tuples. Upon receipt of a **publish**
//! request, this pallet takes charge of the entire transaction process. The process culminates in a
//! callback to the originating pallet detailing the final outcome, which can be used to commit or
//! rollback state. Transaction requests are handled sequentially and requests are queued if
//! required.
//!
//! When sending transactions to Ethereum, the pallet manages:
//! - Accepting and managing external pallet requests to be processed to completion in the order
//!   they arrive.
//!
//! - The packaging and encoding of each transaction to ensure Ethereum compatibility.
//!
//! - The addition of a timestamp, delineating the deadline by which a transaction must reach the
//!   contract.
//!
//! - The addition of a unique transaction ID, against which request data can be stored on the AvN
//!   and the transaction's status in the avn-bridge be later checked.
//!
//! - Collection of the necessary ECDSA signatures from authors, labelled **confirmations**, which
//!   serve to prove AvN consensus for a transaction to the avn-bridge.
//!
//! - Appointing an author responsible for sending the transaction to Ethereum.
//!
//! - Utilising the transaction ID and expiry to check the status of a sent transaction on Ethereum
//!   and arrive at a consensus of that status by providing **corroborations**.
//!
//! - Alerting the originating pallet to the outcome via the BridgeInterfaceNotification callback.
//!
//! The core of the pallet resides in the off-chain worker. The OCW monitors all unresolved
//! transactions, prompting authors to resolve them by invoking one of three unsigned extrinsics:
//!
//! 1. Before a transaction can be dispatched, confirmations are accumulated from non-sending
//!    authors via the **add_confirmation** extrinsic until a consensus is reached. Note: the
//!    sender's confirmation is taken as implicit by the avn-bridge and therefore not requested.
//!
//! 2. Once a transaction has received sufficient confirmations, the chosen sender is prompted to
//!    dispatch it to Ethereum and tag it as sent using the **add_eth_tx_hash** extrinsic.
//!
//! 3. When a transaction possesses an Ethereum tx hash, or if its expiration time has elapsed
//!    without a definitive outcome, all authors are requested to **add_corroboration**s. Achieiving
//!    consensus of corroborations determines the final state which is reported back to the
//!    originating pallet.

#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
};
use codec::{Decode, Encode, MaxEncodedLen};
use core::convert::TryInto;
use frame_support::{
    dispatch::DispatchResultWithPostInfo,
    ensure,
    pallet_prelude::{StorageVersion, Weight},
    traits::{Get, IsSubType},
    weights::WeightMeter,
    BoundedBTreeSet, BoundedVec,
};
use frame_system::{
    ensure_none, ensure_root,
    offchain::{SendTransactionTypes, SubmitTransaction},
    pallet_prelude::{BlockNumberFor, OriginFor},
};
use pallet_avn::{
    self as avn, BridgeInterface, BridgeInterfaceNotification, Error as avn_error,
    EthereumEventsMigration, EventMigration, EventProcessingError,
    NetworkAwareProcessedEventsChecker, ProcessedEventsChecker, MAX_VALIDATOR_ACCOUNTS,
};

use pallet_session::historical::IdentificationTuple;
use sp_staking::offence::ReportOffence;

use crate::{offence::create_and_report_bridge_offence, tx::set_up_active_tx};
use sp_application_crypto::RuntimeAppPublic;
use sp_avn_common::{
    bounds::{MaximumValidatorsBound, ProcessingBatchBound},
    eth::{EthBridgeInstance, EthereumNetwork, LowerParams},
    event_discovery::*,
    event_types::{self, EthEventId, EthProcessedEvent, EthTransactionId, ValidEvents, Validator},
};
use sp_core::{ecdsa, ConstU32, H256};
use sp_io::hashing::twox_64;
use sp_runtime::{scale_info::TypeInfo, traits::Dispatchable, Saturating};
use sp_std::prelude::*;

mod call;
mod eth;
pub mod migration;
mod request;
mod tx;
pub mod types;
mod util;
use crate::types::*;
use sp_avn_common::QuorumPolicy;

pub use call::{submit_ethereum_events, submit_latest_ethereum_block};

mod benchmarking;
#[cfg(test)]
#[path = "tests/event_listener_tests.rs"]
mod event_listener_tests;
#[cfg(test)]
#[path = "tests/incoming_events_tests.rs"]
mod incoming_events_tests;
#[cfg(test)]
#[path = "tests/lower_proof_tests.rs"]
mod lower_proof_tests;
#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

pub use pallet::*;
pub mod default_weights;
pub use default_weights::WeightInfo;

pub mod offence;
use offence::EthBridgeOffence;

pub type AVN<T> = avn::Pallet<T>;
pub type Author<T> =
    Validator<<T as avn::Config>::AuthorityId, <T as frame_system::Config>::AccountId>;

pub type ConfirmationsLimit = ConstU32<MAX_CONFIRMATIONS>; // Max confirmations or corroborations (must be > 1/3 of authors)
pub type FunctionLimit = ConstU32<32>; // Max chars allowed in T1 function name
pub type CallerIdLimit = ConstU32<50>; // Max chars in caller id value
                                       // TODO: make these config constants
pub type ParamsLimit = ConstU32<5>; // Max T1 function params (excluding expiry, t2TxId, and confirmations)
pub type TypeLimit = ConstU32<7>; // Max chars in a param's type
pub type ValueLimit = ConstU32<130>; // Max chars in a param's value

pub const TX_HASH_INVALID: bool = false;
pub type LowerId = u32;

pub const MAX_CONFIRMATIONS: u32 = 100u32;
const PALLET_NAME: &'static [u8] = b"EthBridge";
const ADD_CONFIRMATION_CONTEXT: &'static [u8] = b"EthBridgeConfirmation";
const ADD_CORROBORATION_CONTEXT: &'static [u8] = b"EthBridgeCorroboration";
const ADD_ETH_TX_HASH_CONTEXT: &'static [u8] = b"EthBridgeEthTxHash";
pub const SUBMIT_ETHEREUM_EVENTS_HASH_CONTEXT: &'static [u8] = b"EthBridgeDiscoveredEthEventsHash";
pub const SUBMIT_LATEST_ETH_BLOCK_CONTEXT: &'static [u8] = b"EthBridgeLatestEthereumBlockHash";
pub const DEFAULT_ETH_RANGE: u32 = 20u32;

const STORAGE_VERSION: StorageVersion = StorageVersion::new(5);

#[frame_support::pallet]
pub mod pallet {
    use crate::offence::EthBridgeOffenceType;

    use super::*;
    use frame_support::{
        pallet_prelude::{ValueQuery, *},
        traits::UnixTime,
        Blake2_128Concat,
    };
    use sp_avn_common::{
        eth::EthereumId,
        event_types::{EthEvent, EthEventId, ValidEvents},
    };

    #[pallet::config]
    pub trait Config<I: 'static = ()>:
        frame_system::Config
        + avn::Config
        + scale_info::TypeInfo
        + SendTransactionTypes<Call<Self, I>>
        + pallet_session::historical::Config
    {
        type RuntimeEvent: From<Event<Self, I>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        type TimeProvider: UnixTime;
        type WeightInfo: WeightInfo;
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + IsSubType<Call<Self, I>>
            + From<Call<Self, I>>;
        #[pallet::constant]
        type MaxQueuedTxRequests: Get<u32>;
        #[pallet::constant]
        type MinEthBlockConfirmation: Get<u64>;
        type AccountToBytesConvert: avn::AccountToBytesConverter<Self::AccountId>;
        type BridgeInterfaceNotification: avn::BridgeInterfaceNotification;
        type ReportCorroborationOffence: ReportOffence<
            Self::AccountId,
            IdentificationTuple<Self>,
            EthBridgeOffence<IdentificationTuple<Self>>,
        >;
        type ProcessedEventsChecker: NetworkAwareProcessedEventsChecker;
        type EthereumEventsMigration: EthereumEventsMigration;
        type ProcessedEventsHandler: EthereumEventsFilterTrait;
        type Quorum: QuorumPolicy;
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config<I>, I: 'static = ()> {
        PublishToEthereum {
            tx_id: EthereumId,
            function_name: Vec<u8>,
            params: Vec<(Vec<u8>, Vec<u8>)>,
            caller_id: Vec<u8>,
        },
        LowerProofRequested {
            lower_id: LowerId,
            params: LowerParams,
            caller_id: Vec<u8>,
        },
        EthTxIdUpdated {
            eth_tx_id: EthereumId,
        },
        EthTxLifetimeUpdated {
            eth_tx_lifetime_secs: u64,
        },
        CorroborationOffenceReported {
            offence_type: EthBridgeOffenceType,
            offenders: Vec<IdentificationTuple<T>>,
        },
        ActiveRequestRemoved {
            request_id: EthereumId,
        },
        ActiveRequestRetried {
            function_name: BoundedVec<u8, FunctionLimit>,
            params:
                BoundedVec<(BoundedVec<u8, TypeLimit>, BoundedVec<u8, ValueLimit>), ParamsLimit>,
            caller_id: BoundedVec<u8, CallerIdLimit>,
        },
        EventAccepted {
            eth_event_id: EthEventId,
        },
        EventRejected {
            eth_event_id: EthEventId,
            reason: DispatchError,
        },
        EventMigrated {
            eth_event_id: EthEventId,
            accepted: bool,
        },
        AdditionalEventQueued {
            transaction_hash: EthTransactionId,
        },
    }

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T, I = ()>(_);

    #[pallet::storage]
    #[pallet::getter(fn get_next_tx_id)]
    pub type NextTxId<T: Config<I>, I: 'static = ()> = StorageValue<_, EthereumId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn get_eth_tx_lifetime_secs)]
    pub type EthTxLifetimeSecs<T: Config<I>, I: 'static = ()> = StorageValue<_, u64, ValueQuery>;

    #[pallet::storage]
    pub type RequestQueue<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BoundedVec<Request, T::MaxQueuedTxRequests>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn get_transaction_data)]
    pub type SettledTransactions<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, EthereumId, TransactionData<T::AccountId>, OptionQuery>;

    #[pallet::storage]
    pub type ActiveRequest<T: Config<I>, I: 'static = ()> =
        StorageValue<_, ActiveRequestData<BlockNumberFor<T>, T::AccountId>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_ethereum_range)]
    pub type ActiveEthereumRange<T: Config<I>, I: 'static = ()> =
        StorageValue<_, ActiveEthRange, OptionQuery>;

    #[pallet::storage]
    pub type EthereumEvents<T: Config<I>, I: 'static = ()> = StorageMap<
        _,
        Blake2_128Concat,
        EthereumEventsPartition,
        BoundedBTreeSet<T::AccountId, MaximumValidatorsBound>,
        ValueQuery,
    >;

    #[pallet::storage]
    pub type SubmittedEthBlocks<T: Config<I>, I: 'static = ()> = StorageMap<
        _,
        Blake2_128Concat,
        u32,
        BoundedBTreeSet<T::AccountId, MaximumValidatorsBound>,
        ValueQuery,
    >;

    // The number of blocks that make up a range
    #[pallet::storage]
    pub type EthBlockRangeSize<T: Config<I>, I: 'static = ()> =
        StorageValue<_, u32, ValueQuery, ConstU32<DEFAULT_ETH_RANGE>>;

    #[pallet::storage]
    pub type ProcessedEthereumEvents<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, EthTransactionId, EthProcessedEvent, OptionQuery>;

    /// Simple queue, to store additional events to be added in the next ethereum range.
    /// Entries must be of previous blocks.
    #[pallet::storage]
    pub type AdditionalEthereumEventsQueue<T: Config<I>, I: 'static = ()> =
        StorageValue<_, AdditionalEvents, ValueQuery>;

    /// The instance of the EthBridge contract that this pallet is configured to interact with.
    #[pallet::storage]
    #[pallet::getter(fn instance)]
    pub type Instance<T: Config<I>, I: 'static = ()> =
        StorageValue<_, EthBridgeInstance, ValueQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config<I>, I: 'static = ()> {
        pub eth_tx_lifetime_secs: u64,
        pub next_tx_id: EthereumId,
        pub eth_block_range_size: u32,
        pub instance: EthBridgeInstance,
        #[serde(skip)]
        pub _phantom: sp_std::marker::PhantomData<(T, I)>,
    }

    impl<T: Config<I>, I: 'static> Default for GenesisConfig<T, I> {
        fn default() -> Self {
            Self {
                _phantom: Default::default(),
                eth_tx_lifetime_secs: 60 * 30,
                next_tx_id: 0,
                eth_block_range_size: 20,
                instance: Default::default(),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config<I>, I: 'static> BuildGenesisConfig for GenesisConfig<T, I> {
        fn build(&self) {
            assert!(self.eth_block_range_size > 0, "`EthBlockRangeSize` should be greater than 0");

            EthTxLifetimeSecs::<T, I>::put(self.eth_tx_lifetime_secs);
            NextTxId::<T, I>::put(self.next_tx_id);
            EthBlockRangeSize::<T, I>::put(self.eth_block_range_size);

            STORAGE_VERSION.put::<Pallet<T, I>>();
            log::debug!("Setting EthBridgeInstance: {:?}", &self.instance);
            Instance::<T, I>::put(self.instance.clone());
        }
    }

    #[pallet::error]
    pub enum Error<T, I = ()> {
        CorroborateCallFailed,
        DuplicateConfirmation,
        DuplicateEventSubmission,
        EmptyFunctionName,
        ErrorAssigningSender,
        EthTxHashAlreadySet,
        EthTxHashMustBeSetBySender,
        ExceedsConfirmationLimit,
        ExceedsCorroborationLimit,
        ExceedsFunctionNameLimit,
        EventAlreadyProcessed,
        EventNotProcessed,
        FunctionEncodingError,
        FunctionNameError,
        HandlePublishingResultFailed,
        InvalidBytes,
        InvalidBytesLength,
        InvalidQueryResponseFromEthereum,
        InvalidCorroborateCalldata,
        InvalidCorroborateResult,
        InvalidECDSASignature,
        InvalidHashLength,
        InvalidHexString,
        InvalidParamData,
        InvalidSendCalldata,
        InvalidUint,
        InvalidAccountId,
        InvalidUTF8,
        MsgHashError,
        ParamsLimitExceeded,
        ParamTypeEncodingError,
        SendTransactionFailed,
        TxRequestQueueFull,
        TypeNameLengthExceeded,
        ValueLengthExceeded,
        ErrorGettingEthereumCallData,
        InvalidSendRequest,
        LowerParamsError,
        CallerIdLengthExceeded,
        NoActiveRequest,
        CannotCorroborateOwnTransaction,
        EventVotesFull,
        InvalidEventVote,
        EventVoteExists,
        NonActiveEthereumRange,
        VotingEnded,
        ValidatorNotFound,
        InvalidEthereumBlockRange,
        ErrorGettingFinalisedEthereumBlock,
        InvalidResponse,
        ErrorDecodingU32,
        EventBelongsInFutureRange,
        QuotaReachedForAdditionalEvents,
        EventAlreadyAccepted,
        InvalidInstance,
        AuthorNotSender,
        SigningError,
        InvalidCorroborationData,
    }

    #[pallet::call(weight(<T as Config<I>>::WeightInfo))]
    impl<T: Config<I>, I: 'static> Pallet<T, I> {
        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config<I>>::WeightInfo::add_confirmation(MAX_CONFIRMATIONS))]

        pub fn add_confirmation(
            origin: OriginFor<T>,
            request_id: u32,
            confirmation: ecdsa::Signature,
            author: Author<T>,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;

            if tx::is_active_request::<T, I>(request_id) {
                let mut req = ActiveRequest::<T, I>::get().expect("is active");

                if request::has_enough_confirmations::<T, I>(&req) {
                    return Ok(().into())
                }

                if matches!(req.request, Request::Send(_) if req.tx_data.is_some()) {
                    let sender = &req.tx_data.as_ref().expect("has data").sender;
                    if &author.account_id == sender {
                        return Ok(().into())
                    }
                }

                eth::verify_signature::<T, I>(req.confirmation.msg_hash, &author, &confirmation)?;

                ensure!(
                    !req.confirmation.confirmations.contains(&confirmation),
                    Error::<T, I>::DuplicateConfirmation
                );

                req.confirmation
                    .confirmations
                    .try_push(confirmation)
                    .map_err(|_| Error::<T, I>::ExceedsConfirmationLimit)?;

                match req.request {
                    Request::LowerProof(lower_req)
                        if request::has_enough_confirmations::<T, I>(&req) =>
                        request::complete_lower_proof_request::<T, I>(
                            &lower_req,
                            req.confirmation.confirmations,
                        )?,
                    _ => {
                        save_active_request_to_storage::<T, I>(req);
                    },
                }
            }

            Ok(().into())
        }

        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config<I>>::WeightInfo::add_eth_tx_hash())]

        pub fn add_eth_tx_hash(
            origin: OriginFor<T>,
            tx_id: EthereumId,
            eth_tx_hash: H256,
            author: Author<T>,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;

            if tx::is_active_request::<T, I>(tx_id) {
                let mut tx = ActiveRequest::<T, I>::get().expect("is active");

                if tx.tx_data.is_some() {
                    let mut data = tx.tx_data.expect("has data");

                    ensure!(data.eth_tx_hash == H256::zero(), Error::<T, I>::EthTxHashAlreadySet);
                    ensure!(
                        data.sender == author.account_id,
                        Error::<T, I>::EthTxHashMustBeSetBySender
                    );

                    data.eth_tx_hash = eth_tx_hash;
                    tx.tx_data = Some(data);

                    save_active_request_to_storage::<T, I>(tx);
                }
            }

            Ok(().into())
        }

        #[pallet::call_index(4)]
        #[pallet::weight( <T as pallet::Config<I>>::WeightInfo::add_corroboration().max(
        <T as Config<I>>::WeightInfo::add_corroboration_with_challenge(MAX_VALIDATOR_ACCOUNTS)
        ))]
        pub fn add_corroboration(
            origin: OriginFor<T>,
            tx_id: EthereumId,
            tx_succeeded: bool,
            tx_hash_is_valid: bool,
            author: Author<T>,
            replay_attempt: u16,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;

            if tx::is_active_request::<T, I>(tx_id) {
                let mut tx = ActiveRequest::<T, I>::get().expect("is active");

                if tx.tx_data.is_some() {
                    let data = tx.tx_data.as_mut().expect("has data");
                    ensure!(
                        replay_attempt == data.replay_attempt,
                        Error::<T, I>::InvalidCorroborationData
                    );

                    let author_is_sender = author.account_id == data.sender;
                    ensure!(!author_is_sender, Error::<T, I>::CannotCorroborateOwnTransaction);

                    if !util::requires_corroboration::<T, I>(&data, &author) {
                        return Ok(().into())
                    }

                    let tx_hash_corroborations = if tx_hash_is_valid {
                        &mut data.valid_tx_hash_corroborations
                    } else {
                        &mut data.invalid_tx_hash_corroborations
                    };

                    tx_hash_corroborations
                        .try_push(author.account_id.clone())
                        .map_err(|_| Error::<T, I>::ExceedsCorroborationLimit)?;

                    let matching_corroborations = if tx_succeeded {
                        &mut data.success_corroborations
                    } else {
                        &mut data.failure_corroborations
                    };

                    matching_corroborations
                        .try_push(author.account_id)
                        .map_err(|_| Error::<T, I>::ExceedsCorroborationLimit)?;

                    if util::has_enough_corroborations::<T, I>(matching_corroborations.len()) {
                        tx::finalize_state::<T, I>(tx.as_active_tx::<T, I>()?, tx_succeeded)?;
                    } else {
                        save_active_request_to_storage::<T, I>(tx);
                    }
                }
            }

            Ok(().into())
        }

        #[pallet::call_index(6)]
        #[pallet::weight( <T as
        pallet::Config<I>>::WeightInfo::submit_ethereum_events(MAX_VALIDATOR_ACCOUNTS,
        MAX_INCOMING_EVENTS_BATCH_SIZE).max(
            <T as Config<I>>::WeightInfo::submit_ethereum_events_and_process_batch(MAX_VALIDATOR_ACCOUNTS, MAX_INCOMING_EVENTS_BATCH_SIZE)
        ))]
        pub fn submit_ethereum_events(
            origin: OriginFor<T>,
            author: Author<T>,
            events_partition: EthereumEventsPartition,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;
            let instance = Instance::<T, I>::get();
            ensure!(instance.is_valid(), Error::<T, I>::InvalidInstance);

            let active_range = Self::active_ethereum_range()
                .ok_or_else(|| Error::<T, I>::NonActiveEthereumRange)?;
            ensure!(
                *events_partition.range() == active_range.range &&
                    events_partition.partition() == active_range.partition,
                Error::<T, I>::NonActiveEthereumRange
            );
            ensure!(
                Self::author_has_cast_event_vote(&author.account_id) == false,
                Error::<T, I>::EventVoteExists
            );

            let mut threshold_met = false;
            let mut votes = EthereumEvents::<T, I>::get(&events_partition);
            votes
                .try_insert(author.account_id.to_owned())
                .map_err(|_| Error::<T, I>::EventVotesFull)?;

            if votes.len() < T::Quorum::get_quorum() as usize {
                EthereumEvents::<T, I>::insert(&events_partition, votes);
            } else {
                threshold_met = true;
                process_ethereum_events_partition::<T, I>(
                    &instance.network,
                    &active_range,
                    &events_partition,
                    &author,
                );
                advance_partition::<T, I>(&active_range, &events_partition);
            }

            let final_weight = if threshold_met {
                <T as Config<I>>::WeightInfo::submit_ethereum_events(
                    MAX_VALIDATOR_ACCOUNTS,
                    MAX_INCOMING_EVENTS_BATCH_SIZE,
                )
            } else {
                <T as Config<I>>::WeightInfo::submit_ethereum_events_and_process_batch(
                    MAX_VALIDATOR_ACCOUNTS,
                    MAX_INCOMING_EVENTS_BATCH_SIZE,
                )
            };

            Ok(Some(final_weight).into())
        }

        #[pallet::call_index(7)]
        #[pallet::weight( <T as
        pallet::Config<I>>::WeightInfo::submit_latest_ethereum_block(MAX_VALIDATOR_ACCOUNTS).max(
            <T as Config<I>>::WeightInfo::submit_latest_ethereum_block_with_quorum(MAX_VALIDATOR_ACCOUNTS)
        ))]
        pub fn submit_latest_ethereum_block(
            origin: OriginFor<T>,
            author: Author<T>,
            latest_seen_block: u32,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;
            ensure!(Instance::<T, I>::get().is_valid(), Error::<T, I>::InvalidInstance);
            ensure!(Self::active_ethereum_range().is_none(), Error::<T, I>::VotingEnded);
            ensure!(
                Self::author_has_submitted_latest_block(&author.account_id) == false,
                Error::<T, I>::EventVoteExists
            );

            let eth_block_range_size = EthBlockRangeSize::<T, I>::get();
            let latest_finalised_block =
                events_helpers::compute_start_block_from_finalised_block_number(
                    latest_seen_block,
                    eth_block_range_size,
                )
                .map_err(|_| Error::<T, I>::InvalidEthereumBlockRange)?;
            let mut votes = SubmittedEthBlocks::<T, I>::get(&latest_finalised_block);
            votes.try_insert(author.account_id).map_err(|_| Error::<T, I>::EventVotesFull)?;

            SubmittedEthBlocks::<T, I>::insert(&latest_finalised_block, votes);

            let mut total_votes_count = 0;
            let mut submitted_blocks = Vec::new();

            for (eth_block_num, votes) in SubmittedEthBlocks::<T, I>::iter() {
                let vote_count = votes.len();
                total_votes_count += vote_count;
                submitted_blocks.push((eth_block_num, vote_count));
            }

            submitted_blocks.sort();

            let mut remaining_votes_threshold = T::Quorum::get_supermajority_quorum() as usize;
            let mut threshold_met = false;

            if total_votes_count >= remaining_votes_threshold as usize {
                threshold_met = true;
                let quorum = T::Quorum::get_quorum() as usize;
                let mut selected_range: EthBlockRange = Default::default();

                for (eth_block_num, votes_count) in submitted_blocks.iter() {
                    remaining_votes_threshold.saturating_reduce(*votes_count);
                    if remaining_votes_threshold < quorum {
                        selected_range = EthBlockRange {
                            start_block: *eth_block_num,
                            length: eth_block_range_size,
                        };
                        break
                    }
                }

                ActiveEthereumRange::<T, I>::put(ActiveEthRange {
                    range: selected_range,
                    partition: 0,
                    event_types_filter: T::ProcessedEventsHandler::get(),
                    additional_transactions: AdditionalEthereumEventsQueue::<T, I>::take(),
                });

                let _ = SubmittedEthBlocks::<T, I>::clear(
                    <MaximumValidatorsBound as sp_core::TypedGet>::get(),
                    None,
                );
            }

            let final_weight = if threshold_met {
                <T as Config<I>>::WeightInfo::submit_latest_ethereum_block_with_quorum(
                    MAX_VALIDATOR_ACCOUNTS,
                )
            } else {
                <T as Config<I>>::WeightInfo::submit_latest_ethereum_block(MAX_VALIDATOR_ACCOUNTS)
            };

            Ok(Some(final_weight).into())
        }

        #[pallet::call_index(8)]
        #[pallet::weight(<T as Config<I>>::WeightInfo::set_admin_setting())]

        pub fn set_admin_setting(
            origin: OriginFor<T>,
            value: AdminSettings,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            match value {
                AdminSettings::EthereumTransactionLifetimeSeconds(eth_tx_lifetime_secs) => {
                    EthTxLifetimeSecs::<T, I>::put(eth_tx_lifetime_secs);
                    Self::deposit_event(Event::<T, I>::EthTxLifetimeUpdated {
                        eth_tx_lifetime_secs,
                    });
                },
                AdminSettings::EthereumTransactionId(eth_tx_id) => {
                    NextTxId::<T, I>::put(eth_tx_id);
                    Self::deposit_event(Event::<T, I>::EthTxIdUpdated { eth_tx_id });
                },
                AdminSettings::RemoveActiveRequest => {
                    Self::remove_active_request_impl()?;
                },
                AdminSettings::QueueAdditionalEthereumEvent(transaction_hash) => {
                    ensure!(
                        !Self::ethereum_event_has_already_been_accepted(&transaction_hash),
                        Error::<T, I>::EventAlreadyAccepted
                    );

                    AdditionalEthereumEventsQueue::<T, I>::mutate(|transactions| {
                        transactions.try_insert(transaction_hash.clone())
                    })
                    .map_err(|_| Error::<T, I>::QuotaReachedForAdditionalEvents)?;
                    Self::deposit_event(Event::<T, I>::AdditionalEventQueued { transaction_hash });
                },
                AdminSettings::RestartEventDiscoveryOnRange => {
                    let _ = EthereumEvents::<T, I>::clear(100, None);
                },
                AdminSettings::SetEthBridgeInstance(instance) => {
                    ensure!(instance.is_valid(), Error::<T, I>::InvalidInstance);
                    let _ = EthereumEvents::<T, I>::clear(100, None);
                    let _ = EthereumEvents::<T, I>::clear(1, None);
                    Instance::<T, I>::put(instance);
                    if let Some(tx) = ActiveRequest::<T, I>::get() {
                        match tx.request {
                            Request::Send(tx_data) => {
                                set_up_active_tx::<T, I>(tx_data, None)?;
                            },
                            _ => {},
                        }
                    }
                },
            }

            Ok(().into())
        }
    }

    #[pallet::hooks]
    impl<T: Config<I>, I: 'static> Hooks<BlockNumberFor<T>> for Pallet<T, I> {
        fn offchain_worker(block_number: BlockNumberFor<T>) {
            if let Ok((author, finalised_block_number)) = setup_ocw::<T, I>(block_number) {
                if let Err(e) = process_active_request::<T, I>(author, finalised_block_number) {
                    log::error!("❌ Error processing currently active request: {:?}", e);
                }
            }
        }

        fn on_idle(_n: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            let base_on_idle = <T as pallet::Config<I>>::WeightInfo::base_on_idle();

            // the maximum cost of a processing unit
            let processing_unit = base_on_idle.saturating_add(
                <T as pallet::Config<I>>::WeightInfo::migrate_events_batch(
                    <ProcessingBatchBound as sp_core::Get<u32>>::get(),
                ),
            );

            let mut meter = WeightMeter::with_limit(remaining_weight);
            if !meter.can_consume(processing_unit) {
                return Weight::zero()
            }

            let instance = Instance::<T, I>::get();
            meter.consume(base_on_idle);

            if let Some(network) = T::EthereumEventsMigration::get_network() {
                if instance.network == network && instance.is_valid() {
                    if let Some(events_batch) =
                        T::EthereumEventsMigration::get_events_to_migrate(&instance.network)
                    {
                        let weight = Self::migrate_events_batch(&instance.network, events_batch);
                        meter.consume(weight);
                    }
                }
            }
            meter.consumed()
        }
    }

    fn save_active_request_to_storage<T: Config<I>, I: 'static>(
        mut tx: ActiveRequestData<BlockNumberFor<T>, T::AccountId>,
    ) {
        tx.last_updated = <frame_system::Pallet<T>>::block_number();
        ActiveRequest::<T, I>::put(tx);
    }

    fn setup_ocw<T: Config<I>, I: 'static>(
        block_number: BlockNumberFor<T>,
    ) -> Result<(Author<T>, BlockNumberFor<T>), DispatchError> {
        let caller_id =
            [&PALLET_NAME[..], &hex::encode(twox_64(&Instance::<T, I>::get().encode())).as_bytes()]
                .concat();
        log::debug!(
            "👷 Running offchain worker for block {:?} with caller_id: {:?}",
            block_number,
            caller_id
        );
        AVN::<T>::pre_run_setup(block_number, caller_id).map_err(|e| {
            if sp_io::offchain::is_validator() {
                // If a non validator node has OCW enabled, don't bother logging an error
                if e != DispatchError::from(avn_error::<T>::OffchainWorkerAlreadyRun) {
                    log::error!("❌ Unable to run offchain worker: {:?}", e);
                }
            }
            e
        })
    }

    // The core logic the OCW employs to fully resolve any currently active transaction:
    fn process_active_request<T: Config<I>, I: 'static>(
        author: Author<T>,
        finalised_block_number: BlockNumberFor<T>,
    ) -> Result<(), DispatchError> {
        if let Some(req) = ActiveRequest::<T, I>::get() {
            if finalised_block_number < req.last_updated {
                log::info!(
                    "👷 Last updated block: {:?} is not finalised, skipping confirmation. Request: {:?}, finalised block: {:?}",
                    req.last_updated, req.request, finalised_block_number
                );
                return Ok(())
            }

            let has_enough_confirmations = request::has_enough_confirmations::<T, I>(&req);

            match req.request {
                Request::LowerProof(lower_req) =>
                    if !has_enough_confirmations {
                        let confirmation =
                            eth::sign_msg_hash::<T, I>(&author, &req.confirmation.msg_hash)?;
                        if !req.confirmation.confirmations.contains(&confirmation) {
                            call::add_confirmation::<T, I>(
                                lower_req.lower_id,
                                confirmation,
                                author,
                            );
                        }
                    },
                Request::Send(_) => {
                    let tx = req.as_active_tx::<T, I>()?;
                    let self_is_sender = author.account_id == tx.data.sender;
                    // Plus 1 for sender
                    if !self_is_sender && !has_enough_confirmations {
                        let confirmation =
                            eth::sign_msg_hash::<T, I>(&author, &tx.confirmation.msg_hash)?;
                        if !tx.confirmation.confirmations.contains(&confirmation) {
                            call::add_confirmation::<T, I>(tx.request.tx_id, confirmation, author);
                        }
                    } else {
                        process_active_tx_request::<T, I>(
                            author,
                            tx,
                            self_is_sender,
                            has_enough_confirmations,
                        )?;
                    }
                },
            }
        }

        Ok(())
    }

    fn process_active_tx_request<T: Config<I>, I: 'static>(
        author: Author<T>,
        tx: ActiveTransactionData<T::AccountId>,
        self_is_sender: bool,
        tx_has_enough_confirmations: bool,
    ) -> Result<(), DispatchError> {
        let tx_is_sent = tx.data.eth_tx_hash != H256::zero();
        let tx_is_past_expiry = util::time_now::<T, I>() > tx.data.expiry;

        if self_is_sender && tx_has_enough_confirmations && !tx_is_sent {
            let lock_name =
                format!("eth_bridge_ocw::send::{}", tx.request.tx_id).as_bytes().to_vec();
            let mut lock = AVN::<T>::get_ocw_locker(&lock_name);

            // Protect against sending more than once
            if let Ok(guard) = lock.try_lock() {
                let eth_tx_hash = eth::send_tx::<T, I>(&author, &tx)?;
                call::add_eth_tx_hash::<T, I>(tx.request.tx_id, eth_tx_hash, author);
                guard.forget(); // keep the lock so we don't send again
            } else {
                log::info!(
                    "👷 Skipping sending txId: {:?} because ocw is locked already.",
                    tx.request.tx_id
                );
            };
        } else if !self_is_sender && (tx_is_sent || tx_is_past_expiry) {
            if util::requires_corroboration::<T, I>(&tx.data, &author) {
                match eth::corroborate::<T, I>(&tx, &author)? {
                    (Some(status), tx_hash_is_valid) => call::add_corroboration::<T, I>(
                        tx.request.tx_id,
                        status,
                        tx_hash_is_valid.unwrap_or_default(),
                        author,
                        tx.replay_attempt,
                    ),
                    (None, _) => {},
                }
            }
        }

        Ok(())
    }

    fn advance_partition<T: Config<I>, I: 'static>(
        active_range: &ActiveEthRange,
        approved_partition: &EthereumEventsPartition,
    ) {
        let next_active_range = if approved_partition.is_last() {
            let additional_transactions = AdditionalEthereumEventsQueue::<T, I>::take();
            ActiveEthRange {
                range: active_range.range.next_range(),
                partition: 0,
                event_types_filter: T::ProcessedEventsHandler::get(),
                additional_transactions,
            }
        } else {
            ActiveEthRange {
                partition: active_range.partition.saturating_add(1),
                ..active_range.clone()
            }
        };
        ActiveEthereumRange::<T, I>::put(next_active_range);
    }

    fn process_ethereum_events_partition<T: Config<I>, I: 'static>(
        network: &EthereumNetwork,
        active_range: &ActiveEthRange,
        partition: &EthereumEventsPartition,
        author: &Author<T>,
    ) {
        // Remove entry from storage. Ignore votes.
        let _ = EthereumEvents::<T, I>::take(partition);
        for discovered_event in partition.events().iter() {
            match ValidEvents::try_from(&discovered_event.event.event_id.signature).ok() {
                Some(valid_event) =>
                    if active_range.event_types_filter.contains(&valid_event) {
                        if discovered_event.block > active_range.range.end_block().into() {
                            <Pallet<T, I>>::deposit_event(Event::<T, I>::EventRejected {
                                eth_event_id: discovered_event.event.event_id.clone(),
                                reason: Error::<T, I>::EventBelongsInFutureRange.into(),
                            });
                            continue
                        }

                        if let Err(err) =
                            process_ethereum_event::<T, I>(network, &discovered_event.event)
                        {
                            log::error!(
                                "💔 Invalid event to process: {:?}. Error: {:?}",
                                discovered_event.event,
                                err
                            );
                            <Pallet<T, I>>::deposit_event(Event::<T, I>::EventRejected {
                                eth_event_id: discovered_event.event.event_id.clone(),
                                reason: err,
                            });
                        }
                    } else {
                        log::warn!("Ethereum event signature ({:?}) included in approved range ({:?}), but not part of the expected ones {:?}", &discovered_event.event.event_id.signature, active_range.range, active_range.event_types_filter);
                    },
                None => {
                    log::warn!(
                        "Unknown Ethereum event signature in range {:?}",
                        &discovered_event.event.event_id.signature
                    );
                },
            }
        }

        // Cleanup
        for (partition, votes) in EthereumEvents::<T, I>::drain() {
            log::info!("Collators with invalid votes on ethereum events (range: {:?}, partition: {}): {:?}", partition.range(), partition.partition(), votes);
            create_and_report_bridge_offence::<T, I>(
                &author.account_id,
                &votes.into_iter().collect(),
                offence::EthBridgeOffenceType::InvalidEthereumRangeData,
            )
        }
    }

    fn process_ethereum_event<T: Config<I>, I: 'static>(
        network: &EthereumNetwork,
        event: &EthEvent,
    ) -> Result<(), DispatchError> {
        ensure!(
            false == <<T as pallet::Config<I>>::ProcessedEventsChecker as pallet_avn::NetworkAwareProcessedEventsChecker>::processed_event_exists(network, &event.event_id.clone()),
            Error::<T, I>::EventAlreadyProcessed
        );

        // Add record of succesful processing via ProcessedEventsChecker
        <<T as pallet::Config<I>>::ProcessedEventsChecker as pallet_avn::NetworkAwareProcessedEventsChecker>::add_processed_event(network, &event.event_id.clone(), true)
            .map_err(|_| Error::<T, I>::EventAlreadyProcessed)?;

        match T::BridgeInterfaceNotification::on_incoming_event_processed(&event) {
            Ok(_) => {
                <Pallet<T, I>>::deposit_event(Event::<T, I>::EventAccepted {
                    eth_event_id: event.event_id.clone(),
                });
            },
            Err(err) => {
                log::error!("💔 Processing ethereum event failed: {:?}", err);
                <Pallet<T, I>>::deposit_event(Event::<T, I>::EventRejected {
                    eth_event_id: event.event_id.clone(),
                    reason: err,
                });
            },
        };

        Ok(())
    }

    #[pallet::validate_unsigned]
    impl<T: Config<I>, I: 'static> ValidateUnsigned for Pallet<T, I> {
        type Call = Call<T, I>;
        // Confirm that the call comes from an author before it can enter the pool:
        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            let reduce_priority: TransactionPriority = TransactionPriority::from(1000u64);

            match call {
                Call::add_confirmation { request_id, confirmation, author, signature } =>
                    if AVN::<T>::signature_is_valid(
                        &(
                            Instance::<T, I>::get().hash(),
                            ADD_CONFIRMATION_CONTEXT,
                            request_id,
                            confirmation,
                            &author.account_id,
                        ),
                        &author,
                        signature,
                    ) {
                        ValidTransaction::with_tag_prefix("EthBridgeAddConfirmation")
                            .and_provides((call, request_id))
                            .priority(TransactionPriority::max_value() - reduce_priority)
                            .longevity(64_u64)
                            .propagate(true)
                            .build()
                    } else {
                        InvalidTransaction::Custom(1u8).into()
                    },
                Call::add_eth_tx_hash { tx_id, eth_tx_hash, author, signature } =>
                    if AVN::<T>::signature_is_valid(
                        &(
                            Instance::<T, I>::get().hash(),
                            ADD_ETH_TX_HASH_CONTEXT,
                            tx_id,
                            eth_tx_hash,
                            &author.account_id,
                        ),
                        &author,
                        signature,
                    ) {
                        ValidTransaction::with_tag_prefix("EthBridgeAddReceipt")
                            .and_provides((call, tx_id))
                            .priority(TransactionPriority::max_value() - reduce_priority)
                            .longevity(64_u64)
                            .propagate(true)
                            .build()
                    } else {
                        InvalidTransaction::Custom(2u8).into()
                    },
                Call::add_corroboration {
                    tx_id,
                    tx_succeeded,
                    tx_hash_is_valid,
                    author,
                    replay_attempt,
                    signature,
                } =>
                    if AVN::<T>::signature_is_valid(
                        &(
                            Instance::<T, I>::get().hash(),
                            ADD_CORROBORATION_CONTEXT,
                            tx_id,
                            tx_succeeded,
                            tx_hash_is_valid,
                            &author.account_id,
                            replay_attempt,
                        ),
                        &author,
                        signature,
                    ) {
                        ValidTransaction::with_tag_prefix("EthBridgeAddCorroboration")
                            .and_provides((call, tx_id))
                            .priority(TransactionPriority::max_value() - reduce_priority)
                            .longevity(64_u64)
                            .propagate(true)
                            .build()
                    } else {
                        InvalidTransaction::Custom(3u8).into()
                    },
                Call::submit_ethereum_events { author, events_partition, signature } =>
                    if Self::does_range_matches_active(&events_partition) &&
                        AVN::<T>::signature_is_valid(
                            &(
                                Instance::<T, I>::get().hash(),
                                &SUBMIT_ETHEREUM_EVENTS_HASH_CONTEXT,
                                &author.account_id,
                                events_partition,
                            ),
                            &author,
                            signature,
                        )
                    {
                        ValidTransaction::with_tag_prefix("EthBridgeAddEventRange")
                            .and_provides((
                                call,
                                events_partition.range(),
                                events_partition.partition(),
                            ))
                            .priority(TransactionPriority::max_value() - reduce_priority)
                            .longevity(64_u64)
                            .propagate(true)
                            .build()
                    } else {
                        InvalidTransaction::Custom(4u8).into()
                    },
                Call::submit_latest_ethereum_block { author, latest_seen_block, signature } => {
                    if Self::active_ethereum_range().is_some() {
                        return InvalidTransaction::Custom(5u8).into()
                    }
                    if AVN::<T>::signature_is_valid(
                        &(
                            Instance::<T, I>::get().hash(),
                            &SUBMIT_LATEST_ETH_BLOCK_CONTEXT,
                            &author.account_id,
                            *latest_seen_block,
                        ),
                        &author,
                        signature,
                    ) {
                        ValidTransaction::with_tag_prefix("EthBridgeAddLatestEthBlock")
                            .and_provides((call, latest_seen_block))
                            .priority(TransactionPriority::max_value() - reduce_priority)
                            .longevity(64_u64)
                            .propagate(true)
                            .build()
                    } else {
                        InvalidTransaction::Custom(5u8).into()
                    }
                },
                _ => InvalidTransaction::Call.into(),
            }
        }
    }

    impl<T: Config<I>, I: 'static> BridgeInterface for Pallet<T, I> {
        fn publish(
            function_name: &[u8],
            params: &[(Vec<u8>, Vec<u8>)],
            caller_id: Vec<u8>,
        ) -> Result<EthereumId, DispatchError> {
            let tx_id = request::add_new_send_request::<T, I>(function_name, params, &caller_id)
                .map_err(|e| DispatchError::Other(e.into()))?;

            Self::deposit_event(Event::<T, I>::PublishToEthereum {
                tx_id,
                function_name: function_name.to_vec(),
                params: params.to_vec(),
                caller_id,
            });

            Ok(tx_id)
        }

        fn generate_lower_proof(
            lower_id: LowerId,
            params: &LowerParams,
            caller_id: Vec<u8>,
        ) -> Result<(), DispatchError> {
            // Note: we are not checking the queue for duplicates because we trust the calling
            // pallet
            request::add_new_lower_proof_request::<T, I>(lower_id, params, &caller_id)?;

            Self::deposit_event(Event::<T, I>::LowerProofRequested {
                lower_id,
                params: *params,
                caller_id,
            });

            Ok(())
        }

        fn read_bridge_contract(
            account_id_bytes: Vec<u8>,
            function_name: &[u8],
            params: &[(Vec<u8>, Vec<u8>)],
            eth_block: Option<u32>,
        ) -> Result<Vec<u8>, DispatchError> {
            let account_id = T::AccountId::decode(&mut &account_id_bytes[..])
                .map_err(|_| Error::<T, I>::InvalidAccountId)?;
            let calldata = eth::abi_encode_function::<T, I>(function_name, params)?;

            eth::make_ethereum_call::<Vec<u8>, T, I>(
                &account_id,
                "view",
                calldata,
                |data| Ok(data),
                eth_block,
                None,
            )
        }

        fn latest_finalised_ethereum_block() -> Result<u32, DispatchError> {
            let response = AVN::<T>::get_data_from_service(String::from("/eth/latest_block"))
                .map_err(|e| {
                    log::error!("❌ Error getting finalised ethereum block: {:?}", e);
                    Error::<T, I>::ErrorGettingFinalisedEthereumBlock
                })?;

            let latest_block_bytes = hex::decode(&response).map_err(|e| {
                log::error!("❌ Error decoding finalised eth block data {:?}", e);
                Error::<T, I>::InvalidResponse
            })?;

            let latest_block = u32::decode(&mut &latest_block_bytes[..]).map_err(|e| {
                log::error!("❌ Finalised block is not a valid u32: {:?}", e);
                Error::<T, I>::ErrorDecodingU32
            })?;

            Ok(latest_block)
        }
    }
}

impl<T: Config<I>, I: 'static> Pallet<T, I> {
    pub fn signatures() -> Vec<H256> {
        match Self::active_ethereum_range() {
            Some(active_range) => active_range
                .event_types_filter
                .into_iter()
                .map(|valid_event| valid_event.signature())
                .collect::<Vec<H256>>(),
            None => Default::default(),
        }
    }
    pub fn submit_vote(
        account_id: T::AccountId,
        events_partition: EthereumEventsPartition,
        signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
    ) -> Result<(), ()> {
        let validator: Author<T> = AVN::<T>::validators()
            .into_iter()
            .filter(|v| v.account_id == account_id)
            .nth(0)
            .ok_or_else(|| {
                log::warn!("Events vote sender({:?}) is not a member of authors", &account_id);
                ()
            })?;

        submit_ethereum_events::<T, I>(validator, events_partition, signature)
    }

    pub fn submit_latest_ethereum_block_vote(
        account_id: T::AccountId,
        latest_seen_block: u32,
        signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
    ) -> Result<(), ()> {
        let validator: Author<T> = AVN::<T>::validators()
            .into_iter()
            .filter(|v| v.account_id == account_id)
            .nth(0)
            .ok_or_else(|| {
                log::warn!(
                    "Latest ethereum block vote sender({:?}) is not a member of authors",
                    &account_id
                );
                ()
            })?;

        submit_latest_ethereum_block::<T, I>(validator, latest_seen_block, signature)
    }

    pub fn migrate_events_batch(
        network: &EthereumNetwork,
        events_batch: BoundedVec<EventMigration, ProcessingBatchBound>,
    ) -> Weight {
        let mut counter = 0;
        events_batch.into_iter().for_each(|migration| {
            counter.saturating_inc();
            match <<T as pallet::Config<I>>::ProcessedEventsChecker as pallet_avn::NetworkAwareProcessedEventsChecker>::add_processed_event(
                network,
                &migration.event_id,
                migration.outcome,
            ) {
                Ok(_) => {
                    log::debug!(
                        "Migrated processed event: {:?} to eth-bridge pallet",
                        &migration.event_id
                    );
                    <Pallet<T, I>>::deposit_event(Event::EventMigrated {
                        eth_event_id: migration.event_id,
                        accepted: migration.outcome,
                    });
                },
                Err(error) => {
                    log::error!(
                        "Error {:?} while migrating processed event: {:?} to eth-bridge pallet",
                        error,
                        &migration.event_id
                    );
                    migration.return_entry();
                },
            }
        });

        <T as pallet::Config<I>>::WeightInfo::migrate_events_batch(counter)
    }

    pub fn remove_active_request_impl() -> DispatchResultWithPostInfo {
        let req = ActiveRequest::<T, I>::get();
        ensure!(req.is_some(), Error::<T, I>::NoActiveRequest);

        let request_id;
        match req.expect("request is not empty").request {
            Request::Send(send_req) => {
                request_id = send_req.tx_id;
                let _ = T::BridgeInterfaceNotification::process_result(
                    send_req.tx_id,
                    send_req.caller_id.clone().into(),
                    false,
                );
            },
            Request::LowerProof(lower_req) => {
                request_id = lower_req.lower_id;
                let _ = T::BridgeInterfaceNotification::process_lower_proof_result(
                    lower_req.lower_id,
                    lower_req.caller_id.clone().into(),
                    Err(()),
                );
            },
        };

        request::process_next_request::<T, I>();
        Self::deposit_event(Event::<T, I>::ActiveRequestRemoved { request_id });
        Ok(().into())
    }

    fn ethereum_event_has_already_been_accepted(tx_hash: &H256) -> bool {
        if let Some(processed_event) = ProcessedEthereumEvents::<T, I>::get(tx_hash) {
            if processed_event.accepted {
                return true
            }
        }
        false
    }

    fn does_range_matches_active(events_partition: &EthereumEventsPartition) -> bool {
        if let Some(active_range) = Self::active_ethereum_range() {
            if *events_partition.range() == active_range.range &&
                events_partition.partition() == active_range.partition
            {
                return true
            }
        }
        false
    }

    pub fn author_has_cast_event_vote(author: &T::AccountId) -> bool {
        for (_partition, votes) in EthereumEvents::<T, I>::iter() {
            if votes.contains(&author) {
                return true
            }
        }
        false
    }

    pub fn author_has_submitted_latest_block(author: &T::AccountId) -> bool {
        for (_block_num, votes) in SubmittedEthBlocks::<T, I>::iter() {
            if votes.contains(&author) {
                return true
            }
        }
        false
    }
}

impl<T: Config<I>, I: 'static> ProcessedEventsChecker for Pallet<T, I> {
    fn processed_event_exists(event_id: &EthEventId) -> bool {
        <ProcessedEthereumEvents<T, I>>::contains_key(event_id.transaction_hash)
    }

    fn add_processed_event(event_id: &EthEventId, accepted: bool) -> Result<(), ()> {
        let tx_hash = event_id.transaction_hash.to_owned();

        // Data from other pallets may allow multiple entries of the same tx_id. We want to preserve
        // the one that was accepted.
        ensure!(!Self::ethereum_event_has_already_been_accepted(&tx_hash), ());

        // Handle legacy lifts
        let id = if event_types::LEGACY_LIFT_SIGNATURE
            .iter()
            .any(|legacy| legacy.eq(&event_id.signature))
        {
            ValidEvents::Lifted
        } else {
            ValidEvents::try_from(&event_id.signature)?
        };
        ProcessedEthereumEvents::<T, I>::insert(tx_hash, EthProcessedEvent { id, accepted });
        Ok(())
    }
}

impl<T: Config<I>, I: 'static> NetworkAwareProcessedEventsChecker for Pallet<T, I> {
    fn processed_event_exists(network: &EthereumNetwork, event_id: &EthEventId) -> bool {
        let instance = Instance::<T, I>::get();
        if instance.is_valid() == false || instance.network != *network {
            return false
        }
        <Self as ProcessedEventsChecker>::processed_event_exists(event_id)
    }

    fn add_processed_event(
        network: &EthereumNetwork,
        event_id: &EthEventId,
        accepted: bool,
    ) -> Result<(), EventProcessingError> {
        let instance = Instance::<T, I>::get();
        ensure!(instance.is_valid(), EventProcessingError::InvalidInstance);
        ensure!(instance.network == *network, EventProcessingError::InvalidNetwork);

        <Self as ProcessedEventsChecker>::add_processed_event(event_id, accepted)
            .map_err(|_| EventProcessingError::EventAlreadyProcessed)
    }
}

impl<T: Config<I>, I: 'static> EthereumEventsMigration for Pallet<T, I> {
    fn get_network() -> Option<EthereumNetwork> {
        let instance = Instance::<T, I>::get();
        if instance.is_valid() == false {
            return None
        }
        Some(instance.network)
    }

    fn get_events_to_migrate(
        network: &EthereumNetwork,
    ) -> Option<BoundedVec<EventMigration, ProcessingBatchBound>> {
        let instance = Instance::<T, I>::get();
        if !instance.is_valid() || instance.network != *network {
            return None
        }

        let batch_size: u32 = ProcessingBatchBound::get();
        let entry_return = |event: &EthEventId, outcome: bool| {
            let id = if let Ok(event) = ValidEvents::try_from(&event.signature) {
                event
            } else {
                log::warn!("Unknown event signature: {:?}", &event.signature);
                return
            };

            ProcessedEthereumEvents::<T, I>::insert(
                event.transaction_hash.clone(),
                EthProcessedEvent { id, accepted: outcome },
            )
        };

        let migration_batch: Vec<EventMigration> = ProcessedEthereumEvents::<T, I>::iter()
            .take(batch_size as usize)
            .map(|(tx_hash, event)| {
                let event_id =
                    EthEventId { transaction_hash: tx_hash, signature: event.id.signature() };
                EventMigration {
                    event_id,
                    outcome: event.accepted,
                    entry_return_impl: entry_return,
                }
            })
            .collect();

        if migration_batch.is_empty() {
            return None
        }

        migration_batch.iter().for_each(|event_to_migrate| {
            ProcessedEthereumEvents::<T, I>::remove(&event_to_migrate.event_id.transaction_hash);
        });
        Some(BoundedVec::<EventMigration, ProcessingBatchBound>::truncate_from(migration_batch))
    }
}
