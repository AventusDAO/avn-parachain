#![cfg_attr(not(feature = "std"), no_std)]
// `construct_runtime!` does a lot of recursion and requires us to increase the limit to 256.
#![recursion_limit = "256"]

// Make the WASM binary available.
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

pub mod governance;
pub mod proxy_config;
pub mod xcm_config;

use core::cmp::Ordering;

use codec::{Decode, Encode};
use scale_info::TypeInfo;

use sp_runtime::RuntimeAppPublic;

use cumulus_pallet_parachain_system::RelayNumberStrictlyIncreases;
use polkadot_runtime_common::xcm_sender::NoPriceForMessageDelivery;
use sp_api::impl_runtime_apis;
use sp_core::{crypto::KeyTypeId, ConstU128, OpaqueMetadata, H160};
use sp_runtime::{
    create_runtime_str, generic, impl_opaque_keys,
    traits::{Block as BlockT, ConvertInto},
    transaction_validity::{TransactionPriority, TransactionSource, TransactionValidity},
    ApplyExtrinsicResult,
};
pub extern crate alloc;
use alloc::collections::BTreeSet;

use sp_std::prelude::*;
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;

use cumulus_primitives_core::{AggregateMessageOrigin, ParaId};
use frame_support::{
    construct_runtime, derive_impl,
    dispatch::DispatchClass,
    genesis_builder_helper::{build_config, create_default_config},
    parameter_types,
    traits::{
        fungible::HoldConsideration, AsEnsureOriginWithArg, ConstBool, ConstU32, ConstU64,
        Contains, Currency, Imbalance, LinearStoragePrice, OnUnbalanced, PrivilegeCmp,
        TransformOrigin,
    },
    weights::{constants::WEIGHT_REF_TIME_PER_SECOND, ConstantMultiplier, Weight},
    PalletId,
};
use frame_system::{
    limits::{BlockLength, BlockWeights},
    EnsureRoot, EnsureSigned,
};

use governance::pallet_custom_origins;
use parachains_common::message_queue::{NarrowOriginToSibling, ParaIdToSibling};
use proxy_config::AvnProxyConfig;
pub use sp_consensus_aura::sr25519::AuthorityId as AuraId;
pub use sp_runtime::{MultiAddress, Perbill, Permill, RuntimeDebug};
use xcm_config::XcmOriginToTransactDispatchOrigin;

#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;

// Polkadot imports
use polkadot_runtime_common::{BlockHashCount, SlowAdjustingFeeUpdate};

use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
use pallet_session::historical::{self as pallet_session_historical};
use sp_authority_discovery::AuthorityId as AuthorityDiscoveryId;

use pallet_avn::sr25519::AuthorityId as AvnId;

use pallet_avn_proxy::ProvableProxy;
use sp_avn_common::{
    event_discovery::{
        filters::{AllEventsFilter, NoEventsFilter},
        AdditionalEvents, EthBlockRange, EthBridgeEventsFilter, EthereumEventsPartition,
    },
    event_types::ValidEvents,
    InnerCallValidator, Proof,
};

use pallet_parachain_staking;
pub type NegativeImbalance<T> = <pallet_balances::Pallet<T> as Currency<
    <T as frame_system::Config>::AccountId,
>>::NegativeImbalance;

/// Logic for the staking pot to get all the fees, including tips.
pub struct ToStakingPot<R>(sp_std::marker::PhantomData<R>);
impl<R> OnUnbalanced<NegativeImbalance<R>> for ToStakingPot<R>
where
    R: pallet_balances::Config + pallet_parachain_staking::Config,
    <R as frame_system::Config>::AccountId: From<AccountId>,
    <R as frame_system::Config>::AccountId: Into<AccountId>,
    <R as frame_system::Config>::RuntimeEvent: From<pallet_balances::Event<R>>,
{
    fn on_nonzero_unbalanced(amount: NegativeImbalance<R>) {
        let staking_pot_address =
            <pallet_parachain_staking::Pallet<R>>::compute_reward_pot_account_id();
        <pallet_balances::Pallet<R>>::resolve_creating(&staking_pot_address, amount);
    }
}

pub struct DealWithFees<R>(sp_std::marker::PhantomData<R>);
impl<R> OnUnbalanced<NegativeImbalance<R>> for DealWithFees<R>
where
    R: pallet_balances::Config + pallet_parachain_staking::Config,
    <R as frame_system::Config>::AccountId: From<AccountId>,
    <R as frame_system::Config>::AccountId: Into<AccountId>,
    <R as frame_system::Config>::RuntimeEvent: From<pallet_balances::Event<R>>,
{
    fn on_unbalanceds<B>(mut fees_then_tips: impl Iterator<Item = NegativeImbalance<R>>) {
        if let Some(mut fees) = fees_then_tips.next() {
            if let Some(tips) = fees_then_tips.next() {
                tips.merge_into(&mut fees);
            }
            <ToStakingPot<R> as OnUnbalanced<_>>::on_unbalanced(fees);
        }
    }
}

pub use node_primitives::{AccountId, Signature};
use node_primitives::{Balance, BlockNumber, Hash, Nonce};

use runtime_common::{
    constants::{currency::*, time::*},
    weights, Address, Header, OperationalFeeMultiplier, TransactionByteFee, WeightToFee,
};
use weights::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight};

/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;

/// A Block signed with a Justification
pub type SignedBlock = generic::SignedBlock<Block>;

/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;

/// The SignedExtension to the basic transaction logic.
pub type SignedExtra = (
    frame_system::CheckNonZeroSender<Runtime>,
    frame_system::CheckSpecVersion<Runtime>,
    frame_system::CheckTxVersion<Runtime>,
    frame_system::CheckGenesis<Runtime>,
    frame_system::CheckEra<Runtime>,
    frame_system::CheckNonce<Runtime>,
    frame_system::CheckWeight<Runtime>,
    pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
);

/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic =
    generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, SignedExtra>;

/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
    Runtime,
    Block,
    frame_system::ChainContext<Runtime>,
    Runtime,
    AllPalletsWithSystem,
    (),
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
    spec_name: create_runtime_str!("avn-test-parachain"),
    impl_name: create_runtime_str!("avn-test-parachain"),
    authoring_version: 1,
    spec_version: 105,
    impl_version: 0,
    apis: RUNTIME_API_VERSIONS,
    transaction_version: 1,
    state_version: 1,
};

/// This determines the average expected block time that we are targeting.
/// Blocks will be produced at a minimum duration defined by `SLOT_DURATION`.
/// `SLOT_DURATION` is picked up by `pallet_timestamp` which is in turn picked
/// up by `pallet_aura` to implement `fn slot_duration()`.

// NOTE: Currently it is not possible to change the slot duration after the chain has started.
//       Attempting to do so will brick block production.
pub const SLOT_DURATION: u64 = MILLISECS_PER_BLOCK;

/// The existential deposit. Set to 1/10 of the Connected Relay Chain.
pub const EXISTENTIAL_DEPOSIT: Balance = 0;

/// We assume that ~5% of the block weight is consumed by `on_initialize` handlers. This is
/// used to limit the maximal weight of a single extrinsic.
const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_percent(5);

/// We allow `Normal` extrinsics to fill up the block up to 75%, the rest can be used by
/// `Operational` extrinsics.
const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

/// We allow for 0.5 of a second of compute with a 12 second average block time.
const MAXIMUM_BLOCK_WEIGHT: Weight = Weight::from_parts(
    WEIGHT_REF_TIME_PER_SECOND.saturating_div(2),
    cumulus_primitives_core::relay_chain::MAX_POV_SIZE as u64,
);

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
    NativeVersion { runtime_version: VERSION, can_author_with: Default::default() }
}

parameter_types! {
    pub const Version: RuntimeVersion = VERSION;

    // This part is copied from Substrate's `bin/node/runtime/src/lib.rs`.
    //  The `RuntimeBlockLength` and `RuntimeBlockWeights` exist here because the
    // `DeletionWeightLimit` and `DeletionQueueDepth` depend on those to parameterize
    // the lazy contract deletion.
    pub RuntimeBlockLength: BlockLength =
        BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
    pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
        .base_block(BlockExecutionWeight::get())
        .for_class(DispatchClass::all(), |weights| {
            weights.base_extrinsic = ExtrinsicBaseWeight::get();
        })
        .for_class(DispatchClass::Normal, |weights| {
            weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
        })
        .for_class(DispatchClass::Operational, |weights| {
            weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
            // Operational transactions have some extra reserved space, so that they
            // are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
            weights.reserved = Some(
                MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT
            );
        })
        .avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
        .build_or_panic();
    pub const SS58Prefix: u16 = 42;
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

/// The default types are being injected by [`derive_impl`](`frame_support::derive_impl`) from
/// [`ParaChainDefaultConfig`](`struct@frame_system::config_preludes::ParaChainDefaultConfig`),
/// but overridden as needed.
#[derive_impl(frame_system::config_preludes::ParaChainDefaultConfig as frame_system::DefaultConfig)]
impl frame_system::Config for Runtime {
    /// The identifier used to distinguish between accounts.
    type AccountId = AccountId;
    /// The index type for storing how many extrinsics an account has signed.
    type Nonce = Nonce;
    /// The type for hashing blocks and tries.
    type Hash = Hash;
    /// The header type.
    type Block = Block;
    /// Maximum number of block number to block hash mappings to keep (oldest pruned first).
    type BlockHashCount = BlockHashCount;
    /// Runtime version.
    type Version = Version;
    /// The data to be stored in an account.
    type AccountData = pallet_balances::AccountData<Balance>;
    /// The weight of database operations that the runtime can invoke.
    type DbWeight = RocksDbWeight;
    /// The basic call filter to use in dispatchable.
    type BaseCallFilter = RestrictedEndpointFilter;
    /// Block & extrinsics weights: base values and limits.
    type BlockWeights = RuntimeBlockWeights;
    /// The maximum length of a block (in bytes).
    type BlockLength = RuntimeBlockLength;
    /// This is used as an identifier of the chain. 42 is the generic substrate prefix.
    type SS58Prefix = SS58Prefix;
    /// The action to take on a Runtime Upgrade
    type OnSetCode = cumulus_pallet_parachain_system::ParachainSetCode<Self>;
    type MaxConsumers = frame_support::traits::ConstU32<16>;
}

impl pallet_timestamp::Config for Runtime {
    /// A timestamp: milliseconds since the unix epoch.
    type Moment = u64;
    type OnTimestampSet = Aura;
    type MinimumPeriod = ConstU64<{ SLOT_DURATION / 2 }>;
    type WeightInfo = ();
}

impl pallet_authorship::Config for Runtime {
    type FindAuthor = pallet_session::FindAccountFromAuthorIndex<Self, Aura>;
    type EventHandler = (ParachainStaking,);
}

parameter_types! {
    pub const ExistentialDeposit: Balance = EXISTENTIAL_DEPOSIT;
}

impl pallet_balances::Config for Runtime {
    type MaxLocks = ConstU32<50>;
    /// The type for recording an account's balance.
    type Balance = Balance;
    /// The ubiquitous event type.
    type RuntimeEvent = RuntimeEvent;
    type DustRemoval = ();
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
    type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;
    type MaxReserves = ConstU32<50>;
    type ReserveIdentifier = [u8; 8];
    type RuntimeHoldReason = RuntimeHoldReason;
    type RuntimeFreezeReason = RuntimeFreezeReason;
    type FreezeIdentifier = ();
    type MaxHolds = ConstU32<2>;
    type MaxFreezes = ConstU32<1>;
}

impl pallet_transaction_payment::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction =
        pallet_transaction_payment::CurrencyAdapter<Balances, DealWithFees<Runtime>>;
    type WeightToFee = WeightToFee;
    type LengthToFee = ConstantMultiplier<Balance, TransactionByteFee>;
    type FeeMultiplierUpdate = SlowAdjustingFeeUpdate<Self>;
    type OperationalFeeMultiplier = OperationalFeeMultiplier;
}

parameter_types! {
    pub const ReservedXcmpWeight: Weight = MAXIMUM_BLOCK_WEIGHT.saturating_div(4);
    pub const ReservedDmpWeight: Weight = MAXIMUM_BLOCK_WEIGHT.saturating_div(4);
    pub const RelayOrigin: AggregateMessageOrigin = AggregateMessageOrigin::Parent;
}

impl cumulus_pallet_parachain_system::Config for Runtime {
    type WeightInfo = ();
    type RuntimeEvent = RuntimeEvent;
    type OnSystemEvent = ();
    type SelfParaId = parachain_info::Pallet<Runtime>;
    type OutboundXcmpMessageSource = XcmpQueue;
    type DmpQueue = frame_support::traits::EnqueueWithOrigin<MessageQueue, RelayOrigin>;
    type ReservedDmpWeight = ReservedDmpWeight;
    type XcmpMessageHandler = XcmpQueue;
    type ReservedXcmpWeight = ReservedXcmpWeight;
    type CheckAssociatedRelayNumber = RelayNumberStrictlyIncreases;
}

impl parachain_info::Config for Runtime {}

parameter_types! {
       pub MessageQueueServiceWeight: Weight = Perbill::from_percent(35) * RuntimeBlockWeights::get().max_block;
}

impl pallet_message_queue::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    #[cfg(feature = "runtime-benchmarks")]
    type MessageProcessor = pallet_message_queue::mock_helpers::NoopMessageProcessor<
        cumulus_primitives_core::AggregateMessageOrigin,
    >;
    #[cfg(not(feature = "runtime-benchmarks"))]
    type MessageProcessor = xcm_builder::ProcessXcmMessage<
        AggregateMessageOrigin,
        xcm_executor::XcmExecutor<xcm_config::XcmConfig>,
        RuntimeCall,
    >;
    type Size = u32;
    // The XCMP queue pallet is only ever able to handle the `Sibling(ParaId)` origin:
    type QueueChangeHandler = NarrowOriginToSibling<XcmpQueue>;
    type QueuePausedQuery = NarrowOriginToSibling<XcmpQueue>;
    type HeapSize = sp_core::ConstU32<{ 64 * 1024 }>;
    type MaxStale = sp_core::ConstU32<8>;
    type ServiceWeight = MessageQueueServiceWeight;
}

impl cumulus_pallet_aura_ext::Config for Runtime {}

impl cumulus_pallet_xcmp_queue::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type ChannelInfo = ParachainSystem;
    type VersionWrapper = ();
    // Enqueue XCMP messages from siblings for later processing.
    type XcmpQueue = TransformOrigin<MessageQueue, AggregateMessageOrigin, ParaId, ParaIdToSibling>;
    type MaxInboundSuspended = sp_core::ConstU32<1_000>;
    type ControllerOrigin = EnsureRoot<AccountId>;
    type ControllerOriginConverter = XcmOriginToTransactDispatchOrigin;
    type WeightInfo = ();
    type PriceForSiblingDelivery = NoPriceForMessageDelivery<ParaId>;
}

parameter_types! {
    pub const Period: u32 = 6 * HOURS;
    pub const Offset: u32 = 0;
}

impl pallet_session::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = <Self as frame_system::Config>::AccountId;
    // we don't have stash and controller, thus we don't need the convert as well.
    type ValidatorIdOf = ConvertInto;
    type ShouldEndSession = ParachainStaking;
    type NextSessionRotation = ParachainStaking;
    type SessionManager = ParachainStaking;
    // Essentially just Aura, but let's be pedantic.
    type SessionHandler = <SessionKeys as sp_runtime::traits::OpaqueKeys>::KeyTypeIdProviders;
    type Keys = SessionKeys;
    type WeightInfo = ();
}

impl pallet_aura::Config for Runtime {
    type AuthorityId = AuraId;
    type DisabledValidators = ();
    type MaxAuthorities = ConstU32<100_000>;
    type AllowMultipleBlocksPerSlot = ConstBool<false>;
}

parameter_types! {
    // The accountId that will hold the reward for the staking pallet
    pub const RewardPotId: PalletId = PalletId(*b"av/vamgr");
}
impl pallet_parachain_staking::Config for Runtime {
    type RuntimeCall = RuntimeCall;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    /// Minimum era length is 4 minutes (20 * 12 second block times)
    type MinBlocksPerEra = ConstU32<20>;
    /// Eras before the reward is paid
    type RewardPaymentDelay = ConstU32<2>;
    /// Minimum collators selected per era, default at genesis and minimum forever after
    type MinSelectedCandidates = ConstU32<20>;
    /// Maximum top nominations per candidate
    type MaxTopNominationsPerCandidate = ConstU32<300>;
    /// Maximum bottom nominations per candidate
    type MaxBottomNominationsPerCandidate = ConstU32<50>;
    /// Maximum nominations per nominator
    type MaxNominationsPerNominator = ConstU32<100>;
    /// Minimum stake required to be reserved to be a nominator
    type MinNominationPerCollator = ConstU128<1>;
    type RewardPotId = RewardPotId;
    type ErasPerGrowthPeriod = ConstU32<30>; // 30 eras (~ 1 month if era = 1 day)
    type ProcessedEventsChecker = EthBridge;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type CollatorSessionRegistration = Session;
    type CollatorPayoutDustHandler = TokenManager;
    type WeightInfo = pallet_parachain_staking::weights::SubstrateWeight<Runtime>;
    type MaxCandidates = ConstU32<100>;
    type AccountToBytesConvert = Avn;
    type BridgeInterface = EthBridge;
    type GrowthEnabled = ConstBool<true>;
}

// Substrate pallets that AvN has dependency
impl pallet_authority_discovery::Config for Runtime {
    type MaxAuthorities = ConstU32<100_000>;
}

impl pallet_session::historical::Config for Runtime {
    // TODO review this as originally was using the staking pallet. This is a minimal approach on
    // the Identification
    type FullIdentification = AccountId;
    type FullIdentificationOf = ConvertInto;
}

impl pallet_offences::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type IdentificationTuple = pallet_session::historical::IdentificationTuple<Self>;
    type OnOffenceHandler = AvnOffenceHandler;
}

impl<C> frame_system::offchain::SendTransactionTypes<C> for Runtime
where
    RuntimeCall: From<C>,
{
    type Extrinsic = UncheckedExtrinsic;
    type OverarchingCall = RuntimeCall;
}

parameter_types! {
    pub const ImOnlineUnsignedPriority: TransactionPriority = TransactionPriority::max_value();
    pub const MaxKeys: u16 = 100;
    pub const MaxPeerInHeartbeats: u32 = 10_000;
    pub const MaxPeerDataEncodingSize: u32 = 1_000;
}

impl pallet_im_online::Config for Runtime {
    type AuthorityId = ImOnlineId;
    type RuntimeEvent = RuntimeEvent;
    type NextSessionRotation = ParachainStaking;
    type ValidatorSet = Historical;
    type ReportUnresponsiveness = Offences;
    type UnsignedPriority = ImOnlineUnsignedPriority;
    type WeightInfo = pallet_im_online::weights::SubstrateWeight<Runtime>;
    type MaxKeys = MaxKeys;
    type MaxPeerInHeartbeats = MaxPeerInHeartbeats;
}

impl pallet_sudo::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = ();
}

impl pallet_utility::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = pallet_utility::weights::SubstrateWeight<Runtime>;
}

// AvN pallets
impl pallet_avn_offence_handler::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Enforcer = ValidatorsManager;
    type WeightInfo = pallet_avn_offence_handler::default_weights::SubstrateWeight<Runtime>;
}

impl pallet_avn::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = AvnId;
    type EthereumPublicKeyChecker = ValidatorsManager;
    type NewSessionHandler = ValidatorsManager;
    type DisabledValidatorChecker = ValidatorsManager;
    type WeightInfo = pallet_avn::default_weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    // TODO [TYPE: review][PRI: medium][JIRA: SYS-358]: Configurable in eth-events pallet?
    pub const MinEthBlockConfirmation: u64 = 20;
}

impl pallet_ethereum_events::Config for Runtime {
    type RuntimeCall = RuntimeCall;
    type RuntimeEvent = RuntimeEvent;
    type ProcessedEventHandler = (TokenManager, NftManager);
    type MinEthBlockConfirmation = MinEthBlockConfirmation;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type ReportInvalidEthereumLog = Offences;
    type WeightInfo = pallet_ethereum_events::default_weights::SubstrateWeight<Runtime>;
    type ProcessedEventsHandler = AllEventsFilter;
    type ProcessedEventsChecker = EthBridge;
}

parameter_types! {
    pub const ValidatorManagerVotingPeriod: BlockNumber = 30 * MINUTES;
}

impl pallet_validators_manager::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type ProcessedEventsChecker = EthBridge;
    type VotingPeriod = ValidatorManagerVotingPeriod;
    type AccountToBytesConvert = Avn;
    type ValidatorRegistrationNotifier = AvnOffenceHandler;
    type WeightInfo = pallet_validators_manager::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterface = EthBridge;
}

parameter_types! {
    // TODO [TYPE: review][PRI: high][CRITICAL][JIRA: 434]: review this value.
    pub const AdvanceSlotGracePeriod: BlockNumber = 5;
    pub const MinBlockAge: BlockNumber = 5;
    pub const AvnTreasuryPotId: PalletId = PalletId(*b"Treasury");
    pub const TreasuryGrowthPercentage: Perbill = Perbill::from_percent(75);
    pub const EthereumInstanceId: u8 = 1u8;
    pub const EthAutoSubmitSummaries: bool = true;
    pub const AvnAutoSubmitSummaries: bool = false;
    pub const AvnInstanceId: u8 = 2u8;
    pub const MinBurnPeriod: u32 = 100;
    pub const BurnEnabled: bool = false;
    pub const TreasuryBurnThreshold: Perbill = Perbill::from_percent(15);
    pub const TreasuryBurnCap: u128 = 10 * AVT;
}

pub type EthSummary = pallet_summary::Instance1;
impl pallet_summary::Config<EthSummary> for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AdvanceSlotGracePeriod = AdvanceSlotGracePeriod;
    type MinBlockAge = MinBlockAge;
    type AccountToBytesConvert = Avn;
    type ReportSummaryOffence = Offences;
    type WeightInfo = pallet_summary::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterface = EthBridge;
    type AutoSubmitSummaries = EthAutoSubmitSummaries;
    type InstanceId = EthereumInstanceId;
}

pub type AvnAnchorSummary = pallet_summary::Instance2;
impl pallet_summary::Config<AvnAnchorSummary> for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AdvanceSlotGracePeriod = AdvanceSlotGracePeriod;
    type MinBlockAge = MinBlockAge;
    type AccountToBytesConvert = Avn;
    type ReportSummaryOffence = Offences;
    type WeightInfo = pallet_summary::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterface = EthBridge;
    type AutoSubmitSummaries = AvnAutoSubmitSummaries;
    type InstanceId = AvnInstanceId;
}

impl pallet_avn_anchor::Config for Runtime {
    type RuntimeCall = RuntimeCall;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type WeightInfo = pallet_avn_anchor::default_weights::SubstrateWeight<Runtime>;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type FeeHandler = TokenManager;
    type Signature = Signature;
    type Token = EthAddress;
    type DefaultCheckpointFee = DefaultCheckpointFee;
}

pub type EthAddress = H160;

impl pallet_token_manager::pallet::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type TokenBalance = Balance;
    type TokenId = EthAddress;
    type ProcessedEventsChecker = EthBridge;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type OnGrowthLiftedHandler = ParachainStaking;
    type TreasuryGrowthPercentage = TreasuryGrowthPercentage;
    type AvnTreasuryPotId = AvnTreasuryPotId;
    type WeightInfo = pallet_token_manager::default_weights::SubstrateWeight<Runtime>;
    type Scheduler = Scheduler;
    type Preimages = Preimage;
    type PalletsOrigin = OriginCaller;
    type BridgeInterface = EthBridge;
    type MinBurnPeriod = MinBurnPeriod;
    type BurnEnabled = BurnEnabled;
    type TreasuryBurnThreshold = TreasuryBurnThreshold;
    type TreasuryBurnCap = TreasuryBurnCap;
}

impl pallet_nft_manager::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type ProcessedEventsChecker = EthBridge;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type BatchBound = pallet_nft_manager::BatchNftBound;
    type WeightInfo = pallet_nft_manager::default_weights::SubstrateWeight<Runtime>;
}

impl pallet_avn_proxy::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type Public = <Signature as sp_runtime::traits::Verify>::Signer;
    type Signature = Signature;
    type ProxyConfig = AvnProxyConfig;
    type WeightInfo = pallet_avn_proxy::default_weights::SubstrateWeight<Runtime>;
    type FeeHandler = TokenManager;
    type Token = EthAddress;
}

impl pallet_eth_bridge::Config for Runtime {
    type MaxQueuedTxRequests = ConstU32<100>;
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type MinEthBlockConfirmation = MinEthBlockConfirmation;
    type ProcessedEventsChecker = EthBridge;
    type AccountToBytesConvert = Avn;
    type ReportCorroborationOffence = Offences;
    type TimeProvider = pallet_timestamp::Pallet<Runtime>;
    type WeightInfo = pallet_eth_bridge::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterfaceNotification = (Summary, TokenManager, NftManager, ParachainStaking);
    type ProcessedEventsHandler = NoEventsFilter;
}

// Other pallets
parameter_types! {
    pub const AssetDeposit: Balance = 10 * MILLI_AVT;
    pub const ApprovalDeposit: Balance = 100 * MICRO_AVT;
    pub const StringLimit: u32 = 50;
    pub const MetadataDepositBase: Balance = 1 * MILLI_AVT;
    pub const MetadataDepositPerByte: Balance = 100 * MICRO_AVT;
    pub const DefaultCheckpointFee: Balance = 100 * MILLI_AVT;
}
const ASSET_ACCOUNT_DEPOSIT: Balance = 100 * MICRO_AVT;

impl pallet_assets::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Balance = u128;
    type RemoveItemsLimit = ConstU32<5>;
    type AssetId = u32;
    type AssetIdParameter = u32;
    type Currency = Balances;
    type CreateOrigin = AsEnsureOriginWithArg<EnsureSigned<AccountId>>;
    type ForceOrigin = EnsureRoot<AccountId>;
    type AssetDeposit = AssetDeposit;
    type AssetAccountDeposit = ConstU128<ASSET_ACCOUNT_DEPOSIT>;
    type MetadataDepositBase = MetadataDepositBase;
    type MetadataDepositPerByte = MetadataDepositPerByte;
    type ApprovalDeposit = ApprovalDeposit;
    type StringLimit = StringLimit;
    type Freezer = ();
    type Extra = ();
    type CallbackHandle = ();
    type WeightInfo = pallet_assets::weights::SubstrateWeight<Runtime>;
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = ();
}

parameter_types! {
    pub MaximumSchedulerWeight: Weight = RuntimeBlockWeights::get().max_block;
    pub const MaxScheduledPerBlock: u32 = 50;
    pub const NoPreimagePostponement: Option<u32> = Some(10);
}

/// Used the compare the privilege of an origin inside the scheduler.
pub struct OriginPrivilegeCmp;

impl PrivilegeCmp<OriginCaller> for OriginPrivilegeCmp {
    fn cmp_privilege(left: &OriginCaller, right: &OriginCaller) -> Option<Ordering> {
        if left == right {
            return Some(Ordering::Equal)
        }

        match (left, right) {
            // Root is greater than anything.
            (OriginCaller::system(frame_system::RawOrigin::Root), _) => Some(Ordering::Greater),
            // For every other origin we don't care, as they are not used for `ScheduleOrigin`.
            _ => None,
        }
    }
}

impl pallet_scheduler::Config for Runtime {
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeEvent = RuntimeEvent;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = EnsureRoot<AccountId>;
    type MaxScheduledPerBlock = MaxScheduledPerBlock;
    type WeightInfo = pallet_scheduler::weights::SubstrateWeight<Runtime>;
    type OriginPrivilegeCmp = OriginPrivilegeCmp;
    type Preimages = Preimage;
}

parameter_types! {
    // 5 AVT
    pub const PreimageBaseDeposit: Balance = deposit(5, 64);
    pub const PreimageByteDeposit: Balance = deposit(0, 1);
    pub const PreimageHoldReason: RuntimeHoldReason = RuntimeHoldReason::Preimage(pallet_preimage::HoldReason::Preimage);
}

impl pallet_preimage::Config for Runtime {
    type WeightInfo = pallet_preimage::weights::SubstrateWeight<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type ManagerOrigin = EnsureRoot<AccountId>;
    type Consideration = HoldConsideration<
        AccountId,
        Balances,
        PreimageHoldReason,
        LinearStoragePrice<PreimageBaseDeposit, PreimageByteDeposit, Balance>,
    >;
}

// Create the runtime by composing the FRAME pallets that were previously configured.
construct_runtime!(
    pub struct Runtime
    {
        // System support stuff.
        System: frame_system = 0,
        ParachainSystem: cumulus_pallet_parachain_system = 1,
        Timestamp: pallet_timestamp = 2,
        ParachainInfo: parachain_info = 3,

        // Monetary stuff.
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>} = 10,
        TransactionPayment: pallet_transaction_payment::{Pallet, Storage, Event<T>} = 11,

        // Collator support. The order of these 4 are important and shall not change.
        Authorship: pallet_authorship = 20,
        Session: pallet_session = 22,
        Aura: pallet_aura = 23,
        AuraExt: cumulus_pallet_aura_ext = 24,
        ParachainStaking: pallet_parachain_staking = 96,

        // Since the ValidatorsManager integrates with the ParachainStaking pallet, we want to initialise after it.
        ValidatorsManager: pallet_validators_manager = 18,

        // XCM helpers.
        XcmpQueue: cumulus_pallet_xcmp_queue = 30,
        PolkadotXcm: pallet_xcm = 31,
        CumulusXcm: cumulus_pallet_xcm = 32,
        MessageQueue: pallet_message_queue = 33,

        // Substrate pallets
        Assets: pallet_assets = 60,
        Sudo: pallet_sudo = 62,
        AuthorityDiscovery: pallet_authority_discovery = 70,
        Historical: pallet_session_historical::{Pallet} = 71,
        Offences: pallet_offences = 72,
        ImOnline: pallet_im_online = 73,
        Utility: pallet_utility = 74,

        // Rest of AvN pallets
        Avn: pallet_avn = 81,
        AvnOffenceHandler: pallet_avn_offence_handler = 83,
        EthereumEvents: pallet_ethereum_events = 84,
        NftManager: pallet_nft_manager = 86,
        TokenManager: pallet_token_manager = 87,
        Summary: pallet_summary::<Instance1> = 88,
        AvnProxy: pallet_avn_proxy = 89,
        EthBridge: pallet_eth_bridge = 91,
        AvnAnchor: pallet_avn_anchor = 92,
        AnchorSummary: pallet_summary::<Instance2> = 110,

         // OpenGov pallets
        Preimage: pallet_preimage::{Pallet, Call, Storage, Event<T>, HoldReason} = 97,
        Scheduler: pallet_scheduler::{Pallet, Storage, Event<T>, Call} = 98,
        Origins: pallet_custom_origins::{Origin} = 99,
        ConvictionVoting: pallet_conviction_voting::{Pallet, Call, Storage, Event<T>} = 100,
        Referenda: pallet_referenda::{Pallet, Call, Storage, Event<T>} = 101,
        Whitelist: pallet_whitelist::{Pallet, Call, Storage, Event<T>} = 102
    }
);

#[cfg(feature = "runtime-benchmarks")]
#[macro_use]
extern crate frame_benchmarking;

#[cfg(feature = "runtime-benchmarks")]
mod benches {
    define_benchmarks!(
        [frame_system, SystemBench::<Runtime>]
        [pallet_assets, Assets]
        [pallet_balances, Balances]
        [pallet_avn_offence_handler, AvnOffenceHandler]
        [pallet_avn_proxy, AvnProxy]
        [pallet_avn, Avn]
        [pallet_eth_bridge, EthBridge]
        [pallet_ethereum_events, EthereumEvents]
        [pallet_nft_manager, NftManager]
        [pallet_summary, Summary]
        [pallet_token_manager, TokenManager]
        [pallet_validators_manager, ValidatorsManager]
        [pallet_session, SessionBench::<Runtime>]
        [pallet_timestamp, Timestamp]
        [pallet_message_queue, MessageQueue]
        [pallet_utility, Utility]
        [pallet_parachain_staking, ParachainStaking]
        [pallet_avn_anchor, AvnAnchor]
        [cumulus_pallet_parachain_system, ParachainSystem]
        [cumulus_pallet_xcmp_queue, XcmpQueue]
    );
}

impl_runtime_apis! {
    impl sp_consensus_aura::AuraApi<Block, AuraId> for Runtime {
        fn slot_duration() -> sp_consensus_aura::SlotDuration {
            sp_consensus_aura::SlotDuration::from_millis(Aura::slot_duration())
        }

        fn authorities() -> Vec<AuraId> {
            Aura::authorities().into_inner()
        }
    }

    impl sp_api::Core<Block> for Runtime {
        fn version() -> RuntimeVersion {
            VERSION
        }

        fn execute_block(block: Block) {
            Executive::execute_block(block)
        }

        fn initialize_block(header: &<Block as BlockT>::Header) {
            Executive::initialize_block(header)
        }
    }

    impl sp_api::Metadata<Block> for Runtime {
        fn metadata() -> OpaqueMetadata {
            OpaqueMetadata::new(Runtime::metadata().into())
        }

        fn metadata_at_version(version: u32) -> Option<OpaqueMetadata> {
            Runtime::metadata_at_version(version)
        }

        fn metadata_versions() -> sp_std::vec::Vec<u32> {
            Runtime::metadata_versions()
        }
    }

    impl sp_block_builder::BlockBuilder<Block> for Runtime {
        fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
            Executive::apply_extrinsic(extrinsic)
        }

        fn finalize_block() -> <Block as BlockT>::Header {
            Executive::finalize_block()
        }

        fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
            data.create_extrinsics()
        }

        fn check_inherents(
            block: Block,
            data: sp_inherents::InherentData,
        ) -> sp_inherents::CheckInherentsResult {
            data.check_extrinsics(&block)
        }
    }

    impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
        fn validate_transaction(
            source: TransactionSource,
            tx: <Block as BlockT>::Extrinsic,
            block_hash: <Block as BlockT>::Hash,
        ) -> TransactionValidity {
            Executive::validate_transaction(source, tx, block_hash)
        }
    }

    impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
        fn offchain_worker(header: &<Block as BlockT>::Header) {
            Executive::offchain_worker(header)
        }
    }

    impl sp_session::SessionKeys<Block> for Runtime {
        fn generate_session_keys(seed: Option<Vec<u8>>) -> Vec<u8> {
            SessionKeys::generate(seed)
        }

        fn decode_session_keys(
            encoded: Vec<u8>,
        ) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
            SessionKeys::decode_into_raw_public_keys(&encoded)
        }
    }

    impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
        fn query_info(
            uxt: <Block as BlockT>::Extrinsic,
            len: u32,
        ) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
            TransactionPayment::query_info(uxt, len)
        }
        fn query_fee_details(
            uxt: <Block as BlockT>::Extrinsic,
            len: u32,
        ) -> pallet_transaction_payment::FeeDetails<Balance> {
            TransactionPayment::query_fee_details(uxt, len)
        }

        fn query_weight_to_fee(weight: Weight) -> Balance {
            TransactionPayment::weight_to_fee(weight)
        }
        fn query_length_to_fee(length: u32) -> Balance {
            TransactionPayment::length_to_fee(length)
        }
    }

    impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Nonce> for Runtime {
        fn account_nonce(account: AccountId) -> Nonce {
            System::account_nonce(account)
        }
    }

     impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentCallApi<Block, Balance, RuntimeCall>
        for Runtime
    {
        fn query_call_info(
            call: RuntimeCall,
            len: u32,
        ) -> pallet_transaction_payment::RuntimeDispatchInfo<Balance> {
            TransactionPayment::query_call_info(call, len)
        }
        fn query_call_fee_details(
            call: RuntimeCall,
            len: u32,
        ) -> pallet_transaction_payment::FeeDetails<Balance> {
            TransactionPayment::query_call_fee_details(call, len)
        }

        fn query_weight_to_fee(weight: Weight) -> Balance {
            TransactionPayment::weight_to_fee(weight)
        }
        fn query_length_to_fee(length: u32) -> Balance {
            TransactionPayment::length_to_fee(length)
        }
    }

    impl pallet_eth_bridge_runtime_api::EthEventHandlerApi<Block, AccountId> for Runtime {
        fn query_authors() -> Vec<([u8; 32], [u8; 32])> {
            let validators = Avn::validators().to_vec();
            let res = validators.iter().map(|validator| {
                let mut address: [u8; 32] = Default::default();
                address.copy_from_slice(&validator.account_id.encode()[0..32]);

                let mut key: [u8; 32] = Default::default();
                key.copy_from_slice(&validator.key.to_raw_vec()[0..32]);

                return (address, key)
            }).collect();
            return res
        }

        fn query_active_block_range()-> Option<(EthBlockRange, u16)> {
            if let Some(active_eth_range) =  EthBridge::active_ethereum_range(){
                Some((active_eth_range.range, active_eth_range.partition))
            } else {
                None
            }
        }
        fn query_has_author_casted_vote(account_id: AccountId) -> bool{
           pallet_eth_bridge::author_has_cast_event_vote::<Runtime>(&account_id) ||
           pallet_eth_bridge::author_has_submitted_latest_block::<Runtime>(&account_id)
        }

        fn query_signatures() -> Vec<sp_core::H256> {
            EthBridge::signatures()
        }

        fn query_bridge_contract() -> H160 {
            Avn::get_bridge_contract_address()
        }

        fn submit_vote(author: AccountId,
            events_partition: EthereumEventsPartition,
            signature: sp_core::sr25519::Signature,
        ) -> Option<()>{
            EthBridge::submit_vote(author, events_partition, signature.into()).ok()
        }

        fn submit_latest_ethereum_block(
            author: AccountId,
            latest_seen_block: u32,
            signature: sp_core::sr25519::Signature
        ) -> Option<()>{
            EthBridge::submit_latest_ethereum_block_vote(author, latest_seen_block, signature.into()).ok()
        }

        fn additional_transactions() -> Option<AdditionalEvents> {
            if let Some(active_eth_range) =  EthBridge::active_ethereum_range(){
                Some(active_eth_range.additional_transactions)
            } else {
                None
            }
        }
    }

    impl cumulus_primitives_core::CollectCollationInfo<Block> for Runtime {
        fn collect_collation_info(header: &<Block as BlockT>::Header) -> cumulus_primitives_core::CollationInfo {
            ParachainSystem::collect_collation_info(header)
        }
    }

    #[cfg(feature = "try-runtime")]
    impl frame_try_runtime::TryRuntime<Block> for Runtime {
        fn on_runtime_upgrade(checks: frame_try_runtime::UpgradeCheckSelect) -> (Weight, Weight) {
            log::info!("try-runtime::on_runtime_upgrade avn-test-parachain.");
            let weight = Executive::try_runtime_upgrade(checks).unwrap();
            (weight, RuntimeBlockWeights::get().max_block)
        }

        fn execute_block(
            block: Block,
            state_root_check: bool,
            signature_check: bool,
            select: frame_try_runtime::TryStateSelect,
        ) -> Weight {
            // NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
            // have a backtrace here.
            Executive::try_execute_block(block, state_root_check, signature_check, select).unwrap()
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    impl frame_benchmarking::Benchmark<Block> for Runtime {
        fn benchmark_metadata(extra: bool) -> (
            Vec<frame_benchmarking::BenchmarkList>,
            Vec<frame_support::traits::StorageInfo>,
        ) {
            use frame_benchmarking::{Benchmarking, BenchmarkList};
            use frame_support::traits::StorageInfoTrait;
            use frame_system_benchmarking::Pallet as SystemBench;
            use cumulus_pallet_session_benchmarking::Pallet as SessionBench;

            let mut list = Vec::<BenchmarkList>::new();
            list_benchmarks!(list, extra);

            let storage_info = AllPalletsWithSystem::storage_info();
            return (list, storage_info)
        }

        fn dispatch_benchmark(
            config: frame_benchmarking::BenchmarkConfig
        ) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, sp_runtime::RuntimeString> {
            use frame_benchmarking::{BenchmarkError, Benchmarking, BenchmarkBatch};

            use frame_system_benchmarking::Pallet as SystemBench;
            impl frame_system_benchmarking::Config for Runtime {
                fn setup_set_code_requirements(code: &sp_std::vec::Vec<u8>) -> Result<(), BenchmarkError> {
                    ParachainSystem::initialize_for_set_code_benchmark(code.len() as u32);
                    Ok(())
                }

                fn verify_set_code() {
                    System::assert_last_event(cumulus_pallet_parachain_system::Event::<Runtime>::ValidationFunctionStored.into());
                }
            }

            use cumulus_pallet_session_benchmarking::Pallet as SessionBench;
            impl cumulus_pallet_session_benchmarking::Config for Runtime {}

            use frame_support::traits::WhitelistedStorageKeys;
            let whitelist = AllPalletsWithSystem::whitelisted_storage_keys();

            let mut batches = Vec::<BenchmarkBatch>::new();
            let params = (&config, &whitelist);
            add_benchmarks!(params, batches);

            if batches.is_empty() { return Err("Benchmark not found for this pallet.".into()) }
            Ok(batches)
        }
    }

    impl sp_authority_discovery::AuthorityDiscoveryApi<Block> for Runtime {
        fn authorities() -> Vec<AuthorityDiscoveryId> {
            AuthorityDiscovery::authorities()
        }
    }

    impl sp_genesis_builder::GenesisBuilder<Block> for Runtime {
        fn create_default_config() -> Vec<u8> {
                create_default_config::<RuntimeGenesisConfig>()
        }

        fn build_config(config: Vec<u8>) -> sp_genesis_builder::Result {
                build_config::<RuntimeGenesisConfig>(config)
        }
    }
}

struct CheckInherents;

impl cumulus_pallet_parachain_system::CheckInherents<Block> for CheckInherents {
    fn check_inherents(
        block: &Block,
        relay_state_proof: &cumulus_pallet_parachain_system::RelayChainStateProof,
    ) -> sp_inherents::CheckInherentsResult {
        let relay_chain_slot = relay_state_proof
            .read_slot()
            .expect("Could not read the relay chain slot from the proof");

        let inherent_data =
            cumulus_primitives_timestamp::InherentDataProvider::from_relay_chain_slot_and_duration(
                relay_chain_slot,
                sp_std::time::Duration::from_secs(6),
            )
            .create_inherent_data()
            .expect("Could not create the timestamp inherent data");

        inherent_data.check_extrinsics(block)
    }
}

cumulus_pallet_parachain_system::register_validate_block! {
    Runtime = Runtime,
    BlockExecutor = cumulus_pallet_aura_ext::BlockExecutor::<Runtime, Executive>,
    CheckInherents = CheckInherents,
}
