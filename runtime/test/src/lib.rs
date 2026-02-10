#![cfg_attr(not(feature = "std"), no_std)]
// `construct_runtime!` does a lot of recursion and requires us to increase the limit to 256.
#![recursion_limit = "256"]

// Make the WASM binary available.
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

extern crate alloc;

pub mod apis;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarks;
mod configs;
pub mod proxy_config;

use core::cmp::Ordering;

use codec::{Decode, Encode};
use scale_info::TypeInfo;

use sp_runtime::{generic, impl_opaque_keys};
pub use sp_runtime::{MultiAddress, Perbill, Permill, RuntimeDebug};

use sp_std::{prelude::*, vec::Vec};

#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;

use frame_support::{
    parameter_types,
    traits::{
        fungible::HoldConsideration, AsEnsureOriginWithArg, ConstU32, Contains, Currency,
        LinearStoragePrice, OnUnbalanced, PrivilegeCmp,
    },
    weights::{constants::WEIGHT_REF_TIME_PER_SECOND, Weight},
};
pub use frame_system::{
    limits::{BlockLength, BlockWeights},
    EnsureRoot, EnsureSigned, Event as SystemEvent, EventRecord, Phase,
};
use proxy_config::AvnProxyConfig;
pub use sp_consensus_aura::sr25519::AuthorityId as AuraId;

use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
use pallet_session::historical::{self as pallet_session_historical};
use sp_authority_discovery::AuthorityId as AuthorityDiscoveryId;

use pallet_avn::sr25519::AuthorityId as AvnId;

pub use pallet_avn_proxy::{Event as AvnProxyEvent, ProvableProxy};
use pallet_eth_bridge_runtime_api::InstanceId;
use pallet_parachain_staking;
use sp_avn_common::{
    constants::{currency::*, time::*},
    eth::EthBridgeInstance,
    event_discovery::{AdditionalEvents, EthBlockRange, EthereumEventsPartition},
    InnerCallValidator, Proof,
};

use crate::apis::RUNTIME_API_VERSIONS;
pub use sp_avn_common::primitives::{AccountId, Signature};
pub(crate) use sp_avn_common::primitives::{Balance, BlockNumber, Hash, Moment, Nonce};

use runtime_common::{weights, Address, Header, TransactionByteFee, WeightToFee};

pub type NegativeImbalance<T> = <pallet_balances::Pallet<T> as Currency<
    <T as frame_system::Config>::AccountId,
>>::NegativeImbalance;

/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;

/// A Block signed with a Justification
pub type SignedBlock = generic::SignedBlock<Block>;

/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;

/// The extension to the basic transaction logic.
#[docify::export(template_signed_extra)]
pub type TxExtension = cumulus_pallet_weight_reclaim::StorageWeightReclaim<
    Runtime,
    (
        frame_system::CheckNonZeroSender<Runtime>,
        frame_system::CheckSpecVersion<Runtime>,
        frame_system::CheckTxVersion<Runtime>,
        frame_system::CheckGenesis<Runtime>,
        frame_system::CheckEra<Runtime>,
        frame_system::CheckNonce<Runtime>,
        frame_system::CheckWeight<Runtime>,
        pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
        frame_metadata_hash_extension::CheckMetadataHash<Runtime>,
    ),
>;

/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic =
    generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, TxExtension>;

/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
    Runtime,
    Block,
    frame_system::ChainContext<Runtime>,
    Runtime,
    AllPalletsWithSystem,
    (pallet_validators_manager::migration::ValidatorsManagerMigrations<Runtime>,),
>;

impl_opaque_keys! {
    pub struct SessionKeys {
        pub aura: Aura,
        pub authority_discovery: AuthorityDiscovery,
        pub im_online: ImOnline,
        pub avn: Avn,
    }
}

#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
    spec_name: alloc::borrow::Cow::Borrowed("avn-test-parachain"),
    impl_name: alloc::borrow::Cow::Borrowed("avn-test-parachain"),
    authoring_version: 1,
    spec_version: 201,
    impl_version: 0,
    apis: RUNTIME_API_VERSIONS,
    transaction_version: 1,
    system_version: 1,
};

#[docify::export]
mod block_times {
    use crate::MILLISECS_PER_BLOCK;

    /// This determines the average expected block time that we are targeting. Blocks will be
    /// produced at a minimum duration defined by `SLOT_DURATION`. `SLOT_DURATION` is picked up by
    /// `pallet_timestamp` which is in turn picked up by `pallet_aura` to implement `fn
    /// slot_duration()`.
    ///
    /// Change this to adjust the block time.
    pub const RUNTIME_MILLISECS_PER_BLOCK: u64 = MILLI_SECS_PER_BLOCK;

    // NOTE: Currently it is not possible to change the slot duration after the chain has started.
    // Attempting to do so will brick block production.
    pub const SLOT_DURATION: u64 = RUNTIME_MILLISECS_PER_BLOCK;
}
pub use block_times::*;

/// The existential deposit. Set to 1/10 of the Connected Relay Chain.
pub const EXISTENTIAL_DEPOSIT: Balance = 0;

/// We assume that ~5% of the block weight is consumed by `on_initialize` handlers. This is
/// used to limit the maximal weight of a single extrinsic.
const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_percent(5);

/// We allow `Normal` extrinsics to fill up the block up to 75%, the rest can be used by
/// `Operational` extrinsics.
const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

#[docify::export(max_block_weight)]
/// We allow for 2 seconds of compute with a 6 second average block time.
const MAXIMUM_BLOCK_WEIGHT: Weight = Weight::from_parts(
    WEIGHT_REF_TIME_PER_SECOND.saturating_mul(2),
    cumulus_primitives_core::relay_chain::MAX_POV_SIZE as u64,
);

#[docify::export]
mod async_backing_params {
    /// Maximum number of blocks simultaneously accepted by the Runtime, not yet included
    /// into the relay chain.
    pub(crate) const UNINCLUDED_SEGMENT_CAPACITY: u32 = 3;
    /// How many parachain blocks are processed by the relay chain per parent. Limits the
    /// number of blocks authored per slot.
    pub(crate) const BLOCK_PROCESSING_VELOCITY: u32 = 1;
    /// Relay chain slot duration, in milliseconds.
    pub(crate) const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32 = 6000;
}
pub(crate) use async_backing_params::*;

#[docify::export]
/// Aura consensus hook
type ConsensusHook = cumulus_pallet_aura_ext::FixedVelocityConsensusHook<
    Runtime,
    RELAY_CHAIN_SLOT_DURATION_MILLIS,
    BLOCK_PROCESSING_VELOCITY,
    UNINCLUDED_SEGMENT_CAPACITY,
>;

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
    NativeVersion { runtime_version: VERSION, can_author_with: Default::default() }
}

/// Use this filter to block users from calling extrinsics listed here.
pub struct RestrictedEndpointFilter;
impl Contains<RuntimeCall> for RestrictedEndpointFilter {
    fn contains(c: &RuntimeCall) -> bool {
        !matches!(
            c,
            RuntimeCall::ParachainStaking(pallet_parachain_staking::Call::join_candidates { .. }) |
                RuntimeCall::ParachainStaking(
                    pallet_parachain_staking::Call::schedule_leave_candidates { .. }
                ) |
                RuntimeCall::ParachainStaking(
                    pallet_parachain_staking::Call::execute_leave_candidates { .. }
                ) |
                RuntimeCall::ParachainStaking(
                    pallet_parachain_staking::Call::cancel_leave_candidates { .. }
                ) /* Allow the following direct staking extrinsics: */
                  /*
                      Call::ParachainStaking(pallet_parachain_staking::Call::nominate {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::nominator_bond_more {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::schedule_nominator_bond_less {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::schedule_revoke_nomination {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::execute_nomination_request {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::cancel_nomination_request {..}) |

                      Call::ParachainStaking(pallet_parachain_staking::Call::schedule_candidate_bond_less {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::execute_candidate_bond_less {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::cancel_candidate_bond_less {..}) |

                      Call::ParachainStaking(pallet_parachain_staking::Call::schedule_leave_nominators {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::execute_leave_nominators {..}) |
                      Call::ParachainStaking(pallet_parachain_staking::Call::cancel_leave_nominators {..}) |

                      Call::ParachainStaking(pallet_parachain_staking::Call::hotfix_remove_nomination_requests_exited_candidates{..})
                  */
        )
    }
}

pub type MainEthBridge = pallet_eth_bridge::Instance1;
pub type SecondaryEthBridge = pallet_eth_bridge::Instance2;
const MAIN_ETH_BRIDGE_ID: u8 = 1u8;
const SECONDARY_ETH_BRIDGE_ID: u8 = 2u8;

#[frame_support::runtime]
mod runtime {
    #[runtime::runtime]
    #[runtime::derive(
        RuntimeCall,
        RuntimeEvent,
        RuntimeError,
        RuntimeOrigin,
        RuntimeFreezeReason,
        RuntimeHoldReason,
        RuntimeSlashReason,
        RuntimeLockId,
        RuntimeTask,
        RuntimeViewFunction
    )]
    pub struct Runtime;

    // System support stuff.
    #[runtime::pallet_index(0)]
    pub type System = frame_system;

    #[runtime::pallet_index(1)]
    pub type ParachainSystem = cumulus_pallet_parachain_system;

    #[runtime::pallet_index(2)]
    pub type Timestamp = pallet_timestamp;

    #[runtime::pallet_index(3)]
    pub type ParachainInfo = parachain_info;

    #[runtime::pallet_index(4)]
    pub type WeightReclaim = cumulus_pallet_weight_reclaim;

    // Monetary stuff.
    #[runtime::pallet_index(10)]
    pub type Balances = pallet_balances;

    #[runtime::pallet_index(11)]
    pub type TransactionPayment = pallet_transaction_payment;

    // Collator support. The order of these 4 are important and shall not change.
    #[runtime::pallet_index(20)]
    pub type Authorship = pallet_authorship;

    #[runtime::pallet_index(22)]
    pub type Session = pallet_session;

    #[runtime::pallet_index(23)]
    pub type Aura = pallet_aura;

    #[runtime::pallet_index(24)]
    pub type AuraExt = cumulus_pallet_aura_ext;

    #[runtime::pallet_index(96)]
    pub type ParachainStaking = pallet_parachain_staking;

    // Since the ValidatorsManager integrates with the ParachainStaking pallet, we want to
    // initialise after it.
    #[runtime::pallet_index(18)]
    pub type ValidatorsManager = pallet_validators_manager;

    // XCM helpers.
    #[runtime::pallet_index(30)]
    pub type XcmpQueue = cumulus_pallet_xcmp_queue;

    #[runtime::pallet_index(31)]
    pub type PolkadotXcm = pallet_xcm;

    #[runtime::pallet_index(32)]
    pub type CumulusXcm = cumulus_pallet_xcm;

    #[runtime::pallet_index(33)]
    pub type MessageQueue = pallet_message_queue;

    // Substrate pallets
    #[runtime::pallet_index(60)]
    pub type Assets = pallet_assets;

    #[runtime::pallet_index(62)]
    pub type Sudo = pallet_sudo;

    #[runtime::pallet_index(70)]
    pub type AuthorityDiscovery = pallet_authority_discovery;

    #[runtime::pallet_index(71)]
    pub type Historical = pallet_session_historical;

    #[runtime::pallet_index(72)]
    pub type Offences = pallet_offences;

    #[runtime::pallet_index(73)]
    pub type ImOnline = pallet_im_online;

    #[runtime::pallet_index(74)]
    pub type Utility = pallet_utility;

    // Rest of AvN pallets
    #[runtime::pallet_index(81)]
    pub type Avn = pallet_avn;

    #[runtime::pallet_index(83)]
    pub type AvnOffenceHandler = pallet_avn_offence_handler;

    #[runtime::pallet_index(84)]
    pub type EthereumEvents = pallet_ethereum_events;

    #[runtime::pallet_index(86)]
    pub type NftManager = pallet_nft_manager;

    #[runtime::pallet_index(87)]
    pub type TokenManager = pallet_token_manager;

    #[runtime::pallet_index(88)]
    pub type Summary = pallet_summary<Instance1>;

    #[runtime::pallet_index(89)]
    pub type AvnProxy = pallet_avn_proxy;

    #[runtime::pallet_index(91)]
    pub type EthBridge = pallet_eth_bridge<Instance1>;

    #[runtime::pallet_index(92)]
    pub type AvnAnchor = pallet_avn_anchor;

    #[runtime::pallet_index(110)]
    pub type AnchorSummary = pallet_summary<Instance2>;

    #[runtime::pallet_index(111)]
    pub type EthSecondBridge = pallet_eth_bridge<Instance2>;

    #[runtime::pallet_index(97)]
    pub type Preimage = pallet_preimage;

    #[runtime::pallet_index(98)]
    pub type Scheduler = pallet_scheduler;

    #[runtime::pallet_index(103)]
    pub type CrossChainVoting = pallet_cross_chain_voting;
}

#[docify::export(register_validate_block)]
cumulus_pallet_parachain_system::register_validate_block! {
    Runtime = Runtime,
    BlockExecutor = cumulus_pallet_aura_ext::BlockExecutor::<Runtime, Executive>,
}
