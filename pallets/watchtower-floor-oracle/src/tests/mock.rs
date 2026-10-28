// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{self as pallet_watchtower_floor_oracle, *};
pub use codec::alloc::sync::Arc;
use frame_support::{
    assert_ok, derive_impl, parameter_types,
    traits::{ConstU32, ConstU64, EnsureOrigin, Hooks, Randomness},
    weights::{constants::WEIGHT_REF_TIME_PER_SECOND, Weight},
    WeakBoundedVec,
};
use frame_system::{self as system, EnsureRoot, EnsureSigned, RawOrigin};
use pallet_watchtower::NodesInterface;
pub use parking_lot::RwLock;
pub use sp_avn_common::avn_tests_helpers::utilities::{
    get_test_account_from_mnemonic, TestAccount,
};
pub use sp_core::{
    crypto::{KeyTypeId, DEV_PHRASE},
    offchain::{
        testing::{
            OffchainState, PendingRequest, PoolState, TestOffchainExt, TestTransactionPoolExt,
        },
        OffchainDbExt, OffchainWorkerExt, TransactionPoolExt,
    },
    sr25519, Pair, H160, H256,
};
use sp_keystore::{testing::MemoryKeystore, Keystore, KeystoreExt};
pub use sp_runtime::{
    testing::{TestSignature, TestXt, UintAuthorityId},
    traits::{IdentityLookup, Verify},
    BuildStorage, Perbill,
};
use std::cell::RefCell;

pub type Signature = sr25519::Signature;
pub type AccountId = <Signature as Verify>::Signer;
pub type Extrinsic = TestXt<RuntimeCall, ()>;
pub type BlockNumber = u64;
pub type SignerId = pallet_node_manager::sr25519::AuthorityId;
type Block = frame_system::mocking::MockBlock<TestRuntime>;

pub const ROUND_TIMEOUT_BLOCKS: BlockNumber = 10;
pub const FLOOR_CHAIN: ExternalChainId = 1;
pub const ORACLE_VOTE_DURATION: u32 = 100;
pub const MAX_FLOOR_AGE_SECS: u64 = 300;
pub const VENUE_CHAIN: ExternalChainId = 8453;
pub const MAX_VENUE_AGE_SECS: u64 = 60;
pub const VENUE_TARGET_LAG_SECS: u64 = 6;
pub const VENUE_TARGET_TOLERANCE_SECS: u64 = 6;
pub const MAX_FLOOR_CHECKPOINT_AGE_SECS: u64 = 150;
pub const MIN_FORCED_FLOOR_AGE_SECS: u64 = 30;
pub const CHECKPOINT_VOTE_SLACK_SECS: u64 = 60;
pub const NODE_KEY_TYPE: KeyTypeId = KeyTypeId(*b"nodk");
/// Country the test node reports with its observations.
pub const TEST_COUNTRY: CountryCode = *b"GB";

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
        Avn: pallet_avn::{Pallet, Storage, Event, Config<T>},
        Watchtower: pallet_watchtower::{Pallet, Call, Storage, Event<T>, ValidateUnsigned},
        FloorOracle: pallet_watchtower_floor_oracle::{Pallet, Call, Storage, Event<T>, ValidateUnsigned},
    }
);

impl<LocalCall> frame_system::offchain::CreateTransactionBase<LocalCall> for TestRuntime
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

parameter_types! {
    pub BlockWeights: frame_system::limits::BlockWeights =
        frame_system::limits::BlockWeights::simple_max(
            Weight::from_parts(2u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX),
        );
    pub const RoundTimeoutBlocks: BlockNumber = ROUND_TIMEOUT_BLOCKS;
    pub const OracleRoundThreshold: Perbill = Perbill::from_percent(60);
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type AccountData = pallet_balances::AccountData<u128>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for TestRuntime {
    type Balance = u128;
    type ExistentialDeposit = ConstU128<0>;
    type AccountStore = System;
}
use sp_core::ConstU128;

impl pallet_timestamp::Config for TestRuntime {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = ConstU64<1>;
    type WeightInfo = ();
}

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl pallet_avn::Config for TestRuntime {
    type AuthorityId = UintAuthorityId;
}

impl pallet_watchtower::Config for TestRuntime {
    type RuntimeCall = RuntimeCall;
    type ExternalProposerOrigin = EnsureExternalProposerOrRoot;
    type SignerId = SignerId;
    type Public = AccountId;
    type Signature = Signature;
    type Watchtowers = TestNodeManager;
    type WatchtowerHooks = FloorOracle;
    type VoteFilter = FloorOracle;
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

impl Config for TestRuntime {
    type RuntimeCall = RuntimeCall;
    type TimeProvider = Timestamp;
    type Quorum = Avn;
    type RoundTimeoutBlocks = RoundTimeoutBlocks;
    type CheckpointVoteSlackSecs = ConstU64<CHECKPOINT_VOTE_SLACK_SECS>;
    type OracleRoundThreshold = OracleRoundThreshold;
    type OracleVoteDuration = ConstU32<ORACLE_VOTE_DURATION>;
    type MaxFloorAge = ConstU64<MAX_FLOOR_AGE_SECS>;
    type MaxFloorCheckpointAge = ConstU64<MAX_FLOOR_CHECKPOINT_AGE_SECS>;
    type MinForcedFloorAge = ConstU64<MIN_FORCED_FLOOR_AGE_SECS>;
    type MaxVenueAge = ConstU64<MAX_VENUE_AGE_SECS>;
    type VenueTargetLag = ConstU64<VENUE_TARGET_LAG_SECS>;
    type WeightInfo = ();
}

pub struct TestRandomness;
impl Randomness<H256, u64> for TestRandomness {
    fn random(_subject: &[u8]) -> (H256, u64) {
        (H256::repeat_byte(7), 0)
    }
}

pub struct EnsureExternalProposerOrRoot;
impl EnsureOrigin<RuntimeOrigin> for EnsureExternalProposerOrRoot {
    type Success = Option<AccountId>;

    fn try_origin(o: RuntimeOrigin) -> Result<Self::Success, RuntimeOrigin> {
        if EnsureRoot::<AccountId>::try_origin(o.clone()).is_ok() {
            return Ok(None)
        }
        EnsureSigned::<AccountId>::try_origin(o).map(Some)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<RuntimeOrigin, ()> {
        Ok(RuntimeOrigin::root())
    }
}

// ------------------------------------------------------------------ collators (checkpoints)

/// Collator `i`: an sr25519 account with a `UintAuthorityId(i)` AVN key.
pub fn validator(i: u64) -> Author<TestRuntime> {
    Author::<TestRuntime> { account_id: acc(i), key: UintAuthorityId(i) }
}

pub fn acc(i: u64) -> AccountId {
    TestAccount::new([100 + i as u8; 32]).account_id()
}

pub const VALIDATORS: [u64; 6] = [1, 2, 3, 4, 5, 6];

pub fn set_validators(indices: &[u64]) {
    let validators: Vec<Author<TestRuntime>> = indices.iter().map(|i| validator(*i)).collect();
    pallet_avn::Validators::<TestRuntime>::put(WeakBoundedVec::force_from(
        validators,
        Some("Too many validators"),
    ));
}

/// A checkpoint vote as a collator would build it.
#[derive(Clone)]
pub struct VoteContext {
    pub chain_id: ExternalChainId,
    pub round_id: RoundId,
    pub author: Author<TestRuntime>,
    pub block_number: u32,
}

impl VoteContext {
    pub fn new(
        chain_id: ExternalChainId,
        round_id: RoundId,
        validator_index: u64,
        block_number: u32,
    ) -> Self {
        Self { chain_id, round_id, author: validator(validator_index), block_number }
    }

    pub fn signature(&self) -> TestSignature {
        let payload = FloorOracle::vote_signing_payload(
            self.chain_id,
            self.round_id,
            &self.author.account_id,
            self.block_number,
        );
        self.author.key.sign(&payload).expect("signing works in tests")
    }

    pub fn call(&self) -> Call<TestRuntime> {
        Call::<TestRuntime>::submit_checkpoint_vote {
            chain_id: self.chain_id,
            round_id: self.round_id,
            author: self.author.clone(),
            block_number: self.block_number,
            signature: self.signature(),
        }
    }

    pub fn submit(&self) -> DispatchResultWithPostInfo {
        FloorOracle::submit_checkpoint_vote(
            RuntimeOrigin::none(),
            self.chain_id,
            self.round_id,
            self.author.clone(),
            self.block_number,
            self.signature(),
        )
    }
}

// ------------------------------------------------------------------ watchtowers (floor)

pub fn get_default_voter() -> TestAccount {
    get_test_account_from_mnemonic(DEV_PHRASE)
}

/// Watchtower `i` in 1..=10. Watchtower 1 is the node whose keys live in the test keystore.
pub fn watchtower_account(i: u8) -> TestAccount {
    if i == 1 {
        get_default_voter()
    } else {
        TestAccount::new([10 + i; 32])
    }
}

pub fn watchtower(i: u8) -> AccountId {
    watchtower_account(i).account_id()
}

pub fn watchtower_signing_key(i: u8) -> SignerId {
    watchtower_account(i).public_key().into()
}

pub fn watchtower_owner() -> AccountId {
    TestAccount::new([31u8; 32]).account_id()
}

pub fn requester() -> AccountId {
    TestAccount::new([77u8; 32]).account_id()
}

pub fn all_watchtowers() -> Vec<AccountId> {
    (1..=10).map(watchtower).collect()
}

thread_local! {
    pub static AUTHORIZED_WATCHTOWERS: RefCell<Vec<AccountId>> = RefCell::new(all_watchtowers());
    /// Indexed node count while the node index is backfilling; `None` when it is complete.
    pub static INDEXED_NODES: RefCell<Option<u32>> = RefCell::new(None);
}

pub fn set_authorized_watchtowers(list: Vec<AccountId>) {
    AUTHORIZED_WATCHTOWERS.with(|w| *w.borrow_mut() = list);
}

/// Simulates a node index backfill that has reached `count` nodes, or with `None` a complete one.
pub fn set_indexed_nodes(count: Option<u32>) {
    INDEXED_NODES.with(|n| *n.borrow_mut() = count);
}

pub struct TestNodeManager;
impl NodesInterface<AccountId, SignerId> for TestNodeManager {
    fn is_authorized_watchtower(node: &AccountId) -> bool {
        AUTHORIZED_WATCHTOWERS.with(|w| w.borrow().contains(node))
    }

    fn is_watchtower_owner(who: &AccountId) -> bool {
        *who == watchtower_owner()
    }

    fn get_authorized_watchtowers_count() -> u32 {
        AUTHORIZED_WATCHTOWERS.with(|w| w.borrow().len() as u32)
    }

    fn get_watchtower_voting_weight(who: &AccountId) -> u32 {
        if Self::is_watchtower_owner(who) {
            Self::get_authorized_watchtowers_count()
        } else {
            0
        }
    }

    fn get_node_signing_key(node: &AccountId) -> Option<SignerId> {
        (1..=10u8).find(|i| watchtower(*i) == *node).map(watchtower_signing_key)
    }

    fn get_node_from_local_signing_keys() -> Option<(AccountId, SignerId)> {
        Some((watchtower(1), watchtower_signing_key(1)))
    }

    fn get_node_at_index(index: u32) -> Option<AccountId> {
        AUTHORIZED_WATCHTOWERS.with(|w| w.borrow().get(index as usize).cloned())
    }

    fn get_indexed_nodes_count() -> u32 {
        INDEXED_NODES
            .with(|n| *n.borrow())
            .unwrap_or_else(Self::get_authorized_watchtowers_count)
    }
}

/// Activates the queued watchtower proposal at the head, as the collator OCW would.
pub fn activate_head() -> ProposalId {
    let head = Watchtower::peek_front_id().expect("queue not corrupt").expect("queue head");
    assert_ok!(Watchtower::activate_next_proposal(RawOrigin::None.into(), head));
    head
}

/// A floor report as watchtower `i` would build it.
pub struct ReportContext {
    pub round_id: u64,
    pub observation: FloorObservation,
    pub country: CountryCode,
    pub watchtower_index: u8,
}

impl ReportContext {
    pub fn new(round_id: u64, observation: FloorObservation, watchtower_index: u8) -> Self {
        Self { round_id, observation, country: TEST_COUNTRY, watchtower_index }
    }

    pub fn account(&self) -> AccountId {
        watchtower(self.watchtower_index)
    }

    pub fn signature(&self) -> <SignerId as sp_runtime::RuntimeAppPublic>::Signature {
        // Signed for the running round's market, as the offchain worker does. Rejections
        // checked before the signature do not depend on it.
        let market =
            FloorOracle::active_floor_round().map(|round| round.market).unwrap_or_default();
        let payload = FloorOracle::report_signing_payload(
            self.round_id,
            &market,
            &self.observation,
            &self.country,
            &self.account(),
        );
        let signature: Signature =
            watchtower_account(self.watchtower_index).key_pair().sign(&payload).into();
        signature.into()
    }

    pub fn call(&self) -> Call<TestRuntime> {
        Call::<TestRuntime>::submit_floor_report {
            round_id: self.round_id,
            observation: self.observation,
            country: self.country,
            watchtower: self.account(),
            signature: self.signature(),
        }
    }

    pub fn submit(&self) -> DispatchResult {
        FloorOracle::submit_floor_report(
            RuntimeOrigin::none(),
            self.round_id,
            self.observation,
            self.country,
            self.account(),
            self.signature(),
        )
    }
}

/// A watchtower vote on the floor proposal, signed with the node key.
pub fn vote_call(
    proposal_id: ProposalId,
    in_favor: bool,
    watchtower_index: u8,
) -> pallet_watchtower::Call<TestRuntime> {
    let who = watchtower(watchtower_index);
    let payload = Watchtower::unsigned_vote_signing_payload(
        proposal_id,
        &sp_watchtower::VoteChoice::Binary(in_favor),
        &TEST_COUNTRY,
        &who,
    );
    let signature: Signature =
        watchtower_account(watchtower_index).key_pair().sign(&payload).into();
    pallet_watchtower::Call::<TestRuntime>::unsigned_vote {
        proposal_id,
        in_favor,
        country: TEST_COUNTRY,
        watchtower: who,
        signature: signature.into(),
    }
}

pub fn submit_vote(
    proposal_id: ProposalId,
    in_favor: bool,
    watchtower_index: u8,
) -> DispatchResultWithPostInfo {
    match vote_call(proposal_id, in_favor, watchtower_index) {
        pallet_watchtower::Call::unsigned_vote {
            proposal_id,
            in_favor,
            country,
            watchtower,
            signature,
        } => Watchtower::unsigned_vote(
            RawOrigin::None.into(),
            proposal_id,
            in_favor,
            country,
            watchtower,
            signature,
        ),
        _ => unreachable!(),
    }
}

/// A watchtower answer on the floor proposal, signed with the node key.
pub fn choice_vote_call(
    proposal_id: ProposalId,
    choice: H256,
    watchtower_index: u8,
) -> pallet_watchtower::Call<TestRuntime> {
    let who = watchtower(watchtower_index);
    let payload = Watchtower::unsigned_vote_signing_payload(
        proposal_id,
        &sp_watchtower::VoteChoice::Choice(choice),
        &TEST_COUNTRY,
        &who,
    );
    let signature: Signature =
        watchtower_account(watchtower_index).key_pair().sign(&payload).into();
    pallet_watchtower::Call::<TestRuntime>::unsigned_vote_choice {
        proposal_id,
        choice,
        country: TEST_COUNTRY,
        watchtower: who,
        signature: signature.into(),
    }
}

pub fn submit_choice_vote(
    proposal_id: ProposalId,
    choice: H256,
    watchtower_index: u8,
) -> DispatchResultWithPostInfo {
    match choice_vote_call(proposal_id, choice, watchtower_index) {
        pallet_watchtower::Call::unsigned_vote_choice {
            proposal_id,
            choice,
            country,
            watchtower,
            signature,
        } => Watchtower::unsigned_vote_choice(
            RawOrigin::None.into(),
            proposal_id,
            choice,
            country,
            watchtower,
            signature,
        ),
        _ => unreachable!(),
    }
}

/// The Punks market floor tests configure unless they say otherwise.
pub fn test_floor_market() -> FloorMarket {
    FloorMarket { chain_id: FLOOR_CHAIN, punks: H160::repeat_byte(0xB4) }
}

/// Mocks `GET /floor/punks/{chain_id}/{pinned_block}/{punks}` on the local external service,
/// with the path built exactly as the offchain worker builds it.
pub fn mock_floor_service(
    state: &mut OffchainState,
    market: &FloorMarket,
    pinned_block: u32,
    response: Option<Vec<u8>>,
) {
    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: format!(
            "http://127.0.0.1:2020/{}",
            FloorOracle::floor_service_path(market, pinned_block)
        ),
        response,
        sent: true,
        ..Default::default()
    });
}

pub fn encoded_observation(observation: &FloorObservation) -> Vec<u8> {
    sp_avn_common::http_data_codec::encode_to_http_data(observation).into_bytes()
}

// ------------------------------------------------------------------ watchtowers (venue)

/// The market venue tests configure unless they say otherwise.
pub fn test_market() -> VenueMarket {
    VenueMarket {
        chain_id: VENUE_CHAIN,
        clearing_house: H160::repeat_byte(0x56),
        amm: H160::repeat_byte(0xA1),
        target_tolerance_secs: VENUE_TARGET_TOLERANCE_SECS,
    }
}

/// A venue report as watchtower `i` would build it.
pub struct VenueReportContext {
    pub round_id: u64,
    pub observation: VenueObservation,
    pub country: CountryCode,
    pub watchtower_index: u8,
}

impl VenueReportContext {
    pub fn new(round_id: u64, observation: VenueObservation, watchtower_index: u8) -> Self {
        Self { round_id, observation, country: TEST_COUNTRY, watchtower_index }
    }

    pub fn account(&self) -> AccountId {
        watchtower(self.watchtower_index)
    }

    pub fn signature(&self) -> <SignerId as sp_runtime::RuntimeAppPublic>::Signature {
        let market =
            FloorOracle::active_venue_round().map(|round| round.market).unwrap_or_default();
        let payload = FloorOracle::venue_report_signing_payload(
            self.round_id,
            &market,
            &self.observation,
            &self.country,
            &self.account(),
        );
        let signature: Signature =
            watchtower_account(self.watchtower_index).key_pair().sign(&payload).into();
        signature.into()
    }

    pub fn call(&self) -> Call<TestRuntime> {
        Call::<TestRuntime>::submit_venue_report {
            round_id: self.round_id,
            observation: self.observation,
            country: self.country,
            watchtower: self.account(),
            signature: self.signature(),
        }
    }

    pub fn submit(&self) -> DispatchResult {
        FloorOracle::submit_venue_report(
            RuntimeOrigin::none(),
            self.round_id,
            self.observation,
            self.country,
            self.account(),
            self.signature(),
        )
    }
}

/// Mocks `GET /venue/exotic/{chain_id}/{target_ts}/{clearing_house}/{amm}` on the local
/// external service, with the path built exactly as the offchain worker builds it.
pub fn mock_venue_service(
    state: &mut OffchainState,
    market: &VenueMarket,
    target_ts: u64,
    response: Option<Vec<u8>>,
) {
    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: format!(
            "http://127.0.0.1:2020/{}",
            FloorOracle::venue_service_path(market, target_ts)
        ),
        response,
        sent: true,
        ..Default::default()
    });
}

/// The service's answer carrying `observation`.
pub fn encoded_venue_observation(observation: &VenueObservation) -> Vec<u8> {
    sp_avn_common::http_data_codec::encode_to_http_data(Some(*observation)).into_bytes()
}

/// The service's answer while the venue chain has not passed the target.
pub fn encoded_venue_not_ready() -> Vec<u8> {
    sp_avn_common::http_data_codec::encode_to_http_data(None::<VenueObservation>).into_bytes()
}

/// Mocks `GET /node/country` on the local external service.
pub fn mock_country_service(state: &mut OffchainState, response: Option<Vec<u8>>) {
    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: format!("http://127.0.0.1:2020/{}", NODE_COUNTRY_SERVICE_PATH),
        response,
        sent: true,
        ..Default::default()
    });
}

pub fn encoded_country(country: &CountryCode) -> Vec<u8> {
    sp_avn_common::http_data_codec::encode_to_http_data(country).into_bytes()
}

// ------------------------------------------------------------------ collators (checkpoint OCW)

/// Mocks `GET /checkpoint/block/{chain_id}` on the local external service.
pub fn mock_checkpoint_service(
    state: &mut OffchainState,
    chain_id: ExternalChainId,
    response: Option<Vec<u8>>,
) {
    state.expect_request(PendingRequest {
        method: "GET".into(),
        uri: format!("http://127.0.0.1:2020/{}", FloorOracle::checkpoint_service_path(chain_id)),
        response,
        sent: true,
        ..Default::default()
    });
}

pub fn encoded_block(block: u32) -> Vec<u8> {
    sp_avn_common::http_data_codec::encode_to_http_data(block).into_bytes()
}

/// Makes this node hold the AVN key of collator `i`, so the checkpoint worker votes as it.
pub fn set_local_validator(i: u64) {
    UintAuthorityId::set_all_keys(vec![UintAuthorityId(i)]);
}

pub fn clear_local_validator() {
    UintAuthorityId::set_all_keys(Vec::<UintAuthorityId>::new());
}

// ------------------------------------------------------------------ externalities

pub struct ExtBuilder {
    storage: sp_runtime::Storage,
    pool_state: Option<Arc<RwLock<PoolState>>>,
    offchain_state: Option<Arc<RwLock<OffchainState>>>,
    txpool_extension: Option<TestTransactionPoolExt>,
    offchain_extension: Option<TestOffchainExt>,
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
        }
    }

    pub fn for_offchain_worker(mut self) -> Self {
        let (offchain, offchain_state) = TestOffchainExt::new();
        let (pool, pool_state) = TestTransactionPoolExt::new();
        self.txpool_extension = Some(pool);
        self.offchain_extension = Some(offchain);
        self.pool_state = Some(pool_state);
        self.offchain_state = Some(offchain_state);
        self
    }

    fn keystore() -> MemoryKeystore {
        let keystore = MemoryKeystore::new();
        // Watchtower 1's node key, so the OCW can sign.
        keystore
            .sr25519_generate_new(NODE_KEY_TYPE, Some(DEV_PHRASE))
            .expect("keystore insert works");
        keystore
    }

    /// Externalities at block 1 with six collators, ten watchtowers and the clock at 1 second.
    pub fn as_externality(self) -> sp_io::TestExternalities {
        let (offchain, _) = TestOffchainExt::new();
        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(KeystoreExt(Arc::new(Self::keystore())));
        ext.register_extension(OffchainDbExt::new(offchain));
        ext.execute_with(|| {
            System::set_block_number(1);
            Timestamp::set_timestamp(1_000);
            set_validators(&VALIDATORS);
            set_authorized_watchtowers(all_watchtowers());
        });
        ext
    }

    pub fn as_externality_with_state(
        self,
    ) -> (sp_io::TestExternalities, Arc<RwLock<PoolState>>, Arc<RwLock<OffchainState>>) {
        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(KeystoreExt(Arc::new(Self::keystore())));
        ext.register_extension(OffchainDbExt::new(self.offchain_extension.clone().unwrap()));
        ext.register_extension(OffchainWorkerExt::new(self.offchain_extension.unwrap()));
        ext.register_extension(TransactionPoolExt::new(self.txpool_extension.unwrap()));
        ext.execute_with(|| {
            System::set_block_number(1);
            Timestamp::set_timestamp(1_000);
            set_validators(&VALIDATORS);
            set_authorized_watchtowers(all_watchtowers());
        });
        (ext, self.pool_state.unwrap(), self.offchain_state.unwrap())
    }
}

/// Runs `on_idle` with ample weight until no finished round has rows left.
pub fn run_idle_cleanup() {
    for _ in 0..10 {
        if FloorOracle::next_round_to_clean().is_none() {
            return
        }
        <FloorOracle as Hooks<BlockNumber>>::on_idle(System::block_number(), Weight::MAX);
    }
    panic!("on_idle did not finish cleaning up finished rounds");
}

/// Block time for a test floor observation: when the running floor round's checkpoint was
/// agreed (else the stored checkpoint's), the latest time the pinned block can have. Tests about
/// the block's own age set it explicitly.
pub fn pinned_floor_block_time(chain_id: ExternalChainId) -> u64 {
    FloorOracle::active_floor_round()
        .map(|round| round.checkpoint_agreed_at_sec)
        .or_else(|| FloorOracle::checkpoints(chain_id).map(|checkpoint| checkpoint.agreed_at))
        .unwrap_or(1)
}
