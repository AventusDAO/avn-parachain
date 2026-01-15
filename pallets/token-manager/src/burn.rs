use super::pallet::*;
use crate::{default_weights::WeightInfo, BalanceOf, PALLET_ID};
use frame_support::{
    pallet_prelude::Weight,
    traits::{Currency, ReservableCurrency},
    PalletId,
};
use frame_system::pallet_prelude::BlockNumberFor;
use pallet_avn::BridgeInterface;
use sp_avn_common::BridgeContractMethod;
use sp_runtime::{
    traits::{AccountIdConversion, Saturating, Zero},
    DispatchError,
};
use sp_std::{vec, vec::Vec};

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::format;

impl<T: Config> Pallet<T> {
    pub(crate) fn is_burn_due(now: BlockNumberFor<T>) -> bool {
        now >= NextBurnAt::<T>::get()
    }

    pub(crate) fn burn_pot_account() -> T::AccountId {
        PalletId(sp_avn_common::BURN_POT_ID).into_account_truncating()
    }

    pub(crate) fn schedule_next_burn(now: BlockNumberFor<T>) {
        let next_burn = now.saturating_add(BlockNumberFor::<T>::from(BurnPeriod::<T>::get()));
        NextBurnAt::<T>::put(next_burn);
    }

    pub(crate) fn burn_from_pot() -> Weight {
        let burn_pot = Self::burn_pot_account();
        let amount: BalanceOf<T> = T::Currency::free_balance(&burn_pot);

        if amount.is_zero() {
            return <T as Config>::WeightInfo::on_initialize_burn_due_but_pot_empty();
        }

        let _ = Self::publish_burn_tokens_on_t1(&burn_pot, amount);
        <T as Config>::WeightInfo::on_initialize_burn_due_and_pot_has_funds_to_burn()
    }

    pub(crate) fn publish_burn_tokens_on_t1(
        burner: &T::AccountId,
        amount: BalanceOf<T>,
    ) -> Result<(), DispatchError> {
        T::Currency::reserve(burner, amount).map_err(|_| Error::<T>::ErrorLockingTokens)?;

        let amount_u128: u128 = amount.try_into().map_err(|_| Error::<T>::AmountOverflow)?;

        let function_name: &[u8] = BridgeContractMethod::BurnFees.as_bytes();
        let params = vec![(b"uint128".to_vec(), format!("{}", amount_u128).into_bytes())];

        match T::BridgeInterface::publish(function_name, &params, PALLET_ID.to_vec()) {
            Ok(tx_id) => {
                PendingBurnSubmission::<T>::insert(tx_id, (burner.clone(), amount));
                Self::deposit_event(Event::<T>::BurnFundsRequested {
                    burner: burner.clone(),
                    amount,
                    tx_id,
                });

                Ok(())
            },
            Err(_) => {
                T::Currency::unreserve(burner, amount);
                Err(Error::<T>::FailedToSubmitBurnRequest.into())
            },
        }
    }
}
