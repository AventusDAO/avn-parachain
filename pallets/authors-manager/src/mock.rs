//Copyright 2026 Aventus DAO Ltd

use crate::{self as authors_manager, *};
use frame_support::{
    parameter_types,
    traits::{Currency, OnFinalize, OnInitialize},
};
use sp_state_machine::BasicExternalities;

use hex_literal::hex;
use pallet_avn::{BridgeInterfaceNotification, EthereumPublicKeyChecker, ProcessedEventsChecker};
use pallet_balances as balances;
use pallet_timestamp as timestamp;
use sp_avn_common::{
    avn_tests_helpers::ethereum_converters::*,
    event_types::{AddedValidatorData as AddedAuthorData, EthEventId},
};
use sp_core::{ecdsa::Public, sr25519, ByteArray, ConstU64, Get, Pair, H256};
use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{BlakeTwo256, ConvertInto, IdentityLookup, Verify},
    BuildStorage,
};

use std::cell::RefCell;

pub fn author_id_1() -> AccountId {
    TestAccount::new([1u8; 32]).account_id()
}

pub fn author_id_2() -> AccountId {
    TestAccount::new([2u8; 32]).account_id()
}

pub fn author_id_3() -> AccountId {
    TestAccount::new([3u8; 32]).account_id()
}

pub fn author_id_4() -> AccountId {
    TestAccount::new([4u8; 32]).account_id()
}

pub fn author_id_5() -> AccountId {
    TestAccount::new([5u8; 32]).account_id()
}

pub fn genesis_config_initial_authors() -> [AccountId; 5] {
    [author_id_1(), author_id_2(), author_id_3(), author_id_4(), author_id_5()]
}
pub const REGISTERING_AUTHOR_TIER1_ID: u128 = 200;
pub const EXISTENTIAL_DEPOSIT: u64 = 0;

pub type Extrinsic = TestXt<RuntimeCall, ()>;
pub type AuthorId = <TestRuntime as session::Config>::ValidatorId;
//pub type FullIdentification = AccountId;
/// The signature type used by accounts/transactions.
pub type Signature = sr25519::Signature;
/// An identifier for an account on this system.
pub type AccountId = <Signature as Verify>::Signer;

type Block = frame_system::mocking::MockBlock<TestRuntime>;
// TODO: Refactor this struct to be reused in all tests
#[derive(Clone)]
pub struct TestAccount {
    pub seed: [u8; 32],
}

impl TestAccount {
    pub fn new(seed: [u8; 32]) -> Self {
        TestAccount { seed }
    }

    pub fn from_bytes(seed: &[u8]) -> Self {
        let mut seed_bytes: [u8; 32] = Default::default();
        seed_bytes.copy_from_slice(&seed[0..32]);
        TestAccount { seed: seed_bytes }
    }

    pub fn account_id(&self) -> AccountId {
        return AccountId::decode(&mut self.key_pair().public().to_vec().as_slice()).unwrap()
    }

    pub fn key_pair(&self) -> sr25519::Pair {
        return sr25519::Pair::from_seed(&self.seed)
    }
}

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        AuthorsManager: authors_manager::{Pallet, Call, Storage, Event<T>, Config<T>},
        Session: pallet_session::{Pallet, Call, Storage, Event<T>, Config<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
        AVN: pallet_avn::{Pallet, Storage, Event},
        EthBridge: pallet_eth_bridge::{Pallet, Call, Storage, Event<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
    }
);

use frame_support::derive_impl;
use frame_system::{self as system};
use pallet_session as session;

impl AuthorsManager {
    pub fn insert_authors_action_data(action_id: &ActionId<AccountId>) {
        <AuthorActions<TestRuntime>>::insert(
            action_id.action_account_id,
            action_id.ingress_counter,
            AuthorsActionData::new(
                AuthorsActionStatus::AwaitingConfirmation,
                INITIAL_TRANSACTION_ID,
                AuthorsActionType::Resignation,
            ),
        );
    }

    pub fn event_emitted(event: &RuntimeEvent) -> bool {
        return System::events().iter().any(|a| a.event == *event)
    }

    pub fn create_mock_identification_tuple(account_id: AccountId) -> (AccountId, AccountId) {
        return (account_id, account_id)
    }
}

parameter_types! {
    pub const VotingPeriod: u64 = 2;
    pub const MinimumAuthorsCount: u32 = 2;
}

impl Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type AccountToBytesConvert = AVN;
    type ValidatorRegistrationNotifier = Self;
    type WeightInfo = default_weights::SubstrateWeight<TestRuntime>;
    type BridgeInterface = EthBridge;
    type MinimumAuthorsCount = MinimumAuthorsCount;
}

impl<LocalCall> frame_system::offchain::CreateTransactionBase<LocalCall> for TestRuntime
where
    RuntimeCall: From<LocalCall>,
{
    type Extrinsic = Extrinsic;
    type RuntimeCall = RuntimeCall;
}

impl<LocalCall> frame_system::offchain::CreateInherent<LocalCall> for TestRuntime
where
    RuntimeCall: From<LocalCall>,
{
    fn create_inherent(call: Self::RuntimeCall) -> Self::Extrinsic {
        Extrinsic::new_bare(call)
    }
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type BaseCallFilter = frame_support::traits::Everything;
    type BlockWeights = ();
    type BlockLength = ();
    type DbWeight = ();
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type Nonce = u64;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Block = Block;
    type RuntimeEvent = RuntimeEvent;
    type BlockHashCount = BlockHashCount;
    type Version = ();
    type PalletInfo = PalletInfo;
    type AccountData = balances::AccountData<u128>;
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
    type SS58Prefix = ();
    type OnSetCode = ();
    type MaxConsumers = frame_support::traits::ConstU32<16>;
    type RuntimeTask = ();
}

impl avn::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = UintAuthorityId;
    type EthereumPublicKeyChecker = Self;
    type NewSessionHandler = AuthorsManager;
    type DisabledValidatorChecker = ();
    type WeightInfo = ();
}

parameter_types! {
    pub const ExistentialDeposit: u64 = EXISTENTIAL_DEPOSIT;
}

impl pallet_balances::Config for TestRuntime {
    type MaxLocks = frame_support::traits::ConstU32<1024>;
    type MaxReserves = ();
    type ReserveIdentifier = [u8; 8];
    type Balance = u128;
    type RuntimeEvent = RuntimeEvent;
    type DustRemoval = ();
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
    type WeightInfo = ();
    type RuntimeHoldReason = ();
    type FreezeIdentifier = ();
    type MaxFreezes = ();
    type RuntimeFreezeReason = ();
    type DoneSlashHandler = ();
}

parameter_types! {
    pub const MinimumPeriod: u64 = 3;
}

impl timestamp::Config for TestRuntime {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = MinimumPeriod;
    type WeightInfo = ();
}

impl pallet_eth_bridge::Config for TestRuntime {
    type MaxQueuedTxRequests = frame_support::traits::ConstU32<100>;
    type RuntimeEvent = RuntimeEvent;
    type TimeProvider = Timestamp;
    type MinEthBlockConfirmation = ConstU64<20>;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = ();
    type AccountToBytesConvert = AVN;
    type BridgeInterfaceNotification = Self;
    type ReportCorroborationOffence = ();
    type ProcessedEventsChecker = ();
    type ProcessedEventsHandler = ();
    type EthereumEventsMigration = ();
    type Quorum = AVN;
}

impl BridgeInterfaceNotification for TestRuntime {
    fn process_result(
        _tx_id: u32,
        _caller_id: Vec<u8>,
        _tx_succeeded: bool,
    ) -> sp_runtime::DispatchResult {
        Ok(())
    }
}

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
}

impl session::Config for TestRuntime {
    type SessionManager = AuthorsManager;
    type Keys = UintAuthorityId;
    type ShouldEndSession = pallet_session::PeriodicSessions<Period, Offset>;
    type SessionHandler = (AVN,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
    type WeightInfo = ();
    type DisablingStrategy = ();
}

impl pallet_session::historical::Config for TestRuntime {
    type FullIdentification = AccountId;
    type FullIdentificationOf = ConvertInto;
}

/// An extrinsic type used for tests.
//type IdentificationTuple = (AccountId, AccountId);

pub const INITIAL_TRANSACTION_ID: EthereumId = 0;

thread_local! {
    static PROCESSED_EVENTS: RefCell<Vec<(H256,H256)>> = RefCell::new(vec![]);

    pub static AUTHORS: RefCell<Option<Vec<AccountId>>> = RefCell::new(Some(vec![
        author_id_1(),
        author_id_2(),
        author_id_3(),
        author_id_4(),
        author_id_5(),
    ]));

    static MOCK_TX_ID: RefCell<EthereumId> = RefCell::new(INITIAL_TRANSACTION_ID);
}

impl ProcessedEventsChecker for TestRuntime {
    fn processed_event_exists(event_id: &EthEventId) -> bool {
        return PROCESSED_EVENTS.with(|l| {
            l.borrow_mut().iter().any(|event| {
                &EthEventId { signature: event.0.clone(), transaction_hash: event.1.clone() } ==
                    event_id
            })
        })
    }

    fn add_processed_event(_event_id: &EthEventId, _accepted: bool) -> Result<(), ()> {
        Ok(())
    }
}

// TODO: Do we need to test the ECDSA sig verification logic here? If so, replace this with a call
// to the pallet's get_author_for_eth_public_key method and update the tests to use "real"
// signatures
impl EthereumPublicKeyChecker<AccountId> for TestRuntime {
    fn get_validator_for_eth_public_key(eth_public_key: &ecdsa::Public) -> Option<AccountId> {
        if !EthereumPublicKeys::<TestRuntime>::contains_key(eth_public_key) {
            return None
        }
        return Some(EthereumPublicKeys::<TestRuntime>::get(eth_public_key).unwrap())
    }
}

impl AuthorRegistrationNotifier<AuthorId> for TestRuntime {
    fn on_validator_registration(_author_id: &AuthorId) {}
}

// Derived from [1u8;32] private key
pub(crate) const AUTHOR_1_ETHEREUM_PUPLIC_KEY: [u8; 33] =
    hex!["031b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f"];
// Derived from [2u8;32] private key
pub(crate) const AUTHOR_2_ETHEREUM_PUPLIC_KEY: [u8; 33] =
    hex!["024d4b6cd1361032ca9bd2aeb9d900aa4d45d9ead80ac9423374c451a7254d0766"];
// Derived from [3u8;32] private key

pub(crate) const AUTHOR_3_ETHEREUM_PUPLIC_KEY: [u8; 33] =
    hex!["02531fe6068134503d2723133227c867ac8fa6c83c537e9a44c3c5bdbdcb1fe337"];
// Derived from [4u8;32] private key

pub(crate) const AUTHOR_4_ETHEREUM_PUPLIC_KEY: [u8; 33] =
    hex!["03462779ad4aad39514614751a71085f2f10e1c7a593e4e030efb5b8721ce55b0b"];
// Derived from [5u8;32] private key

pub(crate) const AUTHOR_5_ETHEREUM_PUPLIC_KEY: [u8; 33] =
    hex!["0362c0a046dacce86ddd0343c6d3c7c79c2208ba0d9c9cf24a6d046d21d21f90f7"];

fn initial_authors_public_keys() -> Vec<ecdsa::Public> {
    return vec![
        Public::from_slice(&AUTHOR_1_ETHEREUM_PUPLIC_KEY).unwrap(),
        Public::from_slice(&AUTHOR_2_ETHEREUM_PUPLIC_KEY).unwrap(),
        Public::from_slice(&AUTHOR_3_ETHEREUM_PUPLIC_KEY).unwrap(),
        Public::from_slice(&AUTHOR_4_ETHEREUM_PUPLIC_KEY).unwrap(),
        Public::from_slice(&AUTHOR_5_ETHEREUM_PUPLIC_KEY).unwrap(),
    ]
}

fn initial_maximum_authors_public_keys() -> Vec<ecdsa::Public> {
    let mut public_keys = initial_authors_public_keys();

    for i in public_keys.len() as u32..(<MaximumAuthorsBound as sp_core::TypedGet>::get() as u32) {
        public_keys.push(Public::from_raw([i as u8; 33]));
    }
    public_keys
}

pub struct ExtBuilder {
    pub storage: sp_runtime::Storage,
}

impl ExtBuilder {
    pub fn build_default() -> Self {
        let storage =
            frame_system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();
        Self { storage }
    }

    pub fn as_externality(self) -> sp_io::TestExternalities {
        let mut ext = sp_io::TestExternalities::from(self.storage);
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| {
            Timestamp::set_timestamp(1);
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into())
        });
        ext
    }

    /// Setups a genesis configuration with 5 authors to the genesis state
    pub fn with_authors(self) -> Self {
        let author_account_ids: &Vec<AccountId> = &AUTHORS.with(|l| l.borrow().clone().unwrap());

        self.setup_authors(author_account_ids, initial_authors_public_keys)
    }

    /// Setups a genesis configuration with maximum authors to the genesis state
    pub fn with_maximum_authors(self) -> Self {
        let mut authors_account_ids: Vec<AccountId> = vec![];
        // mock accounts
        for i in 1..=MaximumAuthorsBound::get() {
            let mut seed = [i as u8; 32];
            // [0u8;32] is the identity of the author we add in the tests. Change the seed if its
            // the same.
            if seed.eq(&[0u8; 32]) {
                seed[30] = 1;
            }
            authors_account_ids.push(TestAccount::new(seed).account_id());
        }

        self.setup_authors(&authors_account_ids, initial_maximum_authors_public_keys)
    }

    /// Setups a genesis configuration with N authors to the genesis state
    fn setup_authors(
        mut self,
        author_account_ids: &Vec<AccountId>,
        get_eth_keys: fn() -> Vec<ecdsa::Public>,
    ) -> Self {
        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for ref k in author_account_ids {
                frame_system::Pallet::<TestRuntime>::inc_providers(k);
            }
        });

        // Important: the order of the storage setup is important. Do not change it.
        let _ = authors_manager::GenesisConfig::<TestRuntime> {
            authors: author_account_ids
                .iter()
                .map(|v| v.clone())
                .zip(get_eth_keys().iter().map(|pk| pk.clone()))
                .collect::<Vec<_>>(),
        }
        .assimilate_storage(&mut self.storage);

        let _ = session::GenesisConfig::<TestRuntime> {
            keys: author_account_ids
                .clone()
                .into_iter()
                .enumerate()
                .map(|(i, v)| (v, v, UintAuthorityId((i as u32).into())))
                .collect(),
            ..Default::default()
        }
        .assimilate_storage(&mut self.storage);

        self
    }
}

pub struct MockData {
    pub new_author_id: AccountId,
    pub author_eth_public_key: ecdsa::Public,
}

impl MockData {
    pub fn setup_valid() -> Self {
        let data = Some(LogDataHelper::get_author_data(REGISTERING_AUTHOR_TIER1_ID));
        let topics = MockData::get_author_token_topics();
        let author_data = AddedAuthorData::parse_bytes(data.clone(), topics.clone()).unwrap();
        let author_eth_public_key = ecdsa::Public::from_raw(hex!(
            "02407b0d9f41148bbe3b6c7d4a62585ae66cc32a707441197fa5453abfebd31d57"
        ));
        let new_author_id =
            TestAccount::from_bytes(author_data.t2_address.clone().as_bytes()).account_id();
        Balances::make_free_balance_be(&new_author_id, 100000);
        MockData { new_author_id, author_eth_public_key }
    }

    pub fn get_author_token_topics() -> Vec<Vec<u8>> {
        let topic_event_signature = LogDataHelper::get_topic_32_bytes(10);
        let topic_sender_lhs = LogDataHelper::get_topic_32_bytes(15);
        let topic_sender_rhs = LogDataHelper::get_topic_32_bytes(25);
        let topic_receiver = LogDataHelper::get_topic_32_bytes(30);
        return vec![topic_event_signature, topic_sender_lhs, topic_sender_rhs, topic_receiver]
    }
}

impl AuthorsManager {
    pub fn insert_to_authors(to_insert: &AccountId) {
        <AuthorAccountIds<TestRuntime>>::try_append(to_insert.clone())
            .expect("Too many author accounts in genesis");
    }
}

/// LogData Helper struct that converts values to topics and data
// TODO [TYPE: refactoring][PRI: low] We should consolidate the different versions of these
// functions and make one helper that can be used everywhere
pub struct LogDataHelper {}

impl LogDataHelper {
    pub fn get_author_data(deposit: u128) -> Vec<u8> {
        return into_32_be_bytes(&deposit.to_le_bytes())
    }

    pub fn get_topic_32_bytes(n: u8) -> Vec<u8> {
        return vec![n; 32]
    }
}

// TODO [TYPE: test refactoring][PRI: low]: update this function to work with the mock builder
// pattern Currently, a straightforward replacement of the test setup leads to an error on the
// assert_eq!
pub fn advance_session() {
    let now = System::block_number().max(1);

    Balances::on_finalize(System::block_number());
    System::on_finalize(System::block_number());
    System::set_block_number(now + 1);
    System::on_initialize(System::block_number());
    Balances::on_initialize(System::block_number());
    Session::on_initialize(System::block_number());
}
