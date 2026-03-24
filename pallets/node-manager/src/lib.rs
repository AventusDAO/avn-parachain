// Copyright 2026 Aventus DAO Ltd

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use codec::{Decode, DecodeWithMemTracking, Encode, FullCodec};
use core::convert::TryFrom;
use frame_support::{
    dispatch::DispatchResult,
    pallet_prelude::*,
    storage::{generator::StorageDoubleMap as StorageDoubleMapTrait, PrefixIterator},
    traits::{
        Currency, ExistenceRequirement, IsSubType, ReservableCurrency, StorageVersion, UnixTime,
    },
    PalletId,
};
use frame_system::{
    offchain::{CreateInherent, CreateTransactionBase, SubmitTransaction},
    pallet_prelude::*,
};
use pallet_avn::{
    self as avn, BridgeInterface, BridgeInterfaceNotification, ProcessedEventsChecker,
};
use sp_application_crypto::RuntimeAppPublic;
use sp_avn_common::{
    eth::EthereumId,
    event_types::{EthEvent, EventData, ProcessedEventHandler, TotalSupplyUpdatedData, Validator},
    AppChainInterface, BridgeContractMethod, PaymentHandler, RewardPeriodIndex,
    REGISTERED_NODE_KEY,
};
use sp_core::{MaxEncodedLen, H160};
use sp_runtime::{
    offchain::storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
    scale_info::TypeInfo,
    traits::{
        AccountIdConversion, CheckedAdd, CheckedMul, CheckedSub, Dispatchable, IdentifyAccount,
        SaturatedConversion, Verify, Zero,
    },
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionSource, TransactionValidity,
        ValidTransaction,
    },
    DispatchError, Perbill, Perquintill, RuntimeDebug, Saturating,
};

pub mod offchain;
pub mod reward;
pub mod stake;
pub mod types;
use crate::types::*;
pub mod default_weights;
pub use default_weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;
#[cfg(test)]
#[path = "tests/test_admin.rs"]
mod test_admin;
#[cfg(test)]
#[path = "tests/test_auto_stake_preference.rs"]
mod test_auto_stake_preference;
#[cfg(test)]
#[path = "tests/test_heartbeat.rs"]
mod test_heartbeat;
#[cfg(test)]
#[path = "tests/test_node_deregistration.rs"]
mod test_node_deregistration;
#[cfg(test)]
#[path = "tests/test_node_registration.rs"]
mod test_node_registration;
#[cfg(test)]
#[path = "tests/test_reward_payment.rs"]
mod test_reward_payment;
#[cfg(test)]
#[path = "tests/test_stake_weight.rs"]
mod test_stake_weight;

// Definition of the crypto to use for signing
pub mod sr25519 {
    pub mod app_sr25519 {
        use sp_application_crypto::{app_crypto, sr25519, KeyTypeId};
        app_crypto!(sr25519, KeyTypeId(*b"nodk"));
    }

    pub type AuthorityId = app_sr25519::Public;
}

#[cfg(not(feature = "std"))]
use sp_std::prelude::*;

const PAYOUT_REWARD_CONTEXT: &'static [u8] = b"NodeManager_RewardPayout";
const MINT_REWARDS_CONTEXT: &'static [u8] = b"NodeManager_MintRewards";
const HEARTBEAT_CONTEXT: &'static [u8] = b"NodeManager_heartbeat";
const MAX_BATCH_SIZE: u32 = 1_000;
const MINT_SAFETY_CAP_MULTIPLIER: u32 = 4;
pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);
pub const SIGNED_REGISTER_NODE_CONTEXT: &[u8] = b"register_node";
pub const SIGNED_DEREGISTER_NODE_CONTEXT: &[u8] = b"deregister_node";
pub const MAX_NODES_TO_DEREGISTER: u32 = 64;
pub const MAX_STAKE_CHANGES_PER_PERIOD: u32 = 256;

const PALLET_ID: &'static [u8; 12] = b"node-manager";

// Error codes returned by validate unsigned methods
/// Invalid signature for `paying` transaction
pub const ERROR_CODE_INVALID_PAY_SIGNATURE: u8 = 1;
/// Invalid signature for `heartbeat` transaction
pub const ERROR_CODE_INVALID_HEARTBEAT_SIGNATURE: u8 = 2;
/// Node not found
pub const ERROR_CODE_INVALID_NODE: u8 = 3;
/// Rewards are disabled
pub const ERROR_CODE_REWARD_DISABLED: u8 = 4;
/// Invalid heartbeat submission
pub const ERROR_CODE_INVALID_HEARTBEAT: u8 = 5;
/// Invalid signature for `mint rewards` transaction
pub const ERROR_CODE_INVALID_MINT_SIGNATURE: u8 = 6;

pub type AVN<T> = avn::Pallet<T>;
pub type Author<T> =
    Validator<<T as avn::Config>::AuthorityId, <T as frame_system::Config>::AccountId>;
pub use pallet::*;

pub(crate) type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
pub(crate) type PositiveImbalanceOf<T> = <<T as Config>::Currency as Currency<
    <T as frame_system::Config>::AccountId,
>>::PositiveImbalance;
/// Node account ID
pub(crate) type NodeId<T> = <T as frame_system::Config>::AccountId;
/// Max nodes per deregistration call
pub type MaxNodesToDeregister = ConstU32<MAX_NODES_TO_DEREGISTER>;
/// Max stake changes per period
pub type MaxStakeChangesPerPeriod = ConstU32<MAX_STAKE_CHANGES_PER_PERIOD>;

#[frame_support::pallet]
pub mod pallet {
    use sp_avn_common::{verify_signature, InnerCallValidator, Proof};

    use super::*;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    /// Registered nodes
    #[pallet::storage]
    pub type NodeRegistry<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        NodeId<T>,
        NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
        OptionQuery,
    >;

    /// Signing key to node ID
    #[pallet::storage]
    pub type SigningKeyToNodeId<T: Config> =
        StorageMap<_, Blake2_128Concat, T::SignerId, NodeId<T>, OptionQuery>;

    /// Total registered nodes
    #[pallet::storage]
    pub type TotalRegisteredNodes<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Owner to node mapping
    #[pallet::storage]
    pub type OwnedNodes<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId, // OwnerAddress
        Blake2_128Concat,
        NodeId<T>,
        (),
        OptionQuery,
    >;

    /// Number of nodes owned by each account
    #[pallet::storage]
    pub type OwnedNodesCount<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

    /// Account allowed to register nodes
    #[pallet::storage]
    pub type NodeRegistrar<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    /// Max nodes paid per batch
    #[pallet::storage]
    pub type MaxBatchSize<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Heartbeat period in blocks for the next reward period
    #[pallet::storage]
    pub type NextHeartbeatPeriod<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Length of the next reward period in blocks
    #[pallet::storage]
    pub type NextRewardPeriodLength<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Reward amount for the next reward period
    #[pallet::storage]
    pub type NextRewardAmountPerPeriod<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// Future periods to keep funded
    #[pallet::storage]
    pub type NumPeriodsToMint<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Reward snapshots by period
    #[pallet::storage]
    pub(super) type RewardPot<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        RewardPeriodIndex,
        RewardPotInfo<BalanceOf<T>>,
        OptionQuery,
    >;

    /// Total rewards still to be paid
    #[pallet::storage]
    pub type OutstandingRewardToPay<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// Current reward period
    #[pallet::storage]
    #[pallet::getter(fn current_reward_period)]
    pub(super) type RewardPeriod<T: Config> =
        StorageValue<_, RewardPeriodInfo<BlockNumberFor<T>, BalanceOf<T>>, ValueQuery>;

    /// Oldest unpaid reward period
    #[pallet::storage]
    #[pallet::getter(fn oldest_unpaid_period)]
    pub(super) type OldestUnpaidRewardPeriodIndex<T: Config> =
        StorageValue<_, RewardPeriodIndex, ValueQuery>;

    /// Last paid node pointer
    #[pallet::storage]
    #[pallet::getter(fn last_paid_pointer)]
    pub(super) type LastPaidPointer<T: Config> =
        StorageValue<_, PaymentPointer<T::AccountId>, OptionQuery>;

    /// Node uptime by reward period
    #[pallet::storage]
    #[pallet::getter(fn node_uptime)]
    pub(super) type NodeUptime<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        RewardPeriodIndex,
        Blake2_128Concat,
        NodeId<T>,
        UptimeInfo<BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// Pending mint request state
    #[pallet::storage]
    #[pallet::getter(fn pending_mint_request)]
    pub type PendingMintRequestState<T: Config> =
        StorageValue<_, PendingMintRequest<BalanceOf<T>>, OptionQuery>;

    /// Total uptime by reward period
    #[pallet::storage]
    pub(super) type TotalUptime<T: Config> =
        StorageMap<_, Blake2_128Concat, RewardPeriodIndex, TotalUptimeInfo, ValueQuery>;

    /// Whether rewards are enabled
    #[pallet::storage]
    pub(super) type RewardEnabled<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// Minimum uptime threshold
    #[pallet::storage]
    pub type MinUptimeThreshold<T: Config> = StorageValue<_, Perbill, OptionQuery>;

    /// Auto-stake duration in seconds
    #[pallet::storage]
    pub type AutoStakeDurationSec<T: Config> = StorageValue<_, Duration, ValueQuery>;

    /// Unstake period in seconds
    #[pallet::storage]
    pub type UnstakePeriodSec<T: Config> = StorageValue<_, Duration, ValueQuery>;

    /// Max unstake percentage
    #[pallet::storage]
    pub type MaxUnstakePercentage<T: Config> = StorageValue<_, Perbill, ValueQuery>;

    /// Next node serial number
    #[pallet::storage]
    pub type NextNodeSerialNumber<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Restricted unstake duration in seconds
    #[pallet::storage]
    pub type RestrictedUnstakeDurationSec<T: Config> = StorageValue<_, Duration, ValueQuery>;

    /// Reward fee percentage
    #[pallet::storage]
    pub type RewardFeePercentage<T: Config> = StorageValue<_, Perbill, ValueQuery>;

    /// Total stake by owner
    #[pallet::storage]
    pub type TotalStake<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, BalanceOf<T>, OptionQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub _phantom: sp_std::marker::PhantomData<T>,
        pub max_batch_size: u32,
        pub reward_period: u32,
        pub heartbeat_period: u32,
        pub reward_amount_per_period: BalanceOf<T>,
        pub num_periods_to_mint: u32,
        pub auto_stake_duration_sec: Duration,
        pub max_unstake_percentage: Perbill,
        pub unstake_period_sec: Duration,
        pub restricted_unstake_duration_sec: Duration,
        pub reward_fee_percentage: Perbill,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                _phantom: Default::default(),
                max_batch_size: 1,
                reward_period: 2,
                heartbeat_period: 1,
                reward_amount_per_period: Default::default(),
                num_periods_to_mint: 1,
                auto_stake_duration_sec: 180 * 24 * 60 * 60, // 180 days
                max_unstake_percentage: Perbill::from_percent(10),
                unstake_period_sec: 7 * 24 * 60 * 60, // 1 week
                restricted_unstake_duration_sec: 10 * 7 * 24 * 60 * 60, // 10 weeks
                reward_fee_percentage: Perbill::from_percent(0),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            assert!(self.reward_period > self.heartbeat_period);
            assert!(self.unstake_period_sec > 0);
            let default_threshold = Pallet::<T>::get_default_threshold();

            NextRewardPeriodLength::<T>::set(self.reward_period);
            NextRewardAmountPerPeriod::<T>::set(self.reward_amount_per_period);
            NumPeriodsToMint::<T>::set(self.num_periods_to_mint);
            MaxBatchSize::<T>::set(self.max_batch_size);
            NextHeartbeatPeriod::<T>::set(self.heartbeat_period);
            MinUptimeThreshold::<T>::set(Some(default_threshold));
            AutoStakeDurationSec::<T>::set(self.auto_stake_duration_sec);
            MaxUnstakePercentage::<T>::set(self.max_unstake_percentage);
            UnstakePeriodSec::<T>::set(self.unstake_period_sec);
            RestrictedUnstakeDurationSec::<T>::set(self.restricted_unstake_duration_sec);
            RewardFeePercentage::<T>::set(self.reward_fee_percentage);

            let uptime_threshold =
                Pallet::<T>::calculate_uptime_threshold(self.reward_period, self.heartbeat_period);
            let reward_period: RewardPeriodInfo<BlockNumberFor<T>, BalanceOf<T>> =
                RewardPeriodInfo::new(
                    0u64,
                    0u32.into(),
                    self.reward_period,
                    self.heartbeat_period,
                    uptime_threshold,
                    self.reward_amount_per_period,
                );

            <RewardPeriod<T>>::put(reward_period);
            OutstandingRewardToPay::<T>::put(BalanceOf::<T>::zero());
        }
    }

    // Pallet Events
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Node registered
        NodeRegistered { owner: T::AccountId, node: NodeId<T> },
        /// Reward period length set
        RewardPeriodLengthSet {
            period_index: u64,
            old_reward_period_length: u32,
            new_reward_period_length: u32,
        },
        /// New reward period started
        NewRewardPeriodStarted {
            reward_period_index: RewardPeriodIndex,
            reward_period_length: u32,
            uptime_threshold: u32,
            previous_period_reward: BalanceOf<T>,
        },
        /// Reward payout completed
        RewardPayoutCompleted { reward_period_index: RewardPeriodIndex },
        /// Reward paid
        RewardPaid {
            reward_period: RewardPeriodIndex,
            owner: T::AccountId,
            node: NodeId<T>,
            amount: BalanceOf<T>,
        },
        /// Reward payment failed
        ErrorPayingReward {
            reward_period: RewardPeriodIndex,
            node: NodeId<T>,
            error: DispatchError,
        },
        /// Reward auto-staked
        RewardAutoStaked {
            reward_period: RewardPeriodIndex,
            owner: T::AccountId,
            node: NodeId<T>,
            amount: BalanceOf<T>,
        },
        /// Node registrar set
        NodeRegistrarSet { new_registrar: T::AccountId },
        /// Batch size set
        BatchSizeSet { new_size: u32 },
        /// Heartbeat period set
        NextHeartbeatPeriodSet { new_heartbeat_period: u32 },
        /// Heartbeat received
        HeartbeatReceived { reward_period_index: RewardPeriodIndex, node: NodeId<T> },
        /// Reward amount per period set
        NextRewardAmountPerPeriodSet { new_amount: BalanceOf<T> },
        /// Number of periods to mint set
        NumPeriodsToMintSet { periods: u32 },
        /// Reward payment toggled
        RewardEnabledSet { enabled: bool },
        /// Min uptime threshold set
        MinUptimeThresholdSet { threshold: Perbill },
        /// Max unstake percentage set
        MaxUnstakePercentageSet { percentage: Perbill },
        /// Unstake period set
        UnstakePeriodSet { duration_sec: Duration },
        /// Node deregistered
        NodeDeregistered { owner: T::AccountId, node: NodeId<T> },
        /// Signing key updated
        SigningKeyUpdated { owner: T::AccountId, node: NodeId<T> },
        /// Auto-stake duration set
        AutoStakeDurationSet { duration_sec: Duration },
        /// Stake added
        StakeAdded {
            owner: T::AccountId,
            node_id: NodeId<T>,
            reward_period: RewardPeriodIndex,
            amount: BalanceOf<T>,
            new_total: BalanceOf<T>,
        },
        /// Stake removed
        StakeRemoved {
            owner: T::AccountId,
            node_id: NodeId<T>,
            reward_period: RewardPeriodIndex,
            amount: BalanceOf<T>,
            new_total: BalanceOf<T>,
        },
        /// Restricted unstake duration set
        RestrictedUnstakeDurationSet { duration_sec: Duration },
        /// Reward fee percentage set
        RewardFeePercentageSet { percentage: Perbill },
        /// Auto-stake preference updated
        AutoStakePreferenceUpdated {
            owner: T::AccountId,
            node_id: NodeId<T>,
            auto_stake_rewards: bool,
        },
        /// Mint request submitted
        MintRequestSubmitted { amount: BalanceOf<T>, tx_id: EthereumId },
        /// Mint request resolved
        MintRequestResolved { tx_id: EthereumId, succeeded: bool },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Invalid node registrar
        OriginNotRegistrar,
        /// Invalid last paid node
        InvalidNodePointer,
        /// Invalid last paid period
        InvalidPeriodPointer,
        /// Node registrar not set
        RegistrarNotSet,
        /// Node already registered
        DuplicateNode,
        /// Invalid signing key
        InvalidSigningKey,
        /// Signing key already in use
        SigningKeyAlreadyInUse,
        /// Invalid reward period
        RewardPeriodInvalid,
        /// Invalid batch size
        BatchSizeInvalid,
        /// Invalid heartbeat period
        NextHeartbeatPeriodInvalid,
        /// Heartbeat period is zero
        NextHeartbeatPeriodZero,
        /// Reward pot has insufficient balance
        InsufficientBalanceForReward,
        /// Total uptime not found
        TotalUptimeNotFound,
        /// Node uptime not found
        NodeUptimeNotFound,
        /// Invalid reward payment request
        InvalidRewardPaymentRequest,
        /// Duplicate heartbeat
        DuplicateHeartbeat,
        /// Invalid heartbeat
        InvalidHeartbeat,
        /// Node not registered
        NodeNotRegistered,
        /// Failed to acquire OCW DB lock
        FailedToAcquireOcwDbLock,
        /// Reward amount is zero
        RewardAmountZero,
        /// Sender is not the signer
        SenderIsNotSigner,
        /// Unauthorized signed transaction
        UnauthorizedSignedTransaction,
        /// Signed transaction expired
        SignedTransactionExpired,
        /// Heartbeat threshold reached
        HeartbeatThresholdReached,
        /// Uptime threshold is zero
        UptimeThresholdZero,
        /// Node not owned by owner
        NodeNotOwnedByOwner,
        /// Unauthorized signing key update
        UnauthorizedSigningKeyUpdate,
        /// Signing key must be different
        SigningKeyMustBeDifferent,
        /// Amount must be greater than zero
        ZeroAmount,
        /// Insufficient free balance
        InsufficientFreeBalance,
        /// Insufficient staked balance
        InsufficientStakedBalance,
        /// Reserve failed
        ReserveFailed,
        /// Duration must be greater than zero
        DurationZero,
        /// No stake found
        NoStakeFound,
        /// Auto-stake still active
        AutoStakeStillActive,
        /// No stake available to unstake
        NoAvailableStakeToUnstake,
        /// Node not found
        NodeNotFound,
        /// Balance overflow
        BalanceOverflow,
        /// Balance underflow
        BalanceUnderflow,
        /// Reward pot snapshot not found
        RewardPotNotFound,
        /// Reward amount per period must be greater than zero
        NextRewardAmountPerPeriodZero,
        /// Mint amount cannot fit balance type
        MintAmountOverflow,
        /// No Tier1 event found for reward minting
        NoTier1MintEventFound,
        /// Mint request already in progress
        MintRequestInProgress,
    }

    #[pallet::config]
    pub trait Config:
        frame_system::Config
        + avn::Config
        + CreateInherent<Call<Self>>
        + CreateTransactionBase<Call<Self>>
    {
        /// Runtime event type
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// Runtime call type
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + IsSubType<Call<Self>>
            + From<Call<Self>>;
        /// Currency used by this pallet
        type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;
        // The identifier type for an offchain transaction signer.
        type SignerId: Member
            + Parameter
            + RuntimeAppPublic
            + Ord
            + MaybeSerializeDeserialize
            + MaxEncodedLen;
        /// Account type used for signature verification
        type Public: IdentifyAccount<AccountId = Self::AccountId>;
        /// Time provider
        type TimeProvider: UnixTime;
        /// Signature type
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;
        /// Token identifier type
        type Token: Parameter + Default + Copy + From<H160> + Into<H160> + MaxEncodedLen;
        /// Reward fee handler
        type RewardFeeHandler: PaymentHandler<
            AccountId = Self::AccountId,
            Token = Self::Token,
            TokenBalance = <Self::Currency as Currency<Self::AccountId>>::Balance,
            Error = DispatchError,
        >;
        /// Reward pot ID
        #[pallet::constant]
        type RewardPotId: Get<PalletId>;
        /// Signed transaction lifetime in blocks
        #[pallet::constant]
        type SignedTxLifetime: Get<u32>;
        /// Stake needed for one virtual node bonus
        #[pallet::constant]
        type VirtualNodeStake: Get<BalanceOf<Self>>;
        /// Extrinsic weight provider
        type WeightInfo: WeightInfo;
        /// Interface to the Ethereum bridge pallet
        type BridgeInterface: BridgeInterface;
        /// Hook to check for processed events
        type ProcessedEventsChecker: ProcessedEventsChecker;
        /// Interface to interact with app chains
        type AppChainInterface: AppChainInterface<AccountId = Self::AccountId>;
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Register a new node
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::register_node())]
        pub fn register_node(
            origin: OriginFor<T>,
            node: NodeId<T>,
            owner: T::AccountId,
            signing_key: T::SignerId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            ensure!(who == registrar, Error::<T>::OriginNotRegistrar);

            // Default to auto_stake true
            Self::do_register_node(node, owner, signing_key, true)?;
            Ok(())
        }

        /// Set admin configurations
        #[pallet::call_index(1)]
        #[pallet::weight(
            <T as Config>::WeightInfo::register_node()
            .max(<T as Config>::WeightInfo::set_admin_config_registrar())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_period())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_batch_size())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_heartbeat())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_amount())
            .max(<T as Config>::WeightInfo::set_admin_config_num_periods_to_mint())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_enabled())
            .max(<T as Config>::WeightInfo::set_admin_config_min_threshold())
            .max(<T as Config>::WeightInfo::set_admin_config_auto_stake_duration())
            .max(<T as Config>::WeightInfo::set_admin_config_max_unstake_percentage())
            .max(<T as Config>::WeightInfo::set_admin_config_unstake_period())
            .max(<T as Config>::WeightInfo::set_admin_config_restricted_unstake_duration())
            .max(<T as Config>::WeightInfo::set_admin_config_reward_fee_percentage())
        )]
        pub fn set_admin_config(
            origin: OriginFor<T>,
            config: AdminConfig<T::AccountId, BalanceOf<T>>,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            match config {
                AdminConfig::NodeRegistrar(registrar) => {
                    <NodeRegistrar<T>>::mutate(|maybe_registrar| {
                        *maybe_registrar = Some(registrar.clone())
                    });
                    Self::deposit_event(Event::NodeRegistrarSet { new_registrar: registrar });
                    return Ok(Some(<T as Config>::WeightInfo::set_admin_config_registrar()).into())
                },
                AdminConfig::NextRewardPeriodLength(period) => {
                    let heartbeat = <NextHeartbeatPeriod<T>>::get();
                    ensure!(period > heartbeat, Error::<T>::RewardPeriodInvalid);

                    let period_index = RewardPeriod::<T>::get().current;
                    let old_period = NextRewardPeriodLength::<T>::get();

                    NextRewardPeriodLength::<T>::put(period);

                    Self::deposit_event(Event::RewardPeriodLengthSet {
                        period_index,
                        old_reward_period_length: old_period,
                        new_reward_period_length: period,
                    });

                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_period()).into())
                },
                AdminConfig::BatchSize(size) => {
                    ensure!(size > 0 && size <= MAX_BATCH_SIZE, Error::<T>::BatchSizeInvalid);
                    <MaxBatchSize<T>>::mutate(|s| *s = size);
                    Self::deposit_event(Event::BatchSizeSet { new_size: size });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_batch_size()).into())
                },
                AdminConfig::NextHeartbeatPeriod(period) => {
                    let next_reward_period_length = NextRewardPeriodLength::<T>::get();
                    ensure!(period > 0, Error::<T>::NextHeartbeatPeriodZero);
                    ensure!(
                        period < next_reward_period_length,
                        Error::<T>::NextHeartbeatPeriodInvalid
                    );

                    <NextHeartbeatPeriod<T>>::put(period);

                    Self::deposit_event(Event::NextHeartbeatPeriodSet {
                        new_heartbeat_period: period,
                    });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_heartbeat()).into())
                },
                AdminConfig::NextRewardAmountPerPeriod(amount) => {
                    ensure!(
                        amount > BalanceOf::<T>::zero(),
                        Error::<T>::NextRewardAmountPerPeriodZero
                    );
                    <NextRewardAmountPerPeriod<T>>::put(amount);
                    Self::deposit_event(Event::NextRewardAmountPerPeriodSet { new_amount: amount });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_amount()).into())
                },
                AdminConfig::NumPeriodsToMint(periods) => {
                    <NumPeriodsToMint<T>>::put(periods);
                    Self::deposit_event(Event::NumPeriodsToMintSet { periods });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_num_periods_to_mint())
                        .into())
                },
                AdminConfig::RewardEnabled(enabled) => {
                    <RewardEnabled<T>>::put(enabled);
                    Self::deposit_event(Event::RewardEnabledSet { enabled });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_enabled()).into())
                },
                AdminConfig::MinUptimeThreshold(threshold) => {
                    ensure!(threshold > Perbill::zero(), Error::<T>::UptimeThresholdZero);
                    <MinUptimeThreshold<T>>::put(threshold);

                    Self::deposit_event(Event::MinUptimeThresholdSet { threshold });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_min_threshold()).into())
                },
                AdminConfig::AutoStakeDuration(duration_sec) => {
                    <AutoStakeDurationSec<T>>::put(duration_sec);
                    Self::deposit_event(Event::AutoStakeDurationSet { duration_sec });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_auto_stake_duration())
                        .into())
                },
                AdminConfig::MaxUnstakePercentage(percentage) => {
                    <MaxUnstakePercentage<T>>::put(percentage);
                    Self::deposit_event(Event::MaxUnstakePercentageSet { percentage });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_max_unstake_percentage())
                        .into())
                },
                AdminConfig::UnstakePeriod(duration_sec) => {
                    ensure!(duration_sec > 0, Error::<T>::DurationZero);
                    <UnstakePeriodSec<T>>::put(duration_sec);
                    Self::deposit_event(Event::UnstakePeriodSet { duration_sec });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_unstake_period()).into())
                },
                AdminConfig::RestrictedUnstakeDuration(duration_sec) => {
                    <RestrictedUnstakeDurationSec<T>>::put(duration_sec);
                    Self::deposit_event(Event::RestrictedUnstakeDurationSet { duration_sec });
                    Ok(Some(
                        <T as Config>::WeightInfo::set_admin_config_restricted_unstake_duration(),
                    )
                    .into())
                },
                AdminConfig::RewardFee(percentage) => {
                    <RewardFeePercentage<T>>::put(percentage);
                    Self::deposit_event(Event::RewardFeePercentageSet { percentage });
                    Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_fee_percentage())
                        .into())
                },
            }
        }

        /// Offchain call: pay and remove up to `MAX_BATCH_SIZE` nodes in the oldest unpaid period.
        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::offchain_pay_nodes(MAX_BATCH_SIZE))]
        pub fn offchain_pay_nodes(
            origin: OriginFor<T>,
            reward_period_index: RewardPeriodIndex,
            _author: Author<T>,
            _signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;

            let oldest_period = OldestUnpaidRewardPeriodIndex::<T>::get();
            // Be careful when using current period. Everything here should be based on previous
            // period
            let RewardPeriodInfo { current, .. } = RewardPeriod::<T>::get();

            // Only pay for completed periods
            ensure!(
                reward_period_index == oldest_period && oldest_period < current,
                Error::<T>::InvalidRewardPaymentRequest
            );

            let total_uptime = TotalUptime::<T>::get(&oldest_period);
            let maybe_node_uptime = NodeUptime::<T>::iter_prefix(oldest_period).next();

            if total_uptime.total_weight == 0 && maybe_node_uptime.is_none() {
                // No nodes to pay for this period so complete it
                Self::complete_reward_payout(oldest_period);
                return Ok(Some(<T as Config>::WeightInfo::offchain_pay_nodes(1u32)).into())
            }

            ensure!(total_uptime.total_weight > 0, Error::<T>::TotalUptimeNotFound);
            ensure!(maybe_node_uptime.is_some(), Error::<T>::NodeUptimeNotFound);

            let reward_pot =
                RewardPot::<T>::get(&oldest_period).ok_or(Error::<T>::RewardPotNotFound)?;
            let total_reward = reward_pot.total_reward;

            let mut paid_nodes = Vec::new();
            let mut last_node_paid: Option<T::AccountId> = None;
            let mut iter;

            match LastPaidPointer::<T>::get() {
                Some(pointer) => {
                    iter = Self::get_iterator_from_last_paid(oldest_period, pointer)?;
                },
                None => {
                    iter = NodeUptime::<T>::iter_prefix(oldest_period);
                    // This is a new payout so validate that the reward pot has enough to pay
                    ensure!(
                        Self::reward_pot_balance().ge(&total_reward),
                        Error::<T>::InsufficientBalanceForReward
                    );
                },
            }

            let pay = |node: &NodeId<T>,
                       uptime: UptimeInfo<BlockNumberFor<T>>|
             -> Result<(), DispatchError> {
                let node_info =
                    NodeRegistry::<T>::get(node).ok_or(Error::<T>::NodeNotRegistered)?;

                let node_weight = Self::calculate_node_weight(
                    node,
                    uptime,
                    &node_info,
                    reward_pot.uptime_threshold,
                    reward_pot.reward_end_time,
                );

                let (reward_amount, reward_percentage) =
                    Self::calculate_reward(node_weight, &total_uptime.total_weight, &total_reward)?;

                Self::pay_reward(
                    &oldest_period,
                    node.clone(),
                    &node_info,
                    reward_amount,
                    reward_percentage,
                )?;
                Ok(())
            };

            for (node, uptime) in iter.by_ref().take(MaxBatchSize::<T>::get() as usize) {
                if let Err(e) = pay(&node, uptime) {
                    Self::deposit_event(Event::ErrorPayingReward {
                        reward_period: oldest_period,
                        node: node.clone(),
                        error: e,
                    });
                }
                // We always move on even if payment fails. Failed payments will be handled
                // offchain.
                last_node_paid = Some(node.clone());
                paid_nodes.push(node.clone());
            }

            Self::remove_paid_nodes(oldest_period, &paid_nodes);

            if iter.next().is_some() {
                Self::update_last_paid_pointer(oldest_period, last_node_paid);
            } else {
                Self::complete_reward_payout(oldest_period);
            }
            return Ok(
                Some(<T as Config>::WeightInfo::offchain_pay_nodes(paid_nodes.len() as u32)).into()
            )
        }

        /// Offchain call: Submit heartbeat to show node is still alive
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::offchain_submit_heartbeat())]
        pub fn offchain_submit_heartbeat(
            origin: OriginFor<T>,
            node: NodeId<T>,
            reward_period_index: RewardPeriodIndex,
            // This helps prevent signature re-use
            heartbeat_count: u64,
            _signature: <T::SignerId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;

            Self::validate_heartbeats(node.clone(), reward_period_index, heartbeat_count)?;

            let current_reward_period = RewardPeriod::<T>::get().current;
            // if we pass validation we have a registered node but double check
            let node_info = NodeRegistry::<T>::get(&node).ok_or(Error::<T>::NodeNotRegistered)?;
            let now = frame_system::Pallet::<T>::block_number();

            let weight = <NodeUptime<T>>::mutate(&current_reward_period, &node, |maybe_info| {
                let info = maybe_info.get_or_insert_with(|| UptimeInfo {
                    count: 0,
                    last_reported: now,
                    weight: 0,
                });

                let node_weight =
                    Self::effective_heartbeat_weight(&node_info, Self::time_now_sec());

                info.count = info.count.saturating_add(1);
                info.last_reported = now;
                info.weight = info.weight.saturating_add(node_weight);

                // the total uptime for the period
                node_weight
            });

            <TotalUptime<T>>::mutate(&current_reward_period, |total| {
                total.total_heartbeats = total.total_heartbeats.saturating_add(1);
                total.total_weight = total.total_weight.saturating_add(weight);
            });

            Self::deposit_event(Event::HeartbeatReceived {
                reward_period_index: current_reward_period,
                node,
            });

            Ok(())
        }

        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::signed_register_node())]
        pub fn signed_register_node(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            node: NodeId<T>,
            owner: T::AccountId,
            signing_key: T::SignerId,
            block_number: BlockNumberFor<T>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);

            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            ensure!(registrar == sender, Error::<T>::OriginNotRegistrar);
            ensure!(
                block_number.saturating_add(T::SignedTxLifetime::get().into()) >
                    frame_system::Pallet::<T>::block_number(),
                Error::<T>::SignedTransactionExpired
            );

            // Create and verify the signed payload
            let signed_payload = encode_signed_register_node_params::<T>(
                &proof.relayer,
                &node,
                &owner,
                &signing_key,
                &block_number,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload).is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            // Perform the actual registration. Default to auto_stake_rewards = true
            Self::do_register_node(node, owner, signing_key, true)?;

            Ok(())
        }

        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::deregister_nodes(nodes_to_deregister.len() as u32))]
        pub fn deregister_nodes(
            origin: OriginFor<T>,
            owner: T::AccountId,
            nodes_to_deregister: BoundedVec<NodeId<T>, MaxNodesToDeregister>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;

            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            ensure!(registrar == sender, Error::<T>::OriginNotRegistrar);

            Self::do_deregister_nodes(&owner, &nodes_to_deregister)?;

            Ok(())
        }

        #[pallet::call_index(6)]
        #[pallet::weight(<T as Config>::WeightInfo::signed_deregister_nodes(nodes_to_deregister.len() as u32))]
        pub fn signed_deregister_nodes(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            owner: T::AccountId,
            nodes_to_deregister: BoundedVec<NodeId<T>, MaxNodesToDeregister>,
            block_number: BlockNumberFor<T>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == proof.signer, Error::<T>::SenderIsNotSigner);

            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            ensure!(registrar == sender, Error::<T>::OriginNotRegistrar);
            ensure!(
                block_number.saturating_add(T::SignedTxLifetime::get().into()) >
                    frame_system::Pallet::<T>::block_number(),
                Error::<T>::SignedTransactionExpired
            );

            // Create and verify the signed payload
            let signed_payload = encode_signed_deregister_node_params::<T>(
                &proof.relayer,
                &owner,
                &nodes_to_deregister,
                &(nodes_to_deregister.len() as u32),
                &block_number,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload).is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            Self::do_deregister_nodes(&owner, &nodes_to_deregister)?;

            Ok(())
        }

        /// Update signing key for a registered node
        #[pallet::call_index(7)]
        #[pallet::weight(<T as Config>::WeightInfo::update_signing_key())]
        pub fn update_signing_key(
            origin: OriginFor<T>,
            node: NodeId<T>,
            new_signing_key: T::SignerId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let registrar = NodeRegistrar::<T>::get().ok_or(Error::<T>::RegistrarNotSet)?;
            let current_info =
                NodeRegistry::<T>::get(&node).ok_or(Error::<T>::NodeNotRegistered)?;
            let owner = current_info.owner;

            ensure!(who == registrar || who == owner, Error::<T>::UnauthorizedSigningKeyUpdate);
            // We could remove this and use the check below to catch all cases but this is more user
            // friendly
            ensure!(
                current_info.signing_key != new_signing_key,
                Error::<T>::SigningKeyMustBeDifferent
            );
            ensure!(
                !SigningKeyToNodeId::<T>::contains_key(&new_signing_key),
                Error::<T>::SigningKeyAlreadyInUse
            );

            <NodeRegistry<T>>::mutate(&node, |maybe_info| {
                if let Some(info) = maybe_info.as_mut() {
                    info.signing_key = new_signing_key.clone();
                }
            });

            Self::rotate_signing_key_index(&node, &current_info.signing_key, &new_signing_key)?;
            Self::deposit_event(Event::SigningKeyUpdated { owner, node });

            Ok(())
        }

        #[pallet::call_index(8)]
        #[pallet::weight(<T as Config>::WeightInfo::add_stake())]
        pub fn add_stake(
            origin: OriginFor<T>,
            node_id: NodeId<T>,
            amount: BalanceOf<T>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            ensure!(
                <OwnedNodes<T>>::contains_key(&owner, &node_id),
                Error::<T>::NodeNotOwnedByOwner
            );
            let reward_period = RewardPeriod::<T>::get().current;
            let new_total = Self::do_add_stake(&owner, &node_id, amount)?;

            Self::deposit_event(Event::StakeAdded {
                owner,
                node_id,
                reward_period,
                amount,
                new_total,
            });
            Ok(())
        }

        #[pallet::call_index(9)]
        #[pallet::weight(<T as Config>::WeightInfo::remove_stake())]
        pub fn remove_stake(
            origin: OriginFor<T>,
            node_id: NodeId<T>,
            maybe_amount: Option<BalanceOf<T>>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            ensure!(
                <OwnedNodes<T>>::contains_key(&owner, &node_id),
                Error::<T>::NodeNotOwnedByOwner
            );

            let reward_period = RewardPeriod::<T>::get().current;
            let (amount, new_total) = Self::do_remove_stake(&owner, &node_id, maybe_amount)?;

            Self::deposit_event(Event::StakeRemoved {
                owner,
                node_id,
                reward_period,
                amount,
                new_total,
            });

            Ok(())
        }

        #[pallet::call_index(10)]
        #[pallet::weight(<T as Config>::WeightInfo::update_auto_stake_preference())]
        pub fn update_auto_stake_preference(
            origin: OriginFor<T>,
            node_id: NodeId<T>,
            auto_stake_rewards: bool,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            ensure!(
                <OwnedNodes<T>>::contains_key(&owner, &node_id),
                Error::<T>::NodeNotOwnedByOwner
            );

            <NodeRegistry<T>>::try_mutate(&node_id, |maybe_info| -> Result<(), DispatchError> {
                let info = maybe_info.as_mut().ok_or(Error::<T>::NodeNotFound)?;
                info.auto_stake_rewards = auto_stake_rewards;
                Ok(())
            })?;

            Self::deposit_event(Event::AutoStakePreferenceUpdated {
                owner,
                node_id,
                auto_stake_rewards,
            });

            Ok(())
        }

        #[pallet::call_index(11)]
        #[pallet::weight(<T as Config>::WeightInfo::offchain_mint_rewards())]
        pub fn offchain_mint_rewards(
            origin: OriginFor<T>,
            amount: BalanceOf<T>,
            author: Author<T>,
            signature: <T::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResult {
            ensure_none(origin)?;

            // Signature re-use is not an issue because this transaction is forced to be sent by the
            // validator of this node. Its not propagated on the network
            ensure!(
                AVN::<T>::signature_is_valid(&(MINT_REWARDS_CONTEXT, amount), &author, &signature),
                Error::<T>::UnauthorizedSignedTransaction
            );

            ensure!(amount > BalanceOf::<T>::zero(), Error::<T>::ZeroAmount);
            ensure!(!PendingMintRequestState::<T>::exists(), Error::<T>::MintRequestInProgress);

            let tx_id = Self::send_mint_to_ethereum(amount)?;
            PendingMintRequestState::<T>::put(PendingMintRequest {
                tx_id,
                amount,
                bridge_confirmed: false,
                credit_received: false,
            });

            Self::deposit_event(Event::MintRequestSubmitted { amount, tx_id });

            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        // Keep this logic light and bounded
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            if !RewardEnabled::<T>::get() {
                return <T as Config>::WeightInfo::on_initialise_no_reward_period()
            }

            let reward_period = RewardPeriod::<T>::get();
            if !reward_period.should_update(n) {
                return <T as Config>::WeightInfo::on_initialise_no_reward_period()
            }

            let previous_index = reward_period.current;
            let previous_uptime_threshold = reward_period.uptime_threshold;
            let reward_amount = reward_period.reward_amount;

            // We want to avoid unnecessary reads, so we perform this check and exit early
            let next_reward_period_length = NextRewardPeriodLength::<T>::get();
            let next_heartbeat_period = NextHeartbeatPeriod::<T>::get();
            if next_reward_period_length == 0 || next_heartbeat_period == 0 {
                return <T as Config>::WeightInfo::on_initialise_no_reward_period()
                    .saturating_add(<T as frame_system::Config>::DbWeight::get().reads(2))
            }

            let next_reward_amount = NextRewardAmountPerPeriod::<T>::get();
            let next_uptime_threshold =
                Self::calculate_uptime_threshold(next_reward_period_length, next_heartbeat_period);

            let next_reward_period = reward_period.update(
                n,
                next_reward_period_length,
                next_heartbeat_period,
                next_uptime_threshold,
                next_reward_amount,
            );
            RewardPeriod::<T>::put(&next_reward_period);

            // take a snapshot of the reward pot amount to pay for the previous reward period
            <RewardPot<T>>::insert(
                previous_index,
                RewardPotInfo::<BalanceOf<T>>::new(
                    reward_amount,
                    previous_uptime_threshold,
                    Self::time_now_sec(),
                ),
            );

            OutstandingRewardToPay::<T>::mutate(|outstanding| {
                *outstanding = outstanding.saturating_add(reward_amount);
            });

            // Notify app chains that a new reward period has started.
            let hook_weight =
                T::AppChainInterface::on_new_reward_period(&next_reward_period.current);

            Self::deposit_event(Event::NewRewardPeriodStarted {
                reward_period_index: next_reward_period.current,
                reward_period_length: next_reward_period.length,
                uptime_threshold: next_reward_period.uptime_threshold,
                previous_period_reward: reward_amount,
            });

            <T as Config>::WeightInfo::on_initialise_with_new_reward_period()
                .saturating_add(hook_weight)
        }

        fn offchain_worker(n: BlockNumberFor<T>) {
            log::info!("🛠️  OCW for node manager");

            if <RewardEnabled<T>>::get() == false {
                log::warn!("🛠️  OCW - rewards are disabled, skipping");
                return
            }

            let maybe_author = Self::try_get_node_author(n);
            if let Some(author) = maybe_author {
                Self::trigger_mint_if_required(author.clone());

                let oldest_unpaid_period = OldestUnpaidRewardPeriodIndex::<T>::get();
                Self::trigger_payment_if_required(oldest_unpaid_period, author);
                // If this is an author node, we don't need to send a heartbeat
                return
            }

            Self::send_heartbeat_if_required(n);
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;
        fn validate_unsigned(source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            if <RewardEnabled<T>>::get() == false {
                return InvalidTransaction::Custom(ERROR_CODE_REWARD_DISABLED).into()
            }

            let reduce_priority: TransactionPriority = TransactionPriority::from(1000u64);
            match call {
                Call::offchain_pay_nodes { reward_period_index, author, signature } => {
                    // Discard unsinged tx's not coming from the local OCW.
                    match source {
                        TransactionSource::Local | TransactionSource::InBlock => { /* allowed */ },
                        _ => return InvalidTransaction::Call.into(),
                    }

                    if AVN::<T>::signature_is_valid(
                        // Technically this signature can be replayed for the duration of the
                        // reward period but in reality, since we only
                        // accept locally produced transactions and we don'
                        // t propagate them, only an author can submit this transaction and there
                        // is nothing to gain.
                        &(PAYOUT_REWARD_CONTEXT, reward_period_index),
                        &author,
                        signature,
                    ) {
                        ValidTransaction::with_tag_prefix("NodeManagerPayout")
                            .and_provides((PAYOUT_REWARD_CONTEXT, reward_period_index, author))
                            .priority(TransactionPriority::max_value() - reduce_priority)
                            .longevity(64_u64)
                            // We don't propagate this transaction,
                            // it ensures only block authors can pay rewards
                            .propagate(false)
                            .build()
                    } else {
                        InvalidTransaction::Custom(ERROR_CODE_INVALID_PAY_SIGNATURE).into()
                    }
                },
                Call::offchain_submit_heartbeat {
                    node,
                    reward_period_index,
                    heartbeat_count,
                    signature,
                } => {
                    let node_info = NodeRegistry::<T>::get(&node);
                    match node_info {
                        Some(info) => {
                            if Self::validate_heartbeats(
                                node.clone(),
                                *reward_period_index,
                                *heartbeat_count,
                            )
                            .is_err()
                            {
                                return InvalidTransaction::Custom(ERROR_CODE_INVALID_HEARTBEAT)
                                    .into()
                            }

                            if !Self::offchain_signature_is_valid(
                                &(HEARTBEAT_CONTEXT, heartbeat_count, reward_period_index),
                                &info.signing_key,
                                signature,
                            ) {
                                return InvalidTransaction::Custom(
                                    ERROR_CODE_INVALID_HEARTBEAT_SIGNATURE,
                                )
                                .into()
                            }

                            return ValidTransaction::with_tag_prefix("NodeManagerHeartbeat")
                                .and_provides((
                                    HEARTBEAT_CONTEXT,
                                    node,
                                    reward_period_index,
                                    heartbeat_count,
                                ))
                                .priority(TransactionPriority::max_value() - reduce_priority)
                                .longevity(64_u64)
                                .build()
                        },
                        _ => InvalidTransaction::Custom(ERROR_CODE_INVALID_NODE).into(),
                    }
                },
                Call::offchain_mint_rewards { amount, author, signature } => {
                    match source {
                        TransactionSource::Local | TransactionSource::InBlock => {},
                        _ => return InvalidTransaction::Call.into(),
                    }

                    if PendingMintRequestState::<T>::exists() {
                        return InvalidTransaction::Stale.into()
                    }

                    if AVN::<T>::signature_is_valid(
                        &(MINT_REWARDS_CONTEXT, amount),
                        author,
                        signature,
                    ) {
                        ValidTransaction::with_tag_prefix("NodeManagerMint")
                            .and_provides((MINT_REWARDS_CONTEXT, amount, author))
                            .priority(TransactionPriority::max_value() - reduce_priority)
                            .longevity(64_u64)
                            .propagate(false)
                            .build()
                    } else {
                        InvalidTransaction::Custom(ERROR_CODE_INVALID_MINT_SIGNATURE).into()
                    }
                },
                _ => InvalidTransaction::Call.into(),
            }
        }
    }

    impl<T: Config> Pallet<T> {
        fn validate_heartbeats(
            node: NodeId<T>,
            reward_period_index: RewardPeriodIndex,
            heartbeat_count: u64,
        ) -> DispatchResult {
            ensure!(<NodeRegistry<T>>::contains_key(&node), Error::<T>::NodeNotRegistered);
            let reward_period = RewardPeriod::<T>::get();
            let current_reward_period = reward_period.current;
            let maybe_uptime_info = <NodeUptime<T>>::get(reward_period_index, &node);

            ensure!(current_reward_period == reward_period_index, Error::<T>::InvalidHeartbeat);

            if let Some(uptime_info) = maybe_uptime_info {
                ensure!(
                    uptime_info.count < reward_period.uptime_threshold as u64,
                    Error::<T>::HeartbeatThresholdReached
                );

                let expected_submission = uptime_info.last_reported +
                    BlockNumberFor::<T>::from(reward_period.heartbeat_period);
                ensure!(
                    frame_system::Pallet::<T>::block_number() >= expected_submission,
                    Error::<T>::DuplicateHeartbeat
                );
                ensure!(heartbeat_count == uptime_info.count, Error::<T>::InvalidHeartbeat);
            } else {
                ensure!(heartbeat_count == 0, Error::<T>::InvalidHeartbeat);
            }

            Ok(())
        }

        fn do_deregister_nodes(
            owner: &T::AccountId,
            nodes: &BoundedVec<NodeId<T>, MaxNodesToDeregister>,
        ) -> DispatchResult {
            for node in nodes {
                ensure!(
                    <OwnedNodes<T>>::contains_key(owner, node),
                    Error::<T>::NodeNotOwnedByOwner
                );

                let info = NodeRegistry::<T>::take(node).ok_or(Error::<T>::NodeNotRegistered)?;
                Self::remove_signing_key_index(node, &info.signing_key)?;

                <OwnedNodes<T>>::remove(owner, node);
                <OwnedNodesCount<T>>::mutate(owner, |count| *count = count.saturating_sub(1));
                <TotalRegisteredNodes<T>>::mutate(|n| *n = n.saturating_sub(1));

                // Unreserve stake for this node if there is any
                if !info.stake.amount.is_zero() {
                    Self::update_reserves(owner, info.stake.amount, StakeOperation::Remove)?;
                }

                Self::deposit_event(Event::NodeDeregistered {
                    owner: owner.clone(),
                    node: node.clone(),
                });
            }
            Ok(())
        }

        pub(crate) fn calculate_uptime_threshold(
            reward_period_length: u32,
            heartbeat_period: u32,
        ) -> u32 {
            let threshold = MinUptimeThreshold::<T>::get().unwrap_or(Self::get_default_threshold());

            let max_heartbeats = reward_period_length.saturating_div(heartbeat_period);
            threshold * max_heartbeats
        }

        fn do_register_node(
            node: NodeId<T>,
            owner: T::AccountId,
            signing_key: T::SignerId,
            auto_stake_rewards: bool,
        ) -> DispatchResult {
            ensure!(!<NodeRegistry<T>>::contains_key(&node), Error::<T>::DuplicateNode);
            ensure!(
                !SigningKeyToNodeId::<T>::contains_key(&signing_key),
                Error::<T>::SigningKeyAlreadyInUse
            );

            let auto_stake_expiry = Self::calculate_auto_stake_expiry();

            <OwnedNodes<T>>::insert(&owner, &node, ());
            <OwnedNodesCount<T>>::mutate(&owner, |count| *count = count.saturating_add(1));

            <TotalRegisteredNodes<T>>::mutate(|n| {
                *n = n.saturating_add(1);
            });

            Self::insert_signing_key_index(&node, &signing_key)?;

            let node_serial_number = <NextNodeSerialNumber<T>>::mutate(|n| {
                let current = *n;
                *n = n.saturating_add(1);
                current
            });

            <NodeRegistry<T>>::insert(
                &node,
                NodeInfo::<T::SignerId, T::AccountId, BalanceOf<T>>::new(
                    owner.clone(),
                    signing_key,
                    node_serial_number,
                    auto_stake_expiry,
                    auto_stake_rewards,
                    StakeInfo::<BalanceOf<T>>::new(
                        Zero::zero(),
                        Zero::zero(),
                        None,
                        UnstakeRestriction::Locked,
                    ),
                ),
            );

            Self::deposit_event(Event::NodeRegistered { owner, node });

            Ok(())
        }

        pub fn offchain_signature_is_valid<D: Encode>(
            data: &D,
            signer: &T::SignerId,
            signature: &<T::SignerId as RuntimeAppPublic>::Signature,
        ) -> bool {
            let signature_valid =
                data.using_encoded(|encoded_data| signer.verify(&encoded_data, &signature));

            log::trace!(
                "🪲 Validating signature: [ data {:?} - account {:?} - signature {:?} ] Result: {}",
                data.encode(),
                signer.encode(),
                signature,
                signature_valid
            );
            return signature_valid
        }

        pub fn get_encoded_call_param(
            call: &<T as Config>::RuntimeCall,
        ) -> Option<(&Proof<T::Signature, T::AccountId>, Vec<u8>)> {
            let call = match call.is_sub_type() {
                Some(call) => call,
                None => return None,
            };

            match call {
                Call::signed_register_node {
                    ref proof,
                    ref node,
                    ref owner,
                    ref signing_key,
                    ref block_number,
                } => {
                    let encoded_data = encode_signed_register_node_params::<T>(
                        &proof.relayer,
                        node,
                        owner,
                        signing_key,
                        block_number,
                    );

                    Some((proof, encoded_data))
                },
                Call::signed_deregister_nodes {
                    ref proof,
                    ref owner,
                    ref nodes_to_deregister,
                    ref block_number,
                } => {
                    let encoded_data = encode_signed_deregister_node_params::<T>(
                        &proof.relayer,
                        owner,
                        nodes_to_deregister,
                        &(nodes_to_deregister.len() as u32),
                        block_number,
                    );

                    Some((proof, encoded_data))
                },
                _ => None,
            }
        }

        pub fn get_default_threshold() -> Perbill {
            Perbill::from_percent(33)
        }

        pub fn calculate_auto_stake_expiry() -> Duration {
            let current_time = Self::time_now_sec();
            current_time.saturating_add(AutoStakeDurationSec::<T>::get())
        }

        pub fn next_mint_amount_to_request() -> Option<BalanceOf<T>> {
            if PendingMintRequestState::<T>::exists() {
                return None
            }

            let num_periods = NumPeriodsToMint::<T>::get();
            if num_periods == 0 {
                return None
            }

            let reward_per_period = NextRewardAmountPerPeriod::<T>::get();
            if reward_per_period == BalanceOf::<T>::zero() {
                return None
            }

            let outstanding = OutstandingRewardToPay::<T>::get();
            let current_balance = Self::reward_pot_balance();

            // N periods of runway
            let runway = reward_per_period.checked_mul(&(num_periods.into())).or_else(|| {
                log::error!(
                    "💔 Mint overflow: reward_per_period * num_periods ({:?} * {:?})",
                    reward_per_period,
                    num_periods
                );
                None
            })?;

            // Mint triggers when pot drops below this (N periods of buffer above obligations)
            let refill_threshold = outstanding.checked_add(&runway).or_else(|| {
                log::error!(
                    "💔 Mint overflow: outstanding + runway ({:?} + {:?})",
                    outstanding,
                    runway
                );
                None
            })?;

            // After minting, pot should reach this (2N periods of buffer above obligations)
            let target = refill_threshold.checked_add(&runway).or_else(|| {
                log::error!(
                    "💔 Mint overflow: refill_threshold + runway ({:?} + {:?})",
                    refill_threshold,
                    runway
                );
                None
            })?;

            if current_balance >= refill_threshold {
                // We have enough in the pot to cover obligations + runway, no need to mint yet
                return None
            }

            let mint_amount = target.checked_sub(&current_balance).or_else(|| {
                log::error!(
                    "💔 Mint underflow: target - balance ({:?} - {:?})",
                    target,
                    current_balance
                );
                None
            })?;

            // In normal operation mint ≈ runway (N × reward).
            // Add a cap as a safety ceiling.
            let max_mint = runway.saturating_mul(MINT_SAFETY_CAP_MULTIPLIER.into());
            if mint_amount > max_mint {
                log::error!(
                    "💔💔 Mint amount {:?} exceeds safety cap {:?} ({} x N x reward). There might be bridge issues or payout has stalled.",
                    mint_amount, max_mint, MINT_SAFETY_CAP_MULTIPLIER
                );

                return None
            }

            Some(mint_amount)
        }

        /// Insert signing key reverse index. Fails if key already belongs to another node.
        fn insert_signing_key_index(node: &NodeId<T>, signing_key: &T::SignerId) -> DispatchResult {
            if let Some(existing_node) = SigningKeyToNodeId::<T>::get(signing_key) {
                ensure!(&existing_node == node, Error::<T>::SigningKeyAlreadyInUse);
                // If it already maps to this node, do nothing.
                return Ok(())
            }

            SigningKeyToNodeId::<T>::insert(signing_key, node);
            Ok(())
        }

        /// Remove signing key reverse index. Defensive: only remove if it points at this node.
        fn remove_signing_key_index(node: &NodeId<T>, signing_key: &T::SignerId) -> DispatchResult {
            if let Some(existing_node) = SigningKeyToNodeId::<T>::get(signing_key) {
                ensure!(&existing_node == node, Error::<T>::InvalidSigningKey);
                SigningKeyToNodeId::<T>::remove(signing_key);
            }
            Ok(())
        }

        fn rotate_signing_key_index(
            node: &NodeId<T>,
            old_key: &T::SignerId,
            new_key: &T::SignerId,
        ) -> DispatchResult {
            if old_key == new_key {
                return Ok(())
            }

            Self::remove_signing_key_index(node, old_key)?;
            Self::insert_signing_key_index(node, new_key)?;
            Ok(())
        }

        fn send_mint_to_ethereum(amount: BalanceOf<T>) -> Result<EthereumId, DispatchError> {
            let function_name: &[u8] = BridgeContractMethod::MintRewards.name_as_bytes();
            let amount_u128: u128 = amount.saturated_into();
            let params = vec![(b"uint128".to_vec(), amount_u128.to_string().into_bytes())];

            T::BridgeInterface::publish(function_name, &params, PALLET_ID.to_vec())
                .map_err(|e| DispatchError::Other(e.into()))
        }

        fn credit_reward_pot(raw_amount: u128) -> DispatchResult {
            let amount = <BalanceOf<T> as TryFrom<u128>>::try_from(raw_amount)
                .map_err(|_| Error::<T>::MintAmountOverflow)?;

            let reward_account = Self::compute_reward_account_id();
            let _imbalance: PositiveImbalanceOf<T> =
                <T as Config>::Currency::deposit_creating(&reward_account, amount);

            Ok(())
        }

        fn resolve_pending_mint_request(tx_id: EthereumId, succeeded: bool) {
            PendingMintRequestState::<T>::kill();
            Self::deposit_event(Event::MintRequestResolved { tx_id, succeeded });
        }

        fn process_mint_request_result(tx_id: EthereumId, succeeded: bool) -> DispatchResult {
            match PendingMintRequestState::<T>::get() {
                Some(mut pending) if pending.tx_id == tx_id => {
                    if !succeeded {
                        log::error!("💔 Mint request to Ethereum failed. tx_id: {:?}", tx_id);

                        Self::resolve_pending_mint_request(tx_id, false);
                        return Ok(())
                    }

                    if pending.credit_received {
                        Self::resolve_pending_mint_request(tx_id, true);
                    } else {
                        pending.bridge_confirmed = true;
                        PendingMintRequestState::<T>::put(pending);
                    }
                },
                _ => {
                    // Not the currently tracked node-manager mint request so ignore
                },
            }

            Ok(())
        }

        fn process_rewards_minted(
            event: &EthEvent,
            data: &TotalSupplyUpdatedData,
        ) -> DispatchResult {
            let event_id = &event.event_id;
            let event_validity = T::ProcessedEventsChecker::processed_event_exists(event_id);
            ensure!(event_validity, Error::<T>::NoTier1MintEventFound);

            ensure!(data.amount > 0, Error::<T>::ZeroAmount);

            Self::credit_reward_pot(data.amount)?;

            // t2_tx_id == 0 means this mint was triggered directly by the owner on Ethereum,
            // so it should credit the reward pot but must not affect pallet-tracked mint state.
            if data.t2_tx_id == 0 {
                return Ok(())
            }

            match PendingMintRequestState::<T>::get() {
                Some(mut pending) if pending.tx_id == data.t2_tx_id =>
                    if pending.bridge_confirmed {
                        Self::resolve_pending_mint_request(data.t2_tx_id, true);
                    } else {
                        pending.credit_received = true;
                        PendingMintRequestState::<T>::put(pending);
                    },
                _ => {
                    // This rewards-minted log is either for an owner-triggered mint, some other
                    // caller, or an old request we are no longer tracking. The credit has already
                    // been applied, so nothing else to do.
                },
            }

            Ok(())
        }

        fn processed_event_handler(event: &EthEvent) -> DispatchResult {
            match &event.event_data {
                EventData::LogRewardsMinted(d) => Self::process_rewards_minted(event, d),
                _ => Ok(()),
            }
        }
    }

    impl<T: Config> InnerCallValidator for Pallet<T> {
        type Call = <T as Config>::RuntimeCall;

        fn signature_is_valid(call: &Box<Self::Call>) -> bool {
            if let Some((proof, signed_payload)) = Self::get_encoded_call_param(call) {
                return verify_signature::<T::Signature, T::AccountId>(
                    &proof,
                    &signed_payload.as_slice(),
                )
                .is_ok()
            }

            false
        }
    }

    impl<T: Config> ProcessedEventHandler for Pallet<T> {
        fn on_event_processed(event: &EthEvent) -> DispatchResult {
            Self::processed_event_handler(event)
        }
    }

    impl<T: Config> BridgeInterfaceNotification for Pallet<T> {
        fn process_result(
            tx_id: EthereumId,
            caller_id: Vec<u8>,
            succeeded: bool,
        ) -> DispatchResult {
            if caller_id.as_slice() != PALLET_ID {
                return Ok(())
            }

            Self::process_mint_request_result(tx_id, succeeded)
        }

        fn on_incoming_event_processed(event: &EthEvent) -> DispatchResult {
            Self::processed_event_handler(event)
        }
    }
}

pub fn encode_signed_register_node_params<T: Config>(
    relayer: &T::AccountId,
    node: &NodeId<T>,
    owner: &T::AccountId,
    signing_key: &T::SignerId,
    block_number: &BlockNumberFor<T>,
) -> Vec<u8> {
    (SIGNED_REGISTER_NODE_CONTEXT, relayer.clone(), node, owner, signing_key, block_number).encode()
}

pub fn encode_signed_deregister_node_params<T: Config>(
    relayer: &T::AccountId,
    owner: &T::AccountId,
    nodes_to_deregister: &BoundedVec<NodeId<T>, MaxNodesToDeregister>,
    number_of_nodes_to_deregister: &u32,
    block_number: &BlockNumberFor<T>,
) -> Vec<u8> {
    (
        SIGNED_DEREGISTER_NODE_CONTEXT,
        relayer.clone(),
        owner,
        nodes_to_deregister,
        number_of_nodes_to_deregister,
        block_number,
    )
        .encode()
}
