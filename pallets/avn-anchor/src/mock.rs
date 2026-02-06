use crate::{self as avn_anchor, *};
use codec::{Decode, Encode};
use core::cell::RefCell;
use frame_support::{
    derive_impl,
    pallet_prelude::*,
    parameter_types,
    traits::{
        ConstU16, ConstU32, ConstU64, Currency, EqualPrivilegeOnly, Everything,
        ExistenceRequirement,
    },
    PalletId,
};

use frame_system::{self as system, limits::BlockWeights, DefaultConfig, EnsureRoot};
use pallet_avn::BridgeInterfaceNotification;
use pallet_avn_proxy::{self as avn_proxy, ProvableProxy};
use pallet_session::{self as session, historical as pallet_session_historical};
use scale_info::TypeInfo;
use sp_avn_common::{FeePaymentHandler, InnerCallValidator, Proof};
use sp_core::{sr25519, Pair, H256};

use sp_avn_common::eth::EthereumId;
use sp_keystore::{testing::MemoryKeystore, KeystoreExt};
use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{BlakeTwo256, ConvertInto, IdentityLookup, Verify},
    BuildStorage, Perbill, Saturating,
};
use std::sync::Arc;

type Block = frame_system::mocking::MockBlock<TestRuntime>;

pub type Signature = sr25519::Signature;
pub type Balance = u128;
pub type AccountId = <Signature as Verify>::Signer;
pub type SessionIndex = u32;
pub type Extrinsic = TestXt<RuntimeCall, ()>;
pub const BASE_FEE: u64 = 12;

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);
const MAX_BLOCK_WEIGHT: Weight =
    Weight::from_parts(2_000_000_000_000 as u64, 0).set_proof_size(u64::MAX);
pub const INITIAL_BALANCE: Balance = 1_000_000_000_000;
pub const ONE_AVT: Balance = 1_000000_000000_000000u128;
pub const HUNDRED_AVT: Balance = 100 * ONE_AVT;

// TODO: Refactor this struct to be reused in all tests
pub struct TestAccount {
    pub seed: [u8; 32],
}

impl TestAccount {
    pub fn new(seed: [u8; 32]) -> Self {
        TestAccount { seed }
    }

    pub fn account_id(&self) -> AccountId {
        return AccountId::decode(&mut self.key_pair().public().to_vec().as_slice()).unwrap()
    }

    pub fn key_pair(&self) -> sr25519::Pair {
        return sr25519::Pair::from_seed(&self.seed)
    }
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

pub fn setup_balance<T: Config>(account: &T::AccountId) {
    let min_balance = T::Currency::minimum_balance();
    // Convert default checkpoint fee to the correct balance type
    let default_fee: BalanceOf<T> = T::DefaultCheckpointFee::get();

    // Calculate a large initial balance
    // Use saturating operations to prevent overflow
    let large_multiplier: BalanceOf<T> = 1000u32.into();
    let fee_component = default_fee.saturating_mul(large_multiplier);
    let existential_component = min_balance.saturating_mul(large_multiplier);

    // Add the components together for total initial balance
    let initial_balance = fee_component.saturating_add(existential_component);

    // Set the balance
    T::Currency::make_free_balance_be(account, initial_balance);

    // Ensure the account has enough free balance
    assert!(
        T::Currency::free_balance(account) >= initial_balance,
        "Failed to set up sufficient balance"
    );
}

pub fn ensure_fee_payment_possible<T: Config>(
    chain_id: ChainId,
    account: &T::AccountId,
) -> Result<(), &'static str> {
    let fee = Pallet::<T>::checkpoint_fee(chain_id);
    let balance = T::Currency::free_balance(account);
    if balance < fee {
        return Err("Insufficient balance for fee payment")
    }
    Ok(())
}

thread_local! {
    pub static MOCK_FEE_HANDLER_SHOULD_FAIL: RefCell<bool> = RefCell::new(false);
    // validator accounts (aka public addresses, public keys-ish)
    pub static VALIDATORS: RefCell<Option<Vec<AccountId>>> = RefCell::new(Some(vec![
        validator_id_1(),
        validator_id_2(),
        validator_id_3(),
    ]));
}

pub fn set_mock_fee_handler_should_fail(should_fail: bool) {
    MOCK_FEE_HANDLER_SHOULD_FAIL.with(|f| *f.borrow_mut() = should_fail);
}

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system,
        Balances: pallet_balances,
        Avn: pallet_avn,
        AvnProxy: avn_proxy,
        AvnAnchor: avn_anchor,
        TokenManager: pallet_token_manager,
        EthBridge: pallet_eth_bridge,
        Session: pallet_session,
        Historical: pallet_session_historical,
        Scheduler: pallet_scheduler,
        Timestamp: pallet_timestamp,
    }
);

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
    pub const DisabledValidatorsThreshold: Perbill = Perbill::from_percent(33);
    pub const DefaultCheckpointFee: Balance = 1_000_000_000;
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
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type AccountData = pallet_balances::AccountData<Balance>;
}

parameter_types! {
    pub const ExistentialDeposit: Balance = 1;
    pub const AvnTreasuryPotId: PalletId = PalletId(*b"Treasury");
    pub static TreasuryGrowthPercentage: Perbill = Perbill::from_percent(75);
    pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
        .base_block(Weight::from_parts(10 as u64, 0))
        .for_class(DispatchClass::all(), |weights| {
            weights.base_extrinsic = Weight::from_parts(BASE_FEE as u64, 0);
        })
        .for_class(DispatchClass::Normal, |weights| {
            weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAX_BLOCK_WEIGHT);
        })
        .for_class(DispatchClass::Operational, |weights| {
            weights.max_total = Some(MAX_BLOCK_WEIGHT);
            weights.reserved = Some(
                MAX_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAX_BLOCK_WEIGHT
            );
    })
    .avg_block_initialization(Perbill::from_percent(0))
    .build_or_panic();
    pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) * RuntimeBlockWeights::get().max_block;
}

impl pallet_scheduler::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeOrigin = RuntimeOrigin;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = EnsureRoot<AccountId>;
    type MaxScheduledPerBlock = ConstU32<100>;
    type WeightInfo = ();
    type OriginPrivilegeCmp = EqualPrivilegeOnly;
    type Preimages = ();
    type BlockNumberProvider = System;
}

impl pallet_token_manager::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ProcessedEventsChecker = ();
    type TokenId = sp_core::H160;
    type TokenBalance = u128;
    type Public = AccountId;
    type Signature = Signature;
    type AvnTreasuryPotId = AvnTreasuryPotId;
    type TreasuryGrowthPercentage = TreasuryGrowthPercentage;
    type OnGrowthLiftedHandler = ();
    type WeightInfo = ();
    type Scheduler = Scheduler;
    type Preimages = ();
    type PalletsOrigin = OriginCaller;
    type BridgeInterface = EthBridge;
    type OnIdleHandler = ();
    type AccountToBytesConvert = Avn;
    type TimeProvider = Timestamp;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for TestRuntime {
    type Balance = Balance;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
}

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl pallet_avn::Config for TestRuntime {
    type AuthorityId = UintAuthorityId;
}

impl avn_proxy::Config for TestRuntime {
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

impl pallet_eth_bridge::Config for TestRuntime {
    type MaxQueuedTxRequests = frame_support::traits::ConstU32<100>;
    type RuntimeEvent = RuntimeEvent;
    type TimeProvider = Timestamp;
    type MinEthBlockConfirmation = ConstU64<20>;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = ();
    type AccountToBytesConvert = Avn;
    type BridgeInterfaceNotification = Self;
    type ReportCorroborationOffence = ();
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

impl Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Public = AccountId;
    type Signature = Signature;
    type WeightInfo = default_weights::SubstrateWeight<TestRuntime>;
    type FeeHandler = TokenManager;
    type Token = sp_core::H160;
    type Currency = Balances;
    type DefaultCheckpointFee = DefaultCheckpointFee;
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
            RuntimeCall::AvnAnchor(avn_anchor::Call::signed_register_chain_handler {
                proof,
                ..
            }) |
            RuntimeCall::AvnAnchor(avn_anchor::Call::signed_update_chain_handler {
                proof, ..
            }) |
            RuntimeCall::AvnAnchor(avn_anchor::Call::signed_submit_checkpoint_with_identity {
                proof,
                ..
            }) => Some(proof.clone()),
            _ => None,
        }
    }
}

impl InnerCallValidator for TestAvnProxyConfig {
    type Call = RuntimeCall;

    fn signature_is_valid(call: &Box<Self::Call>) -> bool {
        match **call {
            RuntimeCall::System(..) => return true,
            RuntimeCall::AvnAnchor(..) => return AvnAnchor::signature_is_valid(call),
            _ => false,
        }
    }
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let keystore = MemoryKeystore::new();
    let mut t = system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();
    pallet_balances::GenesisConfig::<TestRuntime> {
        balances: vec![
            (create_account_id(1), INITIAL_BALANCE),
            (create_account_id(2), INITIAL_BALANCE),
            (create_account_id(3), INITIAL_BALANCE),
        ],
        dev_accounts: None,
    }
    .assimilate_storage(&mut t)
    .unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.register_extension(KeystoreExt(Arc::new(keystore)));
    ext.execute_with(|| System::set_block_number(1));
    ext
}

pub fn proxy_event_emitted(
    relayer: AccountId,
    call_hash: <TestRuntime as system::Config>::Hash,
) -> bool {
    System::events().iter().any(|a| {
        a.event ==
            RuntimeEvent::AvnProxy(avn_proxy::Event::<TestRuntime>::CallDispatched {
                relayer,
                hash: call_hash,
            })
    })
}

pub fn inner_call_failed_event_emitted(call_dispatch_error: DispatchError) -> bool {
    System::events().iter().any(|a| match a.event {
        RuntimeEvent::AvnProxy(avn_proxy::Event::<TestRuntime>::InnerCallFailed {
            dispatch_error,
            ..
        }) => dispatch_error == call_dispatch_error,
        _ => false,
    })
}

fn fake_treasury() -> AccountId {
    let seed: [u8; 32] = [01; 32];
    return TestAccount::new(seed).account_id()
}

impl FeePaymentHandler for TestRuntime {
    type Token = sp_core::H160;
    type TokenBalance = u128;
    type AccountId = AccountId;
    type Error = DispatchError;

    fn pay_fee(
        _token: &Self::Token,
        amount: &Self::TokenBalance,
        payer: &Self::AccountId,
        recipient: &Self::AccountId,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn pay_treasury(
        amount: &Self::TokenBalance,
        payer: &Self::AccountId,
    ) -> Result<(), Self::Error> {
        if MOCK_FEE_HANDLER_SHOULD_FAIL.with(|f| *f.borrow()) {
            return Err(DispatchError::Other("Test - Error"))
        }

        let recipient = fake_treasury();

        Balances::transfer(&payer, &recipient, *amount, ExistenceRequirement::KeepAlive)?;

        Ok(())
    }
}
pub fn create_account_id(seed: u8) -> AccountId {
    TestAccount::new([seed; 32]).account_id()
}

pub fn get_balance(account: &AccountId) -> Balance {
    Balances::free_balance(account)
}
