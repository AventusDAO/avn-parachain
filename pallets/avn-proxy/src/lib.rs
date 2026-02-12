//! # Avn proxy pallet
// Copyright 2026 Aventus DAO Ltd

//! The avnProxy pallet is responsible for proxying transactions to a list of whitelisted pallets.
//! The target pallets are responsible for validating the transaction and should not make any
//! assumption about where the transaction is coming from.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
    dispatch::{DispatchResult, DispatchResultWithPostInfo, GetDispatchInfo, PostDispatchInfo},
    ensure,
    pallet_prelude::ValueQuery,
    traits::{Currency, IsSubType},
};
use frame_system::{self as system, ensure_signed};
use sp_avn_common::{verify_multi_signature, FeePaymentHandler, InnerCallValidator, Proof};

use core::convert::TryInto;
pub use pallet::*;
use sp_core::{MaxEncodedLen, H160};
use sp_runtime::{
    scale_info::TypeInfo,
    traits::{Dispatchable, Hash, IdentifyAccount, Member, Verify},
};
use sp_std::prelude::*;

pub const PAYMENT_AUTH_CONTEXT: &'static [u8] = b"authorization for proxy payment";

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{pallet_prelude::*, Blake2_128Concat};
    use frame_system::pallet_prelude::*;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The overarching call type
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
            + GetDispatchInfo
            + From<frame_system::Call<Self>>
            + IsSubType<Call<Self>>;

        /// Currency type for processing fee payment
        type Currency: Currency<Self::AccountId>;

        /// A type that can be used to verify signatures
        type Public: IdentifyAccount<AccountId = Self::AccountId>;

        /// The signature type used by accounts/transactions.
        type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode + TypeInfo;

        type ProxyConfig: Parameter
            + Member
            + Ord
            + PartialOrd
            + Default
            + ProvableProxy<<Self as Config>::RuntimeCall, Self::Signature, Self::AccountId>
            + InnerCallValidator<Call = <Self as Config>::RuntimeCall>;

        type WeightInfo: WeightInfo;

        /// The type of token identifier
        /// (a H160 because this is an Ethereum address)
        type Token: Parameter + Default + Copy + From<H160> + Into<H160> + MaxEncodedLen;

        /// A handler to process relayer fee payments
        type FeeHandler: FeePaymentHandler<
            AccountId = Self::AccountId,
            Token = Self::Token,
            TokenBalance = <Self::Currency as Currency<Self::AccountId>>::Balance,
            Error = DispatchError,
        >;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        CallDispatched { relayer: T::AccountId, hash: T::Hash },
        InnerCallFailed { relayer: T::AccountId, hash: T::Hash, dispatch_error: DispatchError },
    }

    #[pallet::error]
    pub enum Error<T> {
        TransactionNotSupported,
        UnauthorizedFee,
        UnauthorizedProxyTransaction,
    }

    #[pallet::storage]
    #[pallet::getter(fn payment_nonces)]
    /// An account nonce that represents the number of payments from this account
    /// It is shared for all proxy transactions performed by that account
    pub type PaymentNonces<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u64, ValueQuery>;

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(
            <T as pallet::Config>::WeightInfo::charge_fee()
            .saturating_add(call.get_dispatch_info().call_weight)
            .saturating_add(Weight::from_parts(50_000 as u64, 0)))
        ]
        pub fn proxy(
            origin: OriginFor<T>,
            call: Box<<T as Config>::RuntimeCall>,
            payment_info: Option<
                Box<PaymentInfo<T::AccountId, BalanceOf<T>, T::Signature, T::Token>>,
            >,
        ) -> DispatchResultWithPostInfo {
            let relayer = ensure_signed(origin)?;
            let mut final_weight = call
                .get_dispatch_info()
                .call_weight
                .saturating_add(Weight::from_parts(50_000 as u64, 0));

            let proof = <T as Config>::ProxyConfig::get_proof(&call)
                .ok_or(Error::<T>::TransactionNotSupported)?;
            ensure!(relayer == proof.relayer, Error::<T>::UnauthorizedProxyTransaction);

            if let Some(payment_info) = payment_info {
                final_weight = T::WeightInfo::charge_fee()
                    .saturating_add(call.get_dispatch_info().call_weight)
                    .saturating_add(Weight::from_parts(50_000 as u64, 0));

                // Always try to charge a fee, regardless of the outcome of execution.
                // If the payment signature is not valid, the nonce is not incremented and the
                // transaction is rejected. This allows to keep the payment nonce in
                // memory when sending multiple transactions back to back.
                Self::charge_fee(&proof, *payment_info)?;
            }

            // No errors are allowed past this point, otherwise we will undo the payment.
            let call_hash: T::Hash = T::Hashing::hash_of(&call);

            // If the inner call signature does not validate, there is no need to dispatch the tx so
            // return early.
            let inner_call_validation_result = Self::validate_inner_call_signature(&call);
            if let Err(err) = inner_call_validation_result {
                Self::deposit_event(Event::<T>::InnerCallFailed {
                    relayer,
                    hash: call_hash,
                    dispatch_error: err,
                });

                // Return an OK even if the signature is bad. The `InnerCallFailed` event will
                // inform the caller about the failure
                return Ok(Some(final_weight).into());
            }

            let sender: T::RuntimeOrigin =
                frame_system::RawOrigin::Signed(proof.signer.clone()).into();

            let dispatch_result = call.dispatch(sender).map(|_| ()).map_err(|e| e.error);
            match dispatch_result {
                Ok(_) => {
                    Self::deposit_event(Event::<T>::CallDispatched { relayer, hash: call_hash });
                },
                Err(dispatch_error) => {
                    Self::deposit_event(Event::<T>::InnerCallFailed {
                        relayer,
                        hash: call_hash,
                        dispatch_error,
                    });
                },
            }

            Ok(Some(final_weight).into())
        }
    }
}

pub(crate) type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

impl<T: Config> Pallet<T> {
    fn validate_inner_call_signature(call: &Box<<T as Config>::RuntimeCall>) -> DispatchResult {
        let inner_call_sig_valid = <T as Config>::ProxyConfig::signature_is_valid(call);
        if inner_call_sig_valid == false {
            return Err(Error::<T>::UnauthorizedProxyTransaction)?;
        }

        Ok(())
    }

    fn verify_payment_authorisation_signature(
        proof: &Proof<T::Signature, T::AccountId>,
        payment_info: &PaymentInfo<T::AccountId, BalanceOf<T>, T::Signature, T::Token>,
        payment_nonce: u64,
    ) -> Result<(), Error<T>> {
        let encoded_payload = (
            PAYMENT_AUTH_CONTEXT,
            &proof,
            &payment_info.recipient,
            &payment_info.amount,
            &payment_info.token,
            payment_nonce,
        )
            .encode();

        match verify_multi_signature::<T::Signature, T::AccountId>(
            &payment_info.payer,
            &payment_info.signature,
            &encoded_payload.as_slice(),
        ) {
            Ok(()) => Ok(()),
            Err(()) => Err(<Error<T>>::UnauthorizedFee.into()),
        }
    }

    pub(crate) fn charge_fee(
        proof: &Proof<T::Signature, T::AccountId>,
        payment_info: PaymentInfo<T::AccountId, BalanceOf<T>, T::Signature, T::Token>,
    ) -> DispatchResult {
        let payment_nonce = Self::payment_nonces(&payment_info.payer);
        ensure!(
            Self::verify_payment_authorisation_signature(proof, &payment_info, payment_nonce)
                .is_ok(),
            Error::<T>::UnauthorizedFee
        );

        T::FeeHandler::pay_fee(
            &payment_info.token,
            &payment_info.amount,
            &payment_info.payer,
            &payment_info.recipient,
        )?;

        <PaymentNonces<T>>::mutate(&payment_info.payer, |n| *n += 1);
        Ok(())
    }
}

pub trait ProvableProxy<Call, Signature: scale_info::TypeInfo, AccountId>:
    Sized + Send + Sync
{
    fn get_proof(call: &Call) -> Option<Proof<Signature, AccountId>>;
}

#[derive(
    Encode, Decode, Copy, Clone, PartialEq, Eq, Default, Debug, TypeInfo, DecodeWithMemTracking,
)]
pub struct PaymentInfo<AccountId, Balance, Signature: TypeInfo, Token> {
    pub payer: AccountId,
    pub recipient: AccountId,
    pub amount: Balance,
    pub signature: Signature,
    pub token: Token,
}

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;

#[cfg(test)]
#[path = "tests/proxy_tests_no_fees.rs"]
pub mod proxy_tests_no_fees;

#[cfg(test)]
#[path = "tests/proxy_tests_with_fees.rs"]
pub mod proxy_tests_with_fees;

pub mod default_weights;
pub use default_weights::WeightInfo;

mod benchmarking;
