//! # Avn transaction payment
// Copyright 2023 Aventus Network Services (UK) Ltd.

//! This is a wrapper pallet for transaction payment that allows the customisation of chain fees
//! based on defined adjustment configuration and a sender.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
    dispatch::{DispatchResult, GetDispatchInfo, PostDispatchInfo},
    traits::{Currency, Imbalance, OnUnbalanced},
    unsigned::TransactionValidityError,
};
use frame_system::{self as system};

use core::convert::TryInto;
use frame_support::{traits::ExistenceRequirement, PalletId};
use frame_system::Pallet as System;
pub use pallet::*;
use pallet_authorship;
use pallet_transaction_payment::{CurrencyAdapter, OnChargeTransaction};
use sp_runtime::{
    traits::{
        AccountIdConversion, DispatchInfoOf, Dispatchable, One, PostDispatchInfoOf, Saturating,
        Zero,
    },
    transaction_validity::InvalidTransaction,
    FixedPointNumber, FixedU128,
};
use sp_std::{marker::PhantomData, prelude::*};

pub mod fee_adjustment_config;
use fee_adjustment_config::{
    AdjustmentType::{TimeBased, TransactionBased},
    *,
};

// If something happens with the fee calculation, use this value
pub const FALLBACK_MIN_FEE: u128 = 11_090_000u128;

pub trait NativeRateProvider {
    /// Return price of 1 native token in USD (8 decimals), or None if unavailable
    fn native_rate_usd() -> Option<u128>;
}

/// Runtime-provided policy for distributing fees from the fee pot.
pub trait FeeDistributor<T: Config> {
    fn distribute_fees(
        fee_pot: &T::AccountId,
        total_fees: BalanceOf<T>,
        used_weight_ref_time: u128,
        max_weight_ref_time: u128,
    );
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{pallet_prelude::*, Blake2_128Concat};
    use frame_system::pallet_prelude::*;

    #[pallet::config]
    pub trait Config:
        frame_system::Config + pallet_transaction_payment::Config + pallet_authorship::Config
    {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The overarching call type
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
            + GetDispatchInfo
            + From<frame_system::Call<Self>>;

        /// Currency type for processing fee payment
        type Currency: Currency<Self::AccountId>;

        /// The origin that is allowed to set the known senders
        type KnownUserOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        type WeightInfo: WeightInfo;

        /// Provider of the native token USD rate (8 decimals)
        type NativeRateProvider: NativeRateProvider;

        /// Fee distribution strategy configured by the runtime (no default, must be provided).
        type FeeDistributor: FeeDistributor<Self>;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// The base gas fee for a simple token transfer in usd
    #[pallet::storage]
    pub type BaseGasFeeUsd<T: Config> = StorageValue<_, u128, ValueQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub base_gas_fee_usd: u128,
        pub _phantom: sp_std::marker::PhantomData<T>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self { base_gas_fee_usd: FALLBACK_MIN_FEE, _phantom: Default::default() }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            BaseGasFeeUsd::<T>::put(self.base_gas_fee_usd);
        }
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new known sender has been added
        KnownSenderAdded {
            known_sender: T::AccountId,
            adjustment: FeeAdjustmentConfig<T>,
        },
        /// Adjustments have been updated for an existing known sender
        KnownSenderUpdated {
            known_sender: T::AccountId,
            adjustment: FeeAdjustmentConfig<T>,
        },
        // An existing known sender has been removed
        KnownSenderRemoved {
            known_sender: T::AccountId,
        },
        /// An adjusted transaction fee of `fee` has been paid by `who`
        AdjustedTransactionFeePaid {
            who: T::AccountId,
            fee: BalanceOf<T>,
        },
        /// A new base gas fee has been set
        BaseGasFeeUsdSet {
            new_base_gas_fee: u128,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        InvalidFeeConfig,
        InvalidFeeType,
        KnownSenderMustMatchAccount,
        KnownSenderMissing,
        BaseGasFeeZero,
    }

    #[pallet::storage]
    #[pallet::getter(fn known_senders)]
    /// A map of known senders
    pub type KnownSenders<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, FeeAdjustmentConfig<T>, ValueQuery>;

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_known_sender())]
        pub fn set_known_sender(
            origin: OriginFor<T>,
            known_sender: T::AccountId,
            config: AdjustmentInput<T>,
        ) -> DispatchResult {
            T::KnownUserOrigin::ensure_origin(origin)?;

            let mut fee_adjustment_config: FeeAdjustmentConfig<T> = Default::default();
            if config.adjustment_type != AdjustmentType::None {
                match config.adjustment_type {
                    TimeBased(b) => {
                        fee_adjustment_config = FeeAdjustmentConfig::TimeBased(
                            TimeBasedConfig::new(config.fee_type, b.duration),
                        );
                    },
                    TransactionBased(i) => {
                        fee_adjustment_config =
                            FeeAdjustmentConfig::TransactionBased(TransactionBasedConfig::new(
                                config.fee_type,
                                known_sender.clone(),
                                i.number_of_transactions,
                            ));
                    },
                    _ => {},
                }
            } else {
                match config.fee_type {
                    FeeType::FixedFee(f) => {
                        fee_adjustment_config = FeeAdjustmentConfig::FixedFee(f);
                    },
                    FeeType::PercentageFee(p) => {
                        fee_adjustment_config = FeeAdjustmentConfig::PercentageFee(p);
                    },
                    _ => {},
                }
            }

            ensure!(fee_adjustment_config.is_valid() == true, Error::<T>::InvalidFeeConfig);

            let sender_exists = <KnownSenders<T>>::contains_key(&known_sender);
            <KnownSenders<T>>::insert(&known_sender, &fee_adjustment_config);

            if !sender_exists {
                Self::deposit_event(Event::<T>::KnownSenderAdded {
                    known_sender,
                    adjustment: fee_adjustment_config,
                });
            } else {
                Self::deposit_event(Event::<T>::KnownSenderUpdated {
                    known_sender,
                    adjustment: fee_adjustment_config,
                });
            }

            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::remove_known_sender())]
        pub fn remove_known_sender(
            origin: OriginFor<T>,
            known_sender: T::AccountId,
        ) -> DispatchResult {
            T::KnownUserOrigin::ensure_origin(origin)?;

            ensure!(
                <KnownSenders<T>>::contains_key(&known_sender) == true,
                Error::<T>::KnownSenderMissing
            );

            <KnownSenders<T>>::remove(&known_sender);
            Self::deposit_event(Event::<T>::KnownSenderRemoved { known_sender });

            Ok(())
        }

        /// Set the base gas fee in usd (8 decimals)
        #[pallet::call_index(2)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_base_gas_fee_usd())]
        pub fn set_base_gas_fee_usd(origin: OriginFor<T>, base_fee: u128) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(base_fee > 0u128, Error::<T>::BaseGasFeeZero);

            <BaseGasFeeUsd<T>>::mutate(|a| *a = base_fee.clone());
            Self::deposit_event(Event::BaseGasFeeUsdSet { new_base_gas_fee: base_fee });

            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_finalize(_n: BlockNumberFor<T>) {
            let fee_pot = Self::fee_pot_account();

            let total_fees: BalanceOf<T> = T::Currency::free_balance(&fee_pot);
            if total_fees.is_zero() {
                return;
            }

            let used_weight: u128 = System::<T>::block_weight().total().ref_time() as u128;

            let max_weight: u128 =
                <T as frame_system::Config>::BlockWeights::get().max_block.ref_time() as u128;

            T::FeeDistributor::distribute_fees(&fee_pot, total_fees, used_weight, max_weight);
        }
    }

    impl<T: Config> Pallet<T> {
        pub fn get_min_avt_fee() -> u128 {
            // Base fee in USD (8 decimals)
            let min_usd_fee = BaseGasFeeUsd::<T>::get();

            // Price of 1 native token in USD (8 decimals)
            let rate = T::NativeRateProvider::native_rate_usd().unwrap_or(0);

            // Any invalid or zero values → fallback
            if min_usd_fee == 0 || rate == 0 {
                return FALLBACK_MIN_FEE;
            }

            // Safe integer division with fallback
            min_usd_fee.checked_div(rate).unwrap_or(FALLBACK_MIN_FEE)
        }

        pub fn fee_pot_account() -> T::AccountId {
            PalletId(sp_avn_common::FEE_POT_ID).into_account_truncating()
        }

        pub fn burn_pot_account() -> T::AccountId {
            PalletId(sp_avn_common::BURN_POT_ID).into_account_truncating()
        }

        /// ```text
        /// Collator reward ratio formula:
        ///
        ///     collator_ratio = min( desired_fullness / (block_fullness + epsilon), 1 )
        ///
        /// Where:
        ///   block_fullness   = used_weight / max_weight
        ///   desired_fullness = 0.005     (0.5% of block capacity)
        ///   epsilon          = 0.001     (0.1% — avoids division by zero for empty blocks)
        ///
        /// Meaning:
        ///   • If block is < 0.5% full → collator gets 100% of fees
        ///   • If block is > 0.5% full → collator only gets the amount needed to cover costs,
        ///                               and the rest is burned.
        ///
        /// When `max_weight == 0`, the chain cannot compute fullness,
        /// so fallback behavior gives 100% of fees to the collator:
        ///
        ///     collator_ratio = 1
        /// ```
        pub fn collator_ratio_from_weights(used_weight: u128, max_weight: u128) -> FixedU128 {
            let one = FixedU128::one();

            // fallback: if we somehow have no limit, give everything to collator
            if max_weight == 0 {
                return one;
            }

            let fullness =
                FixedU128::saturating_from_rational(used_weight.min(max_weight), max_weight);

            // We want the cutoff at 0.5% fullness.
            // But the formula clamps when: fullness <= desired_fullness - epsilon.
            // So to get a real cutoff of 0.5%, we set:
            // desired_fullness = 0.5% + epsilon = 0.6%.
            let desired_fullness = FixedU128::saturating_from_rational(6u128, 1000u128);

            // epsilon = 0.1% (0.001)
            let epsilon = FixedU128::saturating_from_rational(1u128, 1000u128);

            let denom = fullness.saturating_add(epsilon);
            let mut ratio = desired_fullness / denom;

            if ratio > one {
                ratio = one;
            }
            ratio
        }
    }
}

pub type BalanceOf<T> = <<T as Config>::Currency as frame_support::traits::Currency<
    <T as frame_system::Config>::AccountId,
>>::Balance;

type NegativeImbalanceOf<C, T> =
    <C as Currency<<T as frame_system::Config>::AccountId>>::NegativeImbalance;

impl<T: Config> Pallet<T> {
    pub fn calculate_refund_amount(
        fee_payer: &T::AccountId,
        amount_paid: &BalanceOf<T>,
        corrected_fee: BalanceOf<T>,
        tip: BalanceOf<T>,
    ) -> (bool, BalanceOf<T>) {
        // Calculate how much refund we should return
        let fee_adjustment_config = <KnownSenders<T>>::get(fee_payer);
        // calling is_active does some computation so cache it here
        let has_active_config = fee_adjustment_config.is_active();

        let mut fee_to_pay = corrected_fee.clone();
        if has_active_config {
            let network_fee_only = corrected_fee.saturating_sub(tip);
            match fee_adjustment_config.get_fee(network_fee_only) {
                Ok(fee) => fee_to_pay = fee.saturating_add(tip),
                Err(e) => {
                    log::error!(
                        "💔 Failed to apply an adjustment for known sender: {:?}, adjustment config: {:?}, error: {:?}",
                        fee_payer,
                        fee_adjustment_config,
                        e
                    );
                },
            }
        }

        let refund_amount = amount_paid.saturating_sub(fee_to_pay);
        return (has_active_config, refund_amount)
    }
}

/// Implements the transaction payment for a pallet implementing the `Currency`
/// trait (eg. the pallet_balances) using an unbalance handler (implementing
/// `OnUnbalanced`).
///
/// The unbalance handler is given 2 unbalanceds in [`OnUnbalanced::on_unbalanceds`]: fee and
/// then tip.
pub struct AvnCurrencyAdapter<C, OU>(PhantomData<(C, OU)>);

/// Default implementation for a Currency and an OnUnbalanced handler.
///
/// The unbalance handler is given 2 unbalanceds in [`OnUnbalanced::on_unbalanceds`]: fee and
/// then tip.
impl<T, C, OU> OnChargeTransaction<T> for AvnCurrencyAdapter<C, OU>
where
    T: Config + pallet::Config<Currency = C>,
    C: Currency<<T as frame_system::Config>::AccountId>,
    C::PositiveImbalance: Imbalance<
        <C as Currency<<T as frame_system::Config>::AccountId>>::Balance,
        Opposite = C::NegativeImbalance,
    >,
    C::NegativeImbalance: Imbalance<
        <C as Currency<<T as frame_system::Config>::AccountId>>::Balance,
        Opposite = C::PositiveImbalance,
    >,
    OU: OnUnbalanced<NegativeImbalanceOf<C, T>>,
{
    type LiquidityInfo = Option<NegativeImbalanceOf<C, T>>;
    type Balance = <C as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    /// Withdraw the predicted fee from the transaction origin.
    ///
    /// Note: The `fee` already includes the `tip`.
    fn withdraw_fee(
        who: &<T as frame_system::Config>::AccountId,
        _call: &<T as frame_system::Config>::RuntimeCall,
        _info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        fee: Self::Balance,
        tip: Self::Balance,
    ) -> Result<Self::LiquidityInfo, TransactionValidityError> {
        return <CurrencyAdapter<C, OU> as OnChargeTransaction<T>>::withdraw_fee(
            who, _call, _info, fee, tip,
        )
    }

    /// Hand the fee and the tip over to the `[OnUnbalanced]` implementation.
    /// Since the predicted fee might have been too high, parts of the fee may
    /// be refunded.
    ///
    /// Note: The `corrected_fee` already includes the `tip`.
    fn correct_and_deposit_fee(
        who: &<T as frame_system::Config>::AccountId,
        _dispatch_info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        _post_info: &PostDispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        corrected_fee: Self::Balance,
        tip: Self::Balance,
        already_withdrawn: Self::LiquidityInfo,
    ) -> Result<(), TransactionValidityError> {
        if let Some(paid) = already_withdrawn {
            // Calculate how much refund we should return
            let amount_paid = paid.peek();
            let (has_active_adjustment, refund_amount) =
                Pallet::<T>::calculate_refund_amount(who, &amount_paid, corrected_fee, tip);

            // refund to the account that paid the fees. If this fails, the
            // account might have dropped below the existential balance. In
            // that case we don't refund anything.
            let refund_imbalance = C::deposit_into_existing(who, refund_amount)
                .unwrap_or_else(|_| C::PositiveImbalance::zero());
            // merge the imbalance caused by paying the fees and refunding parts of it again.
            let adjusted_paid = paid
                .offset(refund_imbalance)
                .same()
                .map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::Payment))?;
            // Call someone else to handle the imbalance (fee and tip separately)
            let (tip, fee) = adjusted_paid.split(tip);
            OU::on_unbalanceds(Some(fee).into_iter().chain(Some(tip)));

            //Only deposit event if we are applying an adjustment
            if has_active_adjustment {
                Pallet::<T>::deposit_event(Event::<T>::AdjustedTransactionFeePaid {
                    who: who.clone(),
                    fee: amount_paid.saturating_sub(refund_amount),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;

#[cfg(test)]
#[path = "tests/set_known_sender_tests.rs"]
pub mod set_known_sender_tests;

#[cfg(test)]
#[path = "tests/adjustment_fee_tests.rs"]
pub mod adjustment_fee_tests;

#[cfg(test)]
#[path = "tests/base_fee_tests.rs"]
pub mod base_fee_tests;

pub mod default_weights;
pub use default_weights::WeightInfo;

mod benchmarking;
