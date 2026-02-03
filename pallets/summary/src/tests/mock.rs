// Copyright 2022 Aventus Network Services (UK) Ltd.

pub use crate::{self as summary, *};
use frame_support::{derive_impl, parameter_types};
use sp_state_machine::BasicExternalities;

use frame_system::{self as system, DefaultConfig};
use pallet_avn::{
    self as avn, testing::U64To32BytesConverter, vote::VotingSessionData, EthereumPublicKeyChecker,
};
use pallet_eth_bridge::offence::EthBridgeOffence;
use pallet_session as session;
use parking_lot::RwLock;
use sp_avn_common::{eth::LowerParams, safe_add_block_numbers, safe_sub_block_numbers};
use sp_core::{
    ecdsa,
    offchain::{
        testing::{
            OffchainState, PendingRequest, PoolState, TestOffchainExt, TestTransactionPoolExt,
        },
        OffchainDbExt, OffchainWorkerExt, TransactionPoolExt,
    },
    ConstU64, H256,
};
use sp_runtime::{
    testing::{TestSignature, TestXt, UintAuthorityId},
    traits::ConvertInto,
    BuildStorage,
};
use sp_staking::{
    offence::{OffenceError, ReportOffence},
    SessionIndex,
};
use sp_watchtower::NoopWatchtower;
use std::{cell::RefCell, convert::From, sync::Arc};
use system::pallet_prelude::BlockNumberFor;

pub const APPROVE_ROOT: bool = true;
pub const REJECT_ROOT: bool = false;

pub type Extrinsic = TestXt<RuntimeCall, ()>;

pub type AccountId = <TestRuntime as system::Config>::AccountId;
pub type BlockNumber = BlockNumberFor<TestRuntime>;

impl Summary {
    pub fn get_root_data(root_id: &RootId<BlockNumber>) -> RootData<AccountId> {
        return Roots::<TestRuntime>::get(root_id.range, root_id.ingress_counter)
    }

    pub fn insert_root_hash(
        root_id: &RootId<BlockNumber>,
        root_hash: H256,
        account_id: AccountId,
        tx_id: EthereumId,
    ) {
        Roots::<TestRuntime>::insert(
            root_id.range,
            root_id.ingress_counter,
            RootData::new(root_hash, account_id, Some(tx_id)),
        );
    }

    pub fn set_schedule_and_voting_periods(
        schedule_period: BlockNumber,
        voting_period: BlockNumber,
    ) {
        SchedulePeriod::<TestRuntime>::put(schedule_period);
        VotingPeriod::<TestRuntime>::put(voting_period);
    }

    pub fn set_root_as_validated(root_id: &RootId<BlockNumber>) {
        Roots::<TestRuntime>::mutate(root_id.range, root_id.ingress_counter, |root| {
            root.is_validated = true
        });
    }

    pub fn set_next_block_to_process(next_block_number_to_process: BlockNumber) {
        NextBlockToProcess::<TestRuntime>::put(next_block_number_to_process);
    }

    pub fn set_next_slot_block_number(slot_block_number: BlockNumber) {
        NextSlotAtBlock::<TestRuntime>::put(slot_block_number);
    }

    pub fn set_current_slot(slot: BlockNumber) {
        CurrentSlot::<TestRuntime>::put(slot);
    }

    pub fn set_current_slot_validator(validator_account: AccountId) {
        CurrentSlotsValidator::<TestRuntime>::put(validator_account);
    }

    pub fn set_previous_summary_slot(slot: BlockNumber) {
        SlotOfLastPublishedSummary::<TestRuntime>::put(slot);
    }

    pub fn get_block_number() -> BlockNumber {
        return System::block_number()
    }

    pub fn insert_pending_approval(root_id: &RootId<BlockNumber>) {
        PendingApproval::<TestRuntime>::insert(root_id.range, root_id.ingress_counter);
    }

    pub fn remove_pending_approval(root_range: &RootRange<BlockNumber>) {
        PendingApproval::<TestRuntime>::remove(root_range);
    }

    pub fn get_vote_for_root(
        root_id: &RootId<BlockNumber>,
    ) -> VotingSessionData<AccountId, BlockNumber> {
        VotesRepository::<TestRuntime>::get(root_id)
    }

    pub fn register_root_for_voting(
        root_id: &RootId<BlockNumber>,
        quorum: u32,
        voting_period_end: u64,
    ) {
        VotesRepository::<TestRuntime>::insert(
            root_id,
            VotingSessionData::new(root_id.session_id(), quorum, voting_period_end, 0),
        );
    }

    pub fn deregister_root_for_voting(root_id: &RootId<BlockNumber>) {
        VotesRepository::<TestRuntime>::remove(root_id);
    }

    pub fn record_approve_vote(root_id: &RootId<BlockNumber>, voter: AccountId) {
        VotesRepository::<TestRuntime>::mutate(root_id, |vote| {
            vote.ayes.try_push(voter).expect("Failed to record aye vote");
        });
    }

    pub fn record_reject_vote(root_id: &RootId<BlockNumber>, voter: AccountId) {
        VotesRepository::<TestRuntime>::mutate(root_id, |vote| {
            vote.nays.try_push(voter).expect("Failed to record nay vote");
        });
    }

    pub fn set_total_ingresses(ingress_counter: IngressCounter) {
        <TotalIngresses<TestRuntime>>::put(ingress_counter);
    }

    pub fn emitted_event(event: &RuntimeEvent) -> bool {
        return System::events().iter().any(|a| a.event == *event)
    }

    pub fn emitted_event_for_offence_of_type(offence_type: SummaryOffenceType) -> bool {
        return System::events()
            .iter()
            .any(|e| Self::event_matches_offence_type(&e.event, offence_type.clone()))
    }

    pub fn event_matches_offence_type(event: &RuntimeEvent, this_type: SummaryOffenceType) -> bool {
        return matches!(event,
            mock::RuntimeEvent::Summary(
                crate::Event::<TestRuntime>::SummaryOffenceReported{ offence_type, .. }
            )
            if this_type == *offence_type
        )
    }

    pub fn total_events_emitted() -> usize {
        return System::events().len()
    }

    pub fn create_mock_identification_tuple(account_id: AccountId) -> (AccountId, AccountId) {
        return (account_id, account_id)
    }

    pub fn get_offence_record() -> Vec<(Vec<ValidatorId>, Offence)> {
        return OFFENCES.with(|o| o.borrow().to_vec())
    }

    pub fn reported_offence(
        reporter: AccountId,
        validator_count: u32,
        offenders: Vec<ValidatorId>,
        offence_type: SummaryOffenceType,
    ) -> bool {
        let offences = Self::get_offence_record();

        return offences.iter().any(|o| {
            Self::offence_matches_criteria(
                o,
                vec![reporter],
                validator_count,
                offenders.iter().map(|v| Self::create_mock_identification_tuple(*v)).collect(),
                offence_type.clone(),
            )
        })
    }

    pub fn reported_offence_of_type(offence_type: SummaryOffenceType) -> bool {
        let offences = Self::get_offence_record();

        return offences.iter().any(|o| Self::offence_is_of_type(o, offence_type.clone()))
    }

    fn offence_matches_criteria(
        this_report: &(Vec<ValidatorId>, Offence),
        these_reporters: Vec<ValidatorId>,
        this_count: u32,
        these_offenders: Vec<(ValidatorId, FullIdentification)>,
        this_type: SummaryOffenceType,
    ) -> bool {
        return matches!(
            this_report,
            (
                reporters,
                SummaryOffence {
                    session_index: _,
                    validator_set_count,
                    offenders,
                    offence_type}
            )
            if these_reporters == *reporters
            && this_count == *validator_set_count
            && these_offenders == *offenders
            && this_type == *offence_type
        )
    }

    fn offence_is_of_type(
        this_report: &(Vec<ValidatorId>, Offence),
        this_type: SummaryOffenceType,
    ) -> bool {
        return matches!(
            this_report,
            (
                _,
                SummaryOffence {
                    session_index: _,
                    validator_set_count: _,
                    offenders: _,
                    offence_type}
            )
            if this_type == *offence_type
        )
    }
}

impl AnchorSummary {
    pub fn get_root_data(root_id: &RootId<BlockNumber>) -> RootData<AccountId> {
        return Roots::<TestRuntime, Instance1>::get(root_id.range, root_id.ingress_counter)
    }

    pub fn insert_root_hash(
        root_id: &RootId<BlockNumber>,
        root_hash: H256,
        account_id: AccountId,
        tx_id: EthereumId,
    ) {
        Roots::<TestRuntime, Instance1>::insert(
            root_id.range,
            root_id.ingress_counter,
            RootData::new(root_hash, account_id, Some(tx_id)),
        );
    }

    pub fn set_schedule_and_voting_periods(
        schedule_period: BlockNumber,
        voting_period: BlockNumber,
    ) {
        SchedulePeriod::<TestRuntime, Instance1>::put(schedule_period);
        VotingPeriod::<TestRuntime, Instance1>::put(voting_period);
    }

    pub fn set_root_as_validated(root_id: &RootId<BlockNumber>) {
        Roots::<TestRuntime, Instance1>::mutate(root_id.range, root_id.ingress_counter, |root| {
            root.is_validated = true
        });
    }

    pub fn set_next_block_to_process(next_block_number_to_process: BlockNumber) {
        NextBlockToProcess::<TestRuntime, Instance1>::put(next_block_number_to_process);
    }

    pub fn set_next_slot_block_number(slot_block_number: BlockNumber) {
        NextSlotAtBlock::<TestRuntime, Instance1>::put(slot_block_number);
    }

    pub fn set_current_slot(slot: BlockNumber) {
        CurrentSlot::<TestRuntime, Instance1>::put(slot);
    }

    pub fn set_current_slot_validator(validator_account: AccountId) {
        CurrentSlotsValidator::<TestRuntime, Instance1>::put(validator_account);
    }

    pub fn set_previous_summary_slot(slot: BlockNumber) {
        SlotOfLastPublishedSummary::<TestRuntime, Instance1>::put(slot);
    }

    pub fn insert_pending_approval(root_id: &RootId<BlockNumber>) {
        PendingApproval::<TestRuntime, Instance1>::insert(root_id.range, root_id.ingress_counter);
    }

    pub fn remove_pending_approval(root_range: &RootRange<BlockNumber>) {
        PendingApproval::<TestRuntime, Instance1>::remove(root_range);
    }

    pub fn get_vote_for_root(
        root_id: &RootId<BlockNumber>,
    ) -> VotingSessionData<AccountId, BlockNumber> {
        VotesRepository::<TestRuntime, Instance1>::get(root_id)
    }

    pub fn register_root_for_voting(
        root_id: &RootId<BlockNumber>,
        quorum: u32,
        voting_period_end: u64,
    ) {
        VotesRepository::<TestRuntime, Instance1>::insert(
            root_id,
            VotingSessionData::new(root_id.session_id(), quorum, voting_period_end, 0),
        );
    }

    pub fn record_approve_vote(root_id: &RootId<BlockNumber>, voter: AccountId) {
        VotesRepository::<TestRuntime, Instance1>::mutate(root_id, |vote| {
            vote.ayes.try_push(voter).expect("Failed to record aye vote");
        });
    }

    pub fn set_total_ingresses(ingress_counter: IngressCounter) {
        <TotalIngresses<TestRuntime, Instance1>>::put(ingress_counter);
    }
}

type Block = frame_system::mocking::MockBlock<TestRuntime>;
frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
        Avn: pallet_avn::{Pallet, Storage, Event},
        Summary: summary::{Pallet, Call, Storage, Event<T>, Config<T>},
        Historical: pallet_session::historical::{Pallet, Storage},
        EthBridge: pallet_eth_bridge::{Pallet, Call, Storage, Event<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
        AnchorSummary: summary::<Instance1>::{Pallet, Call, Storage, Event<T>, Config<T>},
    }
);

pub type ValidatorId = u64;
type FullIdentification = u64;

pub const INITIAL_TRANSACTION_ID: EthereumId = 0;
pub const VALIDATOR_COUNT: u32 = 7;
thread_local! {
    // validator accounts (aka public addresses, public keys-ish)
    pub static VALIDATORS: RefCell<Option<Vec<ValidatorId>>> = RefCell::new(Some(vec![
        FIRST_VALIDATOR_INDEX,
        SECOND_VALIDATOR_INDEX,
        THIRD_VALIDATOR_INDEX,
        FOURTH_VALIDATOR_INDEX,
        FIFTH_VALIDATOR_INDEX,
        SIXTH_VALIDATOR_INDEX,
        SEVENTH_VALIDATOR_INDEX
    ]));

    static MOCK_TX_ID: RefCell<EthereumId> = RefCell::new(INITIAL_TRANSACTION_ID);

    static ETH_PUBLIC_KEY_VALID: RefCell<bool> = RefCell::new(true);

    static MOCK_RECOVERED_ACCOUNT_ID: RefCell<AccountId> = RefCell::new(FIRST_VALIDATOR_INDEX);
}

parameter_types! {
    pub const AdvanceSlotGracePeriod: u64 = 5;
    pub const MinBlockAge: u64 = 5;
    pub const ExternalValidationEnabled: bool = true;
    pub const BlockHashCount: u64 = 250;
    pub const AutoSubmitSummaries: bool = true;
    pub const DoNotAutoSubmitAnchor: bool = false;
    pub const InstanceId: u8 = 1u8;
    pub const AnchorInstanceId: u8 = 2u8;
}

impl Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type AdvanceSlotGracePeriod = AdvanceSlotGracePeriod;
    type MinBlockAge = MinBlockAge;
    type AccountToBytesConvert = U64To32BytesConverter;
    type ReportSummaryOffence = OffenceHandler;
    type WeightInfo = ();
    type BridgeInterface = EthBridge;
    type AutoSubmitSummaries = AutoSubmitSummaries;
    type InstanceId = InstanceId;
    type ExternalValidator = NoopWatchtower<AccountId>;
    type ExternalValidationEnabled = ExternalValidationEnabled;
}

type AvnAnchorSummary = summary::Instance1;
impl Config<AvnAnchorSummary> for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type AdvanceSlotGracePeriod = AdvanceSlotGracePeriod;
    type MinBlockAge = MinBlockAge;
    type AccountToBytesConvert = U64To32BytesConverter;
    type ReportSummaryOffence = OffenceHandler;
    type WeightInfo = ();
    type BridgeInterface = EthBridge;
    type AutoSubmitSummaries = DoNotAutoSubmitAnchor;
    type InstanceId = AnchorInstanceId;
    type ExternalValidator = NoopWatchtower<AccountId>;
    type ExternalValidationEnabled = ExternalValidationEnabled;
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
}

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl avn::Config for TestRuntime {
    type AuthorityId = UintAuthorityId;
}

impl pallet_eth_bridge::Config for TestRuntime {
    type MaxQueuedTxRequests = frame_support::traits::ConstU32<100>;
    type RuntimeEvent = RuntimeEvent;
    type TimeProvider = Timestamp;
    type RuntimeCall = RuntimeCall;
    type MinEthBlockConfirmation = ConstU64<20>;
    type WeightInfo = ();
    type AccountToBytesConvert = Avn;
    type BridgeInterfaceNotification = Self;
    type ReportCorroborationOffence = OffenceHandler;
    type ProcessedEventsChecker = ();
    type ProcessedEventsHandler = ();
    type EthereumEventsMigration = ();
    type Quorum = Avn;
}

impl pallet_timestamp::Config for TestRuntime {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = frame_support::traits::ConstU64<12000>;
    type WeightInfo = ();
}

impl BridgeInterfaceNotification for TestRuntime {
    fn process_result(
        _tx_id: EthereumId,
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
impl BridgeInterface for TestRuntime {
    fn publish(
        function_name: &[u8],
        _params: &[(Vec<u8>, Vec<u8>)],
        _caller_id: Vec<u8>,
    ) -> Result<EthereumId, DispatchError> {
        if function_name == BridgeContractMethod::PublishRoot.name_as_bytes() {
            return Ok(INITIAL_TRANSACTION_ID)
        }
        Err(Error::<TestRuntime>::ErrorPublishingSummary.into())
    }

    fn generate_lower_proof(_: u32, _: &LowerParams, _: Vec<u8>) -> Result<(), DispatchError> {
        Ok(())
    }

    fn read_bridge_contract(
        _: Vec<u8>,
        _: &[u8],
        _: &[(Vec<u8>, Vec<u8>)],
        _: Option<u32>,
    ) -> Result<Vec<u8>, DispatchError> {
        Ok(vec![])
    }

    fn latest_finalised_ethereum_block() -> Result<u32, DispatchError> {
        Ok(0)
    }
}

/*********************** Add validators support ********************** */

pub struct TestSessionManager;
impl session::SessionManager<u64> for TestSessionManager {
    fn new_session(_new_index: SessionIndex) -> Option<Vec<ValidatorId>> {
        VALIDATORS.with(|l| l.borrow_mut().take())
    }
    fn end_session(_: SessionIndex) {}
    fn start_session(_: SessionIndex) {}
}

impl pallet_session::historical::Config for TestRuntime {
    type FullIdentification = u64;
    type FullIdentificationOf = ConvertInto;
}

impl pallet_session::historical::SessionManager<ValidatorId, FullIdentification>
    for TestSessionManager
{
    fn new_session(_new_index: SessionIndex) -> Option<Vec<(ValidatorId, FullIdentification)>> {
        VALIDATORS.with(|l| {
            l.borrow_mut()
                .take()
                .map(|validators| validators.iter().map(|v| (*v, *v)).collect())
        })
    }
    fn end_session(_: SessionIndex) {}
    fn start_session(_: SessionIndex) {}
}

type IdentificationTuple = (ValidatorId, FullIdentification);
type Offence = crate::SummaryOffence<IdentificationTuple>;

thread_local! {
    pub static OFFENCES: RefCell<Vec<(Vec<ValidatorId>, Offence)>> = RefCell::new(vec![]);
    pub static ETH_BRIDGE_OFFENCES: RefCell<Vec<(Vec<ValidatorId>, EthBridgeOffence<IdentificationTuple>)>> = RefCell::new(vec![]);
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

impl ReportOffence<AccountId, IdentificationTuple, EthBridgeOffence<IdentificationTuple>>
    for OffenceHandler
{
    fn report_offence(
        reporters: Vec<AccountId>,
        offence: EthBridgeOffence<IdentificationTuple>,
    ) -> Result<(), OffenceError> {
        ETH_BRIDGE_OFFENCES.with(|l| l.borrow_mut().push((reporters, offence)));
        Ok(())
    }

    fn is_known_offence(_offenders: &[IdentificationTuple], _time_slot: &SessionIndex) -> bool {
        false
    }
}

impl session::Config for TestRuntime {
    type SessionManager =
        pallet_session::historical::NoteHistoricalRoot<TestRuntime, TestSessionManager>;
    type Keys = UintAuthorityId;
    type ShouldEndSession = session::PeriodicSessions<Period, Offset>;
    type SessionHandler = (Avn,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = u64;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = session::PeriodicSessions<Period, Offset>;
    type WeightInfo = ();
}

pub struct ExtBuilder {
    pub storage: sp_runtime::Storage,
    offchain_state: Option<Arc<RwLock<OffchainState>>>,
    pool_state: Option<Arc<RwLock<PoolState>>>,
    txpool_extension: Option<TestTransactionPoolExt>,
    offchain_extension: Option<TestOffchainExt>,
    offchain_registered: bool,
}

impl ExtBuilder {
    pub fn build_default() -> Self {
        let storage =
            frame_system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();
        Self {
            storage,
            pool_state: None,
            offchain_state: None,
            txpool_extension: None,
            offchain_extension: None,
            offchain_registered: false,
        }
    }

    pub fn as_externality(self) -> sp_io::TestExternalities {
        let mut ext = sp_io::TestExternalities::from(self.storage);
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| {
            Timestamp::set_timestamp(1);
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into());
            ExternalValidationThreshold::<TestRuntime>::put(51u32);
        });
        ext
    }

    pub fn with_validators(mut self) -> Self {
        let validators: Vec<ValidatorId> = VALIDATORS.with(|l| l.borrow_mut().take().unwrap());
        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for ref k in &validators {
                frame_system::Pallet::<TestRuntime>::inc_providers(k);
            }
        });
        let _ = session::GenesisConfig::<TestRuntime> {
            keys: validators.into_iter().map(|v| (v, v, UintAuthorityId(v))).collect(),
            ..Default::default()
        }
        .assimilate_storage(&mut self.storage);
        self
    }

    pub fn with_validator_count(self, count: u64) -> Self {
        let validators_range: Vec<ValidatorId> = (1..=count).collect();
        VALIDATORS.with(|l| *l.borrow_mut() = Some(validators_range));

        return self.with_validators()
    }

    pub fn with_genesis_config(mut self) -> Self {
        let _ = summary::GenesisConfig::<TestRuntime> {
            schedule_period: 160,
            voting_period: 100,
            _phantom: Default::default(),
        }
        .assimilate_storage(&mut self.storage);
        let _ = summary::GenesisConfig::<TestRuntime, Instance1> {
            schedule_period: 170,
            voting_period: 101,
            _phantom: Default::default(),
        }
        .assimilate_storage(&mut self.storage);
        self
    }

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

    pub fn as_externality_with_state(
        self,
    ) -> (sp_io::TestExternalities, Arc<RwLock<PoolState>>, Arc<RwLock<OffchainState>>) {
        assert!(self.offchain_registered);
        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(OffchainDbExt::new(self.offchain_extension.clone().unwrap()));
        ext.register_extension(OffchainWorkerExt::new(self.offchain_extension.unwrap()));
        ext.register_extension(TransactionPoolExt::new(self.txpool_extension.unwrap()));
        assert!(self.pool_state.is_some());
        assert!(self.offchain_state.is_some());
        ext.execute_with(|| {
            Timestamp::set_timestamp(1);
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into());
            ExternalValidationThreshold::<TestRuntime>::put(51u32);
        });
        (ext, self.pool_state.unwrap(), self.offchain_state.unwrap())
    }
}

impl EthereumPublicKeyChecker<u64> for TestRuntime {
    fn get_validator_for_eth_public_key(_eth_public_key: &ecdsa::Public) -> Option<u64> {
        match ETH_PUBLIC_KEY_VALID.with(|pk| *pk.borrow()) {
            true => Some(MOCK_RECOVERED_ACCOUNT_ID.with(|pk| *pk.borrow())),
            _ => None,
        }
    }
}

pub fn set_mock_recovered_account_id(account_id: AccountId) {
    MOCK_RECOVERED_ACCOUNT_ID.with(|acc_id| {
        *acc_id.borrow_mut() = account_id;
    });
}

/*********************** Mocking ********************** */

pub const ROOT_HASH_HEX_STRING: &'static [u8; 64] =
    b"8736c9e671fe581fe4ef4631112038297dcdecae163e8724c281ece8ad94c8c3";
pub const ROOT_HASH_BYTES: [u8; 32] = [
    135, 54, 201, 230, 113, 254, 88, 31, 228, 239, 70, 49, 17, 32, 56, 41, 125, 205, 236, 174, 22,
    62, 135, 36, 194, 129, 236, 232, 173, 148, 200, 195,
];
const ROOT_HASH_CAUSES_SUBMISSION_TO_T1_ERROR: [u8; 32] = [
    111, 54, 201, 230, 113, 254, 88, 31, 228, 239, 70, 49, 17, 32, 56, 41, 125, 205, 236, 174, 22,
    62, 135, 36, 194, 129, 236, 232, 173, 148, 200, 195,
];
pub const OTHER_CONTEXT: &'static [u8] = b"other_tx_context"; // TODO: Share it in a centralised avt suport pallet for testing
const CURRENT_BLOCK_NUMBER: u64 = 10;
const NEXT_BLOCK_TO_PROCESS: u64 = 3;
pub const CURRENT_SLOT: u64 = 5;
pub const VOTING_PERIOD_END: u64 = 12;
pub const QUORUM: u32 = 3;
pub const FIRST_VALIDATOR_INDEX: u64 = 1;
pub const SECOND_VALIDATOR_INDEX: u64 = 2;
pub const THIRD_VALIDATOR_INDEX: u64 = 3;
pub const FOURTH_VALIDATOR_INDEX: u64 = 4;
pub const FIFTH_VALIDATOR_INDEX: u64 = 5;
pub const SIXTH_VALIDATOR_INDEX: u64 = 6;
pub const SEVENTH_VALIDATOR_INDEX: u64 = 7;
pub const VALIDATORS_COUNT: u64 = 7;
pub const DEFAULT_INGRESS_COUNTER: IngressCounter = 100;

#[derive(Clone)]
pub struct Context {
    pub current_block_number: u64,
    pub next_block_to_process: u64,
    pub last_block_in_range: u64,
    pub validator: Validator<UintAuthorityId, u64>,
    pub root_hash_h256: H256,
    pub root_hash_vec: Vec<u8>,
    pub url_param: String,
    pub record_summary_calculation_signature: TestSignature,
    pub root_id: RootId<BlockNumber>,
    pub tx_id: EthereumId,
    pub current_slot: BlockNumber,
    pub finalised_block_vec: Option<Vec<u8>>,
}

pub const DEFAULT_SCHEDULE_PERIOD: u64 = 2;
pub const DEFAULT_VOTING_PERIOD: u64 = 2;

pub fn setup_context() -> Context {
    Summary::set_schedule_and_voting_periods(DEFAULT_SCHEDULE_PERIOD, DEFAULT_VOTING_PERIOD);

    let current_block_number = CURRENT_BLOCK_NUMBER;
    let next_block_to_process = NEXT_BLOCK_TO_PROCESS;
    let last_block_in_range = next_block_to_process + Summary::schedule_period() - 1;
    let root_hash_h256 = H256::from(ROOT_HASH_BYTES);
    let root_hash_vec = ROOT_HASH_HEX_STRING.to_vec();
    let root_id = RootId::new(
        RootRange::new(next_block_to_process, last_block_in_range),
        DEFAULT_INGRESS_COUNTER,
    );
    let validator = get_validator(FIRST_VALIDATOR_INDEX);
    let tx_id = 0;
    let finalised_block_vec = Some(hex::encode(0u32.encode()).into());

    Context {
        current_block_number,
        current_slot: CURRENT_SLOT,
        next_block_to_process,
        last_block_in_range,
        url_param: get_url_param(next_block_to_process, Summary::schedule_period()),
        validator: validator.clone(),
        root_hash_h256,
        root_hash_vec,
        root_id,
        record_summary_calculation_signature: get_signature_for_record_summary_calculation(
            validator,
            &Summary::update_block_number_context(),
            root_hash_h256,
            root_id.ingress_counter,
            last_block_in_range,
        ),
        tx_id,
        finalised_block_vec,
    }
}

pub fn setup_blocks(context: &Context) {
    System::set_block_number(context.current_block_number);
    Summary::set_next_block_to_process(context.next_block_to_process);
    Summary::set_next_slot_block_number(context.current_block_number + Summary::schedule_period());
    Summary::set_current_slot_validator(context.validator.account_id);
}

pub fn setup_total_ingresses(context: &Context) {
    Summary::set_total_ingresses(context.root_id.ingress_counter - 1);
}

pub fn setup_voting(
    root_id: &RootId<BlockNumber>,
    root_hash_h256: H256,
    validator: &Validator<UintAuthorityId, u64>,
) {
    let tx_id: EthereumId = INITIAL_TRANSACTION_ID;
    Summary::insert_root_hash(root_id, root_hash_h256, validator.account_id.clone(), tx_id);
    Summary::insert_pending_approval(root_id);
    Summary::register_root_for_voting(root_id, QUORUM, VOTING_PERIOD_END);

    assert_eq!(Summary::get_vote(root_id).ayes.is_empty(), true);
    assert_eq!(Summary::get_vote(root_id).nays.is_empty(), true);
}

pub fn setup_voting_for_root_id(context: &Context) {
    setup_blocks(&context);
    setup_voting(&context.root_id, context.root_hash_h256, &context.validator);
}

pub fn mock_response_of_get_roothash(
    state: &mut OffchainState,
    url_param: String,
    response: Option<Vec<u8>>,
) {
    let mut url = "http://127.0.0.1:2020/roothash/".to_string();
    url.push_str(&url_param);

    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: url.into(),
        response,
        sent: true,
        ..Default::default()
    });
}

pub fn mock_response_of_get_finalised_block(state: &mut OffchainState, response: &Option<Vec<u8>>) {
    let url = "http://127.0.0.1:2020/latest_finalised_block".to_string();

    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: url.into(),
        response: response.clone(),
        sent: true,
        ..Default::default()
    });
}

pub fn get_non_validator() -> Validator<UintAuthorityId, u64> {
    get_validator(10)
}

pub fn get_signature_for_approve_cast_vote(
    signer: &Validator<UintAuthorityId, u64>,
    context: &[u8],
    root_id: &RootId<BlockNumber>,
) -> TestSignature {
    signer
        .key
        .sign(&(context, root_id.encode(), APPROVE_ROOT).encode())
        .expect("Signature is signed")
}

pub fn get_signature_for_reject_cast_vote(
    validator: &Validator<UintAuthorityId, u64>,
    context: &[u8],
    root_id: &RootId<BlockNumber>,
) -> TestSignature {
    validator
        .key
        .sign(&(context, root_id.encode(), REJECT_ROOT).encode())
        .expect("Signature is signed")
}

pub fn get_url_param(next_block_to_process: u64, schedule_period: u64) -> String {
    let mut url_param = next_block_to_process.to_string();
    url_param.push_str(&"/".to_string());

    let adjust = if next_block_to_process > 0 { 1 } else { 0 };
    let last_block = next_block_to_process + schedule_period - adjust;
    url_param.push_str(&last_block.to_string());
    url_param
}

pub fn get_validator(index: u64) -> Validator<UintAuthorityId, u64> {
    Validator { account_id: index, key: UintAuthorityId(index) }
}

pub fn get_signature_for_record_summary_calculation(
    validator: Validator<UintAuthorityId, u64>,
    context: &[u8],
    root_hash_h256: H256,
    ingress_counter: IngressCounter,
    last_block_in_range: BlockNumber,
) -> TestSignature {
    validator
        .key
        .sign(&(context, root_hash_h256, ingress_counter, last_block_in_range).encode())
        .expect("Signature is signed")
}

pub fn get_root_hash_return_submit_to_tier1_fails() -> H256 {
    H256::from(ROOT_HASH_CAUSES_SUBMISSION_TO_T1_ERROR)
}

pub fn advance_block_numbers(number_of_blocks: u64) -> BlockNumber {
    let now = System::block_number().max(1);
    let new_block_number =
        safe_add_block_numbers(now, number_of_blocks).expect("Advanced block number is valid");
    System::set_block_number(new_block_number);
    return new_block_number
}

pub fn retreat_block_numbers(number_of_blocks: u64) -> BlockNumber {
    let now = System::block_number().max(1);
    let new_block_number =
        safe_sub_block_numbers(now, number_of_blocks).expect("Retreated block number is valid");
    System::set_block_number(new_block_number);
    return new_block_number
}
