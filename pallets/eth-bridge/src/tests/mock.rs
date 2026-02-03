use super::*;
use crate::{self as eth_bridge, request::add_new_send_request};
use frame_support::{derive_impl, parameter_types};
use sp_state_machine::BasicExternalities;

use frame_system::{self as system, DefaultConfig};
use pallet_avn::{testing::U64To32BytesConverter, EthereumPublicKeyChecker};
use pallet_session as session;
use parking_lot::RwLock;
use sp_avn_common::{
    eth::{concat_lower_data, EthereumId, LowerParams},
    event_discovery::filters::AllPrimaryEventsFilter,
    event_types::EthEvent,
    BridgeContractMethod,
};

use sp_core::{
    offchain::{
        testing::{OffchainState, PoolState, TestOffchainExt, TestTransactionPoolExt},
        OffchainDbExt, OffchainWorkerExt, TransactionPoolExt,
    },
    ConstU32, ConstU64, H160, H256,
};
use sp_runtime::{
    testing::{TestSignature, TestXt, UintAuthorityId},
    traits::ConvertInto,
    BuildStorage, DispatchError, DispatchResult, Perbill,
};
use sp_staking::offence::OffenceError;
use std::{cell::RefCell, convert::From, sync::Arc};

thread_local! {
    pub static OFFENCES: RefCell<Vec<(Vec<AccountId>, Offence)>> = RefCell::new(vec![]);
}

pub type Block = frame_system::mocking::MockBlock<TestRuntime>;
pub type Extrinsic = TestXt<RuntimeCall, ()>;
pub type AccountId = u64;
pub type BlockNumber = u64;

type IdentificationTuple = (AccountId, AccountId);
type Offence = crate::EthBridgeOffence<IdentificationTuple>;
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
#[derive(Clone)]
pub struct Context {
    pub eth_tx_hash: H256,
    pub test_signature: TestSignature,
    pub confirmation_signature: ecdsa::Signature,
    pub author: Author<TestRuntime>,
    pub confirming_author: Author<TestRuntime>,
    pub second_confirming_author: Author<TestRuntime>,
    pub third_confirming_author: Author<TestRuntime>,
    pub request_function_name: Vec<u8>,
    pub request_params: Vec<(Vec<u8>, Vec<u8>)>,
    pub lower_params: LowerParams,
    pub finalised_block_vec: Option<Vec<u8>>,
    pub lower_id: u32,
    pub block_number: BlockNumber,
    pub expected_lower_msg_hash: String,
    pub replay_attempt: u16,
}

impl Context {
    pub fn create_sign_proof(&self, author: AccountId) -> TestSignature {
        let authority = Author::<TestRuntime> { key: UintAuthorityId(author), account_id: author };

        let h256 = H256::from_slice(
            &hex::decode(self.expected_lower_msg_hash.clone()).expect("failed to decode hex"),
        );
        authority.key.sign(&h256.as_ref().to_vec()).expect("sign proof failed")
    }
}

const ROOT_HASH: &str = "30b83f0d722d1d4308ab4660a72dbaf0a7392d5674eca3cd21d57256d42df7a0";

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Timestamp: pallet_timestamp,
        Avn: pallet_avn::{Pallet, Storage, Event},
        EthBridge: eth_bridge::{Pallet, Call, Storage, Event<T>, Config<T>},
        Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
    }
);

impl<LocalCall> frame_system::offchain::SendTransactionTypes<LocalCall> for TestRuntime
where
    RuntimeCall: From<LocalCall>,
{
    type OverarchingCall = RuntimeCall;
    type Extrinsic = Extrinsic;
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const SS58Prefix: u8 = 42;
}

impl Config for TestRuntime {
    type MaxQueuedTxRequests = ConstU32<100>;
    type RuntimeEvent = RuntimeEvent;
    type TimeProvider = Timestamp;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = ();
    type MinEthBlockConfirmation = ConstU64<20>;
    type AccountToBytesConvert = U64To32BytesConverter;
    type BridgeInterfaceNotification = TestRuntime;
    type ReportCorroborationOffence = OffenceHandler;
    type ProcessedEventsChecker = EthBridge;
    type ProcessedEventsHandler = AllPrimaryEventsFilter;
    type EthereumEventsMigration = ();
    type Quorum = Avn;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Nonce = u64;
    type Block = Block;
}

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl avn::Config for TestRuntime {
    type EthereumPublicKeyChecker = Self;
    type AuthorityId = UintAuthorityId;
}

impl pallet_timestamp::Config for TestRuntime {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = ConstU64<12000>;
    type WeightInfo = ();
}

impl EthereumPublicKeyChecker<AccountId> for TestRuntime {
    fn get_validator_for_eth_public_key(_eth_public_key: &ecdsa::Public) -> Option<AccountId> {
        match ETH_PUBLIC_KEY_VALID.with(|pk| *pk.borrow()) {
            true => Some(MOCK_RECOVERED_ACCOUNT_ID.with(|pk| *pk.borrow())),
            _ => None,
        }
    }
}

pub(crate) fn generate_signature(author: Author<TestRuntime>, context: &[u8]) -> TestSignature {
    author.key.sign(&(context).encode()).expect("Signature is signed")
}

pub fn setup_eth_tx_request(context: &Context) -> EthereumId {
    add_new_send_request::<TestRuntime, ()>(
        &context.request_function_name,
        &context.request_params,
        &vec![],
    )
    .unwrap()
}

pub fn create_confirming_author(author_id: u64) -> Author<TestRuntime> {
    Author::<TestRuntime> { key: UintAuthorityId(author_id), account_id: author_id }
}

pub fn lower_is_ready_to_be_claimed(lower_id: &u32) -> bool {
    LOWERSREADYTOCLAIM.with(|lowers| lowers.borrow_mut().iter().any(|l| l == lower_id))
}

pub fn request_failed(id: &u32) -> bool {
    FAILEDREQUESTS.with(|reqs| reqs.borrow_mut().iter().any(|r| r == id))
}

pub fn setup_context() -> Context {
    let primary_validator_id = Avn::advance_primary_validator_for_sending().unwrap();
    let author = Author::<TestRuntime> {
        key: UintAuthorityId(primary_validator_id),
        account_id: primary_validator_id,
    };
    let mut confirming_validator_id: u64 = 1;
    if primary_validator_id == confirming_validator_id {
        confirming_validator_id += 1
    }
    let confirming_author = create_confirming_author(confirming_validator_id);
    let second_confirming_author = create_confirming_author(confirming_validator_id + 1);
    let third_confirming_author = create_confirming_author(confirming_validator_id + 2);
    let test_signature = generate_signature(author.clone(), b"test context");
    let eth_tx_hash = H256::from_slice(&[0u8; 32]);
    let confirmation_signature = ecdsa::Signature::try_from(&[1; 65][0..65]).unwrap();
    let finalised_block_vec = Some(hex::encode(10u32.encode()).into());

    UintAuthorityId::set_all_keys(vec![UintAuthorityId(primary_validator_id)]);

    let lower_id = 10u32;
    let lower_params = create_lower_params(lower_id);

    Context {
        eth_tx_hash,
        test_signature,
        author: author.clone(),
        confirming_author: confirming_author.clone(),
        second_confirming_author: second_confirming_author.clone(),
        third_confirming_author: third_confirming_author.clone(),
        confirmation_signature,
        request_function_name: BridgeContractMethod::PublishRoot.name_as_bytes().to_vec(),
        request_params: vec![(b"bytes32".to_vec(), hex::decode(ROOT_HASH).unwrap())],
        lower_params,
        finalised_block_vec,
        lower_id,
        block_number: 1u64,
        // if request_params changes, this should also change
        expected_lower_msg_hash: "d89f2a698b48feb1e3248027e48e853e973fbf8e090e36dc00e6fd731d9c0df5"
            .to_string(),
        replay_attempt: 0,
    }
}

pub(crate) fn create_lower_params(lower_id: u32) -> LowerParams {
    let token_id = H160::from([3u8; 20]);
    let amount = 100_000_000_000_000_000_000u128;
    let t1_recipient = H160::from([2u8; 20]);
    let t2_sender = H256::from([4u8; 32]);
    let t2_timestamp = 1_000_000_000u64;

    concat_lower_data(lower_id, token_id, &amount, &t1_recipient, t2_sender, t2_timestamp)
}

pub fn set_mock_recovered_account_id(account_id_bytes: [u8; 8]) {
    let account_id = AccountId::decode(&mut account_id_bytes.to_vec().as_slice()).unwrap();
    MOCK_RECOVERED_ACCOUNT_ID.with(|acc_id| {
        *acc_id.borrow_mut() = account_id;
    });
}

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
    pub const DisabledValidatorsThreshold: Perbill = Perbill::from_percent(33);
}

thread_local! {
    // validator accounts (aka public addresses, public keys-ish)
    pub static VALIDATORS: RefCell<Option<Vec<u64>>> = RefCell::new(Some(vec![1, 2, 3, 4, 5, 6]));
    static ETH_PUBLIC_KEY_VALID: RefCell<bool> = RefCell::new(true);
    static MOCK_RECOVERED_ACCOUNT_ID: RefCell<AccountId> = RefCell::new(1);
    pub static LOWERSREADYTOCLAIM: RefCell<Vec<u32>> = RefCell::new(vec![]);
    pub static FAILEDREQUESTS: RefCell<Vec<u32>> = RefCell::new(vec![]);
}

pub type SessionIndex = u32;

pub struct TestSessionManager;
impl session::SessionManager<u64> for TestSessionManager {
    fn new_session(_new_index: SessionIndex) -> Option<Vec<u64>> {
        VALIDATORS.with(|l| l.borrow_mut().take())
    }
    fn end_session(_: SessionIndex) {}
    fn start_session(_: SessionIndex) {}
}

impl session::Config for TestRuntime {
    type SessionManager = TestSessionManager;
    type Keys = UintAuthorityId;
    type ShouldEndSession = session::PeriodicSessions<Period, Offset>;
    type SessionHandler = (Avn,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = u64;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = session::PeriodicSessions<Period, Offset>;
    type WeightInfo = ();
}

impl pallet_session::historical::Config for TestRuntime {
    type FullIdentification = AccountId;
    type FullIdentificationOf = ConvertInto;
}

impl pallet_session::historical::SessionManager<AccountId, AccountId> for TestSessionManager {
    fn new_session(_new_index: SessionIndex) -> Option<Vec<(AccountId, AccountId)>> {
        VALIDATORS.with(|l| {
            l.borrow_mut()
                .take()
                .map(|validators| validators.iter().map(|v| (*v, *v)).collect())
        })
    }
    fn end_session(_: SessionIndex) {}
    fn start_session(_: SessionIndex) {}
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
            System::set_block_number(1)
        });
        ext
    }

    pub fn with_genesis_config(mut self) -> Self {
        let _ = eth_bridge::GenesisConfig::<TestRuntime> {
            _phantom: Default::default(),
            eth_tx_lifetime_secs: 60 * 30,
            next_tx_id: 1,
            eth_block_range_size: 20u32,
            instance: sp_avn_common::eth::EthBridgeInstance {
                network: sp_avn_common::eth::EthereumNetwork::Sepolia,
                bridge_contract: H160::from_slice(&[1u8; 20]),
                name: b"TestBridge".to_vec().try_into().unwrap(),
                version: b"1".to_vec().try_into().unwrap(),
                ..Default::default()
            },
        }
        .assimilate_storage(&mut self.storage);
        self
    }

    pub fn with_validators(mut self) -> Self {
        let validators: Vec<u64> = VALIDATORS.with(|l| l.borrow_mut().take().unwrap());

        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for ref k in &validators {
                frame_system::Pallet::<TestRuntime>::inc_providers(k);
            }
        });

        let _ = pallet_session::GenesisConfig::<TestRuntime> {
            keys: validators.into_iter().map(|v| (v, v, UintAuthorityId(v))).collect(),
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
        });
        (ext, self.pool_state.unwrap(), self.offchain_state.unwrap())
    }
}

impl BridgeInterfaceNotification for TestRuntime {
    fn process_result(
        tx_id: EthereumId,
        _caller_id: Vec<u8>,
        tx_succeeded: bool,
    ) -> sp_runtime::DispatchResult {
        if !tx_succeeded {
            FAILEDREQUESTS.with(|l| l.borrow_mut().push(tx_id));
        }

        Ok(())
    }

    fn process_lower_proof_result(
        lower_id: u32,
        _caller_id: Vec<u8>,
        data: Result<Vec<u8>, ()>,
    ) -> sp_runtime::DispatchResult {
        if let Ok(_) = data {
            LOWERSREADYTOCLAIM.with(|l| l.borrow_mut().push(lower_id));
        } else {
            FAILEDREQUESTS.with(|l| l.borrow_mut().push(lower_id));
        }

        Ok(())
    }

    fn on_incoming_event_processed(event: &EthEvent) -> DispatchResult {
        if event.event_id.transaction_hash == H256::from_slice(&[6u8; 32]) {
            return Err(DispatchError::Other("Test - Bad event"))
        }

        Ok(())
    }
}

pub(crate) fn contains_event(event: RuntimeEvent) -> bool {
    System::events().iter().any(|x| x.event == event)
}
