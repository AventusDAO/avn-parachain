// This file is part of Aventus.
// Copyright (C) 2026 Aventus Network Services (UK) Ltd.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use frame_support::{pallet_prelude::*, traits::Currency, BoundedVec};
use frame_system::pallet_prelude::*;
use sp_avn_common::{recover_ethereum_address_from_ecdsa_signature, HashMessageFormat};
use sp_core::{ecdsa, H160};
use sp_runtime::traits::Zero;
use sp_std::prelude::*;

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub mod default_weights;
pub use default_weights::WeightInfo;

pub const CONTEXT: &[u8] = b"avn:cross-chain-voting:v1";

type BalanceOf<T> =
    <<T as pallet::Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub enum Action {
    Link,
    Unlink,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct LinkPayload<AccountId> {
    pub action: Action,
    pub t1_identity_account: H160,
    pub t2_linked_account: AccountId,
    pub chain_id: u64,
}

impl<AccountId: Encode> LinkPayload<AccountId> {
    /// SCALE encoded bytes (plus CONTEXT) that the Ethereum account signs with EIP-191 prefix
    pub fn signing_bytes(&self) -> Vec<u8> {
        (CONTEXT, self).encode()
    }
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::Blake2_128Concat;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// Native currency for voting weight
        type Currency: Currency<Self::AccountId>;
        /// Max linked T2 accounts per T1 identity (set to 10 in runtime)
        #[pallet::constant]
        type MaxLinkedAccounts: Get<u32>;
        type WeightInfo: WeightInfo;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    // T1 identity -> T2 accounts
    #[pallet::storage]
    #[pallet::getter(fn get_linked_accounts)]
    pub type LinkedAccounts<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        H160,
        BoundedVec<T::AccountId, T::MaxLinkedAccounts>,
        ValueQuery,
    >;

    // T2 account -> T1 identity
    #[pallet::storage]
    #[pallet::getter(fn get_identity_account)]
    pub type LinkedAccountToIdentity<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, H160, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        AccountLinked { t1_identity_account: H160, t2_linked_account: T::AccountId },
        AccountUnlinked { t1_identity_account: H160, t2_linked_account: T::AccountId },
    }

    #[pallet::error]
    pub enum Error<T> {
        AccountLinkedToDifferentIdentity,
        AccountNotLinkedToIdentity,
        BadEcdsaSignature,
        CallerMustBeLinkedAccount,
        InvalidAction,
        LinkedAccountsLimitReached,
        SignerIdentityMismatch,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::link_account())]
        pub fn link_account(
            origin: OriginFor<T>,
            payload: LinkPayload<T::AccountId>,
            identity_account_sig: ecdsa::Signature,
        ) -> DispatchResult {
            let signer = ensure_signed(origin)?;
            ensure!(signer == payload.t2_linked_account, Error::<T>::CallerMustBeLinkedAccount);
            ensure!(payload.action == Action::Link, Error::<T>::InvalidAction);

            Self::verify_t1_signature(&payload, &identity_account_sig)?;

            if let Some(existing) = LinkedAccountToIdentity::<T>::get(&payload.t2_linked_account) {
                ensure!(
                    existing == payload.t1_identity_account,
                    Error::<T>::AccountLinkedToDifferentIdentity
                );

                return Ok(());
            }

            LinkedAccounts::<T>::try_mutate(
                payload.t1_identity_account,
                |vec| -> DispatchResult {
                    if vec.contains(&payload.t2_linked_account) {
                        return Ok(());
                    }
                    vec.try_push(payload.t2_linked_account.clone())
                        .map_err(|_| Error::<T>::LinkedAccountsLimitReached)?;
                    Ok(())
                },
            )?;

            LinkedAccountToIdentity::<T>::insert(
                &payload.t2_linked_account,
                payload.t1_identity_account,
            );

            Self::deposit_event(Event::<T>::AccountLinked {
                t1_identity_account: payload.t1_identity_account,
                t2_linked_account: payload.t2_linked_account,
            });

            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::unlink_account())]
        pub fn unlink_account(
            origin: OriginFor<T>,
            payload: LinkPayload<T::AccountId>,
        ) -> DispatchResult {
            let signer = ensure_signed(origin)?;
            ensure!(signer == payload.t2_linked_account, Error::<T>::CallerMustBeLinkedAccount);
            ensure!(payload.action == Action::Unlink, Error::<T>::InvalidAction);

            let owner = LinkedAccountToIdentity::<T>::get(&payload.t2_linked_account)
                .ok_or(Error::<T>::AccountNotLinkedToIdentity)?;
            ensure!(owner == payload.t1_identity_account, Error::<T>::AccountNotLinkedToIdentity);

            LinkedAccounts::<T>::mutate(payload.t1_identity_account, |vec| {
                if let Some(i) = vec.iter().position(|a| a == &payload.t2_linked_account) {
                    vec.swap_remove(i);
                }
            });

            LinkedAccountToIdentity::<T>::remove(&payload.t2_linked_account);

            Self::deposit_event(Event::<T>::AccountUnlinked {
                t1_identity_account: payload.t1_identity_account,
                t2_linked_account: payload.t2_linked_account,
            });

            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        fn verify_t1_signature(
            payload: &LinkPayload<T::AccountId>,
            sig: &ecdsa::Signature,
        ) -> DispatchResult {
            let msg = payload.signing_bytes();

            let recovered =
                recover_ethereum_address_from_ecdsa_signature(sig, &msg, HashMessageFormat::String)
                    .map_err(|_| Error::<T>::BadEcdsaSignature)?;

            let recovered_h160 = H160::from_slice(&recovered);

            ensure!(
                recovered_h160 == payload.t1_identity_account,
                Error::<T>::SignerIdentityMismatch
            );

            Ok(())
        }

        /// Sum balances across all linked accounts for a T1 identity.
        pub fn get_total_linked_balance(t1_identity_account: H160) -> BalanceOf<T> {
            let linked = LinkedAccounts::<T>::get(t1_identity_account);

            linked
                .into_iter()
                .map(|acc| T::Currency::free_balance(&acc))
                .fold(Zero::zero(), |a, b| a + b)
        }
    }
}
