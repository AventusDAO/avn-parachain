#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::String;

use frame_support::{
    dispatch::DispatchResult,
    ensure,
    traits::{Currency, StorageVersion},
};

pub mod default_weights;
pub use default_weights::WeightInfo;

use codec::{Decode, Encode};
use sp_avn_common::CallDecoder;
use sp_core::{ConstU32, Get, H256};
use sp_runtime::BoundedVec;
use sp_std::prelude::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

mod benchmarking;

pub type MaximumHandlersBound = ConstU32<256>;

pub type ChainNameLimit = ConstU32<32>;

pub const REGISTER_CHAIN_HANDLER: &'static [u8] = b"register_chain_handler";
pub const UPDATE_CHAIN_HANDLER: &'static [u8] = b"update_chain_handler";
pub const SUBMIT_CHECKPOINT: &'static [u8] = b"submit_checkpoint";

const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

pub use self::pallet::*;
pub type ChainId = u32;
pub type CheckpointId = u64;
pub type OriginId = u64;

pub(crate) type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{dispatch::GetDispatchInfo, pallet_prelude::*, traits::IsSubType};
    use frame_system::pallet_prelude::*;
    use sp_avn_common::{verify_signature, FeePaymentHandler, InnerCallValidator, Proof};
    use sp_core::H160;
    use sp_runtime::traits::{Dispatchable, IdentifyAccount, Verify};

    pub type ChainId = u32;
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    #[scale_info(skip_type_params(T))]
    pub struct ChainDataStruct {
        pub chain_id: ChainId,
        pub name: BoundedVec<u8, ChainNameLimit>,
    }
    pub type CheckpointId = u64;

    #[derive(Encode, Decode, Clone, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct CheckpointData {
        pub hash: H256,
        pub origin_id: OriginId,
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
        type FeeHandler: FeePaymentHandler<
            AccountId = Self::AccountId,
            Token = Self::Token,
            TokenBalance = <Self::Currency as Currency<Self::AccountId>>::Balance,
            Error = DispatchError,
        >;

        /// The default fee for checkpoint submission
        type DefaultCheckpointFee: Get<BalanceOf<Self>>;
    }

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new chain handler was registered. [handler_account_id, chain_id, name]
        ChainHandlerRegistered(T::AccountId, ChainId, BoundedVec<u8, ChainNameLimit>),
        /// A chain handler was updated. [old_handler_account_id, new_handler_account_id, chain_id,
        /// name]
        ChainHandlerUpdated(T::AccountId, T::AccountId, ChainId, BoundedVec<u8, ChainNameLimit>),
        /// A new checkpoint was submitted. [handler_account_id, chain_id, checkpoint_id,
        /// checkpoint]
        CheckpointSubmitted(T::AccountId, ChainId, CheckpointId, H256),
        /// The checkpoint fee was updated. [new_fee]
        CheckpointFeeUpdated { chain_id: ChainId, new_fee: BalanceOf<T> },

        /// Fee was charged for checkpoint submission [handler, fee, nonce]
        CheckpointFeeCharged { handler: T::AccountId, chain_id: ChainId, fee: BalanceOf<T> },
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
        NoChainDataAvailable,
        CheckpointOriginAlreadyExists,
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
    #[pallet::getter(fn chain_data)]
    pub type ChainData<T: Config> = StorageMap<_, Blake2_128Concat, ChainId, ChainDataStruct>;

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

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::weight(<T as pallet::Config>::WeightInfo::register_chain_handler())]
        #[pallet::call_index(0)]
        pub fn register_chain_handler(
            origin: OriginFor<T>,
            name: BoundedVec<u8, ChainNameLimit>,
        ) -> DispatchResult {
            let handler = ensure_signed(origin)?;

            Self::do_register_chain_handler(&handler, name)?;

            Ok(())
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

        #[pallet::weight(<T as pallet::Config>::WeightInfo::signed_register_chain_handler())]
        #[pallet::call_index(3)]
        pub fn signed_register_chain_handler(
            origin: OriginFor<T>,
            proof: Proof<T::Signature, T::AccountId>,
            handler: T::AccountId,
            name: BoundedVec<u8, ChainNameLimit>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            ensure!(sender == handler, Error::<T>::SenderNotValid);

            let signed_payload =
                encode_signed_register_chain_handler_params::<T>(&proof.relayer, &handler, &name);

            ensure!(
                verify_signature::<T::Signature, T::AccountId>(&proof, &signed_payload).is_ok(),
                Error::<T>::UnauthorizedSignedTransaction
            );

            Self::do_register_chain_handler(&handler, name)?;

            Ok(())
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
    }

    impl<T: Config> Pallet<T> {
        pub(crate) fn charge_fee(handler: T::AccountId, chain_id: ChainId) -> DispatchResult {
            let checkpoint_fee = Self::checkpoint_fee(chain_id);

            T::FeeHandler::pay_treasury(&checkpoint_fee, &handler)?;

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

        fn do_register_chain_handler(
            handler: &T::AccountId,
            name: BoundedVec<u8, ChainNameLimit>,
        ) -> Result<ChainId, DispatchError> {
            ensure!(
                !ChainHandlers::<T>::contains_key(handler),
                Error::<T>::HandlerAlreadyRegistered
            );
            ensure!(!name.is_empty(), Error::<T>::EmptyChainName);

            let chain_id = Self::get_next_chain_id()?;
            let chain_data = ChainDataStruct { chain_id, name: name.clone() };

            ChainHandlers::<T>::insert(handler, chain_id);
            ChainData::<T>::insert(chain_id, chain_data);
            <Nonces<T>>::insert(chain_id, 0);

            Self::deposit_event(Event::ChainHandlerRegistered(
                handler.clone(),
                chain_id,
                name.clone(),
            ));

            Ok(chain_id)
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

            let chain_data =
                ChainData::<T>::get(chain_id).ok_or(Error::<T>::NoChainDataAvailable)?;
            ChainHandlers::<T>::insert(&new_handler, chain_id);
            ChainHandlers::<T>::remove(&old_handler);

            Self::deposit_event(Event::ChainHandlerUpdated(
                old_handler.clone(),
                new_handler.clone(),
                chain_id,
                chain_data.name,
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
                Call::signed_register_chain_handler { ref proof, ref handler, ref name } => {
                    let encoded_data = encode_signed_register_chain_handler_params::<T>(
                        &proof.relayer,
                        handler,
                        name,
                    );

                    Some((proof, encoded_data))
                },
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
                Call::signed_register_chain_handler { proof, .. } => Ok(proof.clone()),
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

pub fn encode_signed_register_chain_handler_params<T: Config>(
    relayer: &T::AccountId,
    handler: &T::AccountId,
    name: &BoundedVec<u8, ChainNameLimit>,
) -> Vec<u8> {
    (REGISTER_CHAIN_HANDLER, relayer, handler, name).encode()
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

pub fn get_chain_data_for_handler<T: Config>(handler: &T::AccountId) -> Option<ChainDataStruct> {
    Pallet::<T>::chain_handlers(handler).and_then(|chain_id| Pallet::<T>::chain_data(chain_id))
}
