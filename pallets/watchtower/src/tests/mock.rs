use crate::{self as pallet_watchtower, *};
pub use codec::alloc::sync::Arc;
use frame_support::{
    derive_impl, parameter_types,
    traits::ConstU32,
    weights::{constants::WEIGHT_REF_TIME_PER_SECOND, Weight},
};
use frame_system::{self as system, EnsureRoot, EnsureSigned};

pub use sp_avn_common::avn_tests_helpers::utilities::{
    get_test_account_from_mnemonic, TestAccount,
};
pub use sp_core::{crypto::DEV_PHRASE, sr25519, H256};

use sp_keystore::{testing::MemoryKeystore, KeystoreExt};
pub use sp_runtime::{
    testing::TestXt,
    traits::{IdentityLookup, Verify},
    BuildStorage, Perbill,
};
use std::cell::RefCell;

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
        Watchtower: pallet_watchtower::{Pallet, Call, Storage, Event<T>},
    }
);

impl Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type SignerId = SignerId;
    type Public = AccountId;
    type Signature = Signature;
    type WeightInfo = ();
    type ExternalProposerOrigin = EnsureExternalProposerOrRoot;
    type Watchtowers = TestNodeManager;
    type WatchtowerHooks = ();
    type SignedTxLifetime = ConstU32<5>;
    type MaxTitleLen = ConstU32<512>;
    type MaxInlineLen = ConstU32<8192>;
    type MaxUriLen = ConstU32<2040>;
    type MaxInternalProposalLen = ConstU32<100>;
}

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
}

impl<LocalCall> system::offchain::CreateTransactionBase<LocalCall> for TestRuntime
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
    type MinimumPeriod = frame_support::traits::ConstU64<12000>;
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

        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(KeystoreExt(Arc::new(keystore)));
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| {
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into());
        });
        ext
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
        if EnsureRoot::<AccountId>::try_origin(o.clone()).is_ok() {
            return Ok(None)
        }

        match EnsureSigned::<AccountId>::try_origin(o) {
            Ok(who) => {
                match Watchtower::proposal_admin() {
                    Ok(admin) if who == admin => Ok(Some(who)),
                    Ok(_admin) => Err(RuntimeOrigin::signed(who)), // non-admin signer → reject
                    Err(_) => Ok(Some(who)),                       // no admin → allow anyone
                }
            },
            Err(o) => Err(o),
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<RuntimeOrigin, ()> {
        use frame_benchmarking::whitelisted_caller;
        Ok(RuntimeOrigin::signed(whitelisted_caller()))
    }
}

#[derive(Clone)]
pub struct Context {
    pub title: Vec<u8>,
    pub threshold: Perbill,
    pub source: ProposalSource,
    pub decision_rule: DecisionRule,
    pub external_ref: H256,
    pub created_at: u32,
    pub vote_duration: Option<u32>,
}

impl Default for Context {
    fn default() -> Self {
        let external_ref = H256::repeat_byte(1);
        Context {
            title: "Test Proposal".as_bytes().to_vec(),
            external_ref,
            threshold: Perbill::from_percent(50),
            source: ProposalSource::Internal(ProposalType::Summary),
            decision_rule: DecisionRule::SimpleMajority,
            vote_duration: Some(
                MinVotingPeriod::<TestRuntime>::get().saturated_into::<u32>() + 1u32,
            ),
            created_at: 1u32,
        }
    }
}

impl Context {
    pub fn build_internal_request(&self, payload: Vec<u8>) -> ProposalRequest {
        ProposalRequest {
            title: self.title.clone(),
            external_ref: self.external_ref,
            threshold: self.threshold,
            payload: RawPayload::Inline(payload),
            source: self.source.clone(),
            decision_rule: self.decision_rule.clone(),
            created_at: self.created_at,
            vote_duration: self.vote_duration,
        }
    }

    pub fn build_external_request(&self, uri: Vec<u8>) -> ProposalRequest {
        ProposalRequest {
            title: self.title.clone(),
            external_ref: self.external_ref,
            threshold: self.threshold,
            payload: RawPayload::Uri(uri),
            source: ProposalSource::External,
            decision_rule: self.decision_rule.clone(),
            created_at: self.created_at,
            vote_duration: self.vote_duration,
        }
    }

    pub fn build_request(&self, payload: RawPayload, source: ProposalSource) -> ProposalRequest {
        ProposalRequest {
            title: self.title.clone(),
            external_ref: self.external_ref,
            threshold: self.threshold,
            payload,
            source,
            decision_rule: self.decision_rule.clone(),
            created_at: self.created_at,
            vote_duration: self.vote_duration,
        }
    }
}
