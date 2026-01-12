// This file is part of Aventus.
// Copyright (C) 2022 Aventus Network Services (UK) Ltd.

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

//! # Token manager pallet

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::{format, string::String};
use codec::{Decode, Encode};
use core::convert::{TryFrom, TryInto};
use frame_support::{
    dispatch::{DispatchResult, DispatchResultWithPostInfo, GetDispatchInfo},
    ensure,
    pallet_prelude::*,
    traits::{
        schedule::{
            v3::{Anon as ScheduleAnon, Named as ScheduleNamed, TaskName},
            DispatchTime, HARD_DEADLINE,
        },
        Currency, ExistenceRequirement, Get, Imbalance, IsSubType, QueryPreimage,
        ReservableCurrency, StorePreimage, WithdrawReasons,
    },
    BoundedVec, PalletId, Parameter,
};
use frame_system::{ensure_signed, pallet_prelude::BlockNumberFor};
pub use pallet::*;
use pallet_avn::{
    self as avn, BridgeInterface, BridgeInterfaceNotification, CollatorPayoutDustHandler,
    LowerParams, OnGrowthLiftedHandler, ProcessedEventsChecker, PACKED_LOWER_PARAM_SIZE,
};
use sp_avn_common::{
    event_types::{
        AvtGrowthLiftedData, AvtLowerClaimedData, EthEvent, EventData, LiftedData,
        ProcessedEventHandler, TokenInterface,
    },
    verify_signature, CallDecoder, FeePaymentHandler, InnerCallValidator, Proof,
};
use sp_core::{ConstU32, MaxEncodedLen, H160, H256};
use sp_runtime::{
    scale_info::TypeInfo,
    traits::{AtLeast32Bit, CheckedAdd, Dispatchable, Hash, IdentifyAccount, Member, Verify, Zero},
    Perbill,
};
use sp_std::prelude::*;

pub type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
type PositiveImbalanceOf<T> = <<T as Config>::Currency as Currency<
    <T as frame_system::Config>::AccountId,
>>::PositiveImbalance;

type CallOf<T> = <T as Config>::RuntimeCall;
pub type LowerId = u32;
pub type LowerDataLimit = ConstU32<10000>; // Max lower proof len. 10kB

mod benchmarking;
mod burn;
pub mod default_weights;
pub mod migration;
mod treasury;
pub use default_weights::WeightInfo;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod test_avt_tokens;
#[cfg(test)]
mod test_burn_tokens;
#[cfg(test)]
mod test_common_cases;
#[cfg(test)]
mod test_deferred_lower;
#[cfg(test)]
mod test_growth;
#[cfg(test)]
mod test_lower_proof_generation;
#[cfg(test)]
mod test_non_avt_tokens;
#[cfg(test)]
mod test_proxying_signed_lower;
#[cfg(test)]
mod test_proxying_signed_transfer;

pub const DEFAULT_TREASURY_BURN_THRESHOLD_PERCENT: u32 = 15;
pub const SIGNED_TRANSFER_CONTEXT: &'static [u8] = b"authorization for transfer operation";
pub const SIGNED_LOWER_CONTEXT: &'static [u8] = b"authorization for lower operation";
pub const PALLET_ID: &'static [u8; 13] = b"token_manager";

#[frame_support::pallet]
pub mod pallet {

    use super::*;
    use frame_support::Blake2_128Concat;
    use frame_system::{ensure_root, pallet_prelude::*};

    // Public interface of this pallet
    #[pallet::config]
    pub trait Config: frame_system::Config + avn::Config {
        /// The overarching event type.
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
        /// Currency type for lifting
        type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;
        /// The units in which we record balances of tokens others than AVT
        type TokenBalance: Member + Parameter + AtLeast32Bit + Default + Copy + MaxEncodedLen;
        /// The type of token identifier
        /// (a H160 because this is an Ethereum address)
        type TokenId: Parameter + Default + Copy + From<H160> + Into<H160> + MaxEncodedLen;
        type ProcessedEventsChecker: ProcessedEventsChecker;
        /// A type that can be used to verify signatures
        type Public: IdentifyAccount<AccountId = Self::AccountId>;
        /// The signature type used by accounts/transactions.
        #[cfg(not(feature = "runtime-benchmarks"))]
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;

        #[cfg(feature = "runtime-benchmarks")]
        type Signature: Verify<Signer = Self::Public>
            + Member
            + Decode
            + Encode
            + TypeInfo
            + From<sp_core::sr25519::Signature>;
        /// Id of the account that will hold treasury funds
        type AvnTreasuryPotId: Get<PalletId>;
        /// Percentage of growth to store in the treasury
        #[pallet::constant]
        type TreasuryGrowthPercentage: Get<Perbill>;
        /// Handler to notify the runtime when AVT growth is lifted.
        type OnGrowthLiftedHandler: OnGrowthLiftedHandler<BalanceOf<Self>>;
        type Scheduler: ScheduleAnon<BlockNumberFor<Self>, CallOf<Self>, Self::PalletsOrigin>
            + ScheduleNamed<
                BlockNumberFor<Self>,
                CallOf<Self>,
                Self::PalletsOrigin,
                Hasher = Self::Hashing,
            >;

        /// The preimage provider.
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;
        /// Overarching type of all pallets origins.
        type PalletsOrigin: From<frame_system::RawOrigin<Self::AccountId>>;
        type BridgeInterface: BridgeInterface;
        type WeightInfo: WeightInfo;
        /// Minimum Burn Refresh range
        #[pallet::constant]
        type MinBurnPeriod: Get<u32>;
        /// Minimum allowed treasury burn threshold
        #[pallet::constant]
        type TreasuryBurnThreshold: Get<Perbill>;
        /// Flag to enable burn mechanism
        #[pallet::constant]
        type BurnEnabled: Get<bool>;
        /// Max amount allowed to be burned at once
        #[pallet::constant]
        type TreasuryBurnCap: Get<BalanceOf<Self>>;
    }

    #[pallet::pallet]
    #[pallet::storage_version(crate::migration::STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::event]
    /// This attribute generate the function `deposit_event` to deposit one of this pallet event,
    /// it is optional, it is also possible to provide a custom implementation.
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        AVTLifted {
            recipient: T::AccountId,
            amount: BalanceOf<T>,
            eth_tx_hash: H256,
        },
        TokenLifted {
            token_id: T::TokenId,
            recipient: T::AccountId,
            token_balance: T::TokenBalance,
            eth_tx_hash: H256,
        },
        TokenTransferred {
            token_id: T::TokenId,
            sender: T::AccountId,
            recipient: T::AccountId,
            token_balance: T::TokenBalance,
        },
        CallDispatched {
            relayer: T::AccountId,
            call_hash: T::Hash,
        },
        TokenLowered {
            token_id: T::TokenId,
            sender: T::AccountId,
            recipient: T::AccountId,
            amount: u128,
            t1_recipient: H160,
            lower_id: LowerId,
        },
        AvtLowered {
            sender: T::AccountId,
            recipient: T::AccountId,
            amount: u128,
            t1_recipient: H160,
            lower_id: LowerId,
        },
        AvtTransferredFromTreasury {
            recipient: T::AccountId,
            amount: BalanceOf<T>,
        },
        AVTGrowthLifted {
            treasury_share: BalanceOf<T>,
            collators_share: BalanceOf<T>,
            eth_tx_hash: H256,
        },
        LowerRequested {
            token_id: T::TokenId,
            from: T::AccountId,
            amount: u128,
            t1_recipient: H160,
            sender_nonce: Option<u64>,
            lower_id: LowerId,
            schedule_name: TaskName,
        },
        LowerReadyToClaim {
            lower_id: LowerId,
        },
        AvtLowerClaimed {
            lower_id: LowerId,
        },
        FailedToGenerateLowerProof {
            lower_id: LowerId,
        },
        RegeneratingLowerProof {
            lower_id: LowerId,
            requester: T::AccountId,
        },
        RegeneratingFailedLowerProof {
            lower_id: LowerId,
            requester: T::AccountId,
        },
        LowerSchedulePeriodUpdated {
            new_period: BlockNumberFor<T>,
        },
        LoweringEnabled,
        LoweringDisabled,
        /// Event emitted when non native tokens are transferred to this pallet
        TokensDeposited {
            token_id: T::TokenId,
            recipient: T::AccountId,
            token_balance: T::TokenBalance,
        },
        BurnPeriodUpdated {
            burn_period: u32,
        },
        BurnRequested {
            amount: BalanceOf<T>,
        },
        BurnConfirmed {
            tx_id: u32,
            amount: BalanceOf<T>,
        },
        TreasuryExcessSentToBurnPot {
            amount: BalanceOf<T>,
        },
        TreasuryFunded {
            from: T::AccountId,
            amount: BalanceOf<T>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        NoTier1EventForLogLifted,
        NoTier1EventForLogLowerClaimed,
        AmountOverflow,
        DepositFailed,
        LowerFailed,
        AmountIsZero,
        InsufficientSenderBalance,
        TransactionNotSupported,
        SenderNotValid,
        UnauthorizedTransaction,
        UnauthorizedProxyTransaction,
        UnauthorizedSignedTransferTransaction,
        UnauthorizedSignedLowerTransaction,
        ErrorConvertingAccountId,
        ErrorConvertingTokenBalance,
        ErrorConvertingToBalance,
        NoTier1EventForLogAvtGrowthLifted,
        Overflow,
        InvalidLowerCall,
        LowerDataLimitExceeded,
        InvalidLowerId,
        LoweringDisabled,
        InvalidLiftRequest,
        InvalidBurnPeriod,
        ErrorLockingTokens,
        FailedToSubmitBurnRequest,
    }

    #[pallet::storage]
    #[pallet::getter(fn balance)]
    /// The number of units of tokens held by any given account.
    pub type Balances<T: Config> =
        StorageMap<_, Blake2_128Concat, (T::TokenId, T::AccountId), T::TokenBalance, ValueQuery>;

    /// An account nonce that represents the number of transfers from this account
    /// It is shared for all tokens held by the account
    #[pallet::storage]
    #[pallet::getter(fn nonce)]
    pub type Nonces<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, u64, ValueQuery>;

    /// An account without a known private key, that can send transfers (eg Lowering transfers) but
    /// from which no one can send funds. Tokens sent to this account are effectively destroyed.
    #[pallet::storage]
    #[pallet::getter(fn lower_account_id)]
    pub type LowerAccountId<T: Config> = StorageValue<_, H256, ValueQuery>;

    /// The ethereum address of the AVT contract. Default value is the Rinkeby address
    #[pallet::storage]
    #[pallet::getter(fn avt_token_contract)]
    pub type AVTTokenContract<T: Config> = StorageValue<_, H160, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn get_ready_to_claim_lower)]
    pub type LowersReadyToClaim<T: Config> =
        StorageMap<_, Blake2_128Concat, LowerId, LowerProofData, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn get_lower_pending_proof)]
    pub type LowersPendingProof<T: Config> =
        StorageMap<_, Blake2_128Concat, LowerId, LowerParams, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn get_failed_lower_proof)]
    pub type FailedLowerProofs<T: Config> =
        StorageMap<_, Blake2_128Concat, LowerId, LowerParams, OptionQuery>;

    /// A nonce to uniquely identify each lower request
    #[pallet::storage]
    #[pallet::getter(fn lower_id)]
    pub type LowerNonce<T: Config> = StorageValue<_, LowerId, ValueQuery>;

    /// The number of blocks lower transactions are delayed before executing
    #[pallet::storage]
    #[pallet::getter(fn lower_schedule_period)]
    pub type LowerSchedulePeriod<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    /// A flag that controls if lowering is enabled
    #[pallet::storage]
    #[pallet::getter(fn lowers_disabled)]
    pub type LowersDisabled<T: Config> = StorageValue<_, bool, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn next_burn_at)]
    pub type NextBurnAt<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn burn_submission)]
    pub type PendingBurnSubmission<T: Config> =
        StorageMap<_, Blake2_128Concat, u32, BalanceOf<T>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn burn_refresh_range)]
    pub type BurnPeriod<T> = StorageValue<_, u32, ValueQuery, DefaultBurnRefreshRange<T>>;

    #[pallet::storage]
    #[pallet::getter(fn total_supply)]
    pub type TotalSupply<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    #[pallet::type_value]
    pub fn DefaultBurnRefreshRange<T: Config>() -> u32 {
        T::MinBurnPeriod::get()
    }

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub _phantom: sp_std::marker::PhantomData<T>,
        pub lower_account_id: H256,
        pub avt_token_contract: H160,
        pub lower_schedule_period: BlockNumberFor<T>,
        pub balances: Vec<(H160, T::AccountId, u128)>,
        pub treasury_burn_threshold: Perbill,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                _phantom: Default::default(),
                lower_account_id: H256::zero(),
                avt_token_contract: H160::zero(),
                lower_schedule_period: BlockNumberFor::<T>::zero(),
                balances: vec![],
                treasury_burn_threshold: Perbill::from_percent(
                    DEFAULT_TREASURY_BURN_THRESHOLD_PERCENT,
                ),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            crate::migration::STORAGE_VERSION.put::<Pallet<T>>();
            <LowerAccountId<T>>::put(self.lower_account_id);
            <AVTTokenContract<T>>::put(self.avt_token_contract);
            <LowerSchedulePeriod<T>>::put(self.lower_schedule_period);
            for (token_id, recipient, amount) in self.balances.clone().into_iter() {
                let key: (T::TokenId, T::AccountId) = (token_id.into(), recipient);
                let val: T::TokenBalance = <T::TokenBalance as TryFrom<u128>>::try_from(amount)
                    .unwrap_or_else(|_| <T::TokenBalance>::default());
                Balances::<T>::insert(key, val);
            }
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// This extrinsic allows relayer to dispatch a `signed_transfer` or `signed_lower` call for
        /// a sender. As a general rule, every function that can be proxied should follow
        /// this convention:
        /// - its first argument (after origin) should be a public verification key and a signature
        #[pallet::call_index(0)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::proxy_with_non_avt_token().saturating_add(call.get_dispatch_info().weight))]
        pub fn proxy(
            origin: OriginFor<T>,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResult {
            let relayer = ensure_signed(origin)?;

            let proof = Self::get_proof(&*call)?;
            ensure!(relayer == proof.relayer, Error::<T>::UnauthorizedProxyTransaction);

            let call_hash: T::Hash = T::Hashing::hash_of(&call);
            call.dispatch(frame_system::RawOrigin::Signed(proof.signer).into())
                .map(|_| ())
                .map_err(|e| e.error)?;
            Self::deposit_event(Event::<T>::CallDispatched { relayer, call_hash });
            Ok(())
        }

        /// Transfer an amount of token with token_id from sender to receiver with a proof
        #[pallet::call_index(1)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_transfer())]
        pub fn signed_transfer(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            from: T::AccountId,
            to: T::AccountId,
            token_id: T::TokenId,
            amount: T::TokenBalance,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == from, Error::<T>::SenderNotValid);
            let sender_nonce = Self::nonce(&sender);

            let signed_payload = Self::encode_signed_transfer_params(
                &proof,
                &from,
                &to,
                &token_id,
                &amount,
                sender_nonce,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedTransferTransaction
            );

            Self::settle_transfer(&token_id, &from, &to, &amount)?;

            <Nonces<T>>::mutate(from, |n| *n += 1);

            Ok(())
        }

        /// Transfer AVT from the treasury account. The origin must be root.
        #[pallet::call_index(4)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::transfer_from_treasury())]
        pub fn transfer_from_treasury(
            origin: OriginFor<T>,
            recipient: T::AccountId,
            amount: BalanceOf<T>,
        ) -> DispatchResult {
            ensure_root(origin)?;

            Self::transfer_from_treasury_to(&recipient, amount)
        }

        /// Lower an amount of token from tier2 to tier1
        #[pallet::weight(<T as pallet::Config>::WeightInfo::execute_avt_lower().max(<T as pallet::Config>::WeightInfo::execute_non_avt_lower()))]
        #[pallet::call_index(5)]
        pub fn execute_lower(
            origin: OriginFor<T>,
            from: T::AccountId,
            to_account_id: T::AccountId,
            token_id: T::TokenId,
            amount: u128,
            t1_recipient: H160,
            lower_id: LowerId,
        ) -> DispatchResultWithPostInfo {
            let _ = ensure_root(origin)?;
            ensure!(<LowersDisabled<T>>::get() == false, Error::<T>::LoweringDisabled);

            Self::settle_lower(token_id, &from, &to_account_id, amount, t1_recipient, lower_id)?;

            let final_weight = if token_id == Self::avt_token_contract().into() {
                <T as pallet::Config>::WeightInfo::execute_avt_lower()
            } else {
                <T as pallet::Config>::WeightInfo::execute_non_avt_lower()
            };

            Ok(Some(final_weight).into())
        }

        /// Schedule a call to lower an amount of token from tier2 to tier1
        #[pallet::weight(<T as pallet::Config>::WeightInfo::schedule_direct_lower())]
        #[pallet::call_index(6)]
        pub fn schedule_direct_lower(
            origin: OriginFor<T>,
            from: T::AccountId,
            token_id: T::TokenId,
            amount: u128,
            t1_recipient: H160, // the receiver address on tier1
        ) -> DispatchResultWithPostInfo {
            let sender = ensure_signed(origin)?;

            ensure!(sender == from, Error::<T>::SenderNotValid);
            ensure!(<LowersDisabled<T>>::get() == false, Error::<T>::LoweringDisabled);
            ensure!(amount != 0, Error::<T>::AmountIsZero);

            let to_account_id = T::AccountId::decode(&mut Self::lower_account_id().as_bytes())
                .map_err(|_| Error::<T>::ErrorConvertingAccountId)?;

            Self::schedule_lower(&from, to_account_id, token_id, amount, t1_recipient, None)?;

            Ok(().into())
        }

        /// Schedule a call to lower an amount of token from tier2 to tier1 by a relayer
        #[pallet::weight(<T as pallet::Config>::WeightInfo::schedule_signed_lower())]
        #[pallet::call_index(7)]
        pub fn schedule_signed_lower(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            from: T::AccountId,
            token_id: T::TokenId,
            amount: u128,
            t1_recipient: H160, // the receiver address on tier1
        ) -> DispatchResultWithPostInfo {
            let sender = ensure_signed(origin)?;
            ensure!(sender == from, Error::<T>::SenderNotValid);
            ensure!(<LowersDisabled<T>>::get() == false, Error::<T>::LoweringDisabled);
            ensure!(amount != 0, Error::<T>::AmountIsZero);

            let sender_nonce = Self::nonce(&sender);
            let signed_payload = Self::encode_signed_lower_params(
                &proof,
                &from,
                &token_id,
                &amount,
                &t1_recipient,
                sender_nonce,
            );

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload.as_slice())
                    .is_ok(),
                Error::<T>::UnauthorizedSignedLowerTransaction
            );

            let to_account_id = T::AccountId::decode(&mut Self::lower_account_id().as_bytes())
                .map_err(|_| Error::<T>::ErrorConvertingAccountId)?;

            Self::schedule_lower(
                &from,
                to_account_id,
                token_id,
                amount,
                t1_recipient,
                Some(sender_nonce),
            )?;

            <Nonces<T>>::mutate(from, |n| *n += 1);

            Ok(().into())
        }

        #[pallet::call_index(8)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::regenerate_lower_proof())]
        pub fn regenerate_lower_proof(
            origin: OriginFor<T>,
            lower_id: LowerId,
        ) -> DispatchResultWithPostInfo {
            let requester = ensure_signed(origin)?;

            if <LowersReadyToClaim<T>>::contains_key(lower_id) {
                let lower = <LowersReadyToClaim<T>>::take(lower_id).expect("lower exists");
                Self::regenerate_proof(lower_id, lower.params)?;

                Self::deposit_event(Event::<T>::RegeneratingLowerProof { lower_id, requester });

                return Ok(().into())
            } else if <FailedLowerProofs<T>>::contains_key(lower_id) {
                let lower_params = <FailedLowerProofs<T>>::take(lower_id).expect("lower exists");
                Self::regenerate_proof(lower_id, lower_params)?;

                Self::deposit_event(Event::<T>::RegeneratingFailedLowerProof {
                    lower_id,
                    requester,
                });
            } else {
                Err(Error::<T>::InvalidLowerId)?
            }

            Ok(().into())
        }

        #[pallet::call_index(9)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_lower_schedule_period())]
        pub fn set_lower_schedule_period(
            origin: OriginFor<T>,
            new_period: BlockNumberFor<T>,
        ) -> DispatchResult {
            let _ = ensure_root(origin)?;

            <LowerSchedulePeriod<T>>::put(new_period);
            Self::deposit_event(Event::<T>::LowerSchedulePeriodUpdated { new_period });

            return Ok(())
        }

        #[pallet::call_index(10)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::toggle_lowering())]
        pub fn toggle_lowering(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            let _ = ensure_root(origin)?;

            <LowersDisabled<T>>::put(!enabled);

            if enabled {
                Self::deposit_event(Event::<T>::LoweringEnabled);
            } else {
                Self::deposit_event(Event::<T>::LoweringDisabled);
            }

            return Ok(())
        }

        #[pallet::call_index(11)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_burn_period())]
        pub fn set_burn_period(origin: OriginFor<T>, burn_period: u32) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(burn_period >= T::MinBurnPeriod::get(), Error::<T>::InvalidBurnPeriod);

            BurnPeriod::<T>::put(burn_period);

            Self::deposit_event(Event::<T>::BurnPeriodUpdated { burn_period });
            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            if !T::BurnEnabled::get() || !Self::is_burn_due(n) {
                return <T as Config>::WeightInfo::on_initialize_burn_not_due();
            }

            return Self::burn(n);
        }
    }
}

impl<T: Config> Pallet<T> {
    fn settle_transfer(
        token_id: &T::TokenId,
        from: &T::AccountId,
        to: &T::AccountId,
        amount: &T::TokenBalance,
    ) -> DispatchResult {
        if *token_id == Self::avt_token_contract().into() {
            // First convert TokenBalance to u128
            let amount_u128 = TryInto::<u128>::try_into(*amount)
                .map_err(|_| Error::<T>::ErrorConvertingTokenBalance)?;
            // Then convert to Balance
            let transfer_amount = <BalanceOf<T> as TryFrom<u128>>::try_from(amount_u128)
                .or_else(|_error| Err(Error::<T>::ErrorConvertingToBalance))?;

            <T as pallet::Config>::Currency::transfer(
                from,
                to,
                transfer_amount,
                ExistenceRequirement::KeepAlive,
            )?;
        } else {
            let sender_balance = Self::balance((token_id, from));
            ensure!(sender_balance >= *amount, Error::<T>::InsufficientSenderBalance);

            if from != to {
                // If we are transfering to ourselves, we need to be careful when reading the
                // balance because `Self::balance((token_id, from))` ==
                // `Self::balance((token_id, to))` hence the if statement.
                let receiver_balance = Self::balance((token_id, to));
                ensure!(receiver_balance.checked_add(amount).is_some(), Error::<T>::AmountOverflow);
            }

            <Balances<T>>::mutate((token_id, from), |balance| *balance -= *amount);

            <Balances<T>>::mutate((token_id, to), |balance| *balance += *amount);

            Self::deposit_event(Event::<T>::TokenTransferred {
                token_id: token_id.clone(),
                sender: from.clone(),
                recipient: to.clone(),
                token_balance: amount.clone(),
            });
        }

        Ok(())
    }

    fn settle_lower(
        token_id: T::TokenId,
        from: &T::AccountId,
        to: &T::AccountId,
        amount: u128,
        t1_recipient: H160,
        lower_id: LowerId,
    ) -> DispatchResult {
        if token_id == Self::avt_token_contract().into() {
            let lower_amount = <BalanceOf<T> as TryFrom<u128>>::try_from(amount)
                .or_else(|_error| Err(Error::<T>::AmountOverflow))?;
            // Note: Keep account alive when balance is lower than existence requirement,
            // so the SystemNonce will not be reset just in case if any logic relies on the
            // SystemNonce. However all zero AVT account balances will be kept in our
            // runtime storage.
            let imbalance = <T as pallet::Config>::Currency::withdraw(
                &from,
                lower_amount,
                WithdrawReasons::TRANSFER,
                ExistenceRequirement::KeepAlive,
            )?;

            if imbalance.peek() == BalanceOf::<T>::zero() {
                Err(Error::<T>::LowerFailed)?
            }

            // Decreases the total issued AVT when this negative imbalance is dropped
            // so that total issued AVT becomes equal to total supply once again.
            drop(imbalance);

            Self::deposit_event(Event::<T>::AvtLowered {
                sender: from.clone(),
                recipient: to.clone(),
                amount,
                t1_recipient,
                lower_id,
            });
        } else {
            let lower_amount = <T::TokenBalance as TryFrom<u128>>::try_from(amount)
                .or_else(|_error| Err(Error::<T>::AmountOverflow))?;
            let sender_balance = Self::balance((token_id, from));
            ensure!(sender_balance >= lower_amount, Error::<T>::InsufficientSenderBalance);

            <Balances<T>>::mutate((token_id, from), |balance| *balance -= lower_amount);

            Self::deposit_event(Event::<T>::TokenLowered {
                token_id,
                sender: from.clone(),
                recipient: to.clone(),
                amount,
                t1_recipient,
                lower_id,
            });
        }

        let lower_params = Self::concat_lower_data(lower_id, token_id, &amount, &t1_recipient);

        <LowersPendingProof<T>>::insert(lower_id, &lower_params);
        T::BridgeInterface::generate_lower_proof(lower_id, &lower_params, PALLET_ID.to_vec())?;

        Ok(())
    }

    fn update_token_balance(
        transaction_hash: H256,
        token_id: T::TokenId,
        recipient_account_id: T::AccountId,
        raw_amount: u128,
    ) -> DispatchResult {
        let amount =
            Self::do_update_token_balance(token_id, recipient_account_id.clone(), raw_amount)?;
        Self::deposit_event(Event::<T>::TokenLifted {
            token_id,
            recipient: recipient_account_id,
            token_balance: amount,
            eth_tx_hash: transaction_hash,
        });

        Ok(())
    }

    fn do_update_token_balance(
        token_id: T::TokenId,
        recipient_account_id: T::AccountId,
        raw_amount: u128,
    ) -> Result<T::TokenBalance, Error<T>> {
        let amount = <T::TokenBalance as TryFrom<u128>>::try_from(raw_amount)
            .or_else(|_error| Err(Error::<T>::AmountOverflow))?;

        if <Balances<T>>::contains_key((token_id, &recipient_account_id)) {
            Self::increment_token_balance(token_id, &recipient_account_id, &amount)
                .map_err(|_e| Error::<T>::AmountOverflow)?;
        } else {
            <Balances<T>>::insert((token_id, &recipient_account_id), amount);
        }

        Ok(amount)
    }

    fn update_avt_balance(
        recipient_account_id: &T::AccountId,
        raw_amount: u128,
    ) -> Result<BalanceOf<T>, Error<T>> {
        let amount = <BalanceOf<T> as TryFrom<u128>>::try_from(raw_amount)
            .or_else(|_error| Err(Error::<T>::AmountOverflow))?;

        // Drop the imbalance caused by depositing amount into the recipient account without a
        // corresponding deduction.  If the recipient account does not exist,
        // deposit_creating function will create a new one.
        let imbalance: PositiveImbalanceOf<T> =
            <T as pallet::Config>::Currency::deposit_creating(recipient_account_id, amount);

        if imbalance.peek() == BalanceOf::<T>::zero() {
            Err(Error::<T>::DepositFailed)?
        }

        // Increases the total issued AVT when this positive imbalance is dropped
        // so that total issued AVT becomes equal to total supply once again.
        drop(imbalance);

        Ok(amount)
    }

    fn increment_token_balance(
        token_id: T::TokenId,
        recipient_account_id: &T::AccountId,
        amount: &T::TokenBalance,
    ) -> DispatchResult {
        let current_balance = Self::balance((token_id, recipient_account_id));
        let new_balance = current_balance.checked_add(amount).ok_or(Error::<T>::AmountOverflow)?;

        <Balances<T>>::mutate((token_id, recipient_account_id), |balance| *balance = new_balance);

        Ok(())
    }

    fn process_token_deposit(
        token_id: T::TokenId,
        recipient_account_id: T::AccountId,
        raw_amount: u128,
    ) -> DispatchResult {
        let amount =
            Self::do_update_token_balance(token_id, recipient_account_id.clone(), raw_amount)?;

        Self::deposit_event(Event::<T>::TokensDeposited {
            token_id,
            recipient: recipient_account_id,
            token_balance: amount,
        });

        Ok(())
    }

    fn regenerate_proof(lower_id: u32, params: LowerParams) -> DispatchResult {
        <LowersPendingProof<T>>::insert(lower_id, params);
        T::BridgeInterface::generate_lower_proof(lower_id, &params, PALLET_ID.to_vec())?;

        Ok(())
    }

    pub fn concat_lower_data(
        lower_id: LowerId,
        token_id: T::TokenId,
        amount: &u128,
        t1_recipient: &H160,
    ) -> LowerParams {
        let mut lower_params: [u8; PACKED_LOWER_PARAM_SIZE] = [0u8; PACKED_LOWER_PARAM_SIZE];

        // TokenId = 20 bytes
        lower_params[0..20].copy_from_slice(&token_id.into().as_fixed_bytes()[0..20]);
        // TokenBalance = 32 bytes
        lower_params[36..52].copy_from_slice(&amount.to_be_bytes()[0..16]);
        // T1Recipient = 20 bytes
        lower_params[52..72].copy_from_slice(&t1_recipient.as_fixed_bytes()[0..20]);
        // LowerId = 4 bytes
        lower_params[72..PACKED_LOWER_PARAM_SIZE].copy_from_slice(&lower_id.to_be_bytes()[0..4]);

        return lower_params
    }

    fn encode_signed_transfer_params(
        proof: &Proof<T::Signature, T::AccountId>,
        from: &T::AccountId,
        to: &T::AccountId,
        token_id: &T::TokenId,
        amount: &T::TokenBalance,
        sender_nonce: u64,
    ) -> Vec<u8> {
        return (
            SIGNED_TRANSFER_CONTEXT,
            proof.relayer.clone(),
            from,
            to,
            token_id,
            amount,
            sender_nonce,
        )
            .encode()
    }

    fn encode_signed_lower_params(
        proof: &Proof<T::Signature, T::AccountId>,
        from: &T::AccountId,
        token_id: &T::TokenId,
        amount: &u128,
        t1_recipient: &H160,
        sender_nonce: u64,
    ) -> Vec<u8> {
        return (
            SIGNED_LOWER_CONTEXT,
            proof.relayer.clone(),
            from,
            token_id,
            amount,
            t1_recipient,
            sender_nonce,
        )
            .encode()
    }

    fn get_encoded_call_param(
        call: &<T as Config>::RuntimeCall,
    ) -> Option<(&Proof<T::Signature, T::AccountId>, Vec<u8>)> {
        let call = match call.is_sub_type() {
            Some(call) => call,
            None => return None,
        };

        match call {
            Call::signed_transfer { proof, from, to, token_id, amount } => {
                let sender_nonce = Self::nonce(&proof.signer);
                let encoded_data = Self::encode_signed_transfer_params(
                    proof,
                    from,
                    to,
                    token_id,
                    amount,
                    sender_nonce,
                );

                return Some((proof, encoded_data))
            },
            Call::schedule_signed_lower { proof, from, token_id, amount, t1_recipient } => {
                let sender_nonce = Self::nonce(&proof.signer);
                let encoded_data = Self::encode_signed_lower_params(
                    proof,
                    from,
                    token_id,
                    amount,
                    t1_recipient,
                    sender_nonce,
                );

                return Some((proof, encoded_data))
            },
            _ => return None,
        }
    }

    fn process_lift(event: &EthEvent, data: &LiftedData) -> DispatchResult {
        let event_id = &event.event_id;
        let recipient_account_id = T::AccountId::decode(&mut data.receiver_address.as_bytes())
            .expect("32 bytes will always decode into an AccountId");

        let event_validity = T::ProcessedEventsChecker::processed_event_exists(event_id);
        ensure!(event_validity, Error::<T>::NoTier1EventForLogLifted);

        if data.amount == 0 {
            Err(Error::<T>::AmountIsZero)?
        }

        if data.token_contract == Self::avt_token_contract() {
            let updated_amount = Self::update_avt_balance(&recipient_account_id, data.amount)?;

            Self::deposit_event(Event::<T>::AVTLifted {
                recipient: recipient_account_id.clone(),
                amount: updated_amount,
                eth_tx_hash: event_id.transaction_hash,
            });
        } else {
            Self::update_token_balance(
                event_id.transaction_hash,
                data.token_contract.into(),
                recipient_account_id,
                data.amount,
            )?;
        }

        return Ok(())
    }

    fn process_avt_growth_lift(event: &EthEvent, data: &AvtGrowthLiftedData) -> DispatchResult {
        let event_id = &event.event_id;
        let event_validity = T::ProcessedEventsChecker::processed_event_exists(event_id);
        ensure!(event_validity, Error::<T>::NoTier1EventForLogAvtGrowthLifted);

        if data.amount == 0 {
            Err(Error::<T>::AmountIsZero)?
        }

        let treasury_share = T::TreasuryGrowthPercentage::get() * data.amount;

        // Send a portion of the funds to the treasury
        let treasury_amount =
            Self::update_avt_balance(&Self::compute_treasury_account_id(), treasury_share)?;

        // Now let the runtime know we have a lift so we can payout collators
        let remaining_amount =
            <BalanceOf<T> as TryFrom<u128>>::try_from(data.amount - treasury_share)
                .or_else(|_error| Err(Error::<T>::AmountOverflow))?;

        Self::deposit_event(Event::<T>::AVTGrowthLifted {
            treasury_share: treasury_amount,
            collators_share: remaining_amount,
            eth_tx_hash: event_id.transaction_hash,
        });

        T::OnGrowthLiftedHandler::on_growth_lifted(remaining_amount.into(), data.period)?;

        Ok(())
    }

    fn process_lower_claim(event: &EthEvent, data: &AvtLowerClaimedData) -> DispatchResult {
        let event_id = &event.event_id;
        let event_validity = T::ProcessedEventsChecker::processed_event_exists(event_id);
        ensure!(event_validity, Error::<T>::NoTier1EventForLogLowerClaimed);

        ensure!(
            LowersReadyToClaim::<T>::contains_key(data.lower_id) == true,
            Error::<T>::InvalidLowerId
        );
        LowersReadyToClaim::<T>::remove(data.lower_id);
        ensure!(
            LowersReadyToClaim::<T>::contains_key(data.lower_id) == false,
            Error::<T>::InvalidLowerId
        );

        Self::deposit_event(Event::<T>::AvtLowerClaimed { lower_id: data.lower_id });

        Ok(())
    }

    fn schedule_lower(
        from: &T::AccountId,
        to_account_id: T::AccountId,
        token_id: T::TokenId,
        amount: u128,
        t1_recipient: H160,
        sender_nonce: Option<u64>,
    ) -> DispatchResult {
        let lower_id = Self::lower_id();
        let schedule_name = ("Lower", &lower_id).using_encoded(sp_io::hashing::blake2_256);
        let call: CallOf<T> = Call::<T>::execute_lower {
            from: from.clone(),
            to_account_id,
            token_id,
            amount,
            t1_recipient,
            lower_id,
        }
        .into();

        T::Scheduler::schedule_named(
            schedule_name,
            DispatchTime::After(Self::lower_schedule_period()),
            None,
            HARD_DEADLINE,
            frame_system::RawOrigin::Root.into(),
            T::Preimages::bound(CallOf::<T>::from(call))
                .map_err(|_| Error::<T>::InvalidLowerCall)?,
        )?;

        <LowerNonce<T>>::mutate(|nonce| *nonce += 1);

        Self::deposit_event(Event::<T>::LowerRequested {
            token_id,
            from: from.clone(),
            amount,
            t1_recipient,
            sender_nonce,
            lower_id,
            schedule_name,
        });

        Ok(())
    }

    fn processed_event_handler(event: &EthEvent) -> DispatchResult {
        return match &event.event_data {
            EventData::LogLifted(d) => return Self::process_lift(event, d),
            EventData::LogAvtGrowthLifted(d) => return Self::process_avt_growth_lift(event, d),
            EventData::LogLowerClaimed(d) => return Self::process_lower_claim(event, d),

            // Event handled or it is not for us, in which case ignore it.
            _ => Ok(()),
        }
    }

    pub fn get_token_balance(
        account: &T::AccountId,
        token_id: &T::TokenId,
    ) -> Option<T::TokenBalance> {
        if Balances::<T>::contains_key((token_id, account)) {
            return Some(Self::balance((token_id, account)))
        }

        return None
    }
}

impl<T: Config> ProcessedEventHandler for Pallet<T> {
    fn on_event_processed(event: &EthEvent) -> DispatchResult {
        Self::processed_event_handler(event)
    }
}

impl<T: Config> CallDecoder for Pallet<T> {
    type AccountId = T::AccountId;
    type Signature = <T as Config>::Signature;
    type Error = Error<T>;
    type Call = <T as Config>::RuntimeCall;

    fn get_proof(
        call: &Self::Call,
    ) -> Result<Proof<Self::Signature, Self::AccountId>, Self::Error> {
        let call = match call.is_sub_type() {
            Some(call) => call,
            None => return Err(Error::TransactionNotSupported),
        };

        match call {
            Call::signed_transfer { proof, .. } => return Ok(proof.clone()),
            Call::schedule_signed_lower { proof, .. } => return Ok(proof.clone()),
            _ => return Err(Error::TransactionNotSupported),
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

// Deal with any positive imbalance by sending it to the treasury
impl<T: Config> CollatorPayoutDustHandler<BalanceOf<T>> for Pallet<T> {
    fn handle_dust(imbalance: BalanceOf<T>) {
        if let Err(e) =
            T::Currency::deposit_into_existing(&Self::compute_treasury_account_id(), imbalance)
        {
            log::error!("💔💔 Error transferring {:?} AVT to treasury : {:?}", imbalance, e);
        }

        // If the deposit succeeds, when this function goes out of scope, total issuance will
        // increase by "imbalance"
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct LowerProofData {
    pub params: LowerParams,
    pub encoded_lower_data: BoundedVec<u8, LowerDataLimit>,
}

impl<T: Config> BridgeInterfaceNotification for Pallet<T> {
    fn process_result(tx_id: u32, caller_id: Vec<u8>, succeeded: bool) -> DispatchResult {
        if caller_id != PALLET_ID.to_vec() {
            return Ok(()); // Ignore irrelevant transactions
        }

        let amount = match PendingBurnSubmission::<T>::take(tx_id) {
            Some(amount) => amount,
            None => return Ok(()),
        };

        let burn_pot = Self::burn_pot_account();
        T::Currency::unreserve(&burn_pot, amount);

        if succeeded {
            let (imbalance, _) = T::Currency::slash(&burn_pot, amount);
            drop(imbalance);

            Self::deposit_event(Event::<T>::BurnConfirmed { amount, tx_id });
        } else {
            log::error!("Transaction failed on Ethereum. TxId: {:?}", tx_id);
        }

        Ok(())
    }

    fn process_lower_proof_result(
        lower_id: u32,
        caller_id: Vec<u8>,
        data: Result<Vec<u8>, ()>,
    ) -> DispatchResult {
        if LowersPendingProof::<T>::contains_key(&lower_id) && caller_id == PALLET_ID.to_vec() {
            let pending_lower = <LowersPendingProof<T>>::take(lower_id).expect("entry exists");
            if let Ok(lower_data) = data {
                let lower_proof = LowerProofData {
                    params: pending_lower,
                    encoded_lower_data: BoundedVec::<u8, LowerDataLimit>::try_from(lower_data)
                        .map_err(|_| Error::<T>::LowerDataLimitExceeded)?,
                };

                <LowersReadyToClaim<T>>::insert(lower_id, lower_proof);
                <crate::Pallet<T>>::deposit_event(Event::<T>::LowerReadyToClaim { lower_id });
            } else {
                <FailedLowerProofs<T>>::insert(lower_id, pending_lower);
                <crate::Pallet<T>>::deposit_event(Event::<T>::FailedToGenerateLowerProof {
                    lower_id,
                });
            }
        }

        Ok(())
    }

    fn on_incoming_event_processed(event: &EthEvent) -> DispatchResult {
        Self::processed_event_handler(event)
    }
}

impl<T: Config> FeePaymentHandler for Pallet<T> {
    type Token = T::TokenId;
    type TokenBalance = T::TokenBalance;
    type AccountId = T::AccountId;
    type Error = sp_runtime::DispatchError;

    fn pay_fee(
        token_id: &Self::Token,
        amount: &Self::TokenBalance,
        payer: &Self::AccountId,
        recipient: &Self::AccountId,
    ) -> Result<(), Self::Error> {
        Self::settle_transfer(token_id, payer, recipient, amount)
    }
    fn pay_treasury(
        amount: &Self::TokenBalance,
        payer: &Self::AccountId,
    ) -> Result<(), Self::Error> {
        let recipient = Self::compute_treasury_account_id();
        let token: Self::Token = self::AVTTokenContract::<T>::get().into();
        Self::settle_transfer(&token, payer, &recipient, amount)
    }
}

// TODO: The implementation feels too specific to PM, try to generalise it
impl<T: Config> TokenInterface<T::TokenId, T::AccountId> for Pallet<T> {
    fn process_lift(event: &EthEvent) -> DispatchResult {
        return match &event.event_data {
            EventData::LogLiftedToPredictionMarket(d) => {
                let lifted_data = LiftedData::new(d.token_contract, d.receiver_address, d.amount);
                return Self::process_lift(event, &lifted_data)
            },
            EventData::LogErc20Transfer(d) => {
                let lifted_data = LiftedData::new(d.token_contract, d.receiver_address, d.amount);
                return Self::process_lift(event, &lifted_data)
            },

            // Any other event should not be calling this hook, they should use the regular lift
            // pathway
            _ => Err(Error::<T>::InvalidLiftRequest)?,
        }
    }

    fn deposit_tokens(
        token_id: T::TokenId,
        recipient_account_id: T::AccountId,
        raw_amount: u128,
    ) -> DispatchResult {
        Self::process_token_deposit(token_id, recipient_account_id, raw_amount)
    }
}
