use crate::*;

impl<T: Config> Pallet<T> {
    pub fn credit_user_balance(
        token_id: T::TokenId,
        recipient: &T::AccountId,
        raw_amount: u128,
    ) -> Result<BalanceOf<T>, Error<T>> {
        match T::AssetRegistry::asset_id(&AvnAssetLocation::Ethereum(token_id.into())) {
            Some(asset) => Ok(Self::mint_known_token(asset, recipient, raw_amount)?),
            None => Ok(Self::mint_unknown_token(token_id, recipient, raw_amount)?),
        }
    }

    pub fn debit_user_balance(
        token_id: T::TokenId,
        from: &T::AccountId,
        raw_amount: u128,
    ) -> Result<(), Error<T>> {
        match T::AssetRegistry::asset_id(&AvnAssetLocation::Ethereum(token_id.into())) {
            Some(asset) => Ok(Self::burn_known_token(asset, from, raw_amount)?),
            None => Ok(Self::burn_unknown_token(token_id, from, raw_amount)?),
        }
    }

    fn mint_known_token(
        asset: CurrencyId,
        recipient: &T::AccountId,
        raw_amount: u128,
    ) -> Result<BalanceOf<T>, Error<T>> {
        let amount_balance = Self::u128_to_balance(raw_amount)?;
        T::AssetManager::deposit(asset, recipient, amount_balance)
            .map_err(|_| Error::<T>::DepositFailed)?;
        Ok(amount_balance)
    }

    fn mint_unknown_token(
        token_id: T::TokenId,
        recipient: &T::AccountId,
        raw_amount: u128,
    ) -> Result<BalanceOf<T>, Error<T>> {
        ensure!(!Self::is_native_token(token_id), Error::<T>::NativeTokenNotRegistered);
        let amount_token_balance = Self::u128_to_token_balance(raw_amount)?;
        <Balances<T>>::try_mutate((token_id, recipient.clone()), |balance| {
            *balance =
                balance.checked_add(&amount_token_balance).ok_or(Error::<T>::AmountOverflow)?;
            Ok(())
        })?;
        Ok(amount_token_balance.into())
    }

    fn burn_known_token(
        asset: CurrencyId,
        from: &T::AccountId,
        raw_amount: u128,
    ) -> Result<(), Error<T>> {
        let amount_balance = Self::u128_to_balance(raw_amount)?;
        T::AssetManager::withdraw(asset, from, amount_balance, ExistenceRequirement::KeepAlive)
            .map_err(|_| Error::<T>::InsufficientSenderBalance)?;
        Ok(())
    }

    fn burn_unknown_token(
        token_id: T::TokenId,
        from: &T::AccountId,
        raw_amount: u128,
    ) -> Result<(), Error<T>> {
        ensure!(!Self::is_native_token(token_id), Error::<T>::NativeTokenNotRegistered);
        let amount_token_balance = Self::u128_to_token_balance(raw_amount)?;
        <Balances<T>>::try_mutate((token_id, from.clone()), |balance| {
            *balance = balance
                .checked_sub(&amount_token_balance)
                .ok_or(Error::<T>::InsufficientSenderBalance)?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn is_native_token(token_id: T::TokenId) -> bool {
        token_id == Self::avt_token_contract().into()
    }

    /// Convert a `u128` raw amount to `BalanceOf<T>`.
    pub fn u128_to_balance(amount: u128) -> Result<BalanceOf<T>, Error<T>> {
        <BalanceOf<T> as TryFrom<u128>>::try_from(amount).map_err(|_| Error::<T>::AmountOverflow)
    }

    /// Convert a `u128` raw amount to `T::TokenBalance`.
    pub fn u128_to_token_balance(amount: u128) -> Result<T::TokenBalance, Error<T>> {
        <T::TokenBalance as TryFrom<u128>>::try_from(amount).map_err(|_| Error::<T>::AmountOverflow)
    }

    /// The account ID of the AvN treasury.
    /// This actually does computation. If you need to keep using it, then make sure you cache
    /// the value and only call this once.
    pub fn compute_treasury_account_id() -> T::AccountId {
        T::AvnTreasuryPotId::get().into_account_truncating()
    }

    /// The total amount of funds stored in this pallet
    pub fn treasury_balance() -> BalanceOf<T> {
        // Must never be less than 0 but better be safe.
        <T as pallet::Config>::Currency::free_balance(&Self::compute_treasury_account_id())
            .saturating_sub(<T as pallet::Config>::Currency::minimum_balance())
    }
}
