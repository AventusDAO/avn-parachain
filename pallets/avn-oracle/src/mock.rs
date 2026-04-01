#![cfg(any(test, feature = "runtime-benchmarks"))]

use crate as pallet_avn_oracle;
use crate::*;
use codec::{Decode, Encode};
use frame_support::{
    __private::BasicExternalities,
    assert_ok, derive_impl,
    pallet_prelude::ConstU32,
    parameter_types,
    traits::{ConstU16, ConstU64},
    BoundedVec,
};
use frame_system::{
    self as system,
    offchain::{CreateTransaction, CreateTransactionBase},
};
use pallet_session as session;
use sp_avn_common::event_types::Validator;
use sp_core::{sr25519, Pair, H256};
use sp_runtime::{
    testing::{TestSignature, TestXt, UintAuthorityId},
    traits::{BlakeTwo256, ConvertInto, IdentityLookup},
    BuildStorage, RuntimeAppPublic,
};
use std::cell::RefCell;

pub type AccountId = u64;
pub type Extrinsic = TestXt<RuntimeCall, ()>;
type Block = frame_system::mocking::MockBlock<TestRuntime>;

#[derive(Clone)]
pub struct TestAccount {
    pub seed: [u8; 32],
}

impl TestAccount {
    pub fn new(seed: [u8; 32]) -> Self {
        Self { seed }
    }

    pub fn account_id(&self) -> AccountId {
        AccountId::decode(&mut self.key_pair().public().to_vec().as_slice()).unwrap()
    }

    pub fn key_pair(&self) -> sr25519::Pair {
        sr25519::Pair::from_seed(&self.seed)
    }
}

impl CreateTransactionBase<crate::Call<TestRuntime>> for TestRuntime {
    type Extrinsic = Extrinsic;
    type RuntimeCall = RuntimeCall;
}

impl CreateTransaction<crate::Call<TestRuntime>> for TestRuntime {
    type Extension = ();

    fn create_transaction(call: RuntimeCall, _extension: Self::Extension) -> Extrinsic {
        TestXt::new_bare(call)
    }
}

frame_support::construct_runtime!(
    pub enum TestRuntime {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        AVN: pallet_avn::{Pallet, Storage, Event},
        Session: pallet_session::{Pallet, Call, Storage, Event<T>, Config<T>},
        AvnOracle: pallet_avn_oracle::{Pallet, Call, Storage, Event<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
    }
);

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
    pub const ConsensusGracePeriod: u32 = 300;
    pub const MaxCurrencies: u32 = 10;
    pub const MinRatesRefreshRange: u32 = 5;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Nonce = u64;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type RuntimeEvent = RuntimeEvent;
    type AccountData = ();
    type SS58Prefix = ConstU16<42>;
}

impl pallet_avn::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = UintAuthorityId;
    type EthereumPublicKeyChecker = ();
    type NewSessionHandler = ();
    type DisabledValidatorChecker = ();
}

impl pallet_avn_oracle::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    type ConsensusGracePeriod = ConsensusGracePeriod;
    type MaxCurrencies = MaxCurrencies;
    type MinRatesRefreshRange = MinRatesRefreshRange;
    type Quorum = AVN;
}

impl pallet_timestamp::Config for TestRuntime {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = ConstU64<5>;
    type WeightInfo = ();
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
    type SessionHandler = (AVN,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = u64;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = session::PeriodicSessions<Period, Offset>;
    type DisablingStrategy = ();
    type WeightInfo = ();
}

thread_local! {
    pub static VALIDATORS: RefCell<Option<Vec<AccountId>>> = RefCell::new(Some(vec![
        validator_id_1(),
        validator_id_2(),
        validator_id_3(),
        validator_id_4(),
        validator_id_5(),
        validator_id_6(),
        validator_id_7(),
        validator_id_8(),
        validator_id_9(),
        validator_id_10(),
    ]));
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

pub fn validator_id_4() -> AccountId {
    TestAccount::new([4u8; 32]).account_id()
}

pub fn validator_id_5() -> AccountId {
    TestAccount::new([5u8; 32]).account_id()
}

pub fn validator_id_6() -> AccountId {
    TestAccount::new([6u8; 32]).account_id()
}

pub fn validator_id_7() -> AccountId {
    TestAccount::new([7u8; 32]).account_id()
}

pub fn validator_id_8() -> AccountId {
    TestAccount::new([8u8; 32]).account_id()
}

pub fn validator_id_9() -> AccountId {
    TestAccount::new([9u8; 32]).account_id()
}

pub fn validator_id_10() -> AccountId {
    TestAccount::new([10u8; 32]).account_id()
}

pub fn create_validator(author_id: u64) -> Validator<UintAuthorityId, AccountId> {
    Validator {
        key: UintAuthorityId(author_id),
        account_id: TestAccount::new([author_id.try_into().unwrap(); 32]).account_id(),
    }
}

pub fn generate_signature(
    author: &Validator<UintAuthorityId, AccountId>,
    context: &[u8],
) -> TestSignature {
    author.key.sign(&context.encode()).expect("signature should be signed")
}

pub fn register_currency(currency_symbol: Vec<u8>) {
    assert_ok!(AvnOracle::register_currency(RuntimeOrigin::root(), currency_symbol));
}

pub fn create_currency(currency_symbol: Vec<u8>) -> Currency {
    BoundedVec::<u8, ConstU32<{ MAX_CURRENCY_LENGTH }>>::try_from(currency_symbol)
        .expect("currency symbol must be <= MAX_CURRENCY_LENGTH bytes")
}

pub fn create_rates(rates: Vec<(Currency, u128)>) -> Rates {
    rates.try_into().expect("number of rates must be <= MAX_RATES")
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

    pub fn with_validators(mut self) -> Self {
        let validators: Vec<u64> = VALIDATORS.with(|l| l.borrow_mut().take().unwrap());

        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for validator in &validators {
                frame_system::Pallet::<TestRuntime>::inc_providers(validator);
            }
        });

        pallet_session::GenesisConfig::<TestRuntime> {
            keys: validators.into_iter().map(|v| (v, v, UintAuthorityId(v))).collect(),
            non_authority_keys: vec![],
        }
        .assimilate_storage(&mut self.storage)
        .unwrap();

        self
    }

    pub fn as_externality(self) -> sp_io::TestExternalities {
        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.execute_with(|| frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into()));
        ext
    }
}
