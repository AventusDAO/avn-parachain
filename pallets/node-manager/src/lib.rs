#![cfg_attr(not(feature = "std"), no_std)]
use codec::{Decode, DecodeWithMemTracking, Encode, FullCodec};
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
use pallet_avn::{self as avn};
use sp_application_crypto::RuntimeAppPublic;
use sp_avn_common::{event_types::Validator, FeePaymentHandler, REGISTERED_NODE_KEY};
use sp_core::{MaxEncodedLen, H160};
use sp_runtime::{
    offchain::storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
    scale_info::TypeInfo,
    traits::{AccountIdConversion, Dispatchable, IdentifyAccount, Verify, Zero},
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
const HEARTBEAT_CONTEXT: &'static [u8] = b"NodeManager_heartbeat";
const MAX_BATCH_SIZE: u32 = 1_000;
pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);
pub const SIGNED_REGISTER_NODE_CONTEXT: &[u8] = b"register_node";
pub const SIGNED_DEREGISTER_NODE_CONTEXT: &[u8] = b"deregister_node";
pub const MAX_NODES_TO_DEREGISTER: u32 = 64;
pub const MAX_STAKE_CHANGES_PER_PERIOD: u32 = 256;

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

pub type AVN<T> = avn::Pallet<T>;
pub type Author<T> =
    Validator<<T as avn::Config>::AuthorityId, <T as frame_system::Config>::AccountId>;
pub use pallet::*;

pub(crate) type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
pub(crate) type RewardPeriodIndex = u64;
/// A type alias for a unique identifier of a node
pub(crate) type NodeId<T> = <T as frame_system::Config>::AccountId;
/// The max number of nodes that can be deregistered in a single call
pub type MaxNodesToDeregister = ConstU32<MAX_NODES_TO_DEREGISTER>;
/// The max number of stake changes an owner can do in a period
pub type MaxStakeChangesPerPeriod = ConstU32<MAX_STAKE_CHANGES_PER_PERIOD>;

#[frame_support::pallet]
pub mod pallet {
    use sp_avn_common::{verify_signature, InnerCallValidator, Proof};

    use super::*;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    /// Map of registered nodes
    #[pallet::storage]
    pub type NodeRegistry<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        NodeId<T>,
        NodeInfo<T::SignerId, T::AccountId, BalanceOf<T>>,
        OptionQuery,
    >;

    /// Reverse index: signing_key -> node_id
    #[pallet::storage]
    pub type SigningKeyToNodeId<T: Config> =
        StorageMap<_, Blake2_128Concat, T::SignerId, NodeId<T>, OptionQuery>;

    /// Total registered nodes.
    /// Note: This is mainly used for performance reasons. It is better to have a single value
    /// storage than iterate over a huge map.
    #[pallet::storage]
    pub type TotalRegisteredNodes<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Registry of nodes with their owners.
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

    /// Count of nodes owned by an account.
    #[pallet::storage]
    pub type OwnedNodesCount<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

    /// The admin account that can register new nodes
    #[pallet::storage]
    pub type NodeRegistrar<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    /// The maximum batch size to pay rewards
    #[pallet::storage]
    pub type MaxBatchSize<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// The heartbeat period in blocks
    #[pallet::storage]
    pub type HeartbeatPeriod<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// The total amount to pay out for each period
    #[pallet::storage]
    pub type RewardAmount<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// Map of reward pot amounts for each reward period.
    #[pallet::storage]
    pub(super) type RewardPot<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        RewardPeriodIndex,
        RewardPotInfo<BalanceOf<T>>,
        OptionQuery,
    >;

    /// Tracks the current reward period.
    #[pallet::storage]
    #[pallet::getter(fn current_reward_period)]
    pub(super) type RewardPeriod<T: Config> =
        StorageValue<_, RewardPeriodInfo<BlockNumberFor<T>>, ValueQuery>;

    /// The earliest reward period that has not been fully paid.
    #[pallet::storage]
    #[pallet::getter(fn oldest_unpaid_period)]
    pub(super) type OldestUnpaidRewardPeriodIndex<T: Config> =
        StorageValue<_, RewardPeriodIndex, ValueQuery>;

    /// Stores a `PaymentPointer` for the last node we successfully paid in a given period.
    #[pallet::storage]
    #[pallet::getter(fn last_paid_pointer)]
    pub(super) type LastPaidPointer<T: Config> =
        StorageValue<_, PaymentPointer<T::AccountId>, OptionQuery>;

    /// DoubleMap storing each node's uptime for a given reward period.
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

    /// The total uptime for each reward period.
    #[pallet::storage]
    pub(super) type TotalUptime<T: Config> =
        StorageMap<_, Blake2_128Concat, RewardPeriodIndex, TotalUptimeInfo, ValueQuery>;

    /// Controls if rewards are enabled
    #[pallet::storage]
    pub(super) type RewardEnabled<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// The heartbeat period in blocks
    #[pallet::storage]
    pub type MinUptimeThreshold<T: Config> = StorageValue<_, Perbill, OptionQuery>;

    /// The auto staking duration in seconds
    #[pallet::storage]
    pub type AutoStakeDurationSec<T: Config> = StorageValue<_, Duration, ValueQuery>;

    /// The unstake period duration in seconds
    #[pallet::storage]
    pub type UnstakePeriodSec<T: Config> = StorageValue<_, Duration, ValueQuery>;

    /// The maximum percentage of staked balance that can be unstaked at once
    #[pallet::storage]
    pub type MaxUnstakePercentage<T: Config> = StorageValue<_, Perbill, ValueQuery>;

    /// The next node serial number to be assigned
    #[pallet::storage]
    pub type NextNodeSerialNumber<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// The duration in seconds for which unstaking is restricted
    #[pallet::storage]
    pub type RestrictedUnstakeDurationSec<T: Config> = StorageValue<_, Duration, ValueQuery>;

    /// The fee charged by the chain to host app chain nodes
    #[pallet::storage]
    pub type AppChainFeePercentage<T: Config> = StorageValue<_, Perbill, ValueQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub _phantom: sp_std::marker::PhantomData<T>,
        pub max_batch_size: u32,
        pub reward_period: u32,
        pub heartbeat_period: u32,
        pub reward_amount: BalanceOf<T>,
        pub auto_stake_duration_sec: Duration,
        pub max_unstake_percentage: Perbill,
        pub unstake_period_sec: Duration,
        pub restricted_unstake_duration_sec: Duration,
        pub app_chain_fee_percentage: Perbill,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                _phantom: Default::default(),
                max_batch_size: 1,
                reward_period: 2,
                heartbeat_period: 1,
                reward_amount: Default::default(),
                auto_stake_duration_sec: 180 * 24 * 60 * 60, // 180 days
                max_unstake_percentage: Perbill::from_percent(10),
                unstake_period_sec: 7 * 24 * 60 * 60, // 1 week
                restricted_unstake_duration_sec: 10 * 7 * 24 * 60 * 60, // 10 weeks
                app_chain_fee_percentage: Perbill::from_percent(5),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            assert!(self.reward_period > self.heartbeat_period);
            assert!(self.unstake_period_sec > 0);
            let default_threshold = Pallet::<T>::get_default_threshold();

            RewardAmount::<T>::set(self.reward_amount);
            MaxBatchSize::<T>::set(self.max_batch_size);
            HeartbeatPeriod::<T>::set(self.heartbeat_period);
            MinUptimeThreshold::<T>::set(Some(default_threshold));
            AutoStakeDurationSec::<T>::set(self.auto_stake_duration_sec);
            MaxUnstakePercentage::<T>::set(self.max_unstake_percentage);
            UnstakePeriodSec::<T>::set(self.unstake_period_sec);
            RestrictedUnstakeDurationSec::<T>::set(self.restricted_unstake_duration_sec);
            AppChainFeePercentage::<T>::set(self.app_chain_fee_percentage);

            let max_heartbeats = self.reward_period.saturating_div(self.heartbeat_period);
            let uptime_threshold = default_threshold * max_heartbeats;
            let reward_period: RewardPeriodInfo<BlockNumberFor<T>> =
                RewardPeriodInfo::new(0u64, 0u32.into(), self.reward_period, uptime_threshold);
            <RewardPeriod<T>>::put(reward_period);
        }
    }

    // Pallet Events
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new node has been registered
        NodeRegistered { owner: T::AccountId, node: NodeId<T> },
        /// A new reward period (in blocks) was set.
        RewardPeriodLengthSet {
            period_index: u64,
            old_reward_period_length: u32,
            new_reward_period_length: u32,
        },
        /// A new reward period was initialized.
        NewRewardPeriodStarted {
            reward_period_index: RewardPeriodIndex,
            reward_period_length: u32,
            uptime_threshold: u32,
            previous_period_reward: BalanceOf<T>,
        },
        /// We finished paying all nodes for a particular period.
        RewardPayoutCompleted { reward_period_index: RewardPeriodIndex },
        /// Node received a reward.
        RewardPaid {
            reward_period: RewardPeriodIndex,
            owner: T::AccountId,
            node: NodeId<T>,
            amount: BalanceOf<T>,
        },
        /// An error occurred while paying a reward.
        ErrorPayingReward {
            reward_period: RewardPeriodIndex,
            node: NodeId<T>,
            error: DispatchError,
        },
        /// Node reward was auto staked.
        RewardAutoStaked {
            reward_period: RewardPeriodIndex,
            owner: T::AccountId,
            node: NodeId<T>,
            amount: BalanceOf<T>,
        },
        /// A new node registrar has been set
        NodeRegistrarSet { new_registrar: T::AccountId },
        /// A new reward payment batch size has been set
        BatchSizeSet { new_size: u32 },
        /// A new heartbeat period (in blocks) was set.
        HeartbeatPeriodSet { new_heartbeat_period: u32 },
        /// A new heartbeat has been received
        HeartbeatReceived { reward_period_index: RewardPeriodIndex, node: NodeId<T> },
        /// A new reward amount is set
        RewardAmountSet { new_amount: BalanceOf<T> },
        /// Reward payment has been toggled
        RewardToggled { enabled: bool },
        /// A new minimum uptime threshold has been set
        MinUptimeThresholdSet { threshold: Perbill },
        /// A new maximum unstake percentage has been set
        MaxUnstakePercentageSet { percentage: Perbill },
        /// A new unstake period (in seconds) has been set
        UnstakePeriodSet { duration_sec: Duration },
        /// A node has been deregistered
        NodeDeregistered { owner: T::AccountId, node: NodeId<T> },
        /// A node signing key has been updated
        SigningKeyUpdated { owner: T::AccountId, node: NodeId<T> },
        /// Auto stake duration has been set
        AutoStakeDurationSet { duration_sec: Duration },
        /// Node owner added stake to the specified node
        StakeAdded {
            owner: T::AccountId,
            node_id: NodeId<T>,
            reward_period: RewardPeriodIndex,
            amount: BalanceOf<T>,
            new_total: BalanceOf<T>,
        },
        /// Node owner removed stake from the specified node
        StakeRemoved {
            owner: T::AccountId,
            node_id: NodeId<T>,
            reward_period: RewardPeriodIndex,
            amount: BalanceOf<T>,
            new_total: BalanceOf<T>,
        },
        /// The duration for restricted unstaking has been set
        RestrictedUnstakeDurationSet { duration_sec: Duration },
        /// The fee percentage for hosting app chain nodes has been set
        AppChainFeePercentageSet { percentage: Perbill },
    }

    // Pallet Errors
    #[pallet::error]
    pub enum Error<T> {
        /// The node registrar account is invalid
        OriginNotRegistrar,
        /// The node address of the last paid node is not recognised
        InvalidNodePointer,
        /// The period index of the last paid node is invalid
        InvalidPeriodPointer,
        /// The node registrar account is not set
        RegistrarNotSet,
        /// Node has already been registered
        DuplicateNode,
        /// The signing key of the node is invalid
        InvalidSigningKey,
        /// The signing key is already in use by another node
        SigningKeyAlreadyInUse,
        /// The reward period is invalid
        RewardPeriodInvalid,
        /// The batch size is 0 or invalid
        BatchSizeInvalid,
        /// The heartbeat period is invalid
        HeartbeatPeriodInvalid,
        /// The heartbeat period is 0
        HeartbeatPeriodZero,
        /// The reward pot does not have enough funds to pay rewards
        InsufficientBalanceForReward,
        /// The total uptime for the period was not found
        TotalUptimeNotFound,
        /// The node uptime for the period was not found
        NodeUptimeNotFound,
        /// The reward payment request is invalid
        InvalidRewardPaymentRequest,
        /// Heartbeat has already been submitted
        DuplicateHeartbeat,
        /// Heartbeat submission is not valid
        InvalidHeartbeat,
        /// The node is not registered
        NodeNotRegistered,
        /// Failed to aquire a lock on the Offchain db
        FailedToAcquireOcwDbLock,
        /// The reward amount is 0
        RewardAmountZero,
        /// The sender is not the signer
        SenderIsNotSigner,
        /// Proxy signature failed to verification
        UnauthorizedSignedTransaction,
        /// The signed transaction has expired
        SignedTransactionExpired,
        /// The minimum uptime threshold is reached
        HeartbeatThresholdReached,
        /// The minimum uptime threshold is 0
        UptimeThresholdZero,
        /// The specified node is not owned by the owner
        NodeNotOwnedByOwner,
        /// The sender is not authorised to update the signing key
        UnauthorizedSigningKeyUpdate,
        /// The new signing key must be different from the current one
        SigningKeyMustBeDifferent,
        /// Amount must be greater than zero
        ZeroAmount,
        /// The account does not have enough free balance to stake
        InsufficientFreeBalance,
        /// The account does not have enough staked balance to withdraw
        InsufficientStakedBalance,
        /// Failed to reserve balance for staking
        ReserveFailed,
        /// Duration must be greater than zero
        DurationZero,
        /// There is no stake for the owner
        NoStakeFound,
        /// Auto stake is still active, cannot unstake now
        AutoStakeStillActive,
        /// There is no available stake to unstake right now
        NoAvailableStakeToUnstake,
        /// Failed to auto stake reward
        AutoStakeFailed,
        /// Node is not found
        NodeNotFound,
        /// Balance overflow
        BalanceOverflow,
        /// Balance underflow
        BalanceUnderflow,
    }

    #[pallet::config]
    pub trait Config:
        frame_system::Config
        + avn::Config
        + CreateInherent<Call<Self>>
        + CreateTransactionBase<Call<Self>>
    {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// The overarching call type.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + IsSubType<Call<Self>>
            + From<Call<Self>>;
        /// The currency type for this module.
        type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;
        // The identifier type for an offchain transaction signer.
        type SignerId: Member
            + Parameter
            + RuntimeAppPublic
            + Ord
            + MaybeSerializeDeserialize
            + MaxEncodedLen;
        /// A type that can be used to verify signatures
        type Public: IdentifyAccount<AccountId = Self::AccountId>;
        /// Time provider
        type TimeProvider: UnixTime;
        /// The signature type used by accounts/transactions.
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;
        /// The type of token identifier
        /// (a H160 because this is an Ethereum address)
        type Token: Parameter + Default + Copy + From<H160> + Into<H160> + MaxEncodedLen;
        /// Trait to deal with handling the fee charged by the chain to host app chain nodes
        type AppChainFeeHandler: FeePaymentHandler<
            AccountId = Self::AccountId,
            Token = Self::Token,
            TokenBalance = <Self::Currency as Currency<Self::AccountId>>::Balance,
            Error = DispatchError,
        >;
        /// The id of the reward pot.
        #[pallet::constant]
        type RewardPotId: Get<PalletId>;
        /// The lifetime (in blocks) of a signed transaction.
        #[pallet::constant]
        type SignedTxLifetime: Get<u32>;
        /// The amount of AVT required to stake to get a +1 virtual node weight bonus.
        #[pallet::constant]
        type VirtualNodeStake: Get<BalanceOf<Self>>;
        /// The weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
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

            Self::do_register_node(node, owner, signing_key)?;
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
            .max(<T as Config>::WeightInfo::set_admin_config_reward_enabled())
            .max(<T as Config>::WeightInfo::set_admin_config_min_threshold())
            .max(<T as Config>::WeightInfo::set_admin_config_auto_stake_duration())
            .max(<T as Config>::WeightInfo::set_admin_config_max_unstake_percentage())
            .max(<T as Config>::WeightInfo::set_admin_config_unstake_period())
            .max(<T as Config>::WeightInfo::set_admin_config_restricted_unstake_duration())
            .max(<T as Config>::WeightInfo::set_admin_config_appchain_fee_percentage())
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
                AdminConfig::RewardPeriod(period) => {
                    let heartbeat = <HeartbeatPeriod<T>>::get();
                    ensure!(period > heartbeat, Error::<T>::RewardPeriodInvalid);
                    let mut reward_period = RewardPeriod::<T>::get();
                    let (index, old_period) = (reward_period.current, reward_period.length);
                    reward_period.length = period;
                    <RewardPeriod<T>>::mutate(|p| *p = reward_period);
                    Self::deposit_event(Event::RewardPeriodLengthSet {
                        period_index: index,
                        old_reward_period_length: old_period,
                        new_reward_period_length: period,
                    });
                    return Ok(
                        Some(<T as Config>::WeightInfo::set_admin_config_reward_period()).into()
                    )
                },
                AdminConfig::BatchSize(size) => {
                    ensure!(size > 0 && size <= MAX_BATCH_SIZE, Error::<T>::BatchSizeInvalid);
                    <MaxBatchSize<T>>::mutate(|s| *s = size.clone());
                    Self::deposit_event(Event::BatchSizeSet { new_size: size });
                    return Ok(Some(<T as Config>::WeightInfo::set_admin_config_reward_batch_size())
                        .into())
                },
                AdminConfig::Heartbeat(period) => {
                    let reward_period = RewardPeriod::<T>::get().length;
                    ensure!(period > 0, Error::<T>::HeartbeatPeriodZero);
                    ensure!(period < reward_period, Error::<T>::HeartbeatPeriodInvalid);
                    <HeartbeatPeriod<T>>::mutate(|p| *p = period.clone());
                    Self::deposit_event(Event::HeartbeatPeriodSet { new_heartbeat_period: period });
                    return Ok(
                        Some(<T as Config>::WeightInfo::set_admin_config_reward_heartbeat()).into()
                    )
                },
                AdminConfig::RewardAmount(amount) => {
                    ensure!(amount > BalanceOf::<T>::zero(), Error::<T>::RewardAmountZero);
                    <RewardAmount<T>>::mutate(|a| *a = amount.clone());
                    Self::deposit_event(Event::RewardAmountSet { new_amount: amount });
                    return Ok(
                        Some(<T as Config>::WeightInfo::set_admin_config_reward_amount()).into()
                    )
                },
                AdminConfig::RewardToggle(enabled) => {
                    <RewardEnabled<T>>::mutate(|e| *e = enabled.clone());
                    Self::deposit_event(Event::RewardToggled { enabled });
                    return Ok(
                        Some(<T as Config>::WeightInfo::set_admin_config_reward_enabled()).into()
                    )
                },
                AdminConfig::MinUptimeThreshold(threshold) => {
                    ensure!(threshold > Perbill::zero(), Error::<T>::UptimeThresholdZero);
                    <MinUptimeThreshold<T>>::mutate(|t| *t = Some(threshold.clone()));
                    Self::deposit_event(Event::MinUptimeThresholdSet { threshold });
                    return Ok(
                        Some(<T as Config>::WeightInfo::set_admin_config_min_threshold()).into()
                    )
                },
                AdminConfig::AutoStakeDuration(duration_sec) => {
                    <AutoStakeDurationSec<T>>::mutate(|d| *d = duration_sec.clone());
                    Self::deposit_event(Event::AutoStakeDurationSet { duration_sec });
                    return Ok(Some(
                        <T as Config>::WeightInfo::set_admin_config_auto_stake_duration(),
                    )
                    .into())
                },
                AdminConfig::MaxUnstakePercentage(percentage) => {
                    <MaxUnstakePercentage<T>>::mutate(|p| *p = percentage.clone());
                    Self::deposit_event(Event::MaxUnstakePercentageSet { percentage });
                    return Ok(Some(
                        <T as Config>::WeightInfo::set_admin_config_max_unstake_percentage(),
                    )
                    .into())
                },
                AdminConfig::UnstakePeriod(duration_sec) => {
                    ensure!(duration_sec > 0, Error::<T>::DurationZero);
                    <UnstakePeriodSec<T>>::mutate(|d| *d = duration_sec.clone());
                    Self::deposit_event(Event::UnstakePeriodSet { duration_sec });
                    return Ok(
                        Some(<T as Config>::WeightInfo::set_admin_config_unstake_period()).into()
                    )
                },
                AdminConfig::RestrictedUnstakeDuration(duration_sec) => {
                    <RestrictedUnstakeDurationSec<T>>::mutate(|d| *d = duration_sec.clone());
                    Self::deposit_event(Event::RestrictedUnstakeDurationSet { duration_sec });
                    return Ok(Some(
                        <T as Config>::WeightInfo::set_admin_config_restricted_unstake_duration(),
                    )
                    .into())
                },
                AdminConfig::AppChainFee(percentage) => {
                    <AppChainFeePercentage<T>>::mutate(|p| *p = percentage.clone());
                    Self::deposit_event(Event::AppChainFeePercentageSet { percentage });
                    return Ok(Some(
                        <T as Config>::WeightInfo::set_admin_config_appchain_fee_percentage(),
                    )
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
            let RewardPeriodInfo { current, length, .. } = RewardPeriod::<T>::get();

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

            let reward_pot = RewardPot::<T>::get(&oldest_period).unwrap_or_else(|| {
                RewardPotInfo::new(
                    RewardAmount::<T>::get(),
                    Self::calculate_uptime_threshold(length),
                    Self::time_now_sec(),
                )
            });

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
                let reward_amount =
                    Self::calculate_reward(node_weight, &total_uptime.total_weight, &total_reward)?;

                Self::pay_reward(&oldest_period, node.clone(), &node_info, reward_amount)?;
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
                total._total_heartbeats = total._total_heartbeats.saturating_add(1);
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

            // Perform the actual registration
            Self::do_register_node(node, owner, signing_key)?;

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
            let now_sec = Self::time_now_sec();
            // before we read the node, try to snapshot the stake if auto-stake duration has passed.
            Self::set_max_unstake_per_period_if_required(
                &node_id,
                now_sec,
                <MaxUnstakePercentage<T>>::get(),
            );

            let node_info =
                NodeRegistry::<T>::get(&node_id).ok_or(Error::<T>::NodeNotRegistered)?;

            ensure!(node_info.can_unstake(now_sec), Error::<T>::AutoStakeStillActive);

            let mut stake = node_info.stake;
            let current_stake_amount = stake.amount;

            let (available, next_unstake) = stake
                .available_to_unstake(
                    now_sec,
                    node_info.auto_stake_expiry,
                    <UnstakePeriodSec<T>>::get(),
                )
                .map_err(|e| match e {
                    DispatchError::Arithmetic(_) => Error::<T>::BalanceOverflow.into(),
                    other => other,
                })?;

            if let Some(amount) = maybe_amount {
                ensure!(!amount.is_zero(), Error::<T>::ZeroAmount);
                ensure!(current_stake_amount >= amount, Error::<T>::InsufficientStakedBalance);
                ensure!(amount <= available, Error::<T>::NoAvailableStakeToUnstake);
            }

            // If amount is not specified, unstake what is available now
            let amount = maybe_amount.unwrap_or(available);
            ensure!(amount > Zero::zero(), Error::<T>::NoAvailableStakeToUnstake);

            // Reduce stake
            let new_total = current_stake_amount
                .checked_sub(&amount)
                .ok_or(Error::<T>::InsufficientStakedBalance)?;

            stake.amount = new_total;
            stake.next_unstake_time_sec = next_unstake;

            stake.unlocked_stake =
                available.checked_sub(&amount).ok_or(Error::<T>::BalanceUnderflow)?;

            NodeRegistry::<T>::insert(&node_id, NodeInfo { stake, ..node_info });

            Self::update_reserves(&owner, amount, StakeOperation::Remove)?;

            Self::deposit_event(Event::StakeRemoved {
                owner,
                node_id,
                reward_period,
                amount,
                new_total,
            });

            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        // Keep this logic light and bounded
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            let rewards_enabled = <RewardEnabled<T>>::get();
            if !rewards_enabled {
                return <T as Config>::WeightInfo::on_initialise_no_reward_period()
            }

            let reward_period = RewardPeriod::<T>::get();
            let previous_index = reward_period.current;
            let previous_uptime_threshold = reward_period.uptime_threshold;

            if reward_period.should_update(n) {
                let uptime_threshold = Self::calculate_uptime_threshold(reward_period.length);

                let reward_period = reward_period.update(n, uptime_threshold);
                RewardPeriod::<T>::mutate(|p| *p = reward_period);

                // take a snapshot of the reward pot amount to pay for the previous reward period
                let reward_amount = RewardAmount::<T>::get();
                <RewardPot<T>>::insert(
                    previous_index,
                    RewardPotInfo::<BalanceOf<T>>::new(
                        reward_amount,
                        previous_uptime_threshold,
                        Self::time_now_sec(),
                    ),
                );

                Self::deposit_event(Event::NewRewardPeriodStarted {
                    reward_period_index: reward_period.current,
                    reward_period_length: reward_period.length,
                    uptime_threshold: reward_period.uptime_threshold,
                    previous_period_reward: reward_amount,
                });

                return <T as Config>::WeightInfo::on_initialise_with_new_reward_period()
            }

            return <T as Config>::WeightInfo::on_initialise_no_reward_period()
        }

        fn offchain_worker(n: BlockNumberFor<T>) {
            log::info!("🌐 OCW for node manager");

            if <RewardEnabled<T>>::get() == false {
                log::warn!("🌐 OCW - rewards are disabled, skipping");
                return
            }

            let maybe_author = Self::try_get_node_author(n);
            if let Some(author) = maybe_author {
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
                            .and_provides((call, reward_period_index))
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
                                .and_provides(call)
                                .priority(TransactionPriority::max_value() - reduce_priority)
                                .longevity(64_u64)
                                .build()
                        },
                        _ => InvalidTransaction::Custom(ERROR_CODE_INVALID_NODE).into(),
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
                    BlockNumberFor::<T>::from(HeartbeatPeriod::<T>::get());
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

        pub(crate) fn calculate_uptime_threshold(reward_period_length: u32) -> u32 {
            let heartbeat_period = HeartbeatPeriod::<T>::get();
            let threshold = MinUptimeThreshold::<T>::get().unwrap_or(Self::get_default_threshold());

            let max_heartbeats = reward_period_length.saturating_div(heartbeat_period);
            threshold * max_heartbeats
        }

        fn do_register_node(
            node: NodeId<T>,
            owner: T::AccountId,
            signing_key: T::SignerId,
        ) -> DispatchResult {
            ensure!(!<NodeRegistry<T>>::contains_key(&node), Error::<T>::DuplicateNode);
            ensure!(
                !SigningKeyToNodeId::<T>::contains_key(&signing_key),
                Error::<T>::SigningKeyAlreadyInUse
            );

            let auto_stake_expiry = Self::calculate_auto_stake_expiry();
            let staking_restriction_expiry =
                auto_stake_expiry.saturating_add(<RestrictedUnstakeDurationSec<T>>::get());

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
                    StakeInfo::<BalanceOf<T>>::new(
                        Zero::zero(),
                        Zero::zero(),
                        Some(auto_stake_expiry),
                        None,
                        Some(staking_restriction_expiry),
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

            return false
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
