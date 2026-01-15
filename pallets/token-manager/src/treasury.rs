use super::*;
use frame_support::traits::{Currency, ExistenceRequirement};
use sp_runtime::{
    traits::{AccountIdConversion, Saturating, Zero},
    DispatchError,
};

pub trait TreasuryManager<T: Config> {
    fn fund_treasury(from: T::AccountId, amount: BalanceOf<T>) -> Result<(), DispatchError>;
}

impl<T: Config> Pallet<T> {
    /// Computes the account ID of the AvN treasury.
    /// This derives the treasury account by converting the configured `AvnTreasuryPotId`
    /// the value and only call this once.
    pub fn compute_treasury_account_id() -> T::AccountId {
        T::AvnTreasuryPotId::get().into_account_truncating()
    }

    /// The total amount of funds stored in this pallet
    pub fn treasury_balance() -> BalanceOf<T> {
        T::Currency::free_balance(&Self::compute_treasury_account_id())
            .saturating_sub(T::Currency::minimum_balance())
    }

    pub fn treasury_excess() -> Result<BalanceOf<T>, Error<T>> {
        let total_supply = TotalSupply::<T>::get().ok_or(Error::<T>::TotalSupplyNotSet)?;

        ensure!(!total_supply.is_zero(), Error::<T>::TotalSupplyZero);

        let treasury_balance = Self::treasury_balance();
        let threshold = T::TreasuryBurnThreshold::get() * total_supply;

        Ok(treasury_balance.saturating_sub(threshold))
    }

    pub fn move_treasury_excess_if_required() {
        let excess = match Self::treasury_excess() {
            Ok(x) => x,
            Err(_e) => {
                return;
            },
        };

        if excess.is_zero() {
            return;
        }

        let cap = T::TreasuryBurnCap::get();
        let burn_amount = excess.min(cap);

        if burn_amount.is_zero() {
            return;
        }

        let treasury = Self::compute_treasury_account_id();
        let burn_pot = Self::burn_pot_account();

        match T::Currency::transfer(
            &treasury,
            &burn_pot,
            burn_amount,
            ExistenceRequirement::KeepAlive,
        ) {
            Ok(_) => {
                Self::deposit_event(Event::<T>::TreasuryExcessSentToBurnPot {
                    amount: burn_amount,
                });
            },
            Err(e) => {
                log::error!("Failed to sweep {:?} to burn pot: {:?}", burn_amount, e);
            },
        }
    }

    pub fn transfer_treasury_funds(
        recipient: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> DispatchResult {
        ensure!(amount != BalanceOf::<T>::zero(), Error::<T>::AmountIsZero);

        let treasury = Self::compute_treasury_account_id();
        T::Currency::transfer(&treasury, recipient, amount, ExistenceRequirement::KeepAlive)?;

        Self::deposit_event(Event::<T>::TransferFromTreasury {
            recipient: recipient.clone(),
            amount,
        });

        Ok(())
    }
}

impl<T: Config> TreasuryManager<T> for Pallet<T> {
    fn fund_treasury(from: T::AccountId, amount: BalanceOf<T>) -> Result<(), DispatchError> {
        let treasury = Self::compute_treasury_account_id();
        T::Currency::transfer(&from, &treasury, amount, ExistenceRequirement::KeepAlive)?;

        Self::deposit_event(Event::<T>::TreasuryFunded { from, amount });

        if Self::is_burning_enabled() {
            Self::move_treasury_excess_if_required();
        }
        Ok(())
    }
}
