// Copyright 2022 Aventus Systems (UK) Ltd.

#![cfg(test)]

use frame_support::{assert_ok, derive_impl, parameter_types};
use sp_core::{crypto::KeyTypeId, sr25519, Pair, H256};
use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{ConvertInto, IdentityLookup},
    BoundedBTreeSet, BuildStorage, Perbill, WeakBoundedVec,
};
use sp_state_machine::BasicExternalities;
use std::{cell::RefCell, collections::BTreeSet};

use frame_system::{self as system, DefaultConfig};
use hex_literal::hex;
use pallet_avn_proxy::ProvableProxy;
use pallet_session as session;

use codec::{alloc::sync::Arc, DecodeWithMemTracking};
use parking_lot::RwLock;
use sp_core::offchain::{
    testing::{OffchainState, PendingRequest, PoolState, TestOffchainExt, TestTransactionPoolExt},
    OffchainDbExt as OffchainExt, OffchainWorkerExt, TransactionPoolExt,
};
use sp_staking::{
    offence::{OffenceError, ReportOffence},
    SessionIndex,
};

use avn::AvnBridgeContractAddress;
use pallet_avn::{self as avn, Error as avn_error};
use sp_avn_common::{
    bounds::MaximumValidatorsBound, event_discovery::filters::AllEventsFilter,
    event_types::EthEvent, EthQueryRequest, EthQueryResponseType, FeePaymentHandler,
};
use sp_io::TestExternalities;

use crate::{self as pallet_ethereum_events, *};

#[allow(dead_code)]
pub type Signature = sr25519::Signature;
pub type AccountId = <Signature as Verify>::Signer;
pub type BlockNumber = BlockNumberFor<TestRuntime>;
pub type AuthorityId = <TestRuntime as avn::Config>::AuthorityId;

type Block = frame_system::mocking::MockBlock<TestRuntime>;

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Session: pallet_session::{Pallet, Call, Storage, Event<T>, Config<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
        Avn: pallet_avn::{Pallet, Storage, Event, Config<T>},
        AvnProxy: pallet_avn_proxy::{Pallet, Call, Storage, Event<T>},
        EthereumEvents: pallet_ethereum_events::{Pallet, Call, Storage, Event<T>, Config<T>},
        Historical: pallet_session::historical::{Pallet, Storage},
    }
);

pub fn account_id_0() -> AccountId {
    TestAccount::new([0u8; 32]).account_id()
}
pub fn validator_id_1() -> AccountId {
    TestAccount::new([1u8; 32]).account_id()
}
pub fn validator_id_2() -> AccountId {
    TestAccount::new([2u8; 32]).account_id()
}
pub fn validator_id_3() -> AccountId {
    TestAccount::new([3u8; 32]).account_id()
}
pub fn account_id_1() -> AccountId {
    validator_id_1()
}
pub fn checked_by() -> AccountId {
    TestAccount::new([10u8; 32]).account_id()
}

// TODO: Refactor this struct to be reused in all tests
pub struct TestAccount {
    pub seed: [u8; 32],
}

impl TestAccount {
    pub fn new(seed: [u8; 32]) -> Self {
        TestAccount { seed }
    }

    pub fn account_id(&self) -> AccountId {
        return AccountId::decode(&mut self.key_pair().public().to_vec().as_slice()).unwrap();
    }

    pub fn key_pair(&self) -> sr25519::Pair {
        return sr25519::Pair::from_seed(&self.seed);
    }
}

pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"dumy");

pub const GOOD_STATUS: &str = "0x1";
pub const GOOD_BLOCK_CONFIRMATIONS: u64 = 2;
pub const QUORUM_FACTOR: u32 = 3;
pub const EVENT_CHALLENGE_PERIOD: BlockNumber = 2;
pub const EXISTENTIAL_DEPOSIT: u64 = 0;

pub mod crypto {
    use super::KEY_TYPE;
    use sp_runtime::app_crypto::{app_crypto, sr25519};
    app_crypto!(sr25519, KEY_TYPE);
}

pub type Extrinsic = TestXt<RuntimeCall, ()>;

pub type EventsTypesLimit = ConstU32<20>;
pub type EthBridgeEventsFilter = BoundedBTreeSet<ValidEvents, EventsTypesLimit>;

pub struct MyEthereumEventsFilter;

impl EthereumEventsFilterTrait for MyEthereumEventsFilter {
    fn get() -> EthBridgeEventsFilter {
        let mut allowed_events: BTreeSet<ValidEvents> = AllEventsFilter::get().into_inner();
        allowed_events.retain(|e| *e != ValidEvents::AvtLowerClaimed);
        EthBridgeEventsFilter::try_from(allowed_events).unwrap_or_default()
    }
}

#[derive_impl(pallet_ethereum_events::config_preludes::TestDefaultConfig as pallet_ethereum_events::DefaultConfig)]
impl Config for TestRuntime {
    type ProcessedEventHandler = Self;
    type ReportInvalidEthereumLog = OffenceHandler;
    type Public = AccountId;
    type Signature = Signature;
    type ProcessedEventsHandler = MyEthereumEventsFilter;
    type ProcessedEventsChecker = EthereumEvents;
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

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Nonce = u64;
    type Block = Block;
    type AccountData = pallet_balances::AccountData<u128>;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
}

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
    pub const DisabledValidatorsThreshold: Perbill = Perbill::from_percent(33);
}

thread_local! {
    // validator accounts (aka public addresses, public keys-ish)
    pub static VALIDATORS: RefCell<Option<Vec<AccountId>>> = RefCell::new(Some(vec![
        validator_id_1(),
        validator_id_2(),
        validator_id_3(),
    ]));

    pub static PROCESS_EVENT_SUCCESS: RefCell<bool> = RefCell::new(true);
}

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl avn::Config for TestRuntime {
    type AuthorityId = UintAuthorityId;
}
pub struct TestSessionManager;
impl session::SessionManager<AccountId> for TestSessionManager {
    fn new_session(_new_index: SessionIndex) -> Option<Vec<AccountId>> {
        VALIDATORS.with(|l| l.borrow_mut().take())
    }
    fn end_session(_: SessionIndex) {}
    fn start_session(_: SessionIndex) {}
}

impl session::Config for TestRuntime {
    type SessionManager =
        pallet_session::historical::NoteHistoricalRoot<TestRuntime, TestSessionManager>;
    type Keys = UintAuthorityId;
    type ShouldEndSession = session::PeriodicSessions<Period, Offset>;
    type SessionHandler = (Avn,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = session::PeriodicSessions<Period, Offset>;
    type WeightInfo = ();
    type DisablingStrategy = ();
}

parameter_types! {
    pub const ExistentialDeposit: u64 = EXISTENTIAL_DEPOSIT;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for TestRuntime {
    type Balance = u128;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
}

impl pallet_session::historical::Config for TestRuntime {
    type FullIdentification = AccountId;
    type FullIdentificationOf = ConvertInto;
}

impl pallet_session::historical::SessionManager<AccountId, AccountId> for TestSessionManager {
    fn new_session(_new_index: SessionIndex) -> Option<Vec<(AccountId, AccountId)>> {
        VALIDATORS.with(|l| {
            l.borrow_mut().take().map(|validators| validators.iter().map(|v| (*v, *v)).collect())
        })
    }
    fn end_session(_: SessionIndex) {}
    fn start_session(_: SessionIndex) {}
}

impl ProcessedEventHandler for TestRuntime {
    fn on_event_processed(_event: &EthEvent) -> DispatchResult {
        match PROCESS_EVENT_SUCCESS.with(|pk| *pk.borrow()) {
            true => return Ok(()),
            _ => Err(Error::<TestRuntime>::InvalidEventToProcess)?,
        }
    }
}

/// An extrinsic type used for tests.
type IdentificationTuple = (AccountId, AccountId);
type Offence = crate::InvalidEthereumLogOffence<IdentificationTuple>;

thread_local! {
    pub static OFFENCES: RefCell<Vec<(Vec<AccountId>, Offence)>> = RefCell::new(vec![]);
}

/// A mock offence report handler.
pub struct OffenceHandler;
impl ReportOffence<AccountId, IdentificationTuple, Offence> for OffenceHandler {
    fn report_offence(reporters: Vec<AccountId>, offence: Offence) -> Result<(), OffenceError> {
        OFFENCES.with(|l| l.borrow_mut().push((reporters, offence)));
        Ok(())
    }

    fn is_known_offence(_offenders: &[IdentificationTuple], _time_slot: &SessionIndex) -> bool {
        false
    }
}

pub static CUSTOM_BRIDGE_CONTRACT: H160 = H160(hex!("11111AAAAA22222BBBBB11111AAAAA22222BBBBB"));

#[allow(dead_code)]
pub const INDEX_DATA: usize = 2;
#[allow(dead_code)]
pub const INDEX_RESULT_LOGS: usize = 9;
#[allow(dead_code)]
pub const INDEX_RESULT_STATUS: usize = 10;
#[allow(dead_code)]
pub const INDEX_EVENT_ADDRESS: usize = 5;
#[allow(dead_code)]
pub const INDEX_EVENT_DATA: usize = 6;
#[allow(dead_code)]
pub const INDEX_TOPICS: usize = 7;

pub const DEFAULT_INGRESS_COUNTER: IngressCounter = 100;
pub const FIRST_INGRESS_COUNTER: IngressCounter = 1;
pub const DEFAULT_BLOCK: u64 = 1;
pub const CHECKED_AT_BLOCK: u64 = 0;
pub const MIN_CHALLENGE_VOTES: u32 = 1;

impl EthereumEvents {
    pub fn has_events_to_check() -> bool {
        return <UncheckedEvents<TestRuntime>>::get().is_empty() == false;
    }

    pub fn setup_mock_ethereum_contracts_address() {
        AvnBridgeContractAddress::<TestRuntime>::put(CUSTOM_BRIDGE_CONTRACT);
    }

    pub fn set_ingress_counter(new_value: IngressCounter) {
        <TotalIngresses<TestRuntime>>::put(new_value);
    }

    pub fn insert_to_unchecked_events(to_insert: &EthEventId, ingress_counter: IngressCounter) {
        assert_ok!(<UncheckedEvents<TestRuntime>>::try_append((
            to_insert.clone(),
            ingress_counter,
            0
        )));
        Self::set_ingress_counter(ingress_counter);
    }

    pub fn populate_events_pending_challenge(
        checked_by: &AccountId,
        num_of_events: u8,
    ) -> IngressCounter {
        let from = Self::events_pending_challenge().len() as u8;
        let to = from + num_of_events;
        let block_number = EVENT_CHALLENGE_PERIOD;
        let min_challenge_votes = 0;

        for i in from..to {
            let ingress_counter = (i + 1) as IngressCounter;
            Self::insert_to_events_pending_challenge(
                block_number,
                CheckResult::Unknown,
                &Self::get_event_id(i),
                ingress_counter,
                &EventData::EmptyEvent,
                checked_by.clone(),
                block_number - 1,
                min_challenge_votes,
            );
        }
        // returns the first ingress counter
        return (from + 1) as IngressCounter;
    }

    pub fn insert_to_events_pending_challenge_compact(
        block_number: u64,
        event_info: &EthEventCheckResult<BlockNumberFor<TestRuntime>, AuthorityId>,
        checked_by: AccountId,
    ) {
        Self::insert_to_events_pending_challenge(
            block_number,
            event_info.result.clone(),
            &event_info.event.event_id,
            DEFAULT_INGRESS_COUNTER,
            &event_info.event.event_data,
            checked_by.clone(),
            block_number + 4,
            20,
        );
    }

    pub fn insert_to_events_pending_challenge(
        id: u64,
        result: CheckResult,
        event_id: &EthEventId,
        ingress_counter: u64,
        event_data: &EventData,
        checked_by: AccountId,
        checked_at_block: u64,
        min_challenge_votes: u32,
    ) {
        let to_insert = EthEventCheckResult::new(
            id,
            result,
            &event_id,
            event_data,
            checked_by,
            checked_at_block,
            min_challenge_votes,
        );

        assert_ok!(<EventsPendingChallenge<TestRuntime>>::try_append((
            to_insert,
            ingress_counter,
            0
        )));
    }

    pub fn get_event_id(seed: u8) -> EthEventId {
        return EthEventId {
            signature: ValidEvents::AddedValidator.signature(),
            transaction_hash: H256::from([seed; 32]),
        };
    }

    pub fn has_events_to_validate() -> bool {
        return !<EventsPendingChallenge<TestRuntime>>::get().is_empty();
    }

    pub fn validators() -> WeakBoundedVec<Validator<AuthorityId, AccountId>, MaximumValidatorsBound>
    {
        return Avn::active_validators();
    }

    pub fn is_primary_validator_for_block(
        block_number: BlockNumberFor<TestRuntime>,
        validator: &AccountId,
    ) -> Result<bool, avn_error<TestRuntime>> {
        return Avn::is_primary_for_block(block_number, validator);
    }

    pub fn get_validator_for_current_node() -> Option<Validator<AuthorityId, AccountId>> {
        return Avn::get_validator_for_current_node();
    }

    pub fn event_emitted(event: &RuntimeEvent) -> bool {
        return System::events().iter().any(|a| a.event == *event);
    }
}

impl pallet_avn_proxy::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type Public = AccountId;
    type Signature = Signature;
    type ProxyConfig = TestAvnProxyConfig;
    type WeightInfo = ();
    type FeeHandler = Self;
    type Token = sp_core::H160;
}

// Test Avn proxy configuration logic
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Encode,
    Decode,
    Debug,
    TypeInfo,
    DecodeWithMemTracking,
)]
pub struct TestAvnProxyConfig {}
impl Default for TestAvnProxyConfig {
    fn default() -> Self {
        TestAvnProxyConfig {}
    }
}

impl ProvableProxy<RuntimeCall, Signature, AccountId> for TestAvnProxyConfig {
    fn get_proof(call: &RuntimeCall) -> Option<Proof<Signature, AccountId>> {
        match call {
            RuntimeCall::EthereumEvents(
                pallet_ethereum_events::Call::signed_add_ethereum_log {
                    proof,
                    event_type: _,
                    tx_hash: _,
                },
            ) => return Some(proof.clone()),
            _ => None,
        }
    }
}

impl InnerCallValidator for TestAvnProxyConfig {
    type Call = RuntimeCall;

    fn signature_is_valid(call: &Box<Self::Call>) -> bool {
        match **call {
            RuntimeCall::EthereumEvents(..) => return EthereumEvents::signature_is_valid(call),
            _ => false,
        }
    }
}

// TODO [TYPE: test refactoring][PRI: low]: remove this function, when tests in
// session_handler_tests and test_challenges are fixed
#[allow(dead_code)]
pub fn eth_events_test_with_validators() -> TestExternalities {
    let mut ext =
        ExtBuilder::build_default().with_validators().for_offchain_worker().as_externality();

    ext.execute_with(|| System::set_block_number(1));
    return ext;
}

#[allow(dead_code)]
pub fn keys_setup_return_good_validator() -> Validator<AuthorityId, AccountId> {
    let validators = EthereumEvents::validators(); // Validators are tuples (UintAuthorityId(int), int)
    assert_eq!(validators[0], Validator { account_id: validator_id_1(), key: UintAuthorityId(0) });
    assert_eq!(validators[2], Validator { account_id: validator_id_3(), key: UintAuthorityId(2) });
    assert_eq!(validators.len(), 3);

    // AuthorityId type for TestRuntime is UintAuthorityId
    let keys: Vec<UintAuthorityId> = validators.into_iter().map(|v| v.key).collect();
    UintAuthorityId::set_all_keys(keys); // Keys in the setup are either () or (1,2,3). See VALIDATORS.
    let current_node_validator = EthereumEvents::get_validator_for_current_node().unwrap(); // filters validators() to just those corresponding to this validator
    assert_eq!(current_node_validator.key, UintAuthorityId(0));
    assert_eq!(current_node_validator.account_id, validator_id_1());

    assert_eq!(
        current_node_validator,
        Validator { account_id: validator_id_1(), key: UintAuthorityId(0) }
    );

    return current_node_validator;
}

#[allow(dead_code)]
pub fn bad_authority() -> Validator<AuthorityId, AccountId> {
    let validator =
        Validator { account_id: TestAccount::new([0u8; 32]).account_id(), key: UintAuthorityId(0) };

    return validator;
}

pub fn test_json_data(
    tx_hash: &H256,
    event_signature: &H256,
    contract_address: &H160,
    log_data: &str,
    event_topics: &str,
    status: &str,
) -> String {
    format!("
    {{
        \"transactionHash\": \"{}\",
        \"transactionIndex\": \"0x0\",
        \"blockHash\": \"0x5536c9e671fe581fe4ef4631112038297dcdecae163e8724c281ece8ad94c8c3\",
        \"blockNumber\": \"0x2e\",
        \"from\": \"0x3a629a342f842d2e548a372742babf288816da4e\",
        \"to\": \"0x604dd282e3fbe35f40f84405f90965821483827f\",
        \"gasUsed\": \"0x6a4b\",
        \"cumulativeGasUsed\": \"0x6a4b\",
        \"contractAddress\": null,
        \"logs\": [
            {{
                \"logIndex\": \"0x0\",
                \"transactionIndex\": \"0x0\",
                \"transactionHash\": \"0x9ad4d46054b0495fa38e8418263c6107ecb4ffd879675372613edf39af898dcb\",
                \"blockHash\": \"0x5536c9e671fe581fe4ef4631112038297dcdecae163e8724c281ece8ad94c8c3\",
                \"blockNumber\": \"0x2e\",
                \"address\": \"{}\",
                \"data\": \"{}\",
                \"topics\": [
                    \"{}\",
                    \"{}\"

                ],
                \"type\": \"mined\"
            }}
        ],
        \"status\": \"{}\",
        \"logsBloom\": \"0x00000100000000000000000000000000000000000000000000000000000100000000000000000000000000000000400000000000000000000000010000000000000000000000000000000000000000000000000001020000000000000000040000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000100000000000000000000000000000000000000000000000000001000000000000000000000000000000000800000000000000000\",
        \"v\": \"0x1c\",
        \"r\": \"0x8823b54a06401fed57e03ac54b1a4cf81091dc1e44192b9a87ce4f4b9c56d454\",
        \"s\": \"0x842e06a5258c4337148bc677f0b5ca343a8dfda597fb92f540ce443fd2bf340\"
    }}
    ",
        format!("{:?}", tx_hash),
        format!("{:?}", contract_address),
        log_data,
        format!("{:?}", event_signature),
        event_topics,
        status
    )
}

#[allow(dead_code)]
pub fn test_json(
    tx_hash: &H256,
    event_signature: &H256,
    contract_address: &H160,
    log_data: &str,
    event_topics: &str,
    status: &str,
    num_confirmations: u64,
) -> Vec<u8> {
    let data =
        test_json_data(tx_hash, event_signature, contract_address, log_data, event_topics, status);

    hex::encode(
        EthQueryResponse { data: data.as_bytes().to_vec().encode(), num_confirmations }.encode(),
    )
    .into()
}

#[allow(dead_code)]
pub fn inject_ethereum_node_response(
    state: &mut OffchainState,
    tx_hash: &H256,
    expected_response: Option<Vec<u8>>,
) {
    let calldata = EthQueryRequest {
        tx_hash: *tx_hash,
        response_type: EthQueryResponseType::TransactionReceipt,
    };
    let sender = [0; 32];
    let contract_address = Avn::get_bridge_contract_address();
    let ethereum_call = EthTransaction::new(sender, contract_address, calldata.encode());

    state.expect_request(PendingRequest {
        method: "POST".into(),
        uri: "http://127.0.0.1:2020/eth/query".into(),
        response: expected_response,
        headers: vec![],
        body: ethereum_call.encode(),
        sent: true,
        ..Default::default()
    });
}

pub fn simulate_http_response(
    offchain_state: &Arc<RwLock<OffchainState>>,
    unchecked_event: &EthEventId,
    status: &str,
    confirmations: u64,
) {
    let log_data = "0x0000000000000000000000000000000000000000000000000000000005f5e100";
    let event_topics = "0x00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3\",
                      \"0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303\",
                      \"0x0000000000000000000000000000000000000000000000000000000000000001";
    inject_ethereum_node_response(
        &mut offchain_state.write(),
        &unchecked_event.transaction_hash,
        Some(test_json(
            &unchecked_event.transaction_hash,
            &unchecked_event.signature,
            &Avn::get_bridge_contract_address(),
            log_data,
            event_topics,
            status,
            confirmations,
        )),
    );
}

// ==========================================================

pub const BRIDGE_CONTRACT: [u8; 20] = [9u8; 20];
pub static NFT_CONTRACT: [u8; 20] = [10u8; 20];

pub const INITIAL_LIFTS: [[u8; 32]; 4] = [[10u8; 32], [11u8; 32], [12u8; 32], [13u8; 32]];

pub const INITIAL_PROCESSED_EVENTS: [[u8; 32]; 3] = [[15u8; 32], [16u8; 32], [17u8; 32]];

pub fn create_initial_processed_events() -> Vec<(H256, H256, bool)> {
    let initial_processed_events = INITIAL_PROCESSED_EVENTS
        .iter()
        .map(|x| (ValidEvents::AddedValidator.signature(), H256::from(x), true))
        .collect::<Vec<(H256, H256, bool)>>();
    assert_eq!(INITIAL_PROCESSED_EVENTS.len(), initial_processed_events.len());
    return initial_processed_events;
}

pub struct ExtBuilder {
    storage: sp_runtime::Storage,
    offchain_state: Option<Arc<RwLock<OffchainState>>>,
    pool_state: Option<Arc<RwLock<PoolState>>>,
    txpool_extension: Option<TestTransactionPoolExt>,
    offchain_extension: Option<TestOffchainExt>,
    offchain_registered: bool,
}

#[allow(dead_code)]
impl ExtBuilder {
    pub fn build_default() -> Self {
        let storage = pallet_ethereum_events::GenesisConfig::<TestRuntime> {
            quorum_factor: QUORUM_FACTOR,
            event_challenge_period: EVENT_CHALLENGE_PERIOD,
            ..Default::default()
        }
        .build_storage()
        .unwrap();

        Self {
            storage,
            pool_state: None,
            offchain_state: None,
            txpool_extension: None,
            offchain_extension: None,
            offchain_registered: false,
        }
    }

    #[allow(dead_code)]
    pub fn with_genesis_config(mut self) -> Self {
        let _ = pallet_ethereum_events::GenesisConfig::<TestRuntime> {
            nft_t1_contracts: vec![(H160::from(NFT_CONTRACT), ())],
            processed_events: vec![],
            lift_tx_hashes: vec![],
            quorum_factor: QUORUM_FACTOR,
            event_challenge_period: EVENT_CHALLENGE_PERIOD,
        }
        .assimilate_storage(&mut self.storage);

        let _ = pallet_avn::GenesisConfig::<TestRuntime> {
            _phantom: Default::default(),
            bridge_contract_address: H160::from(BRIDGE_CONTRACT),
        }
        .assimilate_storage(&mut self.storage);

        self
    }

    pub fn with_genesis_and_initial_lifts(mut self) -> Self {
        let _ = pallet_ethereum_events::GenesisConfig::<TestRuntime> {
            nft_t1_contracts: vec![(H160::from(NFT_CONTRACT), ())],
            processed_events: create_initial_processed_events(),
            lift_tx_hashes: vec![
                H256::from(INITIAL_LIFTS[0]),
                H256::from(INITIAL_LIFTS[1]),
                H256::from(INITIAL_LIFTS[2]),
                H256::from(INITIAL_LIFTS[3]),
            ],
            quorum_factor: QUORUM_FACTOR,
            event_challenge_period: EVENT_CHALLENGE_PERIOD,
        }
        .assimilate_storage(&mut self.storage);

        let _ = pallet_avn::GenesisConfig::<TestRuntime> {
            _phantom: Default::default(),
            bridge_contract_address: H160::from(BRIDGE_CONTRACT),
        }
        .assimilate_storage(&mut self.storage);

        self
    }

    pub fn invalid_config_with_zero_validator_threshold(mut self) -> Self {
        let _ = pallet_ethereum_events::GenesisConfig::<TestRuntime> {
            quorum_factor: 0,
            event_challenge_period: EVENT_CHALLENGE_PERIOD,
            ..Default::default()
        }
        .assimilate_storage(&mut self.storage);
        self
    }

    #[allow(dead_code)]
    pub fn with_validators(mut self) -> Self {
        let validators: Vec<AccountId> = VALIDATORS.with(|l| l.borrow_mut().take().unwrap());

        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for ref k in &validators {
                frame_system::Pallet::<TestRuntime>::inc_providers(k);
            }
        });

        let _ = pallet_session::GenesisConfig::<TestRuntime> {
            keys: validators
                .into_iter()
                .enumerate()
                .map(|(i, v)| (v, v, UintAuthorityId((i as u32).into())))
                .collect(),
            ..Default::default()
        }
        .assimilate_storage(&mut self.storage);
        self
    }

    #[allow(dead_code)]
    pub fn for_offchain_worker(mut self) -> Self {
        assert!(!self.offchain_registered);
        let (offchain, offchain_state) = TestOffchainExt::new();
        let (pool, pool_state) = TestTransactionPoolExt::new();
        self.txpool_extension = Some(pool);
        self.offchain_extension = Some(offchain);
        self.pool_state = Some(pool_state);
        self.offchain_state = Some(offchain_state);
        self.offchain_registered = true;
        self
    }

    #[allow(dead_code)]
    pub fn as_externality(self) -> sp_io::TestExternalities {
        use env_logger::{Builder, Env};
        let mut ext = sp_io::TestExternalities::from(self.storage);
        let env = Env::new().default_filter_or("off");
        let _ = Builder::from_env(env).is_test(true).try_init();
        ext.execute_with(|| System::set_block_number(1));
        ext
    }

    #[allow(dead_code)]
    pub fn as_externality_with_state(
        self,
    ) -> (TestExternalities, Arc<RwLock<PoolState>>, Arc<RwLock<OffchainState>>) {
        use env_logger::{Builder, Env};
        assert!(self.offchain_registered);
        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(OffchainExt::new(self.offchain_extension.clone().unwrap()));
        ext.register_extension(OffchainWorkerExt::new(self.offchain_extension.unwrap()));
        ext.register_extension(TransactionPoolExt::new(self.txpool_extension.unwrap()));
        assert!(self.pool_state.is_some());
        assert!(self.offchain_state.is_some());
        ext.execute_with(|| System::set_block_number(1));
        let env = Env::new().default_filter_or("off");
        let _ = Builder::from_env(env).is_test(true).try_init();
        (ext, self.pool_state.unwrap(), self.offchain_state.unwrap())
    }
}

impl FeePaymentHandler for TestRuntime {
    type Token = sp_core::H160;
    type TokenBalance = u128;
    type AccountId = AccountId;
    type Error = DispatchError;

    fn pay_fee(
        _token: &Self::Token,
        _amount: &Self::TokenBalance,
        _payer: &Self::AccountId,
        _recipient: &Self::AccountId,
    ) -> Result<(), Self::Error> {
        return Err(DispatchError::Other("Test - Error"));
    }

    fn pay_treasury(
        _amount: &Self::TokenBalance,
        _payer: &Self::AccountId,
    ) -> Result<(), Self::Error> {
        return Err(DispatchError::Other("Test - Error"));
    }
}
