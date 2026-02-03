//Copyright 2022 Aventus Systems (UK) Ltd.

use crate::{self as avn_offence_handler, *};
use frame_support::{derive_impl, parameter_types};
use sp_runtime::{DispatchError, DispatchResult};
use sp_state_machine::BasicExternalities;

use frame_system::{self as system, DefaultConfig};
use pallet_session as session;
use sp_runtime::{testing::UintAuthorityId, traits::ConvertInto, BuildStorage};
use std::cell::RefCell;

pub const VALIDATOR_ID_1: u64 = 1;
pub const VALIDATOR_ID_2: u64 = 2;
pub const VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR: u64 = 3;

type Block = frame_system::mocking::MockBlock<TestRuntime>;

frame_support::construct_runtime!(
    pub enum TestRuntime {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
        Avn: pallet_avn::{Pallet, Storage, Event},
        AvnOffenceHandler: avn_offence_handler::{Pallet, Call, Storage, Event<T>},
    }
);

pub type ValidatorId = <TestRuntime as session::Config>::ValidatorId;

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl pallet_avn::Config for TestRuntime {
    type AuthorityId = UintAuthorityId;
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Nonce = u64;
    type Block = Block;
}

#[derive_impl(crate::config_preludes::TestDefaultConfig as avn_offence_handler::DefaultConfig)]
impl Config for TestRuntime {
    type Enforcer = Self;
}

impl pallet_session::historical::Config for TestRuntime {
    type FullIdentification = u64;
    type FullIdentificationOf = ConvertInto;
}

pub struct TestSessionManager;

impl session::SessionManager<u64> for TestSessionManager {
    fn new_session(_new_index: SessionIndex) -> Option<Vec<u64>> {
        None
    }
    fn end_session(_: SessionIndex) {}
    fn start_session(_: SessionIndex) {}
}

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
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

impl Enforcer<ValidatorId> for TestRuntime {
    fn slash_validator(slashed_validator_id: &ValidatorId) -> DispatchResult {
        if slashed_validator_id == &VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR {
            return Err(DispatchError::Other("Slash validator failed"))
        }
        Ok(())
    }
}

thread_local! {
    static VALIDATORS: RefCell<Option<Vec<u64>>> = RefCell::new(Some(vec![
        VALIDATOR_ID_1,
        VALIDATOR_ID_2,
        VALIDATOR_ID_CAN_CAUSE_SLASH_ERROR,
    ]));
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
        ext.execute_with(|| frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into()));
        ext
    }

    pub fn with_validators(mut self) -> Self {
        let validators: Vec<u64> = VALIDATORS.with(|l| l.borrow_mut().take().unwrap());
        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for ref k in &validators {
                frame_system::Pallet::<TestRuntime>::inc_providers(k);
            }
        });
        let _ = session::GenesisConfig::<TestRuntime> {
            keys: validators.into_iter().map(|v| (v, v, UintAuthorityId(v))).collect(),
        }
        .assimilate_storage(&mut self.storage);
        self
    }
}

impl AvnOffenceHandler {
    pub fn enable_offence() {
        <SlashingEnabled<TestRuntime>>::put(true);
    }

    pub fn disable_offence() {
        <SlashingEnabled<TestRuntime>>::put(false);
    }
}

pub struct TestExternalitiesBuilder {
    _existential_deposit: u64,
}

impl Default for TestExternalitiesBuilder {
    fn default() -> Self {
        Self { _existential_deposit: 1 }
    }
}

impl TestExternalitiesBuilder {
    // Build a genesis storage key/value store
    pub fn build<R>(self, execute: impl FnOnce() -> R) -> sp_io::TestExternalities {
        let storage =
            frame_system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();

        let mut ext = sp_io::TestExternalities::new(storage);
        ext.execute_with(|| {
            System::set_block_number(1);
        });
        ext.execute_with(execute);
        ext
    }
}
