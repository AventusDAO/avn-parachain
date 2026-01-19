#![cfg(test)]

use crate::{self as pallet_summary_watchtower, *};
pub use codec::alloc::sync::Arc;
use frame_support::{
    derive_impl, parameter_types,
    traits::{ConstU32, ConstU64, EnsureOrigin},
    weights::{constants::WEIGHT_REF_TIME_PER_SECOND, Weight},
};
use frame_system::{self as system, EnsureRoot, EnsureSigned};
pub use sp_avn_common::avn_tests_helpers::utilities::{
    get_test_account_from_mnemonic, TestAccount,
};
use sp_core::offchain::{testing::TestOffchainExt, OffchainDbExt};
pub use sp_core::{crypto::DEV_PHRASE, sr25519};
use sp_keystore::{testing::MemoryKeystore, KeystoreExt};
pub use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{IdentityLookup, Verify},
    BuildStorage, Perbill,
};
use std::cell::RefCell;

use pallet_watchtower::NodesInterface;

pub type Signature = sr25519::Signature;
pub type AccountId = <Signature as Verify>::Signer;
pub type Extrinsic = TestXt<RuntimeCall, ()>;

type Block = frame_system::mocking::MockBlock<TestRuntime>;
type SignerId = pallet_node_manager::sr25519::AuthorityId;

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
        AVN: pallet_avn::{Pallet, Storage, Event, Config<T>},
        Watchtower: pallet_watchtower::{Pallet, Call, Storage, Event<T>},
        SummaryWatchtower: pallet_summary_watchtower::{Pallet, Call, Storage, Event<T>},
    }
);

impl pallet_watchtower::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type ExternalProposerOrigin = EnsureExternalProposerOrRoot;
    type SignerId = SignerId;
    type Public = AccountId;
    type Signature = Signature;
    type Watchtowers = TestNodeManager;
    type WatchtowerHooks = SummaryWatchtower;
    type WeightInfo = ();
    type SignedTxLifetime = ConstU32<5>;
    type MaxTitleLen = ConstU32<512>;
    type MaxInlineLen = ConstU32<8192>;
    type MaxUriLen = ConstU32<2040>;
    type MaxInternalProposalLen = ConstU32<100>;
}

impl Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = ();
}

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
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

    pub BlockWeights: frame_system::limits::BlockWeights =
        frame_system::limits::BlockWeights::simple_max(
            Weight::from_parts(2u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX),
        );
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type AccountData = pallet_balances::AccountData<u128>;
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
    type MinimumPeriod = ConstU64<12000>;
    type WeightInfo = ();
}

impl pallet_avn::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = UintAuthorityId;
    type EthereumPublicKeyChecker = ();
    type NewSessionHandler = ();
    type DisabledValidatorChecker = ();
    type WeightInfo = ();
}

pub fn get_default_voter() -> TestAccount {
    get_test_account_from_mnemonic(DEV_PHRASE)
}

pub fn watchtower_1() -> AccountId {
    get_default_voter().account_id()
}
pub fn watchtower_2() -> AccountId {
    TestAccount::new([12u8; 32]).account_id()
}
pub fn watchtower_3() -> AccountId {
    TestAccount::new([13u8; 32]).account_id()
}
pub fn watchtower_4() -> AccountId {
    TestAccount::new([14u8; 32]).account_id()
}
pub fn watchtower_5() -> AccountId {
    TestAccount::new([15u8; 32]).account_id()
}
pub fn watchtower_6() -> AccountId {
    TestAccount::new([16u8; 32]).account_id()
}
pub fn watchtower_7() -> AccountId {
    TestAccount::new([17u8; 32]).account_id()
}
pub fn watchtower_8() -> AccountId {
    TestAccount::new([18u8; 32]).account_id()
}
pub fn watchtower_9() -> AccountId {
    TestAccount::new([19u8; 32]).account_id()
}
pub fn watchtower_10() -> AccountId {
    TestAccount::new([20u8; 32]).account_id()
}

pub fn watchtower_owner_1() -> AccountId {
    TestAccount::new([31u8; 32]).account_id()
}
pub fn watchtower_owner_2() -> AccountId {
    TestAccount::new([32u8; 32]).account_id()
}
pub fn watchtower_owner_3() -> AccountId {
    TestAccount::new([33u8; 32]).account_id()
}

pub fn get_signing_key_for_wt_1() -> SignerId {
    get_default_voter().public_key().into()
}

pub fn signing_key(index: u8) -> SignerId {
    TestAccount::new([index; 32]).public_key().into()
}

#[allow(dead_code)]
pub fn random_user() -> AccountId {
    TestAccount::new([99u8; 32]).account_id()
}

thread_local! {
    pub static AUTHORIZED_WATCHTOWERS: RefCell<Vec<AccountId>> = RefCell::new(vec![
        watchtower_1(),
        watchtower_2(),
        watchtower_3(),
        watchtower_4(),
        watchtower_5(),
        watchtower_6(),
        watchtower_7(),
        watchtower_8(),
        watchtower_9(),
        watchtower_10(),
    ]);

    pub static NODE_SIGNING_KEYS: RefCell<std::collections::HashMap<AccountId, SignerId>> =
        RefCell::new({
            let mut keys = std::collections::HashMap::new();
            keys.insert(watchtower_1(), get_signing_key_for_wt_1());
            keys.insert(watchtower_2(), signing_key(2));
            keys.insert(watchtower_3(), signing_key(3));
            keys.insert(watchtower_4(), signing_key(4));
            keys.insert(watchtower_5(), signing_key(5));
            keys.insert(watchtower_6(), signing_key(6));
            keys.insert(watchtower_7(), signing_key(7));
            keys.insert(watchtower_8(), signing_key(8));
            keys.insert(watchtower_9(), signing_key(9));
            keys.insert(watchtower_10(), signing_key(10));
            keys
        });

    pub static NODE_OWNERS: RefCell<std::collections::HashMap<AccountId, Vec<AccountId>>> =
        RefCell::new({
            let mut keys = std::collections::HashMap::new();
            keys.insert(watchtower_owner_1(), vec![watchtower_1(), watchtower_2(), watchtower_3()]);
            keys.insert(watchtower_owner_2(), vec![watchtower_4(), watchtower_5(), watchtower_6()]);
            keys.insert(watchtower_owner_3(), vec![watchtower_7(), watchtower_8(), watchtower_9(), watchtower_10()]);
            keys
        });
}

pub struct ExtBuilder {
    pub storage: sp_runtime::Storage,
}

impl ExtBuilder {
    pub fn build_default() -> Self {
        let storage = frame_system::GenesisConfig::<TestRuntime>::default()
            .build_storage()
            .unwrap()
            .into();

        Self { storage }
    }

    pub fn as_externality(self) -> sp_io::TestExternalities {
        let keystore = MemoryKeystore::new();
        let (offchain, _) = TestOffchainExt::new();

        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(KeystoreExt(Arc::new(keystore)));
        ext.register_extension(OffchainDbExt::new(offchain));
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| {
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into());
        });
        ext
    }
}

#[allow(dead_code)]
pub(crate) fn roll_forward(num_blocks_to_roll: u64) {
    let mut current_block = System::block_number();
    let target_block = current_block + num_blocks_to_roll;
    while current_block < target_block {
        current_block = roll_one_block();
    }
}

#[allow(dead_code)]
pub(crate) fn roll_one_block() -> u64 {
    Balances::on_finalize(System::block_number());
    System::on_finalize(System::block_number());
    System::set_block_number(System::block_number() + 1);
    System::on_initialize(System::block_number());
    Balances::on_initialize(System::block_number());
    Watchtower::on_idle(System::block_number(), BlockWeights::get().max_block);
    System::block_number()
}

pub struct TestNodeManager;
impl NodesInterface<AccountId, SignerId> for TestNodeManager {
    fn is_authorized_watchtower(node: &AccountId) -> bool {
        AUTHORIZED_WATCHTOWERS.with(|w| w.borrow().contains(node))
    }

    fn is_watchtower_owner(who: &AccountId) -> bool {
        NODE_OWNERS.with(|keys| keys.borrow().contains_key(who))
    }

    fn get_node_signing_key(node: &AccountId) -> Option<SignerId> {
        NODE_SIGNING_KEYS.with(|keys| keys.borrow().get(node).cloned())
    }

    fn get_node_from_local_signing_keys() -> Option<(AccountId, SignerId)> {
        let maybe_watchtower_1 =
            AUTHORIZED_WATCHTOWERS.with(|w| w.borrow().first().unwrap().clone());
        let watchtower_1 = watchtower_1();
        assert!(watchtower_1 == maybe_watchtower_1);
        Some((
            watchtower_1,
            NODE_SIGNING_KEYS.with(|keys| keys.borrow().get(&watchtower_1).unwrap().clone()),
        ))
    }

    fn get_watchtower_voting_weight(owner: &AccountId) -> u32 {
        NODE_OWNERS.with(|keys| keys.borrow().get(owner).map_or(0, |v| v.len() as u32))
    }

    fn get_authorized_watchtowers_count() -> u32 {
        AUTHORIZED_WATCHTOWERS.with(|w| w.borrow().len() as u32)
    }
}

pub struct EnsureExternalProposerOrRoot;
impl EnsureOrigin<RuntimeOrigin> for EnsureExternalProposerOrRoot {
    type Success = Option<AccountId>;

    fn try_origin(o: RuntimeOrigin) -> Result<Self::Success, RuntimeOrigin> {
        match EnsureSigned::<AccountId>::try_origin(o) {
            Ok(who) => Ok(Some(who)),
            Err(o) => EnsureRoot::<AccountId>::try_origin(o).map(|_| None),
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<RuntimeOrigin, ()> {
        use frame_benchmarking::whitelisted_caller;
        Ok(RuntimeOrigin::signed(whitelisted_caller()))
    }
}
