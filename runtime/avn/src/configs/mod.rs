// This file is part of Aventus.
// Copyright 2026 Aventus DAO Ltd

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

mod xcm_config;

// Substrate and Polkadot dependencies
// use cumulus_pallet_parachain_system::RelayNumberStrictlyIncreases;
use cumulus_pallet_parachain_system::RelayNumberMonotonicallyIncreases;
use cumulus_primitives_core::{AggregateMessageOrigin, ParaId};
use frame_support::{
    derive_impl,
    dispatch::DispatchClass,
    parameter_types,
    traits::{ConstBool, ConstU32, ConstU64, TransformOrigin, VariantCountOf},
    weights::{ConstantMultiplier, Weight},
    PalletId,
};
use frame_system::{
    limits::{BlockLength, BlockWeights},
    EnsureRoot,
};
use parachains_common::message_queue::{NarrowOriginToSibling, ParaIdToSibling};
use polkadot_runtime_common::{
    xcm_sender::NoPriceForMessageDelivery, BlockHashCount, SlowAdjustingFeeUpdate,
};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_runtime::Perbill;
use sp_version::RuntimeVersion;

use runtime_common::{
    constants::{currency::*, time::*},
    OperationalFeeMultiplier,
};

use sp_avn_common::event_discovery::filters::{CorePrimaryEventsFilter, NftEventsFilter};
use sp_core::{ConstU128, H160};
use sp_runtime::{traits::ConvertInto, transaction_validity::TransactionPriority};
use sp_watchtower::NoopWatchtower;

// Local module imports
use crate::{
    fungible,
    weights::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight},
    AccountId, AsEnsureOriginWithArg, Aura, Avn, AvnGasFeeAdapter, AvnId, AvnOffenceHandler,
    AvnProxyConfig, Balance, Balances, Block, BlockNumber, ConsensusHook, EnsureSigned, EthBridge,
    Hash, Historical, HoldConsideration, ImOnlineId, Imbalance, LinearStoragePrice, MessageQueue,
    Moment, NftManager, Nonce, Offences, OnUnbalanced, Ordering, OriginCaller, PalletInfo,
    ParachainStaking, ParachainSystem, Preimage, PrivilegeCmp, ResolveTo, RestrictedEndpointFilter,
    Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason, RuntimeHoldReason, RuntimeOrigin,
    RuntimeTask, Scheduler, Session, SessionKeys, Signature, StakingPotAccountId, Summary, System,
    Timestamp, TokenManager, TransactionByteFee, UncheckedExtrinsic, ValidatorsManager,
    WeightToFee, XcmpQueue, AVERAGE_ON_INITIALIZE_RATIO, EXISTENTIAL_DEPOSIT, HOURS,
    MAXIMUM_BLOCK_WEIGHT, NORMAL_DISPATCH_RATIO, SLOT_DURATION, VERSION,
};

use xcm_config::XcmOriginToTransactDispatchOrigin;
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
    /// The block type.
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

/// Configure the palelt weight reclaim tx.
impl cumulus_pallet_weight_reclaim::Config for Runtime {
    type WeightInfo = ();
}

impl pallet_timestamp::Config for Runtime {
    /// A timestamp: milliseconds since the unix epoch.
    type Moment = Moment;
    type OnTimestampSet = Aura;
    // Set to 0 when async backing is enabled
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
    type FreezeIdentifier = RuntimeFreezeReason;
    type MaxFreezes = VariantCountOf<RuntimeFreezeReason>;
    type DoneSlashHandler = ();
}

impl pallet_transaction_payment::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = AvnGasFeeAdapter<Balances, DealWithFees<Runtime>>;
    type WeightToFee = WeightToFee;
    type LengthToFee = ConstantMultiplier<Balance, TransactionByteFee>;
    type FeeMultiplierUpdate = SlowAdjustingFeeUpdate<Self>;
    type OperationalFeeMultiplier = OperationalFeeMultiplier;
    type WeightInfo = pallet_transaction_payment::weights::SubstrateWeight<Runtime>;
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
    // TODO Review this and use RelayNumberStrictlyIncreases and ExpectParentIncluded to disable
    // Asynchronous backing
    type CheckAssociatedRelayNumber = RelayNumberMonotonicallyIncreases;
    type ConsensusHook = ConsensusHook;
    type SelectCore = cumulus_pallet_parachain_system::DefaultCoreSelector<Runtime>;
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
    type HeapSize = sp_core::ConstU32<{ 103 * 1024 }>;
    type MaxStale = sp_core::ConstU32<8>;
    type ServiceWeight = MessageQueueServiceWeight;
    // TODO 1.10 review this
    type IdleMaxServiceWeight = ();
}

impl cumulus_pallet_aura_ext::Config for Runtime {}

impl cumulus_pallet_xcmp_queue::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type ChannelInfo = ParachainSystem;
    type VersionWrapper = ();
    // Enqueue XCMP messages from siblings for later processing.
    type XcmpQueue = TransformOrigin<MessageQueue, AggregateMessageOrigin, ParaId, ParaIdToSibling>;
    type MaxInboundSuspended = sp_core::ConstU32<1_000>;
    type MaxActiveOutboundChannels = ConstU32<128>;
    type MaxPageSize = ConstU32<{ 1 << 16 }>;
    type ControllerOrigin = EnsureRoot<AccountId>;
    type ControllerOriginConverter = XcmOriginToTransactDispatchOrigin;
    type WeightInfo = ();
    type PriceForSiblingDelivery = NoPriceForMessageDelivery<ParaId>;
}

impl cumulus_pallet_xcmp_queue::migration::v5::V5Config for Runtime {
    type ChannelList = ParachainSystem;
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
    type DisablingStrategy = ();
    type WeightInfo = ();
}

#[docify::export(aura_config)]
impl pallet_aura::Config for Runtime {
    type AuthorityId = AuraId;
    type DisabledValidators = ();
    type MaxAuthorities = ConstU32<100_000>;
    type AllowMultipleBlocksPerSlot = ConstBool<false>;
    type SlotDuration = ConstU64<SLOT_DURATION>;
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
    type GrowthEnabled = ConstBool<false>;
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

impl<C> frame_system::offchain::CreateTransactionBase<C> for Runtime
where
    RuntimeCall: From<C>,
{
    type Extrinsic = UncheckedExtrinsic;
    type RuntimeCall = RuntimeCall;
}

impl<C> frame_system::offchain::CreateInherent<C> for Runtime
where
    RuntimeCall: From<C>,
{
    fn create_inherent(call: Self::RuntimeCall) -> Self::Extrinsic {
        UncheckedExtrinsic::new_bare(call)
    }
}

parameter_types! {
    pub const ImOnlineUnsignedPriority: TransactionPriority = TransactionPriority::max_value();
    pub const MaxKeys: u16 = 100;
    pub const MaxPeerInHeartbeats: u32 = 10_000;
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
    type Enforcer = ();
    type WeightInfo = pallet_avn_offence_handler::default_weights::SubstrateWeight<Runtime>;
}

impl pallet_avn::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = AvnId;
    type EthereumPublicKeyChecker = ValidatorsManager;
    type NewSessionHandler = ValidatorsManager;
    type DisabledValidatorChecker = ();
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
    type ProcessedEventsHandler = NftEventsFilter;
    type ProcessedEventsChecker = EthBridge;
}

parameter_types! {
    pub const ValidatorManagerVotingPeriod: BlockNumber = 30 * MINUTES;
    // Minimum 2 validators must remain active
    pub const MinimumValidatorCount: u32 = 2;
}

impl pallet_validators_manager::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type ProcessedEventsChecker = EthBridge;
    type VotingPeriod = ValidatorManagerVotingPeriod;
    type AccountToBytesConvert = Avn;
    type ValidatorRegistrationNotifier = AvnOffenceHandler;
    type WeightInfo = pallet_validators_manager::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterface = EthBridge;
    type MinimumValidatorCount = MinimumValidatorCount;
}

parameter_types! {
    pub const AdvanceSlotGracePeriod: BlockNumber = 5;
    pub const MinBlockAge: BlockNumber = 5;
    pub const AvnTreasuryPotId: PalletId = PalletId(*b"Treasury");
    pub const TreasuryGrowthPercentage: Perbill = Perbill::from_percent(75);
    pub const EthAutoSubmitSummaries: bool = true;
    pub const EthereumInstanceId: u8 = 1u8;
    pub const ExternalValidationEnabled: bool = false;
}

impl pallet_summary::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AdvanceSlotGracePeriod = AdvanceSlotGracePeriod;
    type MinBlockAge = MinBlockAge;
    type AccountToBytesConvert = Avn;
    type ReportSummaryOffence = Offences;
    type WeightInfo = pallet_summary::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterface = EthBridge;
    type AutoSubmitSummaries = EthAutoSubmitSummaries;
    type InstanceId = EthereumInstanceId;
    type ExternalValidationEnabled = ExternalValidationEnabled;
    type ExternalValidator = NoopWatchtower<AccountId>;
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
    type OnIdleHandler = ();
    type AccountToBytesConvert = Avn;
    type TimeProvider = Timestamp;
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

impl pallet_avn_transaction_payment::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type KnownUserOrigin = EnsureRoot<AccountId>;
    type WeightInfo = pallet_avn_transaction_payment::default_weights::SubstrateWeight<Runtime>;
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

impl pallet_eth_bridge::Config for Runtime {
    type MaxQueuedTxRequests = ConstU32<100>;
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type MinEthBlockConfirmation = MinEthBlockConfirmation;
    type ProcessedEventsChecker = EthBridge;
    type AccountToBytesConvert = Avn;
    type TimeProvider = Timestamp;
    type ReportCorroborationOffence = Offences;
    type WeightInfo = pallet_eth_bridge::default_weights::SubstrateWeight<Runtime>;
    type BridgeInterfaceNotification = (Summary, TokenManager, ParachainStaking, ValidatorsManager);
    type ProcessedEventsHandler = CorePrimaryEventsFilter;
    type EthereumEventsMigration = ();
    type Quorum = Avn;
}

parameter_types! {
    pub const MaxLinkedAccounts: u32 = 10;
}

impl pallet_cross_chain_voting::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type MaxLinkedAccounts = MaxLinkedAccounts;
    type WeightInfo = pallet_cross_chain_voting::default_weights::SubstrateWeight<Runtime>;
}

// Other pallets
parameter_types! {
    pub const AssetDeposit: Balance = 10 * MILLI_AVT;
    pub const ApprovalDeposit: Balance = 100 * MICRO_AVT;
    pub const StringLimit: u32 = 50;
    pub const MetadataDepositBase: Balance = 1 * MILLI_AVT;
    pub const MetadataDepositPerByte: Balance = 100 * MICRO_AVT;
    pub const DefaultCheckpointFee: Balance = 60 * MILLI_AVT;
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
    type Holder = ();
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
    type BlockNumberProvider = frame_system::Pallet<Runtime>;
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

pub struct DealWithFees<R>(sp_std::marker::PhantomData<R>);
impl<R> OnUnbalanced<fungible::Credit<R::AccountId, pallet_balances::Pallet<R>>> for DealWithFees<R>
where
    R: pallet_balances::Config + pallet_parachain_staking::Config,
    <R as frame_system::Config>::AccountId: From<AccountId>,
    <R as frame_system::Config>::AccountId: Into<AccountId>,
    <R as frame_system::Config>::RuntimeEvent: From<pallet_balances::Event<R>>,
{
    fn on_unbalanceds(
        mut fees_then_tips: impl Iterator<
            Item = fungible::Credit<R::AccountId, pallet_balances::Pallet<R>>,
        >,
    ) {
        if let Some(mut fees) = fees_then_tips.next() {
            if let Some(tips) = fees_then_tips.next() {
                tips.merge_into(&mut fees);
            }
            ResolveTo::<StakingPotAccountId<R>, pallet_balances::Pallet<R>>::on_unbalanced(fees)
        }
    }
}
