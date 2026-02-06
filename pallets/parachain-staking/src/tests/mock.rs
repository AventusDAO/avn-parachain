// Copyright 2019-2022 PureStake Inc.
// This file is part of Moonbeam.

// Moonbeam is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonbeam is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonbeam.  If not, see <http://www.gnu.org/licenses/>.

//! Test utilities
use crate as pallet_parachain_staking;
use crate::{
    pallet, AwardedPts, Config, Points, Proof, TypeInfo, COLLATOR_LOCK_ID, NOMINATOR_LOCK_ID, *,
};
use codec::{Decode, Encode};
use core::cell::RefCell;
use frame_support::{
    assert_ok, construct_runtime, derive_impl,
    dispatch::{DispatchClass, DispatchInfo, PostDispatchInfo},
    parameter_types,
    traits::{
        ConstU8, Currency, FindAuthor, Imbalance, LockIdentifier, OnFinalize, OnInitialize,
        OnUnbalanced, ValidatorRegistration,
    },
    weights::{Weight, WeightToFee as WeightToFeeT},
    PalletId,
};
use frame_system::{self as system, limits, DefaultConfig};
use pallet_avn::CollatorPayoutDustHandler;
use pallet_avn_proxy::{self as avn_proxy, ProvableProxy};
use pallet_eth_bridge;
use pallet_session as session;
use pallet_transaction_payment::{ChargeTransactionPayment, CurrencyAdapter};
use sp_avn_common::{eth::EthereumId, FeePaymentHandler, InnerCallValidator};
use sp_core::{sr25519, ConstU64, Pair};
use sp_io;
use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{
        transaction_extension::TxBaseImplication, ConvertInto, IdentityLookup,
        TransactionExtension, Verify,
    },
    transaction_validity::TransactionSource,
    BuildStorage, DispatchError, Perbill, SaturatedConversion,
};

pub type AccountId = <Signature as Verify>::Signer;
pub type Signature = sr25519::Signature;
pub type Balance = u128;
pub type BlockNumber = u64;

pub type Extrinsic = TestXt<RuntimeCall, ()>;
type Block = frame_system::mocking::MockBlock<Test>;

// Configure a mock runtime to test the pallet.
construct_runtime!(
    pub enum Test
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
        ParachainStaking: pallet_parachain_staking::{Pallet, Call, Storage, Config<T>, Event<T>},
        Authorship: pallet_authorship::{Pallet, Storage},
        TransactionPayment: pallet_transaction_payment::{Pallet, Storage, Event<T>, Config<T>},
        Avn: pallet_avn::{Pallet, Storage, Event},
        Session: pallet_session::{Pallet, Call, Storage, Event<T>, Config<T>},
        AvnProxy: avn_proxy::{Pallet, Call, Storage, Event<T>},
        Historical: pallet_session::historical::{Pallet, Storage},
        EthBridge: pallet_eth_bridge::{Pallet, Call, Storage, Event<T>},
        Timestamp: pallet_timestamp,
    }
);

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);
const MAX_BLOCK_WEIGHT: Weight = Weight::from_parts(1024, 0).set_proof_size(u64::MAX);
pub static TX_LEN: usize = 1;
pub const BASE_FEE: u64 = 12;

pub struct TestAccount {
    pub seed: [u8; 32],
}

impl TestAccount {
    pub fn new(id: u64) -> Self {
        TestAccount { seed: Self::into_32_bytes(&id) }
    }

    pub fn account_id(&self) -> AccountId {
        return AccountId::decode(&mut self.key_pair().public().to_vec().as_slice()).unwrap()
    }

    pub fn key_pair(&self) -> sr25519::Pair {
        return sr25519::Pair::from_seed(&self.seed)
    }

    fn into_32_bytes(account: &u64) -> [u8; 32] {
        let mut bytes = account.encode();
        let mut bytes32: Vec<u8> = vec![0; 32 - bytes.len()];
        bytes32.append(&mut bytes);
        let mut data: [u8; 32] = Default::default();
        data.copy_from_slice(&bytes32[0..32]);
        data
    }
}

pub fn sign(signer: &sr25519::Pair, message_to_sign: &[u8]) -> Signature {
    return Signature::from(signer.sign(message_to_sign))
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const MaximumBlockWeight: Weight = Weight::from_parts(1024, 0);
    pub const MaximumBlockLength: u32 = 2 * 1024;
    pub const AvailableBlockRatio: Perbill = Perbill::one();
    pub const SS58Prefix: u8 = 42;

    pub BlockLength: limits::BlockLength = limits::BlockLength::max_with_normal_ratio(1024, NORMAL_DISPATCH_RATIO);
    pub RuntimeBlockWeights: limits::BlockWeights = limits::BlockWeights::builder()
        .base_block(Weight::from_parts(10, 0))
        .for_class(DispatchClass::all(), |weights| {
            weights.base_extrinsic = Weight::from_parts(BASE_FEE, 0);
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
}
#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for Test {
    type BlockWeights = RuntimeBlockWeights;
    type Nonce = u64;
    type Block = Block;
    type AccountData = pallet_balances::AccountData<u128>;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
}

parameter_types! {
    pub const ExistentialDeposit: u128 = 0;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for Test {
    type Balance = Balance;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
}

pub struct Author4;
impl FindAuthor<AccountId> for Author4 {
    fn find_author<'a, I>(_digests: I) -> Option<AccountId>
    where
        I: 'a + IntoIterator<Item = (frame_support::ConsensusEngineId, &'a [u8])>,
    {
        Some(TestAccount::new(4u64).account_id())
    }
}

impl pallet_authorship::Config for Test {
    type FindAuthor = Author4;
    type EventHandler = ParachainStaking;
}

parameter_types! {
    pub const MinBlocksPerEra: u32 = 3;
    pub const RewardPaymentDelay: u32 = 2;
    pub const MinSelectedCandidates: u32 = 5;
    pub const MaxTopNominationsPerCandidate: u32 = 4;
    pub const MaxBottomNominationsPerCandidate: u32 = 4;
    pub const MaxNominationsPerNominator: u32 = 10;
    pub const MinNominationPerCollator: u128 = 1;
    pub const ErasPerGrowthPeriod: u32 = 2;
    pub const RewardPotId: PalletId = PalletId(*b"av/vamgr");
    pub const MaxCandidates:u32 = 100;
}

pub struct IsRegistered;
impl ValidatorRegistration<AccountId> for IsRegistered {
    fn is_registered(_id: &AccountId) -> bool {
        true
    }
}

impl<LocalCall> frame_system::offchain::CreateTransactionBase<LocalCall> for Test
where
    RuntimeCall: From<LocalCall>,
{
    type Extrinsic = Extrinsic;
    type RuntimeCall = RuntimeCall;
}

impl<LocalCall> frame_system::offchain::CreateInherent<LocalCall> for Test
where
    RuntimeCall: From<LocalCall>,
{
    fn create_inherent(call: Self::RuntimeCall) -> Self::Extrinsic {
        Extrinsic::new_bare(call)
    }
}

impl Config for Test {
    type RuntimeCall = RuntimeCall;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type RewardPaymentDelay = RewardPaymentDelay;
    type MinBlocksPerEra = MinBlocksPerEra;
    type MinSelectedCandidates = MinSelectedCandidates;
    type MaxTopNominationsPerCandidate = MaxTopNominationsPerCandidate;
    type MaxBottomNominationsPerCandidate = MaxBottomNominationsPerCandidate;
    type MaxNominationsPerNominator = MaxNominationsPerNominator;
    type MinNominationPerCollator = MinNominationPerCollator;
    type RewardPotId = RewardPotId;
    type ErasPerGrowthPeriod = ErasPerGrowthPeriod;
    type ProcessedEventsChecker = ();
    type Public = AccountId;
    type Signature = Signature;
    type CollatorSessionRegistration = IsRegistered;
    type CollatorPayoutDustHandler = TestCollatorPayoutDustHandler;
    type WeightInfo = ();
    type MaxCandidates = MaxCandidates;
    type AccountToBytesConvert = Avn;
    type BridgeInterface = EthBridge;
    type GrowthEnabled = TestGrowthEnabled;
}

// Deal with any positive imbalance by sending it to the fake treasury
pub struct TestCollatorPayoutDustHandler;
impl CollatorPayoutDustHandler<Balance> for TestCollatorPayoutDustHandler {
    fn handle_dust(dust_amount: Balance) {
        // Transfer the amount and drop the imbalance to increase the total issuance
        let imbalance = Balances::deposit_creating(&fake_treasury(), dust_amount);
        drop(imbalance);
    }
}

impl pallet_session::historical::Config for Test {
    type FullIdentification = AccountId;
    type FullIdentificationOf = ConvertInto;
}

parameter_types! {
    pub static WeightToFee: u128 = 1u128;
    pub static TransactionByteFee: u128 = 0u128;
}

thread_local! {
    pub static GROWTH_ENABLED: RefCell<bool> = RefCell::new(true);
}

pub struct TestGrowthEnabled;
impl Get<bool> for TestGrowthEnabled {
    fn get() -> bool {
        GROWTH_ENABLED.with(|enabled| *enabled.borrow())
    }
}

pub fn disable_growth() {
    GROWTH_ENABLED.with(|enabled| *enabled.borrow_mut() = false);
}

pub struct DealWithFees;
impl OnUnbalanced<pallet_balances::NegativeImbalance<Test>> for DealWithFees {
    fn on_unbalanceds(
        mut fees_then_tips: impl Iterator<Item = pallet_balances::NegativeImbalance<Test>>,
    ) {
        if let Some(mut fees) = fees_then_tips.next() {
            if let Some(tips) = fees_then_tips.next() {
                tips.merge_into(&mut fees);
            }
            let staking_pot = ParachainStaking::compute_reward_pot_account_id();
            Balances::resolve_creating(&staking_pot, fees);
        }
    }
}

impl pallet_transaction_payment::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = CurrencyAdapter<Balances, DealWithFees>;
    type LengthToFee = TransactionByteFee;
    type WeightToFee = WeightToFee;
    type FeeMultiplierUpdate = ();
    type OperationalFeeMultiplier = ConstU8<5>;
    type WeightInfo = ();
}

impl WeightToFeeT for WeightToFee {
    type Balance = u128;

    fn weight_to_fee(weight: &Weight) -> Self::Balance {
        Self::Balance::saturated_from(weight.ref_time())
            .saturating_mul(WEIGHT_TO_FEE.with(|v| *v.borrow()))
    }
}

impl WeightToFeeT for TransactionByteFee {
    type Balance = u128;

    fn weight_to_fee(weight: &Weight) -> Self::Balance {
        Self::Balance::saturated_from(weight.ref_time())
            .saturating_mul(TRANSACTION_BYTE_FEE.with(|v| *v.borrow()))
    }
}

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl avn::Config for Test {
    type DisabledValidatorChecker = ();
    type AuthorityId = UintAuthorityId;
}

impl session::Config for Test {
    type SessionManager = ParachainStaking;
    type Keys = UintAuthorityId;
    type ShouldEndSession = ParachainStaking;
    type SessionHandler = (Avn,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = ParachainStaking;
    type WeightInfo = ();
    type DisablingStrategy = ();
}

impl avn_proxy::Config for Test {
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

impl pallet_eth_bridge::Config for Test {
    type MaxQueuedTxRequests = ConstU32<100>;
    type RuntimeEvent = RuntimeEvent;
    type TimeProvider = Timestamp;
    type RuntimeCall = RuntimeCall;
    type MinEthBlockConfirmation = ConstU64<20>;
    type WeightInfo = ();
    type AccountToBytesConvert = Avn;
    type BridgeInterfaceNotification = Self;
    type ReportCorroborationOffence = ();
    type ProcessedEventsChecker = ();
    type ProcessedEventsHandler = ();
    type EthereumEventsMigration = ();
    type Quorum = Avn;
}

impl pallet_timestamp::Config for Test {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = ConstU64<12000>;
    type WeightInfo = ();
}

impl BridgeInterfaceNotification for Test {
    fn process_result(
        _tx_id: EthereumId,
        _caller_id: Vec<u8>,
        _tx_succeeded: bool,
    ) -> sp_runtime::DispatchResult {
        Ok(())
    }
}

// Test Avn proxy configuration logic
// We only allow System::Remark and signed_mint_single_nft calls to be proxied
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
            RuntimeCall::System(frame_system::Call::remark { remark: _msg }) => {
                let signer_account = TestAccount::new(985);
                return Some(Proof {
                    signer: signer_account.account_id(),
                    relayer: TestAccount::new(6547).account_id(),
                    signature: sign(&signer_account.key_pair(), &("").encode()),
                })
            },

            RuntimeCall::ParachainStaking(pallet_parachain_staking::Call::signed_nominate {
                proof,
                targets: _,
                amount: _,
            }) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(pallet_parachain_staking::Call::signed_bond_extra {
                proof,
                extra_amount: _,
            }) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_candidate_bond_extra {
                    proof,
                    extra_amount: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_schedule_candidate_unbond { proof, less: _ },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_schedule_nominator_unbond { proof, less: _ },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_schedule_revoke_nomination {
                    proof,
                    collator: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_schedule_leave_nominators { proof },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_execute_leave_nominators {
                    proof,
                    nominator: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_execute_nomination_request {
                    proof,
                    nominator: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_execute_candidate_unbond {
                    proof,
                    candidate: _,
                },
            ) => return Some(proof.clone()),

            _ => None,
        }
    }
}

impl InnerCallValidator for TestAvnProxyConfig {
    type Call = RuntimeCall;

    fn signature_is_valid(call: &Box<Self::Call>) -> bool {
        match **call {
            RuntimeCall::System(..) => return true,
            RuntimeCall::ParachainStaking(..) => return ParachainStaking::signature_is_valid(call),
            _ => false,
        }
    }
}

pub fn inner_call_failed_event_emitted(call_dispatch_error: DispatchError) -> bool {
    return System::events().iter().any(|a| match a.event {
        RuntimeEvent::AvnProxy(avn_proxy::Event::<Test>::InnerCallFailed {
            dispatch_error,
            ..
        }) =>
            if dispatch_error == call_dispatch_error {
                return true
            } else {
                return false
            },
        _ => false,
    })
}

pub fn build_proof(
    signer: &AccountId,
    relayer: &AccountId,
    signature: Signature,
) -> Proof<Signature, AccountId> {
    return Proof { signer: *signer, relayer: *relayer, signature }
}

#[derive(Clone)]
pub struct Staker {
    pub relayer: AccountId,
    pub account_id: AccountId,
    pub key_pair: sr25519::Pair,
}

impl Default for Staker {
    fn default() -> Self {
        let relayer = TestAccount::new(0).account_id();
        let account = TestAccount::new(10000);

        Staker { relayer, key_pair: account.key_pair(), account_id: account.account_id() }
    }
}

impl Staker {
    pub fn new(relayer_seed: u64, nominator_seed: u64) -> Self {
        let relayer = TestAccount::new(relayer_seed).account_id();
        let account = TestAccount::new(nominator_seed);

        Staker { relayer, key_pair: account.key_pair(), account_id: account.account_id() }
    }
}

pub(crate) struct ExtBuilder {
    // endowed accounts with balances
    balances: Vec<(AccountId, Balance)>,
    // [collator, amount]
    collators: Vec<(AccountId, Balance)>,
    // [nominator, collator, nomination_amount]
    nominations: Vec<(AccountId, AccountId, Balance)>,
    min_collator_stake: Balance,
    min_total_nominator_stake: Balance,
}

impl Default for ExtBuilder {
    fn default() -> ExtBuilder {
        ExtBuilder {
            balances: vec![],
            nominations: vec![],
            collators: vec![],
            min_collator_stake: 10,
            min_total_nominator_stake: 5,
        }
    }
}

impl ExtBuilder {
    pub(crate) fn with_balances(mut self, balances: Vec<(AccountId, Balance)>) -> Self {
        self.balances = balances;
        self
    }

    pub(crate) fn with_candidates(mut self, collators: Vec<(AccountId, Balance)>) -> Self {
        self.collators = collators;
        self
    }

    pub(crate) fn with_nominations(
        mut self,
        nominations: Vec<(AccountId, AccountId, Balance)>,
    ) -> Self {
        self.nominations = nominations;
        self
    }

    pub(crate) fn with_staking_config(
        mut self,
        min_collator_stake: Balance,
        min_total_nominator_stake: Balance,
    ) -> Self {
        self.min_collator_stake = min_collator_stake;
        self.min_total_nominator_stake = min_total_nominator_stake;
        self
    }

    pub(crate) fn build(self) -> sp_io::TestExternalities {
        let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

        pallet_balances::GenesisConfig::<Test> { balances: self.balances, ..Default::default() }
            .assimilate_storage(&mut t)
            .expect("Pallet balances storage can be assimilated");
        pallet_parachain_staking::GenesisConfig::<Test> {
            candidates: self.collators,
            nominations: self.nominations,
            delay: 2,
            min_collator_stake: self.min_collator_stake,
            min_total_nominator_stake: self.min_total_nominator_stake,
        }
        .assimilate_storage(&mut t)
        .expect("Parachain Staking's storage can be assimilated");

        let mut ext = sp_io::TestExternalities::new(t);
        ext.execute_with(|| System::set_block_number(1));
        ext
    }
}

/// Rolls forward one block. Returns the new block number.
pub(crate) fn roll_one_block() -> u64 {
    <pallet_balances::Pallet<mock::Test> as OnFinalize<BlockNumber>>::on_finalize(
        System::block_number(),
    );
    <frame_system::Pallet<mock::Test> as OnFinalize<BlockNumber>>::on_finalize(
        System::block_number(),
    );
    System::set_block_number(System::block_number() + 1);
    <frame_system::Pallet<mock::Test> as OnInitialize<BlockNumber>>::on_initialize(
        System::block_number(),
    );
    <pallet_balances::Pallet<mock::Test> as OnInitialize<BlockNumber>>::on_initialize(
        System::block_number(),
    );
    <pallet::Pallet<mock::Test> as OnInitialize<BlockNumber>>::on_initialize(System::block_number());
    System::block_number()
}

/// Rolls to the desired block. Returns the number of blocks played.
pub(crate) fn roll_to(n: u64) -> u64 {
    let mut num_blocks = 0;
    let mut block = System::block_number();
    while block < n {
        block = roll_one_block();
        num_blocks += 1;
    }
    num_blocks
}

// This matches the genesis era length
pub fn get_default_block_per_era() -> u64 {
    return MinBlocksPerEra::get() as u64 + 2
}

/// Rolls block-by-block to the beginning of the specified era.
/// This will complete the block in which the era change occurs.
/// Returns the number of blocks played.
pub(crate) fn roll_to_era_begin(era: u64) -> u64 {
    let block = (era - 1) * get_default_block_per_era();
    roll_to(block)
}

/// Rolls block-by-block to the end of the specified era.
/// The block following will be the one in which the specified era change occurs.
pub(crate) fn roll_to_era_end(era: u64) -> u64 {
    let block = era * get_default_block_per_era() - 1;
    roll_to(block)
}

pub(crate) fn last_event() -> RuntimeEvent {
    System::events().pop().expect("Event expected").event
}

pub(crate) fn set_reward_pot(amount: Balance) {
    Balances::make_free_balance_be(&ParachainStaking::compute_reward_pot_account_id(), amount);
    crate::LockedEraPayout::<Test>::put(0);
}

pub(crate) fn events() -> Vec<pallet::Event<Test>> {
    System::events()
        .into_iter()
        .map(|r| r.event)
        .filter_map(
            |e| if let RuntimeEvent::ParachainStaking(inner) = e { Some(inner) } else { None },
        )
        .collect::<Vec<_>>()
}

impl ParachainStaking {
    pub fn set_root_as_triggered(period: u32) {
        <Growth<Test>>::mutate(period, |growth| growth.triggered = Some(true));
    }

    pub fn insert_growth_data(period: u32, growth_info: GrowthInfo<AccountId, Balance>) {
        <Growth<Test>>::insert(period, growth_info);
        <GrowthPeriod<Test>>::mutate(|info| {
            info.index = info.index.saturating_add(1);
        });
    }
}

impl FeePaymentHandler for Test {
    type Token = sp_core::H160;
    type TokenBalance = u128;
    type AccountId = AccountId;
    type Error = DispatchError;

    fn pay_fee(
        _token: &Self::Token,
        _amount: &Self::TokenBalance,
        _payer: &Self::AccountId,
        _recipient: &Self::AccountId,
    ) -> Result<(), Self::Error> {
        return Err(DispatchError::Other("Test - Error"))
    }
    fn pay_treasury(
        _amount: &Self::TokenBalance,
        _payer: &Self::AccountId,
    ) -> Result<(), Self::Error> {
        return Err(DispatchError::Other("Test - Error"))
    }
}

/// Assert input equal to the last event emitted
#[macro_export]
macro_rules! assert_last_event {
    ($event:expr) => {
        match &$event {
            e => assert_eq!(*e, crate::mock::last_event()),
        }
    };
}

/// Compares the system events with passed in events
/// Prints highlighted diff iff assert_eq fails
#[macro_export]
macro_rules! assert_eq_events {
    ($events:expr) => {
        match &$events {
            e => similar_asserts::assert_eq!(*e, crate::mock::events()),
        }
    };
}

/// Compares the last N system events with passed in events, where N is the length of events passed
/// in.
///
/// Prints highlighted diff iff assert_eq fails.
/// The last events from frame_system will be taken in order to match the number passed to this
/// macro. If there are insufficient events from frame_system, they will still be compared; the
/// output may or may not be helpful.
///
/// Examples:
/// If frame_system has events [A, B, C, D, E] and events [C, D, E] are passed in, the result would
/// be a successful match ([C, D, E] == [C, D, E]).
///
/// If frame_system has events [A, B, C, D] and events [B, C] are passed in, the result would be an
/// error and a hopefully-useful diff will be printed between [C, D] and [B, C].
///
/// Note that events are filtered to only match parachain-staking (see events()).
#[macro_export]
macro_rules! assert_eq_last_events {
	($events:expr $(,)?) => {
		assert_tail_eq!($events, crate::mock::events());
	};
	($events:expr, $($arg:tt)*) => {
		assert_tail_eq!($events, crate::mock::events(), $($arg)*);
	};
}

/// Assert that one array is equal to the tail of the other. A more generic and testable version of
/// assert_eq_last_events.
#[macro_export]
macro_rules! assert_tail_eq {
	($tail:expr, $arr:expr $(,)?) => {
		if !$tail.is_empty() {
			// 0-length always passes

			if $tail.len() > $arr.len() {
				similar_asserts::assert_eq!($tail, $arr); // will fail
			}

			let len_diff = $arr.len() - $tail.len();
			similar_asserts::assert_eq!($tail, $arr[len_diff..]);
		}
	};
	($tail:expr, $arr:expr, $($arg:tt)*) => {
		if !$tail.is_empty() {
			// 0-length always passes

			if $tail.len() > $arr.len() {
				similar_asserts::assert_eq!($tail, $arr, $($arg)*); // will fail
			}

			let len_diff = $arr.len() - $tail.len();
			similar_asserts::assert_eq!($tail, $arr[len_diff..], $($arg)*);
		}
	};
}

/// Panics if an event is not found in the system log of events
#[macro_export]
macro_rules! assert_event_emitted {
    ($event:expr) => {
        match &$event {
            e => {
                assert!(
                    crate::mock::events().iter().find(|x| *x == e).is_some(),
                    "Event {:?} was not found in events: \n {:?}",
                    e,
                    crate::mock::events()
                );
            },
        }
    };
}

/// Panics if an event is found in the system log of events
#[macro_export]
macro_rules! assert_event_not_emitted {
    ($event:expr) => {
        match &$event {
            e => {
                assert!(
                    crate::mock::events().iter().find(|x| *x == e).is_none(),
                    "Event {:?} was found in events: \n {:?}",
                    e,
                    crate::mock::events()
                );
            },
        }
    };
}

// Same storage changes as ParachainStaking::on_finalize
pub(crate) fn set_author(era: u32, acc: AccountId, pts: u32) {
    <Points<Test>>::mutate(era, |p| *p += pts);
    <AwardedPts<Test>>::mutate(era, acc, |p| *p += pts);
}

/// fn to query the lock amount
pub(crate) fn query_lock_amount(account_id: AccountId, id: LockIdentifier) -> Option<Balance> {
    for lock in Balances::locks(&account_id) {
        if lock.id == id {
            return Some(lock.amount)
        }
    }
    None
}

pub(crate) fn pay_gas_for_transaction(sender: &AccountId, tip: u128) {
    let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
    let info = DispatchInfo { call_weight: Weight::from_parts(1, 0), ..Default::default() };
    let len = TX_LEN;
    let ext = ChargeTransactionPayment::<Test>::from(tip);
    let implicit = ext.implicit().expect("implicit data available");
    let implication = TxBaseImplication(call.clone());
    let origin = RuntimeOrigin::signed(sender.clone());
    let (_valid, val, origin) = ext
        .validate(origin, &call, &info, len, implicit, &implication, TransactionSource::Local)
        .expect("validation should succeed");
    let pre = ext.prepare(val, &origin, &call, &info, len).expect("prepare should charge fee");

    assert_ok!(ChargeTransactionPayment::<Test>::post_dispatch(
        pre,
        &info,
        &mut PostDispatchInfo { actual_weight: None, pays_fee: Default::default() },
        len,
        &Ok(())
    ));
}

fn fake_treasury() -> AccountId {
    return TestAccount::new(8999999998u64).account_id()
}

#[test]
fn genesis() {
    let collator_1 = TestAccount::new(1u64).account_id();
    let collator_2 = TestAccount::new(2u64).account_id();
    let nominator_3 = TestAccount::new(3u64).account_id();
    let nominator_4 = TestAccount::new(4u64).account_id();
    let nominator_5 = TestAccount::new(5u64).account_id();
    let nominator_6 = TestAccount::new(6u64).account_id();
    let user_7 = TestAccount::new(7u64).account_id();
    let user_8 = TestAccount::new(8u64).account_id();
    let user_9 = TestAccount::new(9u64).account_id();

    let acc = |id: u64| -> AccountId { TestAccount::new(id).account_id() };

    ExtBuilder::default()
        .with_balances(vec![
            (collator_1, 1000),
            (collator_2, 300),
            (nominator_3, 100),
            (nominator_4, 100),
            (nominator_5, 100),
            (nominator_6, 100),
            (user_7, 100),
            (user_8, 9),
            (user_9, 4),
        ])
        .with_candidates(vec![(collator_1, 500), (collator_2, 200)])
        .with_nominations(vec![
            (nominator_3, collator_1, 100),
            (nominator_4, collator_1, 100),
            (nominator_5, collator_2, 100),
            (nominator_6, collator_2, 100),
        ])
        .build()
        .execute_with(|| {
            assert!(System::events().is_empty());
            // collators
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&collator_1), 500);
            assert_eq!(query_lock_amount(collator_1, COLLATOR_LOCK_ID), Some(500));
            assert!(ParachainStaking::is_candidate(&collator_1));
            assert_eq!(query_lock_amount(collator_2, COLLATOR_LOCK_ID), Some(200));
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&collator_2), 100);
            assert!(ParachainStaking::is_candidate(&collator_2));
            // nominators
            for x in 3..7 {
                let account_id = acc(x);
                assert!(ParachainStaking::is_nominator(&account_id));
                assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id), 0);
                assert_eq!(query_lock_amount(account_id, NOMINATOR_LOCK_ID), Some(100));
            }
            // uninvolved
            for x in 7..10 {
                let account_id = acc(x);
                assert!(!ParachainStaking::is_nominator(&account_id));
            }
            // no nominator staking locks
            assert_eq!(query_lock_amount(user_7, NOMINATOR_LOCK_ID), None);
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&user_7), 100);
            assert_eq!(query_lock_amount(user_8, NOMINATOR_LOCK_ID), None);
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&user_8), 9);
            assert_eq!(query_lock_amount(user_9, NOMINATOR_LOCK_ID), None);
            assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&user_9), 4);
            // no collator staking locks
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&user_7), 100);
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&user_8), 9);
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&user_9), 4);
        });

    let collator_1 = TestAccount::new(1u64).account_id();
    let collator_2 = TestAccount::new(2u64).account_id();
    let collator_3 = TestAccount::new(3u64).account_id();
    let collator_4 = TestAccount::new(4u64).account_id();
    let collator_5 = TestAccount::new(5u64).account_id();
    let nominator_6 = TestAccount::new(6u64).account_id();
    let nominator_7 = TestAccount::new(7u64).account_id();
    let nominator_8 = TestAccount::new(8u64).account_id();
    let nominator_9 = TestAccount::new(9u64).account_id();
    let nominator_10 = TestAccount::new(10u64).account_id();
    ExtBuilder::default()
        .with_balances(vec![
            (collator_1, 100),
            (collator_2, 100),
            (collator_3, 100),
            (collator_4, 100),
            (collator_5, 100),
            (nominator_6, 100),
            (nominator_7, 100),
            (nominator_8, 100),
            (nominator_9, 100),
            (nominator_10, 100),
        ])
        .with_candidates(vec![
            (collator_1, 20),
            (collator_2, 20),
            (collator_3, 20),
            (collator_4, 20),
            (collator_5, 10),
        ])
        .with_nominations(vec![
            (nominator_6, collator_1, 10),
            (nominator_7, collator_1, 10),
            (nominator_8, collator_2, 10),
            (nominator_9, collator_2, 10),
            (nominator_10, collator_1, 10),
        ])
        .build()
        .execute_with(|| {
            assert!(System::events().is_empty());
            // collators
            for x in 1..5 {
                let account_id = acc(x);
                assert!(ParachainStaking::is_candidate(&account_id));
                assert_eq!(query_lock_amount(account_id, COLLATOR_LOCK_ID), Some(20));
                assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&account_id), 80);
            }
            assert!(ParachainStaking::is_candidate(&collator_5));
            assert_eq!(query_lock_amount(collator_5, COLLATOR_LOCK_ID), Some(10));
            assert_eq!(ParachainStaking::get_collator_stakable_free_balance(&collator_5), 90);
            // nominators
            for x in 6..11 {
                let account_id = acc(x);
                assert!(ParachainStaking::is_nominator(&account_id));
                assert_eq!(query_lock_amount(account_id, NOMINATOR_LOCK_ID), Some(10));
                assert_eq!(ParachainStaking::get_nominator_stakable_free_balance(&account_id), 90);
            }
        });
}

#[test]
fn roll_to_era_begin_works() {
    ExtBuilder::default().build().execute_with(|| {
        // these tests assume blocks-per-era of 5, as established by get_default_block_per_era()
        assert_eq!(System::block_number(), 1); // we start on block 1

        let num_blocks = roll_to_era_begin(1);
        assert_eq!(System::block_number(), 1); // no-op, we're already on this era
        assert_eq!(num_blocks, 0);

        let num_blocks = roll_to_era_begin(2);
        assert_eq!(System::block_number(), 5);
        assert_eq!(num_blocks, 4);

        let num_blocks = roll_to_era_begin(3);
        assert_eq!(System::block_number(), 10);
        assert_eq!(num_blocks, 5);
    });
}

#[test]
fn roll_to_era_end_works() {
    ExtBuilder::default().build().execute_with(|| {
        // these tests assume blocks-per-era of 5, as established by get_default_block_per_era()
        assert_eq!(System::block_number(), 1); // we start on block 1

        let num_blocks = roll_to_era_end(1);
        assert_eq!(System::block_number(), 4);
        assert_eq!(num_blocks, 3);

        let num_blocks = roll_to_era_end(2);
        assert_eq!(System::block_number(), 9);
        assert_eq!(num_blocks, 5);

        let num_blocks = roll_to_era_end(3);
        assert_eq!(System::block_number(), 14);
        assert_eq!(num_blocks, 5);
    });
}

#[test]
fn assert_tail_eq_works() {
    assert_tail_eq!(vec![1, 2], vec![0, 1, 2]);

    assert_tail_eq!(vec![1], vec![1]);

    assert_tail_eq!(
        vec![0u32; 0], // 0 length array
        vec![0u32; 1]  // 1-length array
    );

    assert_tail_eq!(vec![0u32, 0], vec![0u32, 0]);
}

#[test]
#[should_panic]
fn assert_tail_eq_panics_on_non_equal_tail() {
    assert_tail_eq!(vec![2, 2], vec![0, 1, 2]);
}

#[test]
#[should_panic]
fn assert_tail_eq_panics_on_empty_arr() {
    assert_tail_eq!(vec![2, 2], vec![0u32; 0]);
}

#[test]
#[should_panic]
fn assert_tail_eq_panics_on_longer_tail() {
    assert_tail_eq!(vec![1, 2, 3], vec![1, 2]);
}

#[test]
#[should_panic]
fn assert_tail_eq_panics_on_unequal_elements_same_length_array() {
    assert_tail_eq!(vec![1, 2, 3], vec![0, 1, 2]);
}
