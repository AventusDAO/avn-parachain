//! # Aventus Node Pallet
//!
//! This pallet provides functionality that is common for other avn pallets such as handling
//! offchain worker validations, managing a list of validator accounts and their signing keys.
//!
//! The Authority defined here will also be shared by the other pallets that depend on AVN. This
//! means there will be 1 signing key for all AVN pallets (Ethereum-events,
//! Ethereum-transactions...). This key is separate from the rest of the session keys.
//!
//! From a security point of view, the rationale to implement it this way is because if one of the
//! signing keys is compromised, we dont want to trust that node for any other actions they carry
//! out so by having 1 key for our AVN pallets, we don't make our system less secure.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
};

use codec::{Decode, Encode};
use core::convert::TryInto;
use frame_support::{dispatch::DispatchResult, traits::OneSessionHandler};
use frame_system::{
    ensure_root,
    pallet_prelude::{BlockNumberFor, OriginFor},
};
pub use pallet::*;
use sp_application_crypto::RuntimeAppPublic;
use sp_avn_common::{
    bounds::{MaximumValidatorsBound, ProcessingBatchBound},
    event_types::{EthEvent, EthEventId, Validator},
    ocw_lock::{self as OcwLock, OcwStorageError},
    recover_public_key_from_ecdsa_signature, DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER,
    EXTERNAL_SERVICE_PORT_NUMBER_KEY,
};
use sp_core::{ecdsa, H160};
use sp_runtime::{
    offchain::{
        http,
        storage::StorageValueRef,
        storage_lock::{BlockAndTime, StorageLock},
        Duration,
    },
    traits::Member,
    BoundedVec, DispatchError, WeakBoundedVec,
};
use sp_std::prelude::*;

#[path = "tests/testing.rs"]
pub mod testing;
pub mod vote;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub mod default_weights;
pub use default_weights::WeightInfo;

#[cfg(test)]
#[path = "tests/test_set_bridge_contract.rs"]
mod test_set_bridge_contract;

// Definition of the crypto to use for signing
pub mod sr25519 {
    mod app_sr25519 {
        use sp_application_crypto::{app_crypto, sr25519};
        app_crypto!(sr25519, sp_avn_common::AVN_KEY_ID);
    }

    // An identifier using sr25519 as its crypto.
    pub type AuthorityId = app_sr25519::Public;
}

const AVN_SERVICE_CALL_EXPIRY: u32 = 300_000;

// used in benchmarks and weights calculation only
// TODO: centralise this with MaximumValidatorsBound
pub const MAX_VALIDATOR_ACCOUNTS: u32 = 10;

pub const PACKED_LOWER_PARAM_SIZE: usize = 76;
pub type LowerParams = [u8; PACKED_LOWER_PARAM_SIZE];

#[frame_support::pallet]
pub mod pallet {
    use frame_support::pallet_prelude::*;
    use sp_avn_common::bounds::MaximumValidatorsBound;

    use super::*;

    #[pallet::event]
    #[pallet::generate_deposit(pub(crate) fn deposit_event)]
    pub enum Event {
        AvnBridgeContractUpdated { old_contract: H160, new_contract: H160 },
    }

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Overarching event type
        type RuntimeEvent: From<Event> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The identifier type for an authority.
        type AuthorityId: Member
            + Parameter
            + RuntimeAppPublic
            + Ord
            + MaybeSerializeDeserialize
            + MaxEncodedLen;

        type EthereumPublicKeyChecker: EthereumPublicKeyChecker<Self::AccountId>;
        /// A handler that will notify other pallets when a new session starts
        type NewSessionHandler: NewSessionHandler<Self::AuthorityId, Self::AccountId>;
        /// trait that allows the system to check for disabled validators
        type DisabledValidatorChecker: DisabledValidatorChecker<Self::AccountId>;

        type WeightInfo: WeightInfo;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::error]
    pub enum Error<T> {
        NotAValidator,
        NoLocalAccounts,
        OffchainWorkerAlreadyRun,
        ErrorConvertingAccountId,
        ErrorConvertingBlockNumber,
        ErrorConvertingUtf8,
        ErrorDecodingHex,
        ErrorRecordingOffchainWorkerRun,
        NoValidatorsFound,
        UnexpectedStatusCode,
        InvalidResponse,
        InvalidVotingSession,
        InvalidContractAddress,
        DuplicateVote,
        InvalidVote,
        ErrorRecoveringPublicKeyFromSignature,
        InvalidECDSASignature,
        VectorBoundsExceeded,
        MaxValidatorsExceeded,
        ResponseFailed,
        RequestFailed,
        ErrorGettingFinalisedBlock,
        ErrorDecodingU32,
    }

    #[pallet::storage]
    #[pallet::getter(fn validators)]
    /// The current set of validators (address and key) that may issue a transaction from the
    /// offchain worker.
    pub type Validators<T: Config> = StorageValue<
        _,
        WeakBoundedVec<Validator<T::AuthorityId, T::AccountId>, MaximumValidatorsBound>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn get_bridge_contract_address)]
    pub type AvnBridgeContractAddress<T: Config> = StorageValue<_, H160, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn get_primary_collator)]
    pub type PrimaryCollatorIndexForSending<T: Config> = StorageValue<_, u8, ValueQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub _phantom: sp_std::marker::PhantomData<T>,
        pub bridge_contract_address: H160,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self { _phantom: Default::default(), bridge_contract_address: H160::zero() }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            AvnBridgeContractAddress::<T>::put(self.bridge_contract_address);
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_bridge_contract())]
        pub fn set_bridge_contract(origin: OriginFor<T>, contract_address: H160) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(&contract_address != &H160::zero(), Error::<T>::InvalidContractAddress);

            let old_contract = <AvnBridgeContractAddress<T>>::get();
            <AvnBridgeContractAddress<T>>::put(contract_address);
            Self::deposit_event(Event::AvnBridgeContractUpdated {
                old_contract,
                new_contract: contract_address,
            });
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    pub fn pre_run_setup(
        block_number: BlockNumberFor<T>,
        caller_id: Vec<u8>,
    ) -> Result<(Validator<T::AuthorityId, T::AccountId>, BlockNumberFor<T>), DispatchError> {
        if !sp_io::offchain::is_validator() {
            Err(Error::<T>::NotAValidator)?
        }

        let maybe_validator = Self::get_validator_for_current_node();
        if maybe_validator.is_none() {
            Err(Error::<T>::NoLocalAccounts)?
        }

        let finalised_block = Self::get_finalised_block_from_external_service()?;

        // Offchain workers could run multiple times for the same block number (re-orgs...)
        // so we need to make sure we only run this once per block
        OcwLock::record_block_run(block_number, caller_id).map_err(|e| match e {
            OcwStorageError::OffchainWorkerAlreadyRun => {
                log::info!(
                    "❌ Offchain worker has already run for block number: {:?}",
                    block_number
                );
                Error::<T>::OffchainWorkerAlreadyRun
            },
            OcwStorageError::ErrorRecordingOffchainWorkerRun => {
                log::error!(
                    "❌ Unable to record offchain worker run for block {:?}, skipping",
                    block_number
                );
                Error::<T>::ErrorRecordingOffchainWorkerRun
            },
        })?;

        return Ok((maybe_validator.expect("Already checked"), finalised_block))
    }

    pub fn get_default_ocw_lock_expiry() -> u32 {
        let avn_block_generation_in_millisec = 12_000 as u32;
        let delay: u32 = 5;
        let lock_expiry_in_blocks =
            (AVN_SERVICE_CALL_EXPIRY / avn_block_generation_in_millisec) + delay;
        return lock_expiry_in_blocks
    }

    pub fn get_primary_validator_for_sending() -> Result<T::AccountId, Error<T>> {
        let validators = Self::validators();
        // If there are no validators there's no point continuing
        if validators.is_empty() {
            return Err(Error::<T>::NoValidatorsFound)
        }

        let mut index = PrimaryCollatorIndexForSending::<T>::get() as usize;

        if index >= validators.len() {
            // Reset the counter to zero
            index = 0;
            PrimaryCollatorIndexForSending::<T>::put(index as u8);
        };

        Ok(validators[index].account_id.clone())
    }

    // TODO [TYPE: refactoring][PRI: LOW]: choose a better function name
    pub fn is_primary_validator_for_sending(
        current_validator: &T::AccountId,
    ) -> Result<bool, Error<T>> {
        let primary_validator = match Self::get_primary_validator_for_sending() {
            Ok(account_id) => account_id,
            Err(error) => return Err(error),
        };

        return Ok(&primary_validator == current_validator)
    }

    pub fn is_primary_for_block(
        block_number: BlockNumberFor<T>,
        current_validator: &T::AccountId,
    ) -> Result<bool, Error<T>> {
        let primary_validator = Self::calculate_primary_validator_for_block(block_number)?;
        return Ok(&primary_validator == current_validator)
    }

    pub fn advance_primary_validator_for_sending() -> Result<T::AccountId, Error<T>> {
        let validators = Self::validators();

        // If there are no validators there's no point continuing
        if validators.is_empty() {
            return Err(Error::<T>::NoValidatorsFound)
        }

        let ethereum_counter = PrimaryCollatorIndexForSending::<T>::get();
        let validators_len = Self::validators().len() as u8;

        let index = (ethereum_counter.saturating_add(1)) % validators_len;
        PrimaryCollatorIndexForSending::<T>::put(index);

        Ok(validators[index as usize].account_id.clone())
    }

    pub fn calculate_primary_validator_for_block(
        block_number: BlockNumberFor<T>,
    ) -> Result<T::AccountId, Error<T>> {
        let validators = Self::validators();

        // If there are no validators there's no point continuing
        if validators.is_empty() {
            return Err(Error::<T>::NoValidatorsFound)
        }

        let block_number: usize = TryInto::<usize>::try_into(block_number)
            .map_err(|_| Error::<T>::ErrorConvertingBlockNumber)?;

        let index = block_number % validators.len();
        return Ok(validators[index].account_id.clone())
    }

    pub fn get_validator_for_current_node() -> Option<Validator<T::AuthorityId, T::AccountId>> {
        // This will return all keys whose keytype is set to `Ethereum_events`
        let mut local_keys = T::AuthorityId::all();
        let validators = Self::validators();
        local_keys.sort();

        return validators
            .into_iter()
            .enumerate()
            .filter_map(move |(_, validator)| {
                local_keys.binary_search(&validator.key).ok().map(|_| validator)
            })
            .nth(0)
    }

    // Minimum number required to reach the threshold.
    pub fn quorum() -> u32 {
        let total_num_of_validators = Self::validators().len() as u32;
        Self::calculate_quorum(total_num_of_validators)
    }

    pub fn supermajority_quorum() -> u32 {
        let total_num_of_validators = Self::validators().len() as u32;
        total_num_of_validators * 2 / 3
    }

    pub fn calculate_quorum(num: u32) -> u32 {
        num - num * 2 / 3
    }

    pub fn get_data_from_service(url_path: String) -> Result<Vec<u8>, DispatchError> {
        let request = http::Request::default().method(http::Method::Get);
        return Ok(Self::invoke_external_service(request, url_path)?)
    }

    pub fn post_data_to_service(
        url_path: String,
        post_body: Vec<u8>,
    ) -> Result<Vec<u8>, DispatchError> {
        let request = http::Request::default().method(http::Method::Post).body(vec![post_body]);

        return Ok(Self::invoke_external_service(request, url_path)?)
    }

    pub fn request_ecdsa_signature_from_external_service(
        data_to_sign: &str,
    ) -> Result<ecdsa::Signature, DispatchError> {
        let mut url = String::from("eth/sign/");
        url.push_str(data_to_sign);

        log::info!(target: "avn-service", "avn-service sign request (ecdsa) for hex-encoded data {:?}", data_to_sign);

        let ecdsa_signature_utf8 = Self::get_data_from_service(url)?;
        let ecdsa_signature_bytes = core::str::from_utf8(&ecdsa_signature_utf8)
            .map_err(|_| Error::<T>::ErrorConvertingUtf8)?;

        let mut data: [u8; 65] = [0; 65];
        hex::decode_to_slice(ecdsa_signature_bytes, &mut data[0..65])
            .map_err(|_| Error::<T>::ErrorDecodingHex)?;
        Ok(ecdsa::Signature::from_raw(data))
    }

    pub fn signature_is_valid<D: Encode>(
        data: &D,
        validator: &Validator<T::AuthorityId, T::AccountId>,
        signature: &<T::AuthorityId as RuntimeAppPublic>::Signature,
    ) -> bool {
        // verify that the incoming (unverified) pubkey is actually a validator
        let registered_validator = Self::try_get_validator(&validator.account_id);
        if let Some(actual_validator) = registered_validator {
            // check signature (this is expensive so we do it last). Use the key of the actual
            // validator.
            let signature_valid = data.using_encoded(|encoded_data| {
                actual_validator.key.verify(&encoded_data, &signature)
            });

            log::debug!(
                "🪲 Validating signature: [ data {:?} - account {:?} - signature {:?} ] Result: {}",
                data.encode(),
                validator.encode(),
                signature,
                signature_valid
            );
            return signature_valid
        } else {
            log::warn!("Signature validation failed, account {:?}, is not validator", validator);
            return false
        }
    }

    pub fn eth_signature_is_valid(
        data: String,
        validator: &Validator<T::AuthorityId, T::AccountId>,
        signature: &ecdsa::Signature,
    ) -> bool {
        // verify that the incoming (unverified) pubkey is actually a validator
        if !Self::is_validator(&validator.account_id) {
            log::warn!("✋ Account: {:?} is not an authority.", &validator.account_id);
            return false
        }
        let recovered_public_key = recover_public_key_from_ecdsa_signature(signature, &data);
        if recovered_public_key.is_err() {
            log::error!(
                "❌ Recovery of public key from ECDSA Signature: {:?} and data: {:?} failed",
                &signature,
                data
            );
            return false
        }

        match T::EthereumPublicKeyChecker::get_validator_for_eth_public_key(
            &recovered_public_key.expect("Checked for error"),
        ) {
            Some(maybe_validator) => maybe_validator == validator.account_id,
            _ => {
                log::error!(
                    "❌ ECDSA signature validation failed on data {:?} validator: {:?} signature {:?}.",
                    &data,
                    validator,
                    signature
                );
                false
            },
        }
    }

    pub fn convert_block_number_to_u32(block_number: BlockNumberFor<T>) -> Result<u32, Error<T>> {
        let block_number: u32 = TryInto::<u32>::try_into(block_number)
            .map_err(|_| Error::<T>::ErrorConvertingBlockNumber)?;

        Ok(block_number)
    }

    pub fn is_validator(account_id: &T::AccountId) -> bool {
        return Self::validators().into_iter().any(|v| v.account_id == *account_id)
    }

    pub fn active_validators(
    ) -> WeakBoundedVec<Validator<T::AuthorityId, T::AccountId>, MaximumValidatorsBound> {
        return Self::validators()
    }

    pub fn try_get_validator(
        account_id: &T::AccountId,
    ) -> Option<Validator<T::AuthorityId, T::AccountId>> {
        return Self::validators().into_iter().filter(|v| v.account_id == *account_id).nth(0)
    }

    /// This function will mutate storage. Any code after calling this MUST not error.
    pub fn remove_validator_from_active_list(validator_id: &T::AccountId) {
        <Validators<T>>::mutate(|active_validators| {
            if let Some(validator_index) =
                active_validators.iter().position(|v| &v.account_id == validator_id)
            {
                active_validators.remove(validator_index);
            }
        });
    }

    // Called from OCW, no storage changes allowed
    pub fn get_finalised_block_from_external_service() -> Result<BlockNumberFor<T>, Error<T>> {
        let response = Self::get_data_from_service(String::from("latest_finalised_block"))
            .map_err(|e| {
                log::error!("❌ Error getting finalised block from avn service: {:?}", e);
                Error::<T>::ErrorGettingFinalisedBlock
            })?;

        let finalised_block_bytes = hex::decode(&response).map_err(|e| {
            log::error!("❌ Error decoding finalised block data {:?}", e);
            Error::<T>::InvalidResponse
        })?;

        let finalised_block = u32::decode(&mut &finalised_block_bytes[..]).map_err(|e| {
            log::error!("❌ Finalised block is not a valid u32: {:?}", e);
            Error::<T>::ErrorDecodingU32
        })?;

        return Ok(BlockNumberFor::<T>::from(finalised_block))
    }

    pub fn get_external_service_port_number() -> String {
        let stored_value =
            StorageValueRef::persistent(EXTERNAL_SERVICE_PORT_NUMBER_KEY).get::<Vec<u8>>();
        let port_number_bytes = match stored_value {
            Ok(Some(port)) => port,
            _ => DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER.into(),
        };

        let port_number = core::str::from_utf8(&port_number_bytes);
        if let Err(e) = port_number {
            log::trace!(
                "❌ External service port {} is not formatted correctly. Using default port.",
                e
            );
            return DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER.into()
        }

        let port_number = port_number.expect("Already checked for errors");

        if port_number.parse::<u32>().is_err() {
            log::trace!(
                "❌ External service port {} is not a valid number. Using default port.",
                port_number
            );
            return DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER.into()
        }

        return port_number.into()
    }

    fn invoke_external_service(
        request: http::Request<Vec<Vec<u8>>>,
        url_path: String,
    ) -> Result<Vec<u8>, DispatchError> {
        // TODO: Make this configurable
        let deadline =
            sp_io::offchain::timestamp().add(Duration::from_millis(AVN_SERVICE_CALL_EXPIRY as u64));
        let url = format!(
            "http://127.0.0.1:{}/{}",
            Self::get_external_service_port_number(),
            url_path.trim_start_matches('/')
        );

        let response = request
            .deadline(deadline)
            .url(&url)
            .send()
            .map_err(|e| {
                log::error!("❌ Request failed: {:?}", e);
                Error::<T>::RequestFailed
            })?
            .try_wait(deadline)
            .map_err(|e| {
                log::error!("❌ Response failed: {:?}", e);
                Error::<T>::ResponseFailed
            })?
            .map_err(|e| {
                log::error!("❌ Invalid response: {:?}", e);
                Error::<T>::InvalidResponse
            })?;

        if response.code != 200 {
            log::error!("❌ Unexpected status code: {}", response.code);
            return Err(Error::<T>::UnexpectedStatusCode)?
        }

        Ok(response.body().collect())
    }

    pub fn get_ocw_locker<'a>(
        lock_name: &'a [u8],
    ) -> StorageLock<'a, BlockAndTime<frame_system::Pallet<T>>> {
        Self::get_ocw_locker_with_custom_expiry(lock_name, Self::get_default_ocw_lock_expiry())
    }

    pub fn get_ocw_locker_with_custom_expiry<'a>(
        lock_name: &'a [u8],
        expiry_in_blocks: u32,
    ) -> StorageLock<'a, BlockAndTime<frame_system::Pallet<T>>> {
        OcwLock::get_offchain_worker_locker::<frame_system::Pallet<T>>(&lock_name, expiry_in_blocks)
    }
}

// Session pallet interface

impl<T: Config> sp_runtime::BoundToRuntimeAppPublic for Pallet<T> {
    type Public = T::AuthorityId;
}

impl<T: Config> OneSessionHandler<T::AccountId> for Pallet<T> {
    type Key = T::AuthorityId;

    fn on_genesis_session<'a, I: 'a>(validators: I)
    where
        I: Iterator<Item = (&'a T::AccountId, T::AuthorityId)>,
    {
        log::trace!("Avn pallet genesis session entrypoint");
        let avn_validators = WeakBoundedVec::force_from(
            validators.map(|x| Validator::new(x.0.clone(), x.1)).collect::<Vec<_>>(),
            Some("Too many validators for session"),
        );
        if !avn_validators.is_empty() {
            assert!(Validators::<T>::get().is_empty(), "Validators are already initialized!");
            Validators::<T>::put(&avn_validators);

            T::NewSessionHandler::on_genesis_session(&avn_validators);
        }
    }

    fn on_new_session<'a, I: 'a>(changed: bool, validators: I, _queued_validators: I)
    where
        I: Iterator<Item = (&'a T::AccountId, T::AuthorityId)>,
    {
        log::trace!("Avn pallet new session entrypoint");
        // Update the list of validators if it has changed
        let mut disabled_avn_validators: Vec<T::AccountId> = vec![];
        let mut active_avn_validators: Vec<Validator<T::AuthorityId, T::AccountId>> = vec![];

        validators.for_each(|x| {
            if T::DisabledValidatorChecker::is_disabled(x.0) {
                disabled_avn_validators.push(x.0.clone());
            } else {
                active_avn_validators.push(Validator::new(x.0.clone(), x.1));
            }
        });

        if changed {
            let bounded_active_avn_validators = WeakBoundedVec::force_from(
                active_avn_validators.clone(),
                Some("Too many validators for session"),
            );
            Validators::<T>::put(&bounded_active_avn_validators);
        }

        T::NewSessionHandler::on_new_session(
            changed,
            &active_avn_validators,
            &disabled_avn_validators,
        );
    }

    fn on_disabled(_i: u32) {
        // ignore
    }
}

pub trait AccountToBytesConverter<AccountId: Decode + Encode> {
    fn into_bytes(account: &AccountId) -> [u8; 32];
    fn try_from(account_bytes: &[u8; 32]) -> Result<AccountId, DispatchError>;
    /// This function expects valid bytes that can be converted into an accountId. No validation is
    /// done here.
    fn try_from_any(bytes: Vec<u8>) -> Result<AccountId, DispatchError>;
}

impl<T: Config> AccountToBytesConverter<T::AccountId> for Pallet<T> {
    fn into_bytes(account: &T::AccountId) -> [u8; 32] {
        let bytes = account.encode();
        let mut vector: [u8; 32] = Default::default();
        vector.copy_from_slice(&bytes[0..32]);
        return vector
    }

    fn try_from(account_bytes: &[u8; 32]) -> Result<T::AccountId, DispatchError> {
        let account_result = T::AccountId::decode(&mut &account_bytes[..]);
        account_result.map_err(|_| DispatchError::Other("Error converting AccountId"))
    }

    fn try_from_any(bytes: Vec<u8>) -> Result<T::AccountId, DispatchError> {
        let mut account_bytes: [u8; 32] = Default::default();
        account_bytes.copy_from_slice(&bytes[0..32]);

        return T::AccountId::decode(&mut &account_bytes[..])
            .map_err(|_| DispatchError::Other("Error converting to AccountId"))
    }
}

pub trait EthereumPublicKeyChecker<AccountId: Member> {
    fn get_validator_for_eth_public_key(eth_public_key: &ecdsa::Public) -> Option<AccountId>;
}

impl<AccountId: Member> EthereumPublicKeyChecker<AccountId> for () {
    fn get_validator_for_eth_public_key(_eth_public_key: &ecdsa::Public) -> Option<AccountId> {
        None
    }
}

pub trait NewSessionHandler<AuthorityId: Member, AccountId: Member> {
    fn on_genesis_session(validators: &Vec<Validator<AuthorityId, AccountId>>);
    fn on_new_session(
        changed: bool,
        active_validators: &Vec<Validator<AuthorityId, AccountId>>,
        disabled_validators: &Vec<AccountId>,
    );
}

pub trait DisabledValidatorChecker<AccountId: Member> {
    fn is_disabled(validator_account_id: &AccountId) -> bool;
}

impl<AccountId: Member> DisabledValidatorChecker<AccountId> for () {
    fn is_disabled(_validator_account_id: &AccountId) -> bool {
        false
    }
}

pub trait ValidatorRegistrationNotifier<ValidatorId: Member> {
    fn on_validator_registration(validator_id: &ValidatorId);
}

impl<ValidatorId: Member> ValidatorRegistrationNotifier<ValidatorId> for () {
    fn on_validator_registration(_validator_id: &ValidatorId) {}
}

pub trait Enforcer<ValidatorId: Member> {
    fn slash_validator(slashed_validator_id: &ValidatorId) -> DispatchResult;
}

impl<ValidatorId: Member> Enforcer<ValidatorId> for () {
    fn slash_validator(_slashed_validator_id: &ValidatorId) -> DispatchResult {
        Ok(())
    }
}

pub struct EventMigration {
    pub entry_return_impl: fn(&EthEventId, bool),
    pub event_id: EthEventId,
    pub outcome: bool,
}

impl EventMigration {
    pub fn return_entry(&self) {
        (self.entry_return_impl)(&self.event_id, self.outcome);
    }
}

pub trait ProcessedEventsChecker {
    fn processed_event_exists(event_id: &EthEventId) -> bool;
    fn add_processed_event(event_id: &EthEventId, accepted: bool) -> Result<(), ()>;
    fn get_events_to_migrate() -> Option<BoundedVec<EventMigration, ProcessingBatchBound>> {
        None
    }
}

impl ProcessedEventsChecker for () {
    fn processed_event_exists(_event_id: &EthEventId) -> bool {
        false
    }

    fn add_processed_event(_event_id: &EthEventId, _accepted: bool) -> Result<(), ()> {
        Ok(())
    }
}

pub trait OnGrowthLiftedHandler<Balance> {
    fn on_growth_lifted(amount: Balance, growth_period: u32) -> DispatchResult;
}

impl<Balance> OnGrowthLiftedHandler<Balance> for () {
    fn on_growth_lifted(_amount: Balance, _growth_period: u32) -> DispatchResult {
        Ok(())
    }
}

// Trait that handles dust amounts after paying collators for producing blocks
pub trait CollatorPayoutDustHandler<Balance> {
    fn handle_dust(imbalance: Balance);
}

impl<Balance> CollatorPayoutDustHandler<Balance> for () {
    fn handle_dust(_imbalance: Balance) {}
}

pub trait BridgeInterface {
    fn publish(
        function_name: &[u8],
        params: &[(Vec<u8>, Vec<u8>)],
        caller_id: Vec<u8>,
    ) -> Result<u32, DispatchError>;
    fn generate_lower_proof(
        lower_id: u32,
        params: &LowerParams,
        caller_id: Vec<u8>,
    ) -> Result<(), DispatchError>;
    fn read_bridge_contract(
        account_id_bytes: Vec<u8>,
        function_name: &[u8],
        params: &[(Vec<u8>, Vec<u8>)],
        eth_block: Option<u32>,
    ) -> Result<Vec<u8>, DispatchError>;
    fn latest_finalised_ethereum_block() -> Result<u32, DispatchError>;
}

pub trait BridgeInterfaceNotification {
    fn process_result(tx_id: u32, caller_id: Vec<u8>, succeeded: bool) -> DispatchResult;
    fn process_lower_proof_result(_: u32, _: Vec<u8>, _: Result<Vec<u8>, ()>) -> DispatchResult {
        Ok(())
    }
    fn on_incoming_event_processed(_event: &EthEvent) -> DispatchResult {
        Ok(())
    }
}

#[impl_trait_for_tuples::impl_for_tuples(30)]
impl BridgeInterfaceNotification for Tuple {
    fn process_result(_tx_id: u32, _caller_id: Vec<u8>, _succeeded: bool) -> DispatchResult {
        for_tuples!( #( Tuple::process_result(_tx_id, _caller_id.clone(), _succeeded)?; )* );
        Ok(())
    }

    fn process_lower_proof_result(
        _lower_id: u32,
        _caller_id: Vec<u8>,
        _encoded_lower: Result<Vec<u8>, ()>,
    ) -> DispatchResult {
        for_tuples!( #( Tuple::process_lower_proof_result(_lower_id, _caller_id.clone(), _encoded_lower.clone())?; )* );
        Ok(())
    }

    fn on_incoming_event_processed(_event: &EthEvent) -> DispatchResult {
        for_tuples!( #( Tuple::on_incoming_event_processed(_event)?; )* );
        Ok(())
    }
}

#[cfg(test)]
mod mock;

#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/session_handler_tests.rs"]
mod session_handler_tests;
