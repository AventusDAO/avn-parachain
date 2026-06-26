#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::String;

use frame_support::{
    dispatch::DispatchResult,
    ensure,
    traits::{Currency, OnRuntimeUpgrade, StorageVersion},
};

pub mod default_weights;
pub use default_weights::WeightInfo;

use codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
pub use sp_avn_common::{node::Moment, CallDecoder, RewardPeriodIndex};
use sp_core::{ConstU32, Get, H256};
use sp_runtime::{BoundedVec, Perquintill};
use sp_std::prelude::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

pub mod benchmarking;
pub mod migration;
mod reward;

pub type MaximumHandlersBound = ConstU32<256>;

pub type ChainNameLimit = ConstU32<32>;

pub const UPDATE_CHAIN_HANDLER: &'static [u8] = b"update_chain_handler";
pub const SUBMIT_CHECKPOINT: &'static [u8] = b"submit_checkpoint";

const STORAGE_VERSION: StorageVersion = StorageVersion::new(2);

pub use self::pallet::*;
pub type ChainId = u32;
pub type CheckpointId = u64;
pub type OriginId = u64;
/// Node account ID
pub(crate) type NodeId<T> = <T as frame_system::Config>::AccountId;

pub(crate) type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

/// Decides whether a node may receive a specific app chain's reward for a period.
///
/// Implemented by the runtime and checked per `(app chain, node)` at payout time: returning
/// `false` skips that chain's payout for the node.
pub trait AppChainRewardEligibility<AssetId, AccountId> {
    fn is_eligible(
        asset_id: AssetId,
        node_id: &AccountId,
        period: RewardPeriodIndex,
        auto_stake_expiry: Moment,
    ) -> bool;
}

/// Default implementation: every node is eligible for every app chain.
impl<AssetId, AccountId> AppChainRewardEligibility<AssetId, AccountId> for () {
    fn is_eligible(
        _asset_id: AssetId,
        _node_id: &AccountId,
        _period: RewardPeriodIndex,
        _auto_stake_expiry: Moment,
    ) -> bool {
        true
    }
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{
        dispatch::GetDispatchInfo, pallet_prelude::*, traits::IsSubType, weights::WeightMeter,
    };
    use frame_system::pallet_prelude::*;
    use orml_traits::asset_registry::{
        AssetMetadata as RegistryAssetMetadata, AvnAssetLocation, AvnAssetMetadata,
        Inspect as AssetRegistryInspect, Mutate as AssetRegistryMutate,
    };
    use sp_avn_common::{verify_signature, InnerCallValidator, PaymentHandler, Proof};
    use sp_core::H160;
    use sp_runtime::traits::{Dispatchable, IdentifyAccount, Verify, Zero};

    pub type ChainId = u32;
    pub type CheckpointId = u64;

    #[derive(Encode, Decode, Clone, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct CheckpointData {
        pub hash: H256,
        pub origin_id: OriginId,
    }

    /// A node's accrued, unpaid app-chain reward for a single reward period.
    /// The owner is snapshotted at accrual time because nodes can be transferred afterwards.
    #[derive(Encode, Decode, Clone, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct RewardRecord<AccountId> {
        /// The node owner at the time the reward accrued.
        pub owner: AccountId,
        /// The node's share of the period reward pool (chain-independent).
        pub share: Perquintill,
        /// The auto-stake expiry of the node as a UNIX timestamp in seconds.
        pub auto_stake_expiry: Moment,
    }

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_avn::Config {
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The overarching call type.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = <Self as frame_system::Config>::RuntimeOrigin>
            + IsSubType<Call<Self>>
            + From<Call<Self>>
            + GetDispatchInfo
            + From<frame_system::Call<Self>>;

        type Public: IdentifyAccount<AccountId = Self::AccountId>;

        /// The signature type used by accounts/transactions.
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;

        type WeightInfo: WeightInfo;

        /// Currency type for processing fee payment
        type Currency: Currency<Self::AccountId>;

        /// The type of token identifier
        /// (a H160 because this is an Ethereum address)
        type Token: Parameter + Default + Copy + From<H160> + Into<H160> + MaxEncodedLen;

        /// A handler to process relayer fee payments
        type PaymentHandler: PaymentHandler<
            AccountId = Self::AccountId,
            Token = Self::Token,
            TokenBalance = <Self::Currency as Currency<Self::AccountId>>::Balance,
            Error = DispatchError,
        >;

        /// The default fee for checkpoint submission
        type DefaultCheckpointFee: Get<BalanceOf<Self>>;

        /// Maximum number of app chains that can register a token for reward distribution.
        #[pallet::constant]
        type MaxRegisteredAppChains: Get<u32>;

        /// The asset-id type used by the on-chain asset registry.
        type AppChainAssetId: Parameter + Member + Copy + MaxEncodedLen + Default;

        /// String size limit accepted by the asset registry for name/symbol fields.
        #[pallet::constant]
        type AssetRegistryStringLimit: Get<u32>;

        /// Asset registry for registering app chain tokens on-chain.
        type AssetRegistry: AssetRegistryMutate<
            AvnAssetLocation,
            AssetId = Self::AppChainAssetId,
            Balance = BalanceOf<Self>,
            CustomMetadata = AvnAssetMetadata,
            StringLimit = Self::AssetRegistryStringLimit,
        >;

        /// A pre-funded account that app-chain node rewards are paid from (via `PaymentHandler`).
        type RewardPot: Get<Self::AccountId>;

        /// The maximum number of `(period, node)` payouts processed in a single `claim` or
        /// `process_outstanding_rewards` call (also bounds the on_idle sweep batch).
        #[pallet::constant]
        type MaxPeriodsPerPayout: Get<u32>;

        /// Per-`(app chain, node)` eligibility check applied at payout time. A node only receives
        /// an app chain's reward for a period when this returns `true`. Use `()` to make
        /// every node eligible.
        type AppChainRewardEligibility: AppChainRewardEligibility<
            Self::AppChainAssetId,
            Self::AccountId,
        >;
    }

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_runtime_upgrade() -> frame_support::weights::Weight {
            migration::v2::Migration::<T>::on_runtime_upgrade()
        }

        /// Pays outstanding app-chain rewards with leftover block weight.
        fn on_idle(_n: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(remaining_weight);
            Self::sweep(&mut meter, T::MaxPeriodsPerPayout::get());
            meter.consumed()
        }
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A chain handler was updated. [old_handler_account_id, new_handler_account_id, chain_id]
        ChainHandlerUpdated(T::AccountId, T::AccountId, ChainId),
        /// A new checkpoint was submitted. [handler_account_id, chain_id, checkpoint_id,
        /// checkpoint]
        CheckpointSubmitted(T::AccountId, ChainId, CheckpointId, H256),
        /// The checkpoint fee was updated. [new_fee]
        CheckpointFeeUpdated { chain_id: ChainId, new_fee: BalanceOf<T> },

        /// Fee was charged for checkpoint submission [handler, fee, nonce]
        CheckpointFeeCharged { handler: T::AccountId, chain_id: ChainId, fee: BalanceOf<T> },

        /// A new app chain was fully registered: handler, token, and asset registry entry.
        AppChainRegistered {
            chain_id: ChainId,
            handler: T::AccountId,
            token: T::Token,
            asset_id: T::AppChainAssetId,
        },

        /// A handler updated the per-period reward rate for their app chain.
        AppChainRewardAmountPerPeriodUpdated { asset_id: T::AppChainAssetId, amount: BalanceOf<T> },

        /// An app-chain reward was paid to a node owner.
        AppChainRewardPaid {
            reward_period: RewardPeriodIndex,
            node: T::AccountId,
            owner: T::AccountId,
            asset_id: T::AppChainAssetId,
            amount: BalanceOf<T>,
        },

        /// All app-chain rewards owed to a node for a period were settled and its record cleared.
        AppChainRewardSettledForNode { reward_period: RewardPeriodIndex, node: T::AccountId },

        /// Every node's rewards for a period have been paid and the period snapshot reclaimed.
        AppChainRewardPayoutCompleted { reward_period: RewardPeriodIndex },

        /// A `(period, node)` payout failed (e.g. reward pot underfunded) and was left for retry.
        AppChainRewardPayoutFailed {
            reward_period: RewardPeriodIndex,
            node: T::AccountId,
            error: DispatchError,
        },

        /// An app chain was disabled by its handler (reward rate set to zero).
        AppChainDisabled { asset_id: T::AppChainAssetId },

        /// An app chain was enabled by its handler (reward rate set to non-zero).
        AppChainEnabled { asset_id: T::AppChainAssetId },

        /// An app chain was fully deregistered (routing state removed, asset marked non-native).
        AppChainDeregistered { chain_id: ChainId, asset_id: T::AppChainAssetId },
    }

    #[pallet::error]
    pub enum Error<T> {
        ChainNotRegistered,
        HandlerAlreadyRegistered,
        UnauthorizedHandler,
        NoAvailableChainId,
        EmptyChainName,
        NoAvailableCheckpointId,
        UnauthorizedSignedTransaction,
        SenderNotValid,
        TransactionNotSupported,
        UnauthorizedProxyTransaction,
        // Deprecated, keeping so indexes don't break
        _NoChainDataAvailable,
        CheckpointOriginAlreadyExists,
        /// The app chain already has a token registered.
        AppChainTokenAlreadyRegistered,
        /// The app chain has no token registered yet.
        AppChainTokenNotRegistered,
        /// The maximum number of registered app chains has been reached.
        MaxAppChainsReached,
        /// The chain name is too long to fit in the asset registry string limit.
        ChainNameTooLongForRegistry,
        /// The token symbol is too long to fit in the asset registry string limit.
        TokenSymbolTooLongForRegistry,
        /// A token with the same Ethereum address is already registered in the asset registry.
        TokenLocationAlreadyRegistered,
        /// This extrinsic has been deprecated and is no longer available.
        CallDeprecated,
        /// The token symbol provided for the app chain is empty.
        EmptyTokenSymbol,
        /// The per-period reward amount must be greater than zero.
        ZeroRewardAmount,
        /// The asset id is not a registered app chain.
        AppChainAssetNotRegistered,
        /// The sender is not the registered handler for this app chain.
        NotAppChainHandler,
        /// The app chain asset has no payout token resolvable from the asset registry.
        AppChainTokenNotResolvable,
        /// The asset resolves to a token but is not flagged `appchain_native` in the asset
        /// registry.
        AssetNotAppChainNative,
        /// There are no unpaid rewards to process.
        NoUnpaidRewards,
        /// The app chain must be disabled before it can be deregistered.
        AppChainNotDisabled,
        /// The app chain is already disabled or deregistered.
        AppChainNotActive,
    }

    #[pallet::storage]
    #[pallet::getter(fn checkpoint_fee)]
    pub type CheckpointFee<T: Config> =
        StorageMap<_, Blake2_128Concat, ChainId, BalanceOf<T>, ValueQuery, T::DefaultCheckpointFee>;

    #[pallet::storage]
    #[pallet::getter(fn nonces)]
    pub type Nonces<T: Config> = StorageMap<_, Blake2_128Concat, ChainId, u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn chain_handlers)]
    pub type ChainHandlers<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, ChainId>;

    #[pallet::storage]
    #[pallet::getter(fn next_chain_id)]
    pub type NextChainId<T> = StorageValue<_, ChainId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn checkpoints)]
    pub type Checkpoints<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        ChainId,
        Blake2_128Concat,
        CheckpointId,
        CheckpointData,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn origin_id_to_checkpoint)]
    pub type OriginIdToCheckpoint<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        ChainId,
        Blake2_128Concat,
        OriginId,
        CheckpointId,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn next_checkpoint_id)]
    pub type NextCheckpointId<T> =
        StorageMap<_, Blake2_128Concat, ChainId, CheckpointId, ValueQuery>;

    /// Maps a registered asset Id in the asset registry to its on-chain chain Id.
    #[pallet::storage]
    #[pallet::getter(fn asset_chain_id)]
    pub type AssetIdToChainId<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AppChainAssetId, ChainId, OptionQuery>;

    /// Ordered list of asset IDs for all registered app chains.
    /// Bounded by `MaxRegisteredAppChains` to allow safe iteration.
    #[pallet::storage]
    #[pallet::getter(fn registered_appchains)]
    pub type RegisteredAppchains<T: Config> =
        StorageValue<_, BoundedVec<T::AppChainAssetId, T::MaxRegisteredAppChains>, ValueQuery>;

    /// The total reward amount to be snapshotted at the beginning of the next reward
    /// period. Stored separately so changing it does not affect unpaid reward periods.
    #[pallet::storage]
    #[pallet::getter(fn appchain_period_reward)]
    pub type NextRewardAmountPerPeriod<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AppChainAssetId, (T::Token, BalanceOf<T>), OptionQuery>;

    /// A snapshotted value reflecting the total reward amount distributed to nodes for a period.
    #[pallet::storage]
    pub type PeriodChainReward<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        RewardPeriodIndex,
        Blake2_128Concat,
        T::AppChainAssetId,
        (T::Token, BalanceOf<T>),
        OptionQuery,
    >;

    /// Unpaid appchain rewards per period
    #[pallet::storage]
    pub type UnpaidByPeriod<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        RewardPeriodIndex,
        Blake2_128Concat,
        NodeId<T>,
        RewardRecord<T::AccountId>,
        OptionQuery,
    >;

    /// Index of unpaid periods per node, so an owner can claim a node without supplying periods.
    #[pallet::storage]
    pub type UnpaidByNode<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        NodeId<T>,
        Blake2_128Concat,
        RewardPeriodIndex,
        (),
        OptionQuery,
    >;

    /// The last `(period, node)` examined. The next sweep
    /// resumes strictly after it, so a retained (failed) payout is stepped over by position rather
    /// than blocking the records behind it. `None` restarts the pass from the beginning.
    #[pallet::storage]
    pub type SweepCursor<T: Config> = StorageValue<_, (RewardPeriodIndex, NodeId<T>), OptionQuery>;

    /// Periods for which node-manager has finished paying out (and therefore finished recording app
    /// chain shares via `on_reward_paid`).
    #[pallet::storage]
    pub type PeriodPayoutCompleted<T: Config> =
        StorageMap<_, Blake2_128Concat, RewardPeriodIndex, (), OptionQuery>;

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Deprecated: use `register_appchain` instead.
        /// This call index is preserved to avoid misrouting transactions.
        #[pallet::weight(<T as pallet::Config>::WeightInfo::register_chain_handler())]
        #[pallet::call_index(0)]
        pub fn register_chain_handler(
            origin: OriginFor<T>,
            _name: BoundedVec<u8, ChainNameLimit>,
        ) -> DispatchResult {
            ensure_signed(origin)?;
            Err(Error::<T>::CallDeprecated.into())
        }

        #[pallet::weight(<T as pallet::Config>::WeightInfo::update_chain_handler())]
        #[pallet::call_index(1)]
        pub fn update_chain_handler(
            origin: OriginFor<T>,
            new_handler: T::AccountId,
        ) -> DispatchResult {
            let old_handler = ensure_signed(origin)?;

            ensure!(
                !ChainHandlers::<T>::contains_key(&new_handler),
                Error::<T>::HandlerAlreadyRegistered
            );

            let chain_id =
                ChainHandlers::<T>::get(&old_handler).ok_or(Error::<T>::ChainNotRegistered)?;

            Self::do_update_chain_handler(&old_handler, &new_handler, chain_id)?;

            Ok(())
        }

        #[pallet::weight(<T as pallet::Config>::WeightInfo::submit_checkpoint_with_identity())]
        #[pallet::call_index(2)]
        pub fn submit_checkpoint_with_identity(
            origin: OriginFor<T>,
            checkpoint: H256,
            origin_id: OriginId,
        ) -> DispatchResult {
            let handler = ensure_signed(origin)?;

            let chain_id =
                ChainHandlers::<T>::get(&handler).ok_or(Error::<T>::ChainNotRegistered)?;

            Self::do_submit_checkpoint(&handler, checkpoint, chain_id, origin_id)?;
            Ok(())
        }

        /// Deprecated: use `register_appchain` instead.
        /// This call index is preserved to avoid misrouting transactions.
        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_register_chain_handler())]
        #[pallet::call_index(3)]
        pub fn signed_register_chain_handler(
            origin: OriginFor<T>,
            _proof: Proof<T::Signature, T::AccountId>,
            _handler: T::AccountId,
            _name: BoundedVec<u8, ChainNameLimit>,
        ) -> DispatchResult {
            ensure_signed(origin)?;
            Err(Error::<T>::CallDeprecated.into())
        }

        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_update_chain_handler())]
        #[pallet::call_index(4)]
        pub fn signed_update_chain_handler(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            old_handler: T::AccountId,
            new_handler: T::AccountId,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == old_handler, Error::<T>::SenderNotValid);

            let chain_id =
                ChainHandlers::<T>::get(&old_handler).ok_or(Error::<T>::ChainNotRegistered)?;
            let nonce = Self::nonces(chain_id);

            let signed_payload = encode_signed_update_chain_handler_params::<T>(
                &proof.relayer,
                &old_handler,
                &new_handler,
                chain_id,
                nonce,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            Self::do_update_chain_handler(&old_handler, &new_handler, chain_id)?;

            <Nonces<T>>::mutate(chain_id, |n| *n += 1);

            Ok(())
        }

        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_submit_checkpoint_with_identity())]
        #[pallet::call_index(5)]
        pub fn signed_submit_checkpoint_with_identity(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            handler: T::AccountId,
            checkpoint: H256,
            origin_id: OriginId,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == handler, Error::<T>::SenderNotValid);

            let chain_id =
                ChainHandlers::<T>::get(&handler).ok_or(Error::<T>::ChainNotRegistered)?;
            let nonce = Self::nonces(chain_id);

            let signed_payload = encode_signed_submit_checkpoint_params::<T>(
                &proof.relayer,
                &handler,
                &checkpoint,
                chain_id,
                nonce,
                &origin_id,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            Self::do_submit_checkpoint(&handler, checkpoint, chain_id, origin_id)?;

            Ok(())
        }

        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_checkpoint_fee())]
        #[pallet::call_index(6)]
        pub fn set_checkpoint_fee(
            origin: OriginFor<T>,
            chain_id: ChainId,
            new_fee: BalanceOf<T>,
        ) -> DispatchResult {
            ensure_root(origin)?;

            CheckpointFee::<T>::insert(chain_id, new_fee);
            Self::deposit_event(Event::CheckpointFeeUpdated { chain_id, new_fee });

            Ok(())
        }
        /// Register a new app chain: assigns a chain ID, stores the handler, registers the native
        /// token, and creates an asset-registry entry with `appchain_native: true`.
        ///
        /// The `asset_id` must be a unique `CurrencyId` not yet registered in the asset registry.
        #[pallet::weight(<T as pallet::Config>::WeightInfo::register_appchain())]
        #[pallet::call_index(7)]
        pub fn register_appchain(
            origin: OriginFor<T>,
            handler: T::AccountId,
            name: BoundedVec<u8, ChainNameLimit>,
            symbol: BoundedVec<u8, ChainNameLimit>,
            token: T::Token,
            asset_id: T::AppChainAssetId,
            decimals: u32,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                !ChainHandlers::<T>::contains_key(&handler),
                Error::<T>::HandlerAlreadyRegistered
            );
            ensure!(!name.is_empty(), Error::<T>::EmptyChainName);
            ensure!(!symbol.is_empty(), Error::<T>::EmptyTokenSymbol);
            ensure!(
                T::AssetRegistry::asset_id(&AvnAssetLocation::Ethereum(token.into())).is_none(),
                Error::<T>::TokenLocationAlreadyRegistered
            );

            let name_inner = name.into_inner();
            let name_bytes: BoundedVec<u8, T::AssetRegistryStringLimit> = name_inner
                .clone()
                .try_into()
                .map_err(|_| Error::<T>::ChainNameTooLongForRegistry)?;

            let symbol_inner = symbol.into_inner();
            let symbol_bytes: BoundedVec<u8, T::AssetRegistryStringLimit> =
                symbol_inner.try_into().map_err(|_| Error::<T>::TokenSymbolTooLongForRegistry)?;

            let metadata = RegistryAssetMetadata {
                decimals,
                name: name_bytes,
                symbol: symbol_bytes,
                existential_deposit: BalanceOf::<T>::default(),
                location: Some(AvnAssetLocation::Ethereum(token.into())),
                additional: AvnAssetMetadata { appchain_native: true },
            };

            let chain_id = Self::get_next_chain_id()?;
            ChainHandlers::<T>::insert(handler.clone(), chain_id);

            // This handles duplicate asset_id checks
            T::AssetRegistry::register_asset(Some(asset_id), metadata)?;

            AssetIdToChainId::<T>::insert(asset_id, chain_id);
            RegisteredAppchains::<T>::try_mutate(|ids| ids.try_push(asset_id))
                .map_err(|_| Error::<T>::MaxAppChainsReached)?;

            Self::deposit_event(Event::AppChainRegistered { chain_id, handler, token, asset_id });
            Ok(())
        }

        /// Set the total per-period reward an app chain pays its nodes. Sender must be the
        /// registered handler for `asset_id`. The amount must be greater than zero.
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_appchain_period_reward())]
        #[pallet::call_index(8)]
        pub fn set_appchain_period_reward(
            origin: OriginFor<T>,
            asset_id: T::AppChainAssetId,
            amount: BalanceOf<T>,
        ) -> DispatchResult {
            Self::ensure_reward_manager(origin, asset_id)?;

            ensure!(amount > Zero::zero(), Error::<T>::ZeroRewardAmount);
            let was_active = Self::appchain_is_active(asset_id);

            // Resolve the payout token now so it is captured alongside the rate. This makes the
            // per-period snapshot infallible and guarantees a chain with a rate is always paid out.
            let token = Self::resolve_payout_token(&asset_id)?;

            NextRewardAmountPerPeriod::<T>::insert(asset_id, (token, amount));

            Self::deposit_event(Event::AppChainRewardAmountPerPeriodUpdated { asset_id, amount });

            if !was_active {
                Self::deposit_event(Event::AppChainEnabled { asset_id });
            }

            Ok(())
        }

        /// Claim all outstanding app-chain rewards for a single node, across every unpaid period.
        /// Permissionless: rewards always go to the snapshotted owner regardless of caller.
        #[pallet::weight(<T as pallet::Config>::WeightInfo::claim(T::MaxPeriodsPerPayout::get(), T::MaxRegisteredAppChains::get()))]
        #[pallet::call_index(9)]
        pub fn claim(origin: OriginFor<T>, node: T::AccountId) -> DispatchResult {
            ensure_signed(origin)?;
            let processed = Self::claim_node(&node, T::MaxPeriodsPerPayout::get());
            ensure!(processed > 0, Error::<T>::NoUnpaidRewards);
            Ok(())
        }

        /// Permissionless sweep that pays outstanding app-chain rewards round-robin (resuming from
        /// `SweepCursor`), up to `MaxPeriodsPerPayout` payouts. Guarantees
        /// progress independent of `on_idle`. A failed payout cannot
        /// block the rest.
        #[pallet::weight(<T as pallet::Config>::WeightInfo::process_outstanding_rewards(T::MaxPeriodsPerPayout::get(), T::MaxRegisteredAppChains::get()))]
        #[pallet::call_index(10)]
        pub fn process_outstanding_rewards(origin: OriginFor<T>) -> DispatchResult {
            ensure_signed(origin)?;
            // Count-bounded (not weight-bounded): the declared extrinsic weight covers the batch.
            let mut meter = WeightMeter::with_limit(Weight::MAX);
            let processed = Self::sweep(&mut meter, T::MaxPeriodsPerPayout::get());
            ensure!(processed > 0, Error::<T>::NoUnpaidRewards);
            Ok(())
        }

        /// Disable an app chain and prevent it from paying out rewards. Already accrued rewards are
        /// unaffected.
        #[pallet::weight(<T as pallet::Config>::WeightInfo::disable_appchain())]
        #[pallet::call_index(11)]
        pub fn disable_appchain(
            origin: OriginFor<T>,
            asset_id: T::AppChainAssetId,
        ) -> DispatchResult {
            Self::ensure_reward_manager(origin, asset_id)?;
            ensure!(Self::appchain_is_active(asset_id), Error::<T>::AppChainNotActive);

            NextRewardAmountPerPeriod::<T>::remove(asset_id);

            Self::deposit_event(Event::AppChainDisabled { asset_id });
            Ok(())
        }

        /// Fully deregister an app chain. The chain must already be disabled.
        /// Already-accrued rewards are unaffected but the same `asset_id` / token
        /// address can never be registered again.
        #[pallet::weight(<T as pallet::Config>::WeightInfo::deregister_appchain())]
        #[pallet::call_index(12)]
        pub fn deregister_appchain(
            origin: OriginFor<T>,
            handler: T::AccountId,
            asset_id: T::AppChainAssetId,
        ) -> DispatchResult {
            ensure_root(origin)?;

            let chain_id = AssetIdToChainId::<T>::get(asset_id)
                .ok_or(Error::<T>::AppChainAssetNotRegistered)?;
            ensure!(
                ChainHandlers::<T>::get(&handler) == Some(chain_id),
                Error::<T>::NotAppChainHandler
            );
            ensure!(!Self::appchain_is_active(asset_id), Error::<T>::AppChainNotDisabled);

            // Mark the asset non-native; the registry entry (and its Ethereum location) is
            // retained.
            T::AssetRegistry::update_asset(
                asset_id,
                None,
                None,
                None,
                None,
                None,
                Some(AvnAssetMetadata { appchain_native: false }),
            )?;

            RegisteredAppchains::<T>::mutate(|ids| ids.retain(|id| id != &asset_id));
            AssetIdToChainId::<T>::remove(asset_id);
            ChainHandlers::<T>::remove(&handler);
            // This should have been removed already but remove it just in case.
            NextRewardAmountPerPeriod::<T>::remove(asset_id);

            Self::deposit_event(Event::AppChainDeregistered { chain_id, asset_id });
            Ok(())
        }

        /// Root-only recovery: re-attempt reclaiming a completed period's `PeriodChainReward`
        /// snapshot. This is a no-op unless the period is marked completed and all its rewards are
        /// settled;
        #[pallet::weight(<T as pallet::Config>::WeightInfo::on_reward_period_completed())]
        #[pallet::call_index(13)]
        pub fn reclaim_period(origin: OriginFor<T>, period: RewardPeriodIndex) -> DispatchResult {
            ensure_root(origin)?;
            Self::try_reclaim_period(period);
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        pub(crate) fn charge_fee(handler: T::AccountId, chain_id: ChainId) -> DispatchResult {
            let checkpoint_fee = Self::checkpoint_fee(chain_id);

            T::PaymentHandler::pay_treasury(&checkpoint_fee, &handler)?;

            Self::deposit_event(Event::CheckpointFeeCharged {
                handler: handler.clone(),
                fee: checkpoint_fee,
                chain_id,
            });

            Ok(())
        }

        fn get_next_chain_id() -> Result<ChainId, DispatchError> {
            NextChainId::<T>::try_mutate(|id| {
                let current_id = *id;
                *id = id.checked_add(1).ok_or(Error::<T>::NoAvailableChainId)?;
                Ok(current_id)
            })
        }

        fn get_next_checkpoint_id(chain_id: ChainId) -> Result<CheckpointId, DispatchError> {
            NextCheckpointId::<T>::try_mutate(chain_id, |id| {
                let current_id = *id;
                *id = id.checked_add(1).ok_or(Error::<T>::NoAvailableCheckpointId)?;
                Ok(current_id)
            })
        }

        fn do_update_chain_handler(
            old_handler: &T::AccountId,
            new_handler: &T::AccountId,
            chain_id: ChainId,
        ) -> DispatchResult {
            ensure!(
                !ChainHandlers::<T>::contains_key(new_handler),
                Error::<T>::HandlerAlreadyRegistered
            );

            ensure!(ChainHandlers::<T>::contains_key(&old_handler), Error::<T>::ChainNotRegistered);

            ChainHandlers::<T>::insert(&new_handler, chain_id);
            ChainHandlers::<T>::remove(&old_handler);

            Self::deposit_event(Event::ChainHandlerUpdated(
                old_handler.clone(),
                new_handler.clone(),
                chain_id,
            ));

            Ok(())
        }

        fn do_submit_checkpoint(
            handler: &T::AccountId,
            checkpoint: H256,
            chain_id: ChainId,
            origin_id: OriginId,
        ) -> DispatchResult {
            ensure!(
                !Self::has_checkpoint_origin(chain_id, origin_id),
                Error::<T>::CheckpointOriginAlreadyExists
            );

            let checkpoint_id = Self::get_next_checkpoint_id(chain_id)?;

            let checkpoint_data = CheckpointData { hash: checkpoint, origin_id };

            Checkpoints::<T>::insert(chain_id, checkpoint_id, checkpoint_data.clone());

            OriginIdToCheckpoint::<T>::insert(chain_id, origin_id, checkpoint_id);

            Self::deposit_event(Event::CheckpointSubmitted(
                handler.clone(),
                chain_id,
                checkpoint_id,
                checkpoint,
            ));

            <Nonces<T>>::mutate(chain_id, |n| *n += 1);
            Self::charge_fee(handler.clone(), chain_id)?;
            Ok(())
        }

        pub fn has_checkpoint_origin(chain_id: ChainId, origin_id: OriginId) -> bool {
            OriginIdToCheckpoint::<T>::contains_key(chain_id, origin_id)
        }

        pub fn get_checkpoint_id_by_origin(
            chain_id: ChainId,
            origin_id: OriginId,
        ) -> Option<CheckpointId> {
            OriginIdToCheckpoint::<T>::get(chain_id, origin_id)
        }

        pub fn in_code_storage_version() -> StorageVersion {
            StorageVersion::get::<Pallet<T>>()
        }

        fn get_encoded_call_param(
            call: &<T as Config>::RuntimeCall,
        ) -> Option<(&Proof<T::Signature, T::AccountId>, Vec<u8>)> {
            let call = match call.is_sub_type() {
                Some(call) => call,
                None => return None,
            };

            match call {
                Call::signed_update_chain_handler {
                    ref proof,
                    ref old_handler,
                    ref new_handler,
                } => {
                    let chain_id = ChainHandlers::<T>::get(old_handler)
                        .ok_or(Error::<T>::ChainNotRegistered)
                        .ok()?;

                    let nonce = Self::nonces(chain_id);
                    let encoded_data = encode_signed_update_chain_handler_params::<T>(
                        &proof.relayer,
                        old_handler,
                        new_handler,
                        chain_id,
                        nonce,
                    );

                    Some((proof, encoded_data))
                },
                Call::signed_submit_checkpoint_with_identity {
                    ref proof,
                    ref handler,
                    ref checkpoint,
                    ref origin_id,
                } => {
                    let chain_id = ChainHandlers::<T>::get(handler.clone())
                        .ok_or(Error::<T>::ChainNotRegistered)
                        .ok()?;

                    let nonce = Self::nonces(chain_id);
                    let encoded_data = encode_signed_submit_checkpoint_params::<T>(
                        &proof.relayer,
                        handler,
                        checkpoint,
                        chain_id,
                        nonce,
                        origin_id,
                    );

                    Some((proof, encoded_data))
                },
                _ => None,
            }
        }

        fn appchain_is_active(asset_id: T::AppChainAssetId) -> bool {
            let r = NextRewardAmountPerPeriod::<T>::get(asset_id);
            r.map_or(false, |(_, amt)| amt > Zero::zero())
        }

        /// Ensure `origin` can manage `asset_id`'s reward rate.
        fn ensure_reward_manager(
            origin: OriginFor<T>,
            asset_id: T::AppChainAssetId,
        ) -> DispatchResult {
            let chain_id = AssetIdToChainId::<T>::get(asset_id)
                .ok_or(Error::<T>::AppChainAssetNotRegistered)?;

            if let Some(sender) = ensure_signed_or_root(origin)? {
                ensure!(
                    ChainHandlers::<T>::get(&sender) == Some(chain_id),
                    Error::<T>::NotAppChainHandler
                );
            }
            Ok(())
        }
    }

    impl<T: Config> CallDecoder for Pallet<T> {
        type AccountId = T::AccountId;
        type Signature = T::Signature;
        type Error = Error<T>;
        type Call = <T as Config>::RuntimeCall;

        fn get_proof(
            call: &Self::Call,
        ) -> Result<Proof<Self::Signature, Self::AccountId>, Self::Error> {
            let call = match call.is_sub_type() {
                Some(call) => call,
                None => return Err(Error::<T>::TransactionNotSupported),
            };

            match call {
                Call::signed_update_chain_handler { proof, .. } => Ok(proof.clone()),
                Call::signed_submit_checkpoint_with_identity { proof, .. } => Ok(proof.clone()),
                _ => Err(Error::<T>::TransactionNotSupported),
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

            return false
        }
    }
}

pub fn encode_signed_update_chain_handler_params<T: Config>(
    relayer: &T::AccountId,
    old_handler: &T::AccountId,
    new_handler: &T::AccountId,
    chain_id: ChainId,
    nonce: u64,
) -> Vec<u8> {
    (UPDATE_CHAIN_HANDLER, relayer.clone(), old_handler, new_handler, chain_id, nonce).encode()
}

pub fn encode_signed_submit_checkpoint_params<T: Config>(
    relayer: &T::AccountId,
    handler: &T::AccountId,
    checkpoint: &H256,
    chain_id: ChainId,
    nonce: u64,
    origin_id: &CheckpointId,
) -> Vec<u8> {
    (SUBMIT_CHECKPOINT, relayer.clone(), handler, checkpoint, chain_id, nonce, *origin_id).encode()
}
