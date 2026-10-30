#![cfg(test)]

use crate::{self as pallet_bridge_watchtower, *};
use frame_support::{
    derive_impl,
    traits::{ConstU32, EnsureOrigin},
};
use frame_system::{self as system, EnsureRoot};
use pallet_watchtower::NodesInterface;
pub use sp_core::sr25519;
pub use sp_runtime::{
    testing::TestXt,
    traits::{IdentityLookup, Verify},
    BuildStorage,
};

pub type Signature = sr25519::Signature;
pub type AccountId = <Signature as Verify>::Signer;
pub type Extrinsic = TestXt<RuntimeCall, ()>;

type Block = frame_system::mocking::MockBlock<TestRuntime>;
type SignerId = pallet_node_manager::sr25519::AuthorityId;

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Watchtower: pallet_watchtower::{Pallet, Call, Storage, Event<T>},
        BridgeWatchtower: pallet_bridge_watchtower::{Pallet, Call, Storage, Event<T>},
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
}

impl pallet_watchtower::Config for TestRuntime {
    type RuntimeCall = RuntimeCall;
    type ExternalProposerOrigin = EnsureRootAsProposer;
    type SignerId = SignerId;
    type Public = AccountId;
    type Signature = Signature;
    type Watchtowers = TestNodeManager;
    type WatchtowerHooks = BridgeWatchtower;
    type VoteFilter = ();
    type WeightInfo = ();
    type SignedTxLifetime = ConstU32<5>;
    type MaxTitleLen = ConstU32<512>;
    type MaxInlineLen = ConstU32<8192>;
    type MaxUriLen = ConstU32<2040>;
    type MaxInternalProposalLen = ConstU32<100>;
    type MinCommitteeSize = ConstU32<2>;
    type MaxCommitteeSize = ConstU32<5>;
    type Randomness = TestRandomness;
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = ();
}

impl Config for TestRuntime {}

impl<LocalCall> system::offchain::CreateTransactionBase<LocalCall> for TestRuntime
where
    RuntimeCall: From<LocalCall>,
{
    type Extrinsic = Extrinsic;
    type RuntimeCall = RuntimeCall;
}

impl<LocalCall> frame_system::offchain::CreateBare<LocalCall> for TestRuntime
where
    RuntimeCall: From<LocalCall>,
{
    fn create_bare(call: Self::RuntimeCall) -> Self::Extrinsic {
        Extrinsic::new_bare(call)
    }
}

pub struct TestRandomness;
impl frame_support::traits::Randomness<H256, u64> for TestRandomness {
    fn random(subject: &[u8]) -> (H256, u64) {
        (H256::from(sp_io::hashing::blake2_256(subject)), System::block_number())
    }
}

/// No watchtowers are registered: the bridge watchtower has no voting logic to exercise yet.
pub struct TestNodeManager;
impl NodesInterface<AccountId, SignerId> for TestNodeManager {
    fn is_authorized_watchtower(_node: &AccountId) -> bool {
        false
    }

    fn is_watchtower_owner(_who: &AccountId) -> bool {
        false
    }

    fn get_node_signing_key(_node: &AccountId) -> Option<SignerId> {
        None
    }

    fn get_node_from_local_signing_keys() -> Option<(AccountId, SignerId)> {
        None
    }

    fn get_watchtower_voting_weight(_owner: &AccountId) -> u32 {
        0
    }

    fn get_authorized_watchtowers_count() -> u32 {
        0
    }

    fn get_node_at_index(_index: u32) -> Option<AccountId> {
        None
    }

    fn get_indexed_nodes_count() -> u32 {
        0
    }

    fn get_indexed_owners_count() -> u32 {
        0
    }

    fn get_owner_at_index(_index: u32) -> Option<(AccountId, u32)> {
        None
    }

    fn get_owner_node_at_index(_owner: &AccountId, _index: u32) -> Option<AccountId> {
        None
    }
}

pub struct EnsureRootAsProposer;
impl EnsureOrigin<RuntimeOrigin> for EnsureRootAsProposer {
    type Success = Option<AccountId>;

    fn try_origin(o: RuntimeOrigin) -> Result<Self::Success, RuntimeOrigin> {
        EnsureRoot::<AccountId>::try_origin(o).map(|_| None)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<RuntimeOrigin, ()> {
        Ok(RuntimeOrigin::root())
    }
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
        let mut ext = sp_io::TestExternalities::from(self.storage);
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| {
            frame_system::Pallet::<TestRuntime>::set_block_number(1u32.into());
        });
        ext
    }
}
