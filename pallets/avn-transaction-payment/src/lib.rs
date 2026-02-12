//! # Avn transaction payment
// Copyright 2026 Aventus DAO Ltd

//! This is a wrapper pallet for transaction payment that allows the customisation of chain fees
//! based on defined adjustment configuration and a sender.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
    dispatch::{DispatchResult, GetDispatchInfo, PostDispatchInfo},
    traits::{
        fungible::{Balanced, Credit, Debt, Inspect},
        tokens::{Precision, WithdrawConsequence},
        Imbalance, OnUnbalanced,
    },
    unsigned::TransactionValidityError,
};
use frame_system::{self as system};

use core::convert::TryInto;
pub use pallet::*;
use sp_runtime::{
    traits::{DispatchInfoOf, Dispatchable, PostDispatchInfoOf, Saturating, Zero},
    transaction_validity::InvalidTransaction,
};

use pallet_transaction_payment::OnChargeTransaction;
use sp_std::{marker::PhantomData, prelude::*};

pub mod fee_adjustment_config;
use fee_adjustment_config::{
    AdjustmentType::{TimeBased, TransactionBased},
    *,
};

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{pallet_prelude::*, Blake2_128Concat};
    use frame_system::pallet_prelude::*;

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_transaction_payment::Config {
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
        type Currency: Inspect<Self::AccountId>;

        /// The origin that is allowed to set the known senders
        type KnownUserOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        type WeightInfo: WeightInfo;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

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
    }

    #[pallet::error]
    pub enum Error<T> {
        InvalidFeeConfig,
        InvalidFeeType,
        KnownSenderMustMatchAccount,
        KnownSenderMissing,
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
    }
}

type BalanceOf<T> =
    <<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

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
        return (has_active_config, refund_amount);
    }
}

// The fungible changes are copied from PolkadotSdk:
// https://github.com/paritytech/polkadot-sdk/commit/bda4e75ac49786a7246531cf729b25c208cd38e6
pub struct AvnGasFeeAdapter<F, OU>(PhantomData<(F, OU)>);

/// Default implementation for a Fungible and an OnUnbalanced handler.
///
/// The unbalance handler is given 2 unbalanceds in [`OnUnbalanced::on_unbalanceds`]: fee and
/// then tip.
impl<T, F, OU> OnChargeTransaction<T> for AvnGasFeeAdapter<F, OU>
where
    T: Config + pallet::Config<Currency = F>,
    F: Balanced<T::AccountId>,
    OU: OnUnbalanced<Credit<T::AccountId, F>>,
{
    type LiquidityInfo = Option<Credit<T::AccountId, F>>;
    type Balance = <F as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

    /// Withdraw the predicted fee from the transaction origin.
    ///
    /// Note: The `fee` already includes the `tip`.
    fn withdraw_fee(
        who: &<T as frame_system::Config>::AccountId,
        _call: &<T as frame_system::Config>::RuntimeCall,
        _info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        fee: Self::Balance,
        _tip: Self::Balance,
    ) -> Result<Self::LiquidityInfo, TransactionValidityError> {
        if fee.is_zero() {
            return Ok(None);
        }

        match F::withdraw(
            who,
            fee,
            Precision::Exact,
            frame_support::traits::tokens::Preservation::Preserve,
            frame_support::traits::tokens::Fortitude::Polite,
        ) {
            Ok(imbalance) => Ok(Some(imbalance)),
            Err(_) => Err(InvalidTransaction::Payment.into()),
        }
    }

    /// Check if the predicted fee from the transaction origin can be withdrawn.
    ///
    /// Note: The `fee` already includes the `tip`.
    fn can_withdraw_fee(
        who: &<T as frame_system::Config>::AccountId,
        _call: &<T as frame_system::Config>::RuntimeCall,
        _dispatch_info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        fee: Self::Balance,
        _tip: Self::Balance,
    ) -> Result<(), TransactionValidityError> {
        if fee.is_zero() {
            return Ok(());
        }

        match F::can_withdraw(who, fee) {
            WithdrawConsequence::Success => Ok(()),
            _ => Err(InvalidTransaction::Payment.into()),
        }
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
            let refund_imbalance =
                if refund_amount > Zero::zero() && F::total_balance(who) > F::Balance::zero() {
                    F::deposit(who, refund_amount, Precision::BestEffort)
                        .unwrap_or_else(|_| Debt::<T::AccountId, F>::zero())
                } else {
                    Debt::<T::AccountId, F>::zero()
                };

            // merge the imbalance caused by paying the fees and refunding parts of it again.
            let adjusted_paid: Credit<T::AccountId, F> = paid
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

    #[cfg(feature = "runtime-benchmarks")]
    fn endow_account(_who: &T::AccountId, _amount: Self::Balance) {
        unimplemented!()
    }
    #[cfg(feature = "runtime-benchmarks")]
    fn minimum_balance() -> Self::Balance {
        unimplemented!()
    }
}

// Vanilla fungible adapter from polkadot sdk without any discount logic
pub struct FungibleAdapter<F, OU>(PhantomData<(F, OU)>);

/// Default implementation for a Fungible and an OnUnbalanced handler.
///
/// The unbalance handler is given 2 unbalanceds in [`OnUnbalanced::on_unbalanceds`]: fee and
/// then tip.
impl<T, F, OU> OnChargeTransaction<T> for FungibleAdapter<F, OU>
where
    T: Config + pallet::Config<Currency = F>,
    F: Balanced<T::AccountId>,
    OU: OnUnbalanced<Credit<T::AccountId, F>>,
{
    type LiquidityInfo = Option<Credit<T::AccountId, F>>;
    type Balance = <F as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

    /// Withdraw the predicted fee from the transaction origin.
    ///
    /// Note: The `fee` already includes the `tip`.
    fn withdraw_fee(
        who: &<T as frame_system::Config>::AccountId,
        _call: &<T as frame_system::Config>::RuntimeCall,
        _info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        fee: Self::Balance,
        _tip: Self::Balance,
    ) -> Result<Self::LiquidityInfo, TransactionValidityError> {
        if fee.is_zero() {
            return Ok(None);
        }

        match F::withdraw(
            who,
            fee,
            Precision::Exact,
            frame_support::traits::tokens::Preservation::Preserve,
            frame_support::traits::tokens::Fortitude::Polite,
        ) {
            Ok(imbalance) => Ok(Some(imbalance)),
            Err(_) => Err(InvalidTransaction::Payment.into()),
        }
    }

    /// Check if the predicted fee from the transaction origin can be withdrawn.
    ///
    /// Note: The `fee` already includes the `tip`.
    fn can_withdraw_fee(
        who: &<T as frame_system::Config>::AccountId,
        _call: &<T as frame_system::Config>::RuntimeCall,
        _dispatch_info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        fee: Self::Balance,
        _tip: Self::Balance,
    ) -> Result<(), TransactionValidityError> {
        if fee.is_zero() {
            return Ok(());
        }

        match F::can_withdraw(who, fee) {
            WithdrawConsequence::Success => Ok(()),
            _ => Err(InvalidTransaction::Payment.into()),
        }
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
            let refund_amount = paid.peek().saturating_sub(corrected_fee);
            // refund to the the account that paid the fees if it exists. otherwise, don't refind
            // anything.
            let refund_imbalance = if F::total_balance(who) > F::Balance::zero() {
                F::deposit(who, refund_amount, Precision::BestEffort)
                    .unwrap_or_else(|_| Debt::<T::AccountId, F>::zero())
            } else {
                Debt::<T::AccountId, F>::zero()
            };
            // merge the imbalance caused by paying the fees and refunding parts of it again.
            let adjusted_paid: Credit<T::AccountId, F> = paid
                .offset(refund_imbalance)
                .same()
                .map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::Payment))?;
            // Call someone else to handle the imbalance (fee and tip separately)
            let (tip, fee) = adjusted_paid.split(tip);
            OU::on_unbalanceds(Some(fee).into_iter().chain(Some(tip)));
        }

        Ok(())
    }
    #[cfg(feature = "runtime-benchmarks")]
    fn endow_account(_who: &T::AccountId, _amount: Self::Balance) {
        unimplemented!()
    }
    #[cfg(feature = "runtime-benchmarks")]
    fn minimum_balance() -> Self::Balance {
        unimplemented!()
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

pub mod default_weights;
pub use default_weights::WeightInfo;

mod benchmarking;
