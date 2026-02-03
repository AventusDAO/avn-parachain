//! # Authors manager Pallet
//!
//! This pallet provides functionality to add/remove authors.
//!
//! The pallet is based on the Substrate session pallet and implements related traits for session
//! management when authors are added or removed.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::String;

use sp_avn_common::{eth::EthereumId, BridgeContractMethod};

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::{dispatch::DispatchResult, ensure, transactional};
use pallet_session::{self as session, Config as SessionConfig};
use sp_core::{bounded::BoundedVec, ecdsa, Get};
use sp_runtime::{
    scale_info::TypeInfo,
    traits::{Convert, Member},
    DispatchError,
};
use sp_std::prelude::*;

use pallet_avn::{
    self as avn, AccountToBytesConverter, BridgeInterface, BridgeInterfaceNotification,
    NewSessionHandler, ValidatorRegistrationNotifier as AuthorRegistrationNotifier,
};
pub use sp_avn_common::{
    bounds::MaximumValidatorsBound as MaximumAuthorsBound,
    eth_key_actions::decompress_eth_public_key, event_types::Validator as Author, IngressCounter,
};

pub use pallet::*;

const PALLET_ID: &'static [u8; 14] = b"author_manager";

pub mod default_weights;
pub use default_weights::WeightInfo;

pub type AVN<T> = avn::Pallet<T>;
pub mod migration;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{assert_ok, pallet_prelude::*};
    use frame_system::{offchain::SendTransactionTypes, pallet_prelude::*};
    pub use pallet_avn::{EthereumPublicKeyChecker, MAX_VALIDATOR_ACCOUNTS as MAX_AUTHOR_ACCOUNTS};
    use sp_core::ecdsa;

    #[pallet::pallet]
    #[pallet::storage_version(migration::STORAGE_VERSION)]
    pub struct Pallet<T>(PhantomData<T>);

    #[pallet::config]
    pub trait Config:
        SendTransactionTypes<Call<Self>>
        + frame_system::Config
        + session::Config
        + pallet_avn::Config
        + pallet_session::historical::Config
    {
        /// Overarching event type
        type RuntimeEvent: From<Event<Self>>
            + Into<<Self as frame_system::Config>::RuntimeEvent>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        type AccountToBytesConvert: AccountToBytesConverter<Self::AccountId>;

        type ValidatorRegistrationNotifier: AuthorRegistrationNotifier<
            <Self as session::Config>::ValidatorId,
        >;

        type WeightInfo: WeightInfo;

        type BridgeInterface: BridgeInterface;
        /// Minimum number of authors that must remain active
        #[pallet::constant]
        type MinimumAuthorsCount: Get<u32>;
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new author has been registered. \[author_id, eth_key\]
        AuthorRegistered {
            author_id: T::AccountId,
            eth_key: ecdsa::Public,
        },
        /// An author has been deregistered. \[author_id\]
        AuthorDeregistered {
            author_id: T::AccountId,
        },
        /// An author has activation has started. \[author_id\]
        AuthorActivationStarted {
            author_id: T::AccountId,
        },
        /// An author action has been confirmed. \[action_id\]
        AuthorActionConfirmed {
            action_id: ActionId<T::AccountId>,
        },
        /// Validator action was successfully sent to Ethereum via the bridge
        AuthorActionPublished {
            author_id: T::AccountId,
            action_type: AuthorsActionType,
            tx_id: u32,
        },
        /// Failed to send author action to Ethereum bridge
        FailedToPublishAuthorAction {
            author_id: T::AccountId,
            action_type: AuthorsActionType,
            reason: Vec<u8>,
        },
        /// Author action transaction confirmed on Ethereum
        AuthorActionConfirmedOnEthereum {
            author_id: T::AccountId,
            action_type: AuthorsActionType,
            tx_id: u32,
        },
        /// Author action transaction failed on Ethereum
        AuthorActionFailedOnEthereum {
            author_id: T::AccountId,
            action_type: AuthorsActionType,
            tx_id: u32,
        },
        AuthorRegistrationFailed {
            author_id: T::AccountId,
            reason: Vec<u8>,
        },
        AuthorDeregistrationFailed {
            author_id: T::AccountId,
            reason: Vec<u8>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// There is no Tier1 event for adding authors
        NoTier1EventForAddingAuthor,
        /// There is no Tier1 event for removing authors
        NoTier1EventForRemovingAuthor,
        /// There are no authors in the chain
        NoAuthors,
        /// Author already exists
        AuthorAlreadyExists,
        /// The ingress counter is not valid
        InvalidIngressCounter,
        /// The minimum number of authors has been reached
        MinimumAuthorsReached,
        /// There was an nerror ending the voting period
        ErrorEndingVotingPeriod,
        /// The voting session is not valid
        VotingSessionIsNotValid,
        /// There was an error submitting transaction to Tier1
        ErrorSubmitCandidateTxnToTier1,
        /// There was an error calculating the primary author
        ErrorCalculatingPrimaryAuthor,
        /// Not action data found for author
        AuthorsActionDataNotFound,
        /// Removal already requested
        RemovalAlreadyRequested,
        /// A validator action is already in progress
        ValidatorActionAlreadyInProgress,
        /// There was an error converting accountId to AuthorId
        ErrorConvertingAccountIdToAuthorId,
        /// Slashed author is not found
        SlashedAuthorIsNotFound,
        /// Author not found
        AuthorNotFound,
        /// Invalid public key
        InvalidPublicKey,
        /// The ethereum public key of this author already exists
        AuthorEthKeyAlreadyExists,
        /// There was an error removing account from authors
        ErrorRemovingAccountFromAuthors,
        /// The maximum number of authors has been reached
        MaximumAuthorsReached,
        /// Transaction not found
        TransactionNotFound,
        /// Invalid action status for transaction
        InvalidActionStatus,
        /// Author session keys are not set
        AuthorSessionKeysNotSet,
    }

    #[pallet::storage]
    #[pallet::getter(fn author_account_ids)]
    pub type AuthorAccountIds<T: Config> =
        StorageValue<_, BoundedVec<T::AccountId, MaximumAuthorsBound>>;

    #[pallet::storage]
    pub type AuthorActions<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        IngressCounter,
        AuthorsActionData,
        OptionQuery,
        GetDefault,
    >;

    #[pallet::storage]
    #[pallet::getter(fn get_author_by_eth_public_key)]
    pub type EthereumPublicKeys<T: Config> =
        StorageMap<_, Blake2_128Concat, ecdsa::Public, T::AccountId>;

    #[pallet::storage]
    #[pallet::getter(fn get_ingress_counter)]
    pub type TotalIngresses<T: Config> = StorageValue<_, IngressCounter, ValueQuery>;

    /// Storage map providing a reverse mapping from `AccountId` to Ethereum public key.
    /// This enables O(1) lookup of the Ethereum public key associated with a given account.
    /// Used for efficient author identification and key management.
    #[pallet::storage]
    pub type AccountIdToEthereumKeys<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, ecdsa::Public>;

    #[pallet::storage]
    pub type TransactionToAction<T: Config> =
        StorageMap<_, Blake2_128Concat, EthereumId, (T::AccountId, IngressCounter), OptionQuery>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub authors: Vec<(T::AccountId, ecdsa::Public)>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self { authors: Vec::<(T::AccountId, ecdsa::Public)>::new() }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            log::debug!(
                "Authors Manager Genesis build entrypoint - total authors: {}",
                self.authors.len()
            );
            for (author_account_id, eth_public_key) in &self.authors {
                assert_ok!(<AuthorAccountIds<T>>::try_append(author_account_id));
                <EthereumPublicKeys<T>>::insert(eth_public_key, author_account_id);
                <AccountIdToEthereumKeys<T>>::insert(author_account_id, eth_public_key);
            }
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::add_author())]
        #[transactional]
        pub fn add_author(
            origin: OriginFor<T>,
            author_account_id: T::AccountId,
            author_eth_public_key: ecdsa::Public,
        ) -> DispatchResult {
            ensure_root(origin)?;

            // Validate the registration request
            Self::validate_author_registration_request(&author_account_id, &author_eth_public_key)?;

            // Send to T1 - actual registration happens in callback
            Self::send_author_registration_to_t1(&author_account_id, &author_eth_public_key)?;

            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::remove_author(MAX_AUTHOR_ACCOUNTS))]
        #[transactional]
        pub fn remove_author(
            origin: OriginFor<T>,
            author_account_id: T::AccountId,
        ) -> DispatchResult {
            let _ = ensure_root(origin)?;

            // Validate the deregistration request
            Self::validate_author_deregistration_request(&author_account_id)?;

            // Send to T1 - actual deregistration happens in callback
            Self::send_author_deregistration_to_t1(&author_account_id)?;

            Ok(())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::rotate_author_ethereum_key())]
        #[transactional]
        pub fn rotate_author_ethereum_key(
            origin: OriginFor<T>,
            author_account_id: T::AccountId,
            author_old_eth_public_key: ecdsa::Public,
            author_new_eth_public_key: ecdsa::Public,
        ) -> DispatchResult {
            let _ = ensure_root(origin)?;

            ensure!(
                !<EthereumPublicKeys<T>>::contains_key(&author_new_eth_public_key),
                Error::<T>::AuthorEthKeyAlreadyExists
            );
            ensure!(
                author_old_eth_public_key != author_new_eth_public_key,
                Error::<T>::AuthorEthKeyAlreadyExists
            );

            let author_id = EthereumPublicKeys::<T>::take(&author_old_eth_public_key)
                .ok_or(Error::<T>::AuthorNotFound)?;
            ensure!(author_id == author_account_id, Error::<T>::AuthorNotFound);

            EthereumPublicKeys::<T>::insert(author_new_eth_public_key, author_id);
            return Ok(())
        }
    }

    impl<T: Config> EthereumPublicKeyChecker<T::AccountId> for Pallet<T> {
        fn get_validator_for_eth_public_key(
            eth_public_key: &ecdsa::Public,
        ) -> Option<T::AccountId> {
            Self::get_author_by_eth_public_key(eth_public_key)
        }
    }
}

#[derive(Encode, Decode, Default, Clone, Copy, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct ActionId<AccountId: Member> {
    pub action_account_id: AccountId,
    pub ingress_counter: IngressCounter,
}

#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, Debug, TypeInfo, MaxEncodedLen)]
pub enum AuthorsActionType {
    /// Author has asked to leave voluntarily
    Resignation,
    /// Author is being forced to leave due to a malicious behaviour
    Slashed,
    /// Author activates himself after he joins an active session
    Activation,
    /// Default value
    Unknown,
    /// Author registration pending T1 confirmation
    Registration,
}

impl Default for AuthorsActionType {
    fn default() -> Self {
        AuthorsActionType::Unknown
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, Debug, TypeInfo, MaxEncodedLen)]
pub enum AuthorsActionStatus {
    /// Action enters this state immediately upon a request from the author.
    AwaitingConfirmation,
    /// The action has completed
    Confirmed,
    /// The request has been actioned (ex: sent to Ethereum and executed successfully)
    Actioned,
    /// Default value, status is unknown
    None,
}

impl Default for AuthorsActionStatus {
    fn default() -> Self {
        AuthorsActionStatus::None
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, Debug, TypeInfo, MaxEncodedLen)]
pub struct AuthorsActionData {
    pub status: AuthorsActionStatus,
    pub eth_transaction_id: EthereumId,
    pub action_type: AuthorsActionType,
}

impl AuthorsActionData {
    fn new(
        status: AuthorsActionStatus,
        eth_transaction_id: EthereumId,
        action_type: AuthorsActionType,
    ) -> Self {
        return AuthorsActionData { status, eth_transaction_id, action_type }
    }
}

impl AuthorsActionType {
    fn is_deregistration(&self) -> bool {
        match self {
            AuthorsActionType::Resignation => true,
            AuthorsActionType::Slashed => true,
            _ => false,
        }
    }

    fn is_activation(&self) -> bool {
        match self {
            AuthorsActionType::Activation => true,
            _ => false,
        }
    }

    fn is_registration(&self) -> bool {
        match self {
            AuthorsActionType::Registration => true,
            _ => false,
        }
    }
}

impl<AccountId: Member + Encode> ActionId<AccountId> {
    fn new(action_account_id: AccountId, ingress_counter: IngressCounter) -> Self {
        return ActionId::<AccountId> { action_account_id, ingress_counter }
    }
}

impl<T: Config> Pallet<T> {
    fn get_ethereum_public_key_if_exists(account_id: &T::AccountId) -> Option<ecdsa::Public> {
        return <EthereumPublicKeys<T>>::iter()
            .filter(|(_, acc)| acc == account_id)
            .map(|(pk, _)| pk)
            .nth(0)
    }

    fn remove_ethereum_public_key_if_required(author_id: &T::AccountId) {
        let public_key_to_remove = Self::get_ethereum_public_key_if_exists(&author_id);
        if let Some(public_key_to_remove) = public_key_to_remove {
            <EthereumPublicKeys<T>>::remove(public_key_to_remove);
        }
    }

    fn validate_author_registration_request(
        account_id: &T::AccountId,
        eth_public_key: &ecdsa::Public,
    ) -> DispatchResult {
        let author_account_ids = Self::author_account_ids().ok_or(Error::<T>::NoAuthors)?;
        ensure!(!author_account_ids.is_empty(), Error::<T>::NoAuthors);

        ensure!(!author_account_ids.contains(account_id), Error::<T>::AuthorAlreadyExists);

        ensure!(
            !<EthereumPublicKeys<T>>::contains_key(eth_public_key),
            Error::<T>::AuthorEthKeyAlreadyExists
        );

        ensure!(
            author_account_ids.len() < (<MaximumAuthorsBound as sp_core::TypedGet>::get() as usize),
            Error::<T>::MaximumAuthorsReached
        );

        let validator_id = <T as SessionConfig>::ValidatorIdOf::convert(account_id.clone())
            .ok_or(Error::<T>::ErrorConvertingAccountIdToAuthorId)?;

        ensure!(
            <pallet_session::NextKeys<T>>::contains_key(&validator_id),
            Error::<T>::AuthorSessionKeysNotSet
        );

        // Disallow starting a registration if any author action is already in progress
        ensure!(!Self::has_any_active_action(), Error::<T>::ValidatorActionAlreadyInProgress);

        Ok(())
    }

    fn validate_author_deregistration_request(account_id: &T::AccountId) -> DispatchResult {
        let author_account_ids = Self::author_account_ids().ok_or(Error::<T>::NoAuthors)?;

        ensure!(
            author_account_ids.len() > T::MinimumAuthorsCount::get() as usize,
            Error::<T>::MinimumAuthorsReached
        );

        // Check if the author exists in the current list of authors
        ensure!(author_account_ids.contains(account_id), Error::<T>::AuthorNotFound);

        // Check if this author has any active actions (registration or deregistration)
        ensure!(!Self::has_any_active_action(), Error::<T>::ValidatorActionAlreadyInProgress);

        Ok(())
    }

    /// Check if any author has any active action (registration, activation, or deregistration)
    fn has_any_active_action() -> bool {
        <AuthorActions<T>>::iter().any(|(_, _, authors_action_data)| {
            Self::action_state_is_active(authors_action_data.status)
        })
    }

    /// Check if an action status indicates the action is still active
    fn action_state_is_active(status: AuthorsActionStatus) -> bool {
        matches!(status, AuthorsActionStatus::AwaitingConfirmation | AuthorsActionStatus::Confirmed)
    }

    /// Send validator registration request to T1
    fn send_author_registration_to_t1(
        author_account_id: &T::AccountId,
        author_eth_public_key: &ecdsa::Public,
    ) -> Result<EthereumId, DispatchError> {
        // Add eth key mapping immediately (before T1 confirmation)
        <EthereumPublicKeys<T>>::insert(author_eth_public_key, author_account_id);
        <AccountIdToEthereumKeys<T>>::insert(author_account_id, author_eth_public_key);

        // Prepare data for T1
        let decompressed_eth_public_key = decompress_eth_public_key(*author_eth_public_key)
            .map_err(|_| Error::<T>::InvalidPublicKey)?;

        let author_id_bytes =
            <T as pallet::Config>::AccountToBytesConvert::into_bytes(author_account_id);

        let function_name = BridgeContractMethod::AddAuthor.name_as_bytes();
        let params = vec![
            (b"bytes".to_vec(), decompressed_eth_public_key.to_fixed_bytes().to_vec()),
            (b"bytes32".to_vec(), author_id_bytes.to_vec()),
        ];

        let tx_id = <T as pallet::Config>::BridgeInterface::publish(
            function_name,
            &params,
            PALLET_ID.to_vec(),
        )
        .map_err(|_| {
            Self::deposit_event(Event::<T>::FailedToPublishAuthorAction {
                author_id: author_account_id.clone(),
                action_type: AuthorsActionType::Registration,
                reason: b"Failed to submit transaction to Ethereum bridge".to_vec(),
            });
            Error::<T>::ErrorSubmitCandidateTxnToTier1
        })?;

        // Now create authorActions entry with the actual tx_id (single insert, no mutation)
        let ingress_counter = Self::get_ingress_counter() + 1;
        TotalIngresses::<T>::put(ingress_counter);

        <AuthorActions<T>>::insert(
            author_account_id,
            ingress_counter,
            AuthorsActionData::new(
                AuthorsActionStatus::AwaitingConfirmation,
                tx_id,
                AuthorsActionType::Registration,
            ),
        );

        TransactionToAction::<T>::insert(tx_id, (author_account_id.clone(), ingress_counter));

        Self::deposit_event(Event::<T>::AuthorActionPublished {
            author_id: author_account_id.clone(),
            action_type: AuthorsActionType::Registration,
            tx_id,
        });

        Ok(tx_id)
    }

    /// Send validator deregistration request to T1
    fn send_author_deregistration_to_t1(
        author_account_id: &T::AccountId,
    ) -> Result<EthereumId, DispatchError> {
        // Prepare data for T1
        let eth_public_key = <AccountIdToEthereumKeys<T>>::get(author_account_id)
            .ok_or(Error::<T>::AuthorNotFound)?;

        let decompressed_eth_public_key =
            decompress_eth_public_key(eth_public_key).map_err(|_| Error::<T>::InvalidPublicKey)?;

        let author_id_bytes =
            <T as pallet::Config>::AccountToBytesConvert::into_bytes(author_account_id);

        let function_name = BridgeContractMethod::RemoveAuthor.name_as_bytes();
        let params = vec![
            (b"bytes32".to_vec(), author_id_bytes.to_vec()),
            (b"bytes".to_vec(), decompressed_eth_public_key.to_fixed_bytes().to_vec()),
        ];

        // Send to T1 and get tx_id FIRST
        let tx_id = <T as pallet::Config>::BridgeInterface::publish(
            function_name,
            &params,
            PALLET_ID.to_vec(),
        )
        .map_err(|_| {
            Self::deposit_event(Event::<T>::FailedToPublishAuthorAction {
                author_id: author_account_id.clone(),
                action_type: AuthorsActionType::Resignation,
                reason: b"Failed to submit transaction to Ethereum bridge".to_vec(),
            });
            Error::<T>::ErrorSubmitCandidateTxnToTier1
        })?;

        // Now create ValidatorActions entry with the actual tx_id (single insert, no mutation)
        let ingress_counter = Self::get_ingress_counter() + 1;
        TotalIngresses::<T>::put(ingress_counter);

        <AuthorActions<T>>::insert(
            author_account_id,
            ingress_counter,
            AuthorsActionData::new(
                AuthorsActionStatus::AwaitingConfirmation,
                tx_id,
                AuthorsActionType::Resignation,
            ),
        );

        TransactionToAction::<T>::insert(tx_id, (author_account_id.clone(), ingress_counter));

        Self::deposit_event(Event::<T>::AuthorActionPublished {
            author_id: author_account_id.clone(),
            action_type: AuthorsActionType::Resignation,
            tx_id,
        });

        Ok(tx_id)
    }

    /// Rollback and cleanup state when T1 operation fails
    fn rollback_failed_author_action(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
        action_type: AuthorsActionType,
        tx_id: EthereumId,
    ) {
        // Type-specific cleanup
        if action_type.is_registration() {
            Self::cleanup_registration_storage(&account_id, ingress_counter);
        } else {
            // For non-registration actions, just remove the author action entry
            <AuthorActions<T>>::remove(&account_id, ingress_counter);
        }

        Self::deposit_event(Event::<T>::AuthorActionFailedOnEthereum {
            author_id: account_id.clone(),
            action_type,
            tx_id,
        });
    }

    fn cleanup_registration_storage(account_id: &T::AccountId, ingress_counter: IngressCounter) {
        // Remove the eth key mapping if it exists
        if let Some(eth_key) = <AccountIdToEthereumKeys<T>>::get(&account_id) {
            <EthereumPublicKeys<T>>::remove(eth_key);
            <AccountIdToEthereumKeys<T>>::remove(&account_id);
        }

        // Remove author action entry
        <AuthorActions<T>>::remove(&account_id, ingress_counter);
    }

    fn complete_author_registration(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
    ) -> DispatchResult {
        // Add to active authors list
        match <AuthorAccountIds<T>>::try_append(account_id.clone()) {
            Ok(_) => {},
            Err(_) => {
                // Cleanup on failure (no deposit to clean as it's already been used for staking)
                Self::handle_registration_failure(
                    &account_id,
                    ingress_counter,
                    "Failed to append author to active authors list",
                );
                return Err(Error::<T>::MaximumAuthorsReached.into())
            },
        }

        // Notify author registration
        let new_author_id = <T as SessionConfig>::ValidatorIdOf::convert(account_id.clone())
            .ok_or(Error::<T>::ErrorConvertingAccountIdToAuthorId)?;

        T::ValidatorRegistrationNotifier::on_validator_registration(&new_author_id);

        // Update ValidatorActions for activation process
        <AuthorActions<T>>::mutate(&account_id, ingress_counter, |authors_action_data_maybe| {
            if let Some(authors_action_data) = authors_action_data_maybe {
                authors_action_data.action_type = AuthorsActionType::Activation;
                authors_action_data.status = AuthorsActionStatus::Actioned;
            }
        });

        Self::deposit_event(Event::<T>::AuthorActivationStarted { author_id: account_id.clone() });

        Ok(())
    }

    fn complete_author_deregistration(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
    ) -> DispatchResult {
        // Immediately clean up author manager storage
        // Remove from active authors list
        AuthorAccountIds::<T>::mutate(|maybe_validators| {
            if let Some(validators) = maybe_validators {
                validators.retain(|v| v != account_id);
            }
        });

        Self::remove_ethereum_public_key_if_required(&account_id);

        <AuthorActions<T>>::mutate(&account_id, ingress_counter, |authors_action_data_maybe| {
            if let Some(authors_action_data) = authors_action_data_maybe {
                authors_action_data.status = AuthorsActionStatus::Actioned;
            }
        });

        Self::deposit_event(Event::<T>::AuthorDeregistered { author_id: account_id.clone() });

        Ok(())
    }

    fn handle_registration_failure(
        account_id: &T::AccountId,
        ingress_counter: IngressCounter,
        reason: &str,
    ) {
        log::error!("Validator registration failed for {:?}: {}", account_id, reason);

        Self::cleanup_registration_storage(&account_id, ingress_counter);

        Self::deposit_event(Event::<T>::AuthorRegistrationFailed {
            author_id: account_id.clone(),
            reason: reason.as_bytes().to_vec(),
        });
    }

    fn author_permanently_removed(
        active_authors: &Vec<Author<T::AuthorityId, T::AccountId>>,
        disabled_authors: &Vec<T::AccountId>,
        deregistered_author: &T::AccountId,
    ) -> bool {
        // If the author exists in either vectors then they have not been removed from the
        // session
        return !active_authors.iter().any(|v| &v.account_id == deregistered_author) &&
            !disabled_authors.iter().any(|v| v == deregistered_author)
    }

    fn clean_up_author_data(action_account_id: T::AccountId, ingress_counter: IngressCounter) {
        <AuthorActions<T>>::mutate(
            &action_account_id,
            ingress_counter,
            |authors_action_data_maybe| {
                if let Some(authors_action_data) = authors_action_data_maybe {
                    authors_action_data.status = AuthorsActionStatus::Confirmed
                }
            },
        );
        Self::remove_ethereum_public_key_if_required(&action_account_id);

        let action_id = ActionId::new(action_account_id, ingress_counter);

        Self::deposit_event(Event::<T>::AuthorActionConfirmed { action_id });
    }
}

impl<T: Config> NewSessionHandler<T::AuthorityId, T::AccountId> for Pallet<T> {
    fn on_genesis_session(_authors: &Vec<Author<T::AuthorityId, T::AccountId>>) {
        log::trace!("Authors manager on_genesis_session");
    }

    fn on_new_session(
        _changed: bool,
        active_authors: &Vec<Author<T::AuthorityId, T::AccountId>>,
        disabled_authors: &Vec<T::AccountId>,
    ) {
        log::trace!("Authors manager on_new_session");
        if <AuthorActions<T>>::iter().count() > 0 {
            for (action_account_id, ingress_counter, authors_action_data) in
                <AuthorActions<T>>::iter()
            {
                if authors_action_data.status == AuthorsActionStatus::AwaitingConfirmation &&
                    authors_action_data.action_type.is_deregistration() &&
                    Self::author_permanently_removed(
                        &active_authors,
                        &disabled_authors,
                        &action_account_id,
                    )
                {
                    Self::clean_up_author_data(action_account_id, ingress_counter);
                } else if authors_action_data.status == AuthorsActionStatus::AwaitingConfirmation &&
                    authors_action_data.action_type.is_activation()
                {
                    <AuthorActions<T>>::mutate(
                        &action_account_id,
                        ingress_counter,
                        |authors_action_data_maybe| {
                            if let Some(authors_action_data) = authors_action_data_maybe {
                                authors_action_data.status = AuthorsActionStatus::Confirmed
                            }
                        },
                    );

                    Self::deposit_event(Event::<T>::AuthorActivationStarted {
                        author_id: action_account_id.clone(),
                    });
                }
            }
        }
    }
}

impl<T: Config> session::SessionManager<T::AccountId> for Pallet<T> {
    fn new_session(new_index: u32) -> Option<Vec<T::AccountId>> {
        // Retrieve the authors from storage
        let authors_option = AuthorAccountIds::<T>::get();

        if let Some(authors) = authors_option {
            if authors.is_empty() {
                // We never want to pass an empty set of authors. This would brick the chain.
                log::error!("💥 keeping old session because of empty author set!");
                None
            } else {
                log::debug!(
                    "[AUTH-MGR] assembling new authors for new session {} with these authors {:#?} at #{:?}",
                    new_index,
                    authors,
                    <frame_system::Pallet<T>>::block_number(),
                );

                Some(authors.into())
            }
        } else {
            // Handle the case where no authors are present in storage
            log::error!("💥 keeping old session because no authors found in storage!");
            None
        }
    }

    fn end_session(_end_index: u32) {
        // nothing to do here
    }

    fn start_session(_start_index: u32) {
        // nothing to do here
    }
}

impl<T: Config> BridgeInterfaceNotification for Pallet<T> {
    fn process_result(tx_id: u32, caller_id: Vec<u8>, succeeded: bool) -> DispatchResult {
        if caller_id != PALLET_ID.to_vec() {
            return Ok(())
        }

        let Some((account_id, ingress_counter)) = TransactionToAction::<T>::take(tx_id) else {
            return Ok(())
        };
        let action_data = <AuthorActions<T>>::get(&account_id, ingress_counter)
            .ok_or(Error::<T>::AuthorsActionDataNotFound)?;
        let action_type = action_data.action_type;

        if !succeeded {
            Self::rollback_failed_author_action(&account_id, ingress_counter, action_type, tx_id);
            return Ok(())
        }

        // T1 succeeded - emit confirmation event and complete the operation
        Self::deposit_event(Event::<T>::AuthorActionConfirmedOnEthereum {
            author_id: account_id.clone(),
            action_type,
            tx_id,
        });

        // Complete the operation based on action type
        if action_type.is_registration() {
            Self::complete_author_registration(&account_id, ingress_counter)?;
        } else if action_type.is_deregistration() {
            Self::complete_author_deregistration(&account_id, ingress_counter)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

mod benchmarking;
