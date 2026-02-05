//! # Validators manager Pallet
//!
//! This pallet provides functionality to add/remove validators.
//!
//! The pallet is based on the Substrate session pallet and implements related traits for session
//! management when validators are added or removed.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::String;

use sp_avn_common::eth::EthereumId;

use frame_support::{
    dispatch::DispatchResult,
    ensure,
    pallet_prelude::StorageVersion,
    traits::{Currency, Get},
    transactional,
};
use frame_system::{
    offchain::{CreateInherent, CreateTransactionBase},
    pallet_prelude::BlockNumberFor,
    RawOrigin,
};
use pallet_session::{self as session, Config as SessionConfig};
use sp_runtime::{
    scale_info::TypeInfo,
    traits::{Convert, Member},
    DispatchError,
};
use sp_std::prelude::*;

use codec::{Decode, Encode, MaxEncodedLen};
use pallet_avn::{
    self as avn, AccountToBytesConverter, BridgeInterfaceNotification, EthereumPublicKeyChecker,
    NewSessionHandler, ProcessedEventsChecker, ValidatorRegistrationNotifier,
    MAX_VALIDATOR_ACCOUNTS,
};

use sp_avn_common::{
    bounds::MaximumValidatorsBound, eth_key_actions::decompress_eth_public_key,
    event_types::Validator, BridgeContractMethod, IngressCounter,
};

#[cfg(any(test, feature = "runtime-benchmarks"))]
use sp_avn_common::eth_key_actions::compress_eth_public_key;
use sp_core::{bounded::BoundedVec, ecdsa};

use pallet_parachain_staking::ValidatorRegistration;

pub use pallet_parachain_staking::{self as parachain_staking, BalanceOf, PositiveImbalanceOf};

use pallet_avn::BridgeInterface;

pub use pallet::*;

const PALLET_ID: &'static [u8; 14] = b"author_manager";

pub mod migration;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{assert_ok, pallet_prelude::*};
    use frame_system::pallet_prelude::*;

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(PhantomData<T>);

    #[pallet::config]
    pub trait Config:
        frame_system::Config
        + session::Config
        + avn::Config
        + parachain_staking::Config
        + pallet_session::historical::Config
        + CreateTransactionBase<Call<Self>>
        + CreateInherent<Call<Self>>
    {
        /// Overarching event type
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// A trait that allows to subscribe to notifications triggered when ethereum event
        /// processes an event
        type ProcessedEventsChecker: ProcessedEventsChecker;
        /// A period (in block number) where validators are allowed to vote
        type VotingPeriod: Get<BlockNumberFor<Self>>;
        /// A trait that allows converting between accountIds <-> public keys
        type AccountToBytesConvert: AccountToBytesConverter<Self::AccountId>;
        /// A trait that allows extra work to be done during validator registration
        type ValidatorRegistrationNotifier: ValidatorRegistrationNotifier<
            <Self as session::Config>::ValidatorId,
        >;

        /// Weight information for the extrinsics in this pallet.
        type WeightInfo: WeightInfo;

        type BridgeInterface: avn::BridgeInterface;
        /// Minimum number of validators that must remain active
        #[pallet::constant]
        type MinimumValidatorCount: Get<u32>;
    }

    #[pallet::error]
    pub enum Error<T> {
        NoTier1EventForAddingValidator,
        NoTier1EventForRemovingValidator,
        NoValidators,
        ValidatorAlreadyExists,
        InvalidIngressCounter,
        MinimumValidatorsReached,
        ErrorEndingVotingPeriod,
        VotingSessionIsNotValid,
        ErrorSubmitCandidateTxnToTier1,
        ErrorCalculatingPrimaryValidator,
        ErrorGeneratingEthDescription,
        ValidatorsActionDataNotFound,
        RemovalAlreadyRequested,
        ErrorConvertingAccountIdToValidatorId,
        ValidatorNotFound,
        InvalidPublicKey,
        /// The ethereum public key of this validator alredy exists
        ValidatorEthKeyAlreadyExists,
        ErrorRemovingAccountFromCollators,
        MaximumValidatorsReached,
        /// Account is already a candidate
        AlreadyCandidate,
        /// Deposit is below minimum required stake
        DepositBelowMinimum,
        /// Account has insufficient balance for deposit
        InsufficientBalance,
        /// Validator session keys not found - account must be registered in session pallet
        CandidateSessionKeysNotFound,
        /// A validator action is already in progress
        ValidatorActionAlreadyInProgress,
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ValidatorRegistered {
            validator_id: T::AccountId,
            eth_key: ecdsa::Public,
        },
        ValidatorDeregistered {
            validator_id: T::AccountId,
        },
        ValidatorActivationStarted {
            validator_id: T::AccountId,
        },
        ValidatorActionConfirmed {
            action_id: ActionId<T::AccountId>,
        },
        /// Validator action was successfully sent to Ethereum via the bridge
        ValidatorActionPublished {
            validator_id: T::AccountId,
            action_type: ValidatorsActionType,
            tx_id: u32,
        },
        /// Failed to send validator action to Ethereum bridge
        FailedToPublishValidatorAction {
            validator_id: T::AccountId,
            action_type: ValidatorsActionType,
            reason: Vec<u8>,
        },
        /// Validator action transaction confirmed on Ethereum
        ValidatorActionConfirmedOnEthereum {
            validator_id: T::AccountId,
            action_type: ValidatorsActionType,
            tx_id: u32,
        },
        /// Validator action transaction failed on Ethereum
        ValidatorActionFailedOnEthereum {
            validator_id: T::AccountId,
            action_type: ValidatorsActionType,
            tx_id: u32,
        },
        ValidatorRegistrationFailed {
            validator_id: T::AccountId,
            reason: Vec<u8>,
        },
        ValidatorDeregistrationFailed {
            validator_id: T::AccountId,
            reason: Vec<u8>,
        },
    }

    #[pallet::storage]
    #[pallet::getter(fn validator_account_ids)]
    pub type ValidatorAccountIds<T: Config> =
        StorageValue<_, BoundedVec<T::AccountId, MaximumValidatorsBound>>;

    #[pallet::storage]
    pub type ValidatorActions<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        IngressCounter,
        ValidatorsActionData,
        OptionQuery,
        GetDefault,
    >;

    #[pallet::storage]
    #[pallet::getter(fn get_validator_by_eth_public_key)]
    pub type EthereumPublicKeys<T: Config> =
        StorageMap<_, Blake2_128Concat, ecdsa::Public, T::AccountId>;

    /// Reverse mapping from account_id to ethereum public key for O(1) lookup
    #[pallet::storage]
    pub type AccountIdToEthereumKeys<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, ecdsa::Public>;

    #[pallet::storage]
    #[pallet::getter(fn get_ingress_counter)]
    pub type TotalIngresses<T: Config> = StorageValue<_, IngressCounter, ValueQuery>;

    /// Stores deposit amounts for pending registrations
    #[pallet::storage]
    pub type PendingRegistrationDeposits<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, BalanceOf<T>>;

    #[pallet::storage]
    pub type TransactionIdToAction<T: Config> =
        StorageMap<_, Blake2_128Concat, EthereumId, (T::AccountId, IngressCounter), OptionQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub validators: Vec<(T::AccountId, ecdsa::Public)>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self { validators: Vec::<(T::AccountId, ecdsa::Public)>::new() }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            log::debug!(
                "Validators Manager Genesis build entrypoint - total validators: {}",
                self.validators.len()
            );
            for (validator_account_id, eth_public_key) in &self.validators {
                assert_ok!(<ValidatorAccountIds<T>>::try_append(validator_account_id));
                <EthereumPublicKeys<T>>::insert(eth_public_key, validator_account_id);
                <AccountIdToEthereumKeys<T>>::insert(validator_account_id, eth_public_key);
            }

            // Set storage version
            STORAGE_VERSION.put::<Pallet<T>>();
            log::debug!(
                "Validators manager storage chain/current storage version: {:?} / {:?}",
                Pallet::<T>::on_chain_storage_version(),
                Pallet::<T>::in_code_storage_version(),
            );
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Sudo function to add a collator.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::add_collator())]
        #[transactional]
        pub fn add_collator(
            origin: OriginFor<T>,
            collator_account_id: T::AccountId,
            collator_eth_public_key: ecdsa::Public,
            deposit: Option<BalanceOf<T>>,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            // Validate the registration request
            Self::validate_validator_registration_request(
                &collator_account_id,
                &collator_eth_public_key,
            )?;

            let bond =
                deposit.unwrap_or_else(|| parachain_staking::Pallet::<T>::min_collator_stake());

            Self::validate_staking_preconditions(&collator_account_id, bond)?;

            // Store deposit for use in callback
            PendingRegistrationDeposits::<T>::insert(&collator_account_id, bond);

            // Send to T1 - actual registration happens in callback
            Self::send_validator_registration_to_t1(
                &collator_account_id,
                &collator_eth_public_key,
            )?;

            Ok(().into())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::remove_validator(MAX_VALIDATOR_ACCOUNTS))]
        #[transactional]
        pub fn remove_validator(
            origin: OriginFor<T>,
            collator_account_id: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            let _ = ensure_root(origin)?;

            // Validate the deregistration request
            Self::validate_validator_deregistration_request(&collator_account_id)?;

            // Send to T1 - actual deregistration happens in callback
            Self::send_validator_deregistration_to_t1(&collator_account_id)?;

            Ok(().into())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::rotate_validator_ethereum_key())]
        #[transactional]
        pub fn rotate_validator_ethereum_key(
            origin: OriginFor<T>,
            author_account_id: T::AccountId,
            author_old_eth_public_key: ecdsa::Public,
            author_new_eth_public_key: ecdsa::Public,
        ) -> DispatchResult {
            let _ = ensure_root(origin)?;

            ensure!(
                !<EthereumPublicKeys<T>>::contains_key(&author_new_eth_public_key),
                Error::<T>::ValidatorEthKeyAlreadyExists
            );
            ensure!(
                author_old_eth_public_key != author_new_eth_public_key,
                Error::<T>::ValidatorEthKeyAlreadyExists
            );

            let author_id = EthereumPublicKeys::<T>::take(&author_old_eth_public_key)
                .ok_or(Error::<T>::ValidatorNotFound)?;
            ensure!(author_id == author_account_id, Error::<T>::ValidatorNotFound);

            let cloned_author_id = author_id.clone();
            EthereumPublicKeys::<T>::insert(author_new_eth_public_key, author_id);
            <AccountIdToEthereumKeys<T>>::insert(&cloned_author_id, author_new_eth_public_key);
            Ok(())
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, Debug, TypeInfo, MaxEncodedLen)]
pub enum ValidatorsActionStatus {
    /// Action enters this state immediately upon a request from the validator.
    AwaitingConfirmation,
    /// The action has completed
    Confirmed,
    /// The request has been actioned (ex: sent to Ethereum and executed successfully)
    Actioned,
    /// Default value, status is unknown
    None,
}

#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, Debug, TypeInfo, MaxEncodedLen)]
pub enum ValidatorsActionType {
    /// Validator has asked to leave voluntarily
    Resignation,
    /// Validator is being forced to leave due to a malicious behaviour
    Slashed,
    /// Validator activates himself after he joins an active session
    Activation,
    /// Default value
    Unknown,
    /// Validator registration pending T1 confirmation
    Registration,
}

impl ValidatorsActionType {
    fn is_deregistration(&self) -> bool {
        match self {
            ValidatorsActionType::Resignation => true,
            _ => false,
        }
    }

    fn is_activation(&self) -> bool {
        match self {
            ValidatorsActionType::Activation => true,
            _ => false,
        }
    }

    fn is_registration(&self) -> bool {
        match self {
            ValidatorsActionType::Registration => true,
            _ => false,
        }
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, TypeInfo, MaxEncodedLen)]
pub struct ValidatorsActionData {
    pub status: ValidatorsActionStatus,
    pub eth_transaction_id: EthereumId,
    pub action_type: ValidatorsActionType,
}

impl ValidatorsActionData {
    fn new(
        status: ValidatorsActionStatus,
        eth_transaction_id: EthereumId,
        action_type: ValidatorsActionType,
    ) -> Self {
        ValidatorsActionData { status, eth_transaction_id, action_type }
    }
}

#[cfg(test)]
#[path = "tests/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/mock.rs"]
mod mock;

mod benchmarking;

pub mod default_weights;
pub use default_weights::WeightInfo;

pub const STORAGE_VERSION: StorageVersion = StorageVersion::new(2);

pub type AVN<T> = avn::Pallet<T>;

impl<T: Config> Pallet<T> {
    /// Helper function to compress eth public key (exposed for testing and benchmarking)
    #[cfg(any(test, feature = "runtime-benchmarks"))]
    pub fn compress_eth_public_key(full_public_key: sp_core::H512) -> ecdsa::Public {
        compress_eth_public_key(full_public_key)
    }

    fn remove_ethereum_public_key_if_required(validator_id: &T::AccountId) {
        if let Some(public_key_to_remove) = <AccountIdToEthereumKeys<T>>::get(validator_id) {
            <EthereumPublicKeys<T>>::remove(public_key_to_remove);
            <AccountIdToEthereumKeys<T>>::remove(validator_id);
        }
    }

    fn validate_validator_registration_request(
        account_id: &T::AccountId,
        eth_public_key: &ecdsa::Public,
    ) -> DispatchResult {
        let validator_account_ids =
            Self::validator_account_ids().ok_or(Error::<T>::NoValidators)?;
        ensure!(!validator_account_ids.is_empty(), Error::<T>::NoValidators);

        ensure!(!validator_account_ids.contains(account_id), Error::<T>::ValidatorAlreadyExists);

        ensure!(
            !<EthereumPublicKeys<T>>::contains_key(eth_public_key),
            Error::<T>::ValidatorEthKeyAlreadyExists
        );

        ensure!(
            validator_account_ids.len() <
                (<MaximumValidatorsBound as sp_core::TypedGet>::get() as usize),
            Error::<T>::MaximumValidatorsReached
        );

        ensure!(
            <T as parachain_staking::Config>::CollatorSessionRegistration::is_registered(
                account_id
            ),
            Error::<T>::CandidateSessionKeysNotFound
        );

        // Disallow starting a registration if any validator action is already in progress
        ensure!(!Self::has_any_active_action(), Error::<T>::ValidatorActionAlreadyInProgress);

        Ok(())
    }

    fn validate_validator_deregistration_request(account_id: &T::AccountId) -> DispatchResult {
        let validator_account_ids =
            Self::validator_account_ids().ok_or(Error::<T>::NoValidators)?;

        ensure!(
            validator_account_ids.len() > T::MinimumValidatorCount::get() as usize,
            Error::<T>::MinimumValidatorsReached
        );

        ensure!(validator_account_ids.contains(account_id), Error::<T>::ValidatorNotFound);

        // Check if this validator has any active actions (registration or deregistration)
        ensure!(!Self::has_any_active_action(), Error::<T>::ValidatorActionAlreadyInProgress);

        Ok(())
    }

    /// Check if any validator has any active action (registration, activation, or deregistration)
    fn has_any_active_action() -> bool {
        <ValidatorActions<T>>::iter().any(|(_, _, validators_action_data)| {
            Self::action_state_is_active(validators_action_data.status)
        })
    }

    /// Check if an action status indicates the action is still active
    fn action_state_is_active(status: ValidatorsActionStatus) -> bool {
        matches!(
            status,
            ValidatorsActionStatus::AwaitingConfirmation | ValidatorsActionStatus::Confirmed
        )
    }

    fn validate_staking_preconditions(
        account_id: &T::AccountId,
        deposit: BalanceOf<T>,
    ) -> DispatchResult {
        // Check 1: Not already a candidate
        ensure!(
            parachain_staking::Pallet::<T>::candidate_info(account_id).is_none(),
            Error::<T>::AlreadyCandidate
        );

        // Check 2: Deposit meets minimum
        let min_stake = parachain_staking::Pallet::<T>::min_collator_stake();
        ensure!(deposit >= min_stake, Error::<T>::DepositBelowMinimum);

        // Check 3: Account has sufficient free balance
        type CurrencyOf<T> = <T as parachain_staking::Config>::Currency;
        let free_balance = CurrencyOf::<T>::free_balance(account_id);

        ensure!(free_balance >= deposit, Error::<T>::InsufficientBalance);

        Ok(())
    }

    fn exit_from_staking(action_account_id: T::AccountId) -> Result<(), ()> {
        // Cleanup staking state for the collator we are removing
        let staking_state = parachain_staking::Pallet::<T>::candidate_info(&action_account_id);
        if staking_state.is_none() {
            log::error!(
                "Unable to find staking candidate info for collator: {:?}",
                action_account_id
            );
            return Err(())
        }

        let staking_state = staking_state.expect("Checked for none already");

        let result = parachain_staking::Pallet::<T>::execute_leave_candidates(
            <T as frame_system::Config>::RuntimeOrigin::from(RawOrigin::Signed(
                action_account_id.clone(),
            )),
            action_account_id.clone(),
            staking_state.nomination_count,
        );

        if result.is_err() {
            log::error!(
                "Error removing staking data for collator {:?}: {:?}",
                action_account_id,
                result
            );
            return Err(())
        }

        Ok(())
    }

    fn confirm_action(action_account_id: T::AccountId, ingress_counter: IngressCounter) {
        <ValidatorActions<T>>::mutate(
            &action_account_id,
            ingress_counter,
            |validators_action_data_maybe| {
                if let Some(validators_action_data) = validators_action_data_maybe {
                    validators_action_data.status = ValidatorsActionStatus::Confirmed
                }
            },
        );

        let action_id = ActionId::new(action_account_id.clone(), ingress_counter);
        Self::deposit_event(Event::<T>::ValidatorActionConfirmed { action_id });
    }

    /// Send validator registration request to T1
    fn send_validator_registration_to_t1(
        validator_account_id: &T::AccountId,
        validator_eth_public_key: &ecdsa::Public,
    ) -> Result<EthereumId, DispatchError> {
        // Add eth key mapping immediately (before T1 confirmation)
        <EthereumPublicKeys<T>>::insert(validator_eth_public_key, validator_account_id);
        <AccountIdToEthereumKeys<T>>::insert(validator_account_id, validator_eth_public_key);

        // Prepare data for T1
        let decompressed_eth_public_key = decompress_eth_public_key(*validator_eth_public_key)
            .map_err(|_| Error::<T>::InvalidPublicKey)?;

        let validator_id_bytes =
            <T as pallet::Config>::AccountToBytesConvert::into_bytes(validator_account_id);

        let function_name = BridgeContractMethod::AddAuthor.name_as_bytes();
        let params = vec![
            (b"bytes".to_vec(), decompressed_eth_public_key.to_fixed_bytes().to_vec()),
            (b"bytes32".to_vec(), validator_id_bytes.to_vec()),
        ];

        let tx_id = <T as pallet::Config>::BridgeInterface::publish(
            function_name,
            &params,
            PALLET_ID.to_vec(),
        )
        .map_err(|_| {
            Self::deposit_event(Event::<T>::FailedToPublishValidatorAction {
                validator_id: validator_account_id.clone(),
                action_type: ValidatorsActionType::Registration,
                reason: b"Failed to submit transaction to Ethereum bridge".to_vec(),
            });
            Error::<T>::ErrorSubmitCandidateTxnToTier1
        })?;

        // Now create ValidatorActions entry with the actual tx_id (single insert, no mutation)
        let ingress_counter = Self::get_ingress_counter() + 1;
        TotalIngresses::<T>::put(ingress_counter);

        <ValidatorActions<T>>::insert(
            validator_account_id,
            ingress_counter,
            ValidatorsActionData::new(
                ValidatorsActionStatus::AwaitingConfirmation,
                tx_id,
                ValidatorsActionType::Registration,
            ),
        );

        TransactionIdToAction::<T>::insert(tx_id, (validator_account_id.clone(), ingress_counter));

        Self::deposit_event(Event::<T>::ValidatorActionPublished {
            validator_id: validator_account_id.clone(),
            action_type: ValidatorsActionType::Registration,
            tx_id,
        });

        Ok(tx_id)
    }

    /// Send validator deregistration request to T1
    fn send_validator_deregistration_to_t1(
        validator_account_id: &T::AccountId,
    ) -> Result<EthereumId, DispatchError> {
        // Prepare data for T1
        let eth_public_key = <AccountIdToEthereumKeys<T>>::get(validator_account_id)
            .ok_or(Error::<T>::ValidatorNotFound)?;

        let decompressed_eth_public_key =
            decompress_eth_public_key(eth_public_key).map_err(|_| Error::<T>::InvalidPublicKey)?;

        let validator_id_bytes =
            <T as pallet::Config>::AccountToBytesConvert::into_bytes(validator_account_id);

        let function_name = BridgeContractMethod::RemoveAuthor.name_as_bytes();
        let params = vec![
            (b"bytes32".to_vec(), validator_id_bytes.to_vec()),
            (b"bytes".to_vec(), decompressed_eth_public_key.to_fixed_bytes().to_vec()),
        ];

        // Send to T1 and get tx_id FIRST
        let tx_id = <T as pallet::Config>::BridgeInterface::publish(
            function_name,
            &params,
            PALLET_ID.to_vec(),
        )
        .map_err(|_| {
            Self::deposit_event(Event::<T>::FailedToPublishValidatorAction {
                validator_id: validator_account_id.clone(),
                action_type: ValidatorsActionType::Resignation,
                reason: b"Failed to submit transaction to Ethereum bridge".to_vec(),
            });
            Error::<T>::ErrorSubmitCandidateTxnToTier1
        })?;

        // Now create ValidatorActions entry with the actual tx_id (single insert, no mutation)
        let ingress_counter = Self::get_ingress_counter() + 1;
        TotalIngresses::<T>::put(ingress_counter);

        <ValidatorActions<T>>::insert(
            validator_account_id,
            ingress_counter,
            ValidatorsActionData::new(
                ValidatorsActionStatus::AwaitingConfirmation,
                tx_id,
                ValidatorsActionType::Resignation,
            ),
        );

        TransactionIdToAction::<T>::insert(tx_id, (validator_account_id.clone(), ingress_counter));

        Self::deposit_event(Event::<T>::ValidatorActionPublished {
            validator_id: validator_account_id.clone(),
            action_type: ValidatorsActionType::Resignation,
            tx_id,
        });

        Ok(tx_id)
    }

    fn add_validator_to_staking(account_id: &T::AccountId) -> DispatchResult {
        let deposit = PendingRegistrationDeposits::<T>::take(&account_id)
            .unwrap_or_else(|| parachain_staking::Pallet::<T>::min_collator_stake());

        let candidate_count = parachain_staking::Pallet::<T>::candidate_pool().0.len() as u32;

        match parachain_staking::Pallet::<T>::join_candidates(
            <T as frame_system::Config>::RuntimeOrigin::from(RawOrigin::Signed(account_id.clone())),
            deposit,
            candidate_count,
        ) {
            Ok(_) => return Ok(()),
            Err(e) => return Err(e.error),
        }
    }

    fn complete_validator_registration(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
    ) -> DispatchResult {
        // Add to active validators list
        match <ValidatorAccountIds<T>>::try_append(account_id.clone()) {
            Ok(_) => {},
            Err(_) => {
                // Cleanup on failure (no deposit to clean as it's already been used for staking)
                Self::handle_registration_failure(
                    &account_id,
                    ingress_counter,
                    "Failed to append validator to active validators list",
                    false,
                );
                return Err(Error::<T>::MaximumValidatorsReached.into())
            },
        }

        // Add to staking candidates
        match Self::add_validator_to_staking(&account_id) {
            Ok(_) => {},
            Err(e) => {
                Self::handle_registration_failure(
                    &account_id,
                    ingress_counter,
                    "Failed to add validator to staking candidates",
                    false,
                );
                return Err(e)
            },
        }

        // Notify validator registration
        let new_validator_id = <T as SessionConfig>::ValidatorIdOf::convert(account_id.clone())
            .ok_or(Error::<T>::ErrorConvertingAccountIdToValidatorId)?;

        T::ValidatorRegistrationNotifier::on_validator_registration(&new_validator_id);

        // Update ValidatorActions for activation process
        <ValidatorActions<T>>::mutate(
            &account_id,
            ingress_counter,
            |validators_action_data_maybe| {
                if let Some(validators_action_data) = validators_action_data_maybe {
                    validators_action_data.action_type = ValidatorsActionType::Activation;
                    validators_action_data.status = ValidatorsActionStatus::Actioned;
                }
            },
        );

        Self::deposit_event(Event::<T>::ValidatorActivationStarted {
            validator_id: account_id.clone(),
        });

        Ok(())
    }

    fn complete_validator_deregistration(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
    ) -> DispatchResult {
        // Initiate the staking exit
        // This will cause ParachainStaking to remove them from next session
        let candidate_count = parachain_staking::Pallet::<T>::candidate_pool().0.len() as u32;
        match parachain_staking::Pallet::<T>::schedule_leave_candidates(
            <T as frame_system::Config>::RuntimeOrigin::from(RawOrigin::Signed(account_id.clone())),
            candidate_count,
        ) {
            Ok(_) => {},
            Err(e) => {
                Self::handle_deregistration_failure(
                    &account_id,
                    ingress_counter,
                    "Failed to schedule leave candidates",
                );
                return Err(e.error)
            },
        }

        // Immediately clean up validator manager storage
        // Remove from active validators list
        ValidatorAccountIds::<T>::mutate(|maybe_validators| {
            if let Some(validators) = maybe_validators {
                validators.retain(|v| v != account_id);
            }
        });

        Self::remove_ethereum_public_key_if_required(&account_id);

        <ValidatorActions<T>>::mutate(
            &account_id,
            ingress_counter,
            |validators_action_data_maybe| {
                if let Some(validators_action_data) = validators_action_data_maybe {
                    validators_action_data.status = ValidatorsActionStatus::Actioned;
                }
            },
        );

        Self::deposit_event(Event::<T>::ValidatorDeregistered { validator_id: account_id.clone() });

        Ok(())
    }

    fn cleanup_registration_storage(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
        cleanup_deposit: bool,
    ) {
        // Remove the eth key mapping if it exists
        if let Some(eth_key) = <AccountIdToEthereumKeys<T>>::get(&account_id) {
            <EthereumPublicKeys<T>>::remove(eth_key);
            <AccountIdToEthereumKeys<T>>::remove(&account_id);
        }

        // Cleanup stored deposit if requested
        if cleanup_deposit {
            PendingRegistrationDeposits::<T>::remove(&account_id);
        }

        // Remove validator action entry
        <ValidatorActions<T>>::remove(&account_id, ingress_counter);
    }

    fn handle_registration_failure(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
        reason: &str,
        cleanup_deposit: bool,
    ) {
        log::error!("Validator registration failed for {:?}: {}", account_id, reason);

        Self::cleanup_registration_storage(&account_id, ingress_counter, cleanup_deposit);

        Self::deposit_event(Event::<T>::ValidatorRegistrationFailed {
            validator_id: account_id.clone(),
            reason: reason.as_bytes().to_vec(),
        });
    }

    fn handle_deregistration_failure(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
        reason: &str,
    ) {
        log::error!("Validator deregistration failed for {:?}: {}", account_id, reason);

        <ValidatorActions<T>>::remove(&account_id, ingress_counter);

        Self::deposit_event(Event::<T>::ValidatorDeregistrationFailed {
            validator_id: account_id.clone(),
            reason: reason.as_bytes().to_vec(),
        });
    }

    /// Rollback and cleanup state when T1 operation fails
    fn rollback_failed_validator_action(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
        action_type: ValidatorsActionType,
        tx_id: EthereumId,
    ) {
        // Type-specific cleanup
        if action_type.is_registration() {
            Self::cleanup_registration_storage(&account_id, ingress_counter, true);
        } else {
            // For non-registration actions, just remove the validator action entry
            <ValidatorActions<T>>::remove(&account_id, ingress_counter);
        }

        Self::deposit_event(Event::<T>::ValidatorActionFailedOnEthereum {
            validator_id: account_id.clone(),
            action_type,
            tx_id,
        });
    }
}

impl<T: Config> BridgeInterfaceNotification for Pallet<T> {
    fn process_result(tx_id: u32, caller_id: Vec<u8>, succeeded: bool) -> DispatchResult {
        if caller_id != PALLET_ID.to_vec() {
            return Ok(())
        }

        let Some((account_id, ingress_counter)) = TransactionIdToAction::<T>::take(tx_id) else {
            return Ok(());
        };
        let action_data = <ValidatorActions<T>>::get(&account_id, ingress_counter)
            .ok_or(Error::<T>::ValidatorsActionDataNotFound)?;
        let action_type = action_data.action_type;

        if !succeeded {
            Self::rollback_failed_validator_action(
                &account_id,
                ingress_counter,
                action_type,
                tx_id,
            );
            return Ok(())
        }

        // T1 succeeded - emit confirmation event and complete the operation
        Self::deposit_event(Event::<T>::ValidatorActionConfirmedOnEthereum {
            validator_id: account_id.clone(),
            action_type,
            tx_id,
        });

        // Complete the operation based on action type
        if action_type.is_registration() {
            Self::complete_validator_registration(&account_id, ingress_counter)?;
        } else if action_type.is_deregistration() {
            Self::complete_validator_deregistration(&account_id, ingress_counter)?;
        }

        Ok(())
    }
}

#[derive(Encode, Decode, Default, Clone, Copy, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct ActionId<AccountId: Member> {
    pub action_account_id: AccountId,
    pub ingress_counter: IngressCounter,
}

impl<AccountId: Member + Encode> ActionId<AccountId> {
    fn new(action_account_id: AccountId, ingress_counter: IngressCounter) -> Self {
        ActionId::<AccountId> { action_account_id, ingress_counter }
    }
}

impl<T: Config> NewSessionHandler<T::AuthorityId, T::AccountId> for Pallet<T> {
    fn on_genesis_session(_validators: &Vec<Validator<T::AuthorityId, T::AccountId>>) {
        log::trace!("Validators manager on_genesis_session");
    }

    fn on_new_session(
        _changed: bool,
        active_validators: &Vec<Validator<T::AuthorityId, T::AccountId>>,
        disabled_validators: &Vec<T::AccountId>,
    ) {
        log::trace!("Validators manager on_new_session");
        if <ValidatorActions<T>>::iter().count() > 0 {
            for (action_account_id, ingress_counter, validators_action_data) in
                <ValidatorActions<T>>::iter()
            {
                if validators_action_data.status == ValidatorsActionStatus::Actioned &&
                    validators_action_data.action_type.is_deregistration()
                {
                    // Check if account is still part of the session
                    let is_account_part_of_session =
                        active_validators.iter().any(|v| v.account_id == action_account_id) ||
                            disabled_validators.iter().any(|v| *v == action_account_id);

                    if !is_account_part_of_session {
                        if let Ok(()) = Self::exit_from_staking(action_account_id.clone()) {
                            Self::confirm_action(action_account_id, ingress_counter);
                        }
                    }
                } else if validators_action_data.status == ValidatorsActionStatus::Actioned &&
                    validators_action_data.action_type.is_activation()
                {
                    // check if active_validators contains action_account_id
                    let is_now_active =
                        active_validators.iter().any(|v| v.account_id == action_account_id);
                    if is_now_active {
                        Self::confirm_action(action_account_id.clone(), ingress_counter);

                        // Get eth key for event (we know it exists because we added it earlier)
                        let eth_key = match <AccountIdToEthereumKeys<T>>::get(&action_account_id) {
                            Some(key) => key,
                            None => {
                                log::error!(
                                    "Ethereum pub key not found. Validator: {:?}",
                                    action_account_id
                                );
                                return
                            },
                        };

                        // Emit success event
                        Self::deposit_event(Event::<T>::ValidatorRegistered {
                            validator_id: action_account_id.clone(),
                            eth_key,
                        });
                    }
                } else if validators_action_data.status == ValidatorsActionStatus::Confirmed {
                    // Remove completed actions to prevent storage bloat
                    <ValidatorActions<T>>::remove(&action_account_id, ingress_counter);
                }
            }
        }
    }
}

/// We use accountId for validatorId for simplicity
pub struct ValidatorOf<T>(sp_std::marker::PhantomData<T>);

impl<T: Config> Convert<T::AccountId, Option<T::AccountId>> for ValidatorOf<T> {
    fn convert(account: T::AccountId) -> Option<T::AccountId> {
        Some(account)
    }
}

impl Default for ValidatorsActionStatus {
    fn default() -> Self {
        ValidatorsActionStatus::None
    }
}

impl Default for ValidatorsActionType {
    fn default() -> Self {
        ValidatorsActionType::Unknown
    }
}

impl<T: Config> EthereumPublicKeyChecker<T::AccountId> for Pallet<T> {
    fn get_validator_for_eth_public_key(eth_public_key: &ecdsa::Public) -> Option<T::AccountId> {
        Self::get_validator_by_eth_public_key(eth_public_key)
    }
}
