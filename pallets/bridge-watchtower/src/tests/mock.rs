#![cfg(test)]

use crate::{self as pallet_bridge_watchtower, *};
use frame_support::{
    derive_impl, parameter_types,
    traits::{ConstU32, EnsureOrigin},
};
use frame_system::{self as system, EnsureRoot};
use pallet_watchtower::NodesInterface;
pub use sp_avn_common::eth::LowerParams;
pub use sp_core::sr25519;
pub use sp_runtime::{
    testing::TestXt,
    traits::{IdentityLookup, Verify},
    BuildStorage, DispatchError,
};
use std::{cell::RefCell, collections::HashMap};

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

parameter_types! {
    pub const LiftCheckThreshold: Perbill = Perbill::from_percent(51);
}

impl Config for TestRuntime {
    type BridgeInterface = TestBridgeInterface;
    type LowerDataProvider = TestLowerDataProvider;
    type WeightInfo = ();
    type ExternalValidator = TestWatchtower;
    type LoweringControl = TestLoweringControl;
    type LiftCheckThreshold = LiftCheckThreshold;
    type LiftCheckCommitteeSize = ConstU32<20>;
}

pub type PublishedTx = (Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>, Vec<u8>);

/// Storage key the mock bridge writes before failing, to check failed publishes are reverted.
pub const PARTIAL_PUBLISH_KEY: &[u8] = b"test_bridge::partial_publish";

thread_local! {
    static PUBLISHED: RefCell<Vec<PublishedTx>> = RefCell::new(vec![]);
    static PUBLISH_FAILS: RefCell<bool> = RefCell::new(false);
    static UNCLAIMED_LOWERS: RefCell<HashMap<LowerId, LowerParams>> = RefCell::new(HashMap::new());
    static PROPOSALS: RefCell<Vec<ProposalRequest>> = RefCell::new(vec![]);
    static PROPOSAL_FAILS: RefCell<bool> = RefCell::new(false);
    static LOWERING_DISABLED: RefCell<bool> = RefCell::new(false);
}

pub fn proposals() -> Vec<ProposalRequest> {
    PROPOSALS.with(|p| p.borrow().clone())
}

pub fn set_proposal_fails(fails: bool) {
    PROPOSAL_FAILS.with(|f| *f.borrow_mut() = fails);
}

pub fn lowering_disabled() -> bool {
    LOWERING_DISABLED.with(|d| *d.borrow())
}

pub fn published() -> Vec<PublishedTx> {
    PUBLISHED.with(|p| p.borrow().clone())
}

pub fn set_publish_fails(fails: bool) {
    PUBLISH_FAILS.with(|f| *f.borrow_mut() = fails);
}

pub fn add_unclaimed_lower(lower_id: LowerId, params: LowerParams) {
    UNCLAIMED_LOWERS.with(|l| l.borrow_mut().insert(lower_id, params));
}

pub struct TestBridgeInterface;
impl pallet_avn::BridgeInterface for TestBridgeInterface {
    fn publish(
        function_name: &[u8],
        params: &[(Vec<u8>, Vec<u8>)],
        caller_id: Vec<u8>,
    ) -> Result<EthereumId, DispatchError> {
        // Like eth-bridge, write to storage before a failure can happen
        frame_support::storage::unhashed::put(PARTIAL_PUBLISH_KEY, &true);
        if PUBLISH_FAILS.with(|f| *f.borrow()) {
            return Err(DispatchError::Other("publish failed"))
        }

        PUBLISHED.with(|p| {
            let mut published = p.borrow_mut();
            published.push((function_name.to_vec(), params.to_vec(), caller_id));
            Ok(published.len() as EthereumId)
        })
    }

    fn generate_lower_proof(
        _lower_id: u32,
        _params: &LowerParams,
        _caller_id: Vec<u8>,
    ) -> Result<(), DispatchError> {
        Ok(())
    }

    fn read_bridge_contract(
        _account_id_bytes: Vec<u8>,
        _function_name: &[u8],
        _params: &[(Vec<u8>, Vec<u8>)],
        _eth_block: Option<u32>,
    ) -> Result<Vec<u8>, DispatchError> {
        Ok(vec![])
    }

    fn latest_finalised_ethereum_block() -> Result<u32, DispatchError> {
        Ok(1)
    }
}

pub struct TestLowerDataProvider;
impl LowerDataProvider for TestLowerDataProvider {
    fn unclaimed_lower_params(lower_id: u32) -> Option<LowerParams> {
        UNCLAIMED_LOWERS.with(|l| l.borrow().get(&lower_id).copied())
    }
}

/// Storage key the mock watchtower writes before failing, to check failed requests are reverted.
pub const PARTIAL_PROPOSAL_KEY: &[u8] = b"test_watchtower::partial_proposal";

pub struct TestWatchtower;
impl WatchtowerInterface for TestWatchtower {
    type AccountId = AccountId;

    fn submit_proposal(_proposer: Option<AccountId>, proposal: ProposalRequest) -> DispatchResult {
        frame_support::storage::unhashed::put(PARTIAL_PROPOSAL_KEY, &true);
        if PROPOSAL_FAILS.with(|f| *f.borrow()) {
            return Err(DispatchError::Other("submission failed"))
        }

        PROPOSALS.with(|p| p.borrow_mut().push(proposal));
        Ok(())
    }

    fn get_proposal_status(_proposal_id: ProposalId) -> ProposalStatusEnum {
        ProposalStatusEnum::Unknown
    }

    fn get_proposer(_proposal_id: ProposalId) -> Option<AccountId> {
        None
    }

    fn min_committee_size() -> u32 {
        10
    }

    fn max_committee_size() -> u32 {
        150
    }

    fn ensure_committee_ready(_size: u32) -> DispatchResult {
        Ok(())
    }
}

pub struct TestLoweringControl;
impl LoweringControl for TestLoweringControl {
    fn disable_lowering() {
        LOWERING_DISABLED.with(|d| *d.borrow_mut() = true);
    }
}

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
        PROPOSALS.with(|p| p.borrow_mut().clear());
        LOWERING_DISABLED.with(|d| *d.borrow_mut() = false);
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
