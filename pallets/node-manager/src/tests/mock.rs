// Copyright 2026 Aventus DAO.

#![cfg(test)]

use crate::{self as pallet_node_manager, *};
pub use codec::alloc::sync::Arc;
use frame_support::{derive_impl, parameter_types, weights::Weight};
use frame_system as system;
use pallet_session as session;
pub use parking_lot::RwLock;
pub use sp_avn_common::{
    avn_tests_helpers::utilities::TestAccount, constants::currency::AVT, NODE_MANAGER_PALLET_ID,
};
pub use sp_core::{
    offchain::{
        testing::{
            OffchainState, PendingRequest, PoolState, TestOffchainExt, TestTransactionPoolExt,
        },
        OffchainDbExt, OffchainWorkerExt, TransactionPoolExt,
    },
    sr25519,
};
use sp_keystore::{testing::MemoryKeystore, KeystoreExt};
pub use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{ConvertInto, IdentityLookup, Verify},
    BuildStorage, Perbill,
};
use sp_state_machine::BasicExternalities;
use std::cell::RefCell;

pub type Signature = sr25519::Signature;
pub type AccountId = <Signature as Verify>::Signer;
pub type Extrinsic = TestXt<RuntimeCall, ()>;

type Block = frame_system::mocking::MockBlock<TestRuntime>;

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
        NodeManager: pallet_node_manager::{Pallet, Call, Storage, Event<T>, Config<T>},
        AVN: pallet_avn::{Pallet, Storage, Event, Config<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
        Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
    }
);

parameter_types! {
    pub const RewardPotId: PalletId = NODE_MANAGER_PALLET_ID;
}

impl Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type SignerId = UintAuthorityId;
    type Public = AccountId;
    type Signature = Signature;
    type RewardPotId = RewardPotId;
    type TimeProvider = pallet_timestamp::Pallet<TestRuntime>;
    type SignedTxLifetime = ConstU32<64>;
    type WeightInfo = ();
}

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
}

pub struct TestSessionManager;
impl session::SessionManager<AccountId> for TestSessionManager {
    fn new_session(_new_index: u32) -> Option<Vec<AccountId>> {
        AUTHORS.with(|l| l.borrow_mut().take())
    }
    fn end_session(_: u32) {}
    fn start_session(_: u32) {}
}

impl session::Config for TestRuntime {
    type SessionManager = TestSessionManager;
    type Keys = UintAuthorityId;
    type ShouldEndSession = session::PeriodicSessions<Period, Offset>;
    type SessionHandler = (AVN,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = session::PeriodicSessions<Period, Offset>;
    type WeightInfo = ();
}

impl<LocalCall> system::offchain::SendTransactionTypes<LocalCall> for TestRuntime
where
    RuntimeCall: From<LocalCall>,
{
    type OverarchingCall = RuntimeCall;
    type Extrinsic = Extrinsic;
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const MaximumBlockWeight: Weight = Weight::from_parts(1024 as u64, 0);
    pub const MaximumBlockLength: u32 = 2 * 1024;
    pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
    pub const ChallengePeriod: u64 = 2;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type AccountData = pallet_balances::AccountData<u128>;
}

impl pallet_avn::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = UintAuthorityId;
    type EthereumPublicKeyChecker = ();
    type NewSessionHandler = ();
    type DisabledValidatorChecker = ();
    type WeightInfo = ();
}

parameter_types! {
    pub const ExistentialDeposit: u64 = 0u64;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for TestRuntime {
    type Balance = u128;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
}

impl pallet_timestamp::Config for TestRuntime {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = frame_support::traits::ConstU64<12000>;
    type WeightInfo = ();
}

pub fn author_id_1() -> AccountId {
    TestAccount::new([17u8; 32]).account_id()
}
pub fn author_id_2() -> AccountId {
    TestAccount::new([19u8; 32]).account_id()
}

thread_local! {
    pub static AUTHORS: RefCell<Option<Vec<AccountId>>> = RefCell::new(Some(vec![
        author_id_1(),
        author_id_2(),
    ]));
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
        let storage = frame_system::GenesisConfig::<TestRuntime>::default()
            .build_storage()
            .unwrap()
            .into();

        Self {
            storage,
            pool_state: None,
            offchain_state: None,
            txpool_extension: None,
            offchain_extension: None,
            offchain_registered: false,
        }
    }

    pub fn with_genesis_config(mut self) -> Self {
        let _ = pallet_node_manager::GenesisConfig::<TestRuntime> {
            _phantom: Default::default(),
            reward_period: 200u32,
            max_batch_size: 10u32,
            heartbeat_period: 5u32,
            reward_amount: 20 * AVT,
            auto_stake_duration_sec: 3600u64,
        }
        .assimilate_storage(&mut self.storage);
        self
    }

    pub fn with_authors(mut self) -> Self {
        let authors: Vec<AccountId> = AUTHORS.with(|l| l.borrow_mut().take().unwrap());

        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for ref k in &authors {
                frame_system::Pallet::<TestRuntime>::inc_providers(k);
            }
        });

        let _ = pallet_session::GenesisConfig::<TestRuntime> {
            keys: authors
                .into_iter()
                .enumerate()
                .map(|(i, v)| (v, v, UintAuthorityId((i as u32).into())))
                .collect(),
            ..Default::default()
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

    pub fn as_externality(self) -> sp_io::TestExternalities {
        let keystore = MemoryKeystore::new();

        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(KeystoreExt(Arc::new(keystore)));
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| {
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into());
            Timestamp::set_timestamp(1);
            RewardEnabled::<TestRuntime>::put(true);
        });
        ext
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
            RewardEnabled::<TestRuntime>::put(true);
        });
        (ext, self.pool_state.unwrap(), self.offchain_state.unwrap())
    }
}

/// Rolls desired block number of times.
pub(crate) fn roll_forward(num_blocks_to_roll: u64) {
    let mut current_block = System::block_number();
    let target_block = current_block + num_blocks_to_roll;
    while current_block < target_block {
        current_block = roll_one_block();
    }
}

pub(crate) fn roll_one_block() -> u64 {
    Balances::on_finalize(System::block_number());
    System::on_finalize(System::block_number());
    System::set_block_number(System::block_number() + 1);
    System::on_initialize(System::block_number());
    Balances::on_initialize(System::block_number());
    NodeManager::on_initialize(System::block_number());
    System::block_number()
}

pub fn mock_get_finalised_block(state: &mut OffchainState, response: &Option<Vec<u8>>) {
    let url = "http://127.0.0.1:2020/latest_finalised_block".to_string();

    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: url.into(),
        response: response.clone(),
        sent: true,
        ..Default::default()
    });
}
