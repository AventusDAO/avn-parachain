// This file is part of Aventus.
// Copyright (C) 2022 Aventus Network Services (UK) Ltd.

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use super::*;
use crate::{self as token_manager, Balances as TokenManagerBalances};
use frame_support::{
    derive_impl,
    dispatch::{DispatchClass, DispatchInfo},
    parameter_types,
    traits::{ConstU8, EqualPrivilegeOnly, Hooks},
    weights::{Weight, WeightToFee as WeightToFeeT},
    PalletId,
};
use frame_system::DefaultConfig;

use frame_system::{self as system, limits, EnsureRoot};
use sp_state_machine::BasicExternalities;

use hex_literal::hex;
use pallet_avn::BridgeInterfaceNotification;
use pallet_parachain_staking::{self as parachain_staking};
use pallet_session as session;
use pallet_transaction_payment::CurrencyAdapter;
use sp_avn_common::{
    avn_tests_helpers::ethereum_converters::*,
    eth::EthereumId,
    event_types::{EthEventId, LiftedData, ValidEvents},
    OnIdleHandler,
};
use sp_core::{sr25519, ConstU64, Pair, H256};
use sp_keystore::{testing::MemoryKeystore, KeystoreExt};
use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{ConvertInto, IdentifyAccount, IdentityLookup, Verify},
    BuildStorage, Perbill, SaturatedConversion,
};
use std::{cell::RefCell, sync::Arc};

/// The signature type used by accounts/transactions.
pub type Signature = sr25519::Signature;
/// An identifier for an account on this system.
pub type AccountId = <Signature as Verify>::Signer;
pub type Extrinsic = TestXt<RuntimeCall, ()>;

pub const AVT_TOKEN_CONTRACT: H160 = H160(hex!("dB1Cff52f66195f0a5Bd3db91137db98cfc54AE6"));
pub const ONE_TOKEN: u128 = 1_000000_000000_000000u128;
pub const AMOUNT_100_TOKEN: u128 = 100 * ONE_TOKEN;
pub const AMOUNT_123_TOKEN: u128 = 123 * ONE_TOKEN;
pub const EXISTENTIAL_DEPOSIT: u64 = 0;
pub const NON_AVT_TOKEN_ID: H160 = H160(hex!("1414141414141414141414141414141414141414"));
pub const NON_AVT_TOKEN_ID_2: H160 = H160(hex!("2020202020202020202020202020202020202020"));

const TOPIC_RECEIVER_INDEX: usize = 2;

type Block = frame_system::mocking::MockBlock<TestRuntime>;

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
        Avn: pallet_avn::{Pallet, Storage, Event},
        TokenManager: token_manager::{Pallet, Call, Storage, Event<T>, Config<T>},
        TransactionPayment: pallet_transaction_payment::{Pallet, Storage, Event<T>, Config<T>},
        Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
        ParachainStaking: parachain_staking::{Pallet, Call, Storage, Config<T>, Event<T>},
        Historical: pallet_session::historical::{Pallet, Storage},
        EthBridge: pallet_eth_bridge::{Pallet, Call, Storage, Event<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
        Preimage: pallet_preimage,
        Scheduler: pallet_scheduler::{Pallet, Call, Storage, Event<T>},
    }
);

parameter_types! {
    pub const AvnTreasuryPotId: PalletId = PalletId(*b"Treasury");
    pub static TreasuryGrowthPercentage: Perbill = Perbill::from_percent(75);
}

impl token_manager::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ProcessedEventsChecker = Self;
    type TokenId = sp_core::H160;
    type TokenBalance = u128;
    type Public = AccountId;
    type Signature = Signature;
    type AvnTreasuryPotId = AvnTreasuryPotId;
    type TreasuryGrowthPercentage = TreasuryGrowthPercentage;
    type OnGrowthLiftedHandler = ParachainStaking;
    type WeightInfo = ();
    type Scheduler = Scheduler;
    type Preimages = Preimage;
    type PalletsOrigin = OriginCaller;
    type BridgeInterface = EthBridge;
    type OnIdleHandler = TestOnIdleHandler;
    type AccountToBytesConvert = Avn;
    type TimeProvider = Timestamp;
}

parameter_types! {
    pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) * RuntimeBlockWeights::get().max_block;
}

impl pallet_preimage::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    type Currency = Balances;
    type ManagerOrigin = EnsureRoot<AccountId>;
    type Consideration = ();
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
    type Preimages = Preimage;
}

impl sp_runtime::BoundToRuntimeAppPublic for TestRuntime {
    type Public = <mock::TestRuntime as avn::Config>::AuthorityId;
}

impl<LocalCall> system::offchain::SendTransactionTypes<LocalCall> for TestRuntime
where
    RuntimeCall: From<LocalCall>,
{
    type OverarchingCall = RuntimeCall;
    type Extrinsic = Extrinsic;
}

pub const BASE_FEE: u64 = 12;

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);
const MAX_BLOCK_WEIGHT: Weight =
    Weight::from_parts(2_000_000_000_000 as u64, 0).set_proof_size(u64::MAX);

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    // Creating custom runtime block weights similar with substrate/frame/system/src/mock.rs
    pub BlockLength: limits::BlockLength = limits::BlockLength::max_with_normal_ratio(1024, NORMAL_DISPATCH_RATIO);
    pub RuntimeBlockWeights: limits::BlockWeights = limits::BlockWeights::builder()
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
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type BlockWeights = RuntimeBlockWeights;
    type Nonce = u64;
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type AccountData = pallet_balances::AccountData<u128>;
}

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl avn::Config for TestRuntime {
    type AuthorityId = UintAuthorityId;
}

parameter_types! {
    pub const ExistentialDeposit: u64 = EXISTENTIAL_DEPOSIT;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for TestRuntime {
    type Balance = u128;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
}

parameter_types! {
    pub static WeightToFee: u128 = 1u128;
    pub static TransactionByteFee: u128 = 0u128;
}
impl pallet_transaction_payment::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = CurrencyAdapter<Balances, ()>;
    type LengthToFee = TransactionByteFee;
    type WeightToFee = WeightToFee;
    type FeeMultiplierUpdate = ();
    type OperationalFeeMultiplier = ConstU8<5>;
}

impl session::Config for TestRuntime {
    type SessionManager = ParachainStaking;
    type Keys = UintAuthorityId;
    type ShouldEndSession = ParachainStaking;
    type SessionHandler = (Avn,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = ParachainStaking;
    type WeightInfo = ();
}

parameter_types! {
    pub const MinBlocksPerEra: u32 = 2;
    pub const DefaultBlocksPerEra: u32 = 2;
    pub const MinSelectedCandidates: u32 = 10;
    pub const MaxTopNominationsPerCandidate: u32 = 4;
    pub const MaxBottomNominationsPerCandidate: u32 = 4;
    pub const MaxNominationsPerNominator: u32 = 4;
    pub const MinNominationPerCollator: u128 = 3;
    pub const ErasPerGrowthPeriod: u32 = 2;
    pub const RewardPaymentDelay: u32 = 2;
    pub const RewardPotId: PalletId = PalletId(*b"av/vamgr");
    pub const MaxCandidates: u32 = 100;
    pub const GrowthEnabled: bool = true;
}

impl parachain_staking::Config for TestRuntime {
    type RuntimeCall = RuntimeCall;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type MinBlocksPerEra = MinBlocksPerEra;
    type RewardPaymentDelay = RewardPaymentDelay;
    type MinSelectedCandidates = MinSelectedCandidates;
    type MaxTopNominationsPerCandidate = MaxTopNominationsPerCandidate;
    type MaxBottomNominationsPerCandidate = MaxBottomNominationsPerCandidate;
    type MaxNominationsPerNominator = MaxNominationsPerNominator;
    type MinNominationPerCollator = MinNominationPerCollator;
    type RewardPotId = RewardPotId;
    type ErasPerGrowthPeriod = ErasPerGrowthPeriod;
    type Public = AccountId;
    type Signature = Signature;
    type CollatorSessionRegistration = Session;
    type CollatorPayoutDustHandler = TokenManager;
    type ProcessedEventsChecker = ();
    type WeightInfo = ();
    type MaxCandidates = MaxCandidates;
    type AccountToBytesConvert = Avn;
    type BridgeInterface = EthBridge;
    type GrowthEnabled = GrowthEnabled;
}

impl pallet_session::historical::Config for TestRuntime {
    type FullIdentification = AccountId;
    type FullIdentificationOf = ConvertInto;
}

impl pallet_eth_bridge::Config for TestRuntime {
    type MaxQueuedTxRequests = frame_support::traits::ConstU32<100>;
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

/// create a transaction info struct from weight. Handy to avoid building the whole struct.
pub fn info_from_weight(w: Weight) -> DispatchInfo {
    DispatchInfo { weight: w, ..Default::default() }
}

#[derive(Clone)]
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

thread_local! {
    static PROCESSED_EVENTS: RefCell<Vec<EthEventId>> = RefCell::new(vec![]);
    static ON_IDLE_RUN: RefCell<bool> = RefCell::new(false);
}

pub fn set_on_idle_run(run: bool) {
    ON_IDLE_RUN.with(|f| *f.borrow_mut() = run);
}

pub fn on_idle_has_run() -> bool {
    return ON_IDLE_RUN.with(|f| *f.borrow())
}

pub fn insert_to_mock_processed_events(event_id: &EthEventId) {
    PROCESSED_EVENTS.with(|l| l.borrow_mut().push(event_id.clone()));
}

impl ProcessedEventsChecker for TestRuntime {
    fn processed_event_exists(event_id: &EthEventId) -> bool {
        return PROCESSED_EVENTS.with(|l| l.borrow_mut().iter().any(|event| event == event_id))
    }

    fn add_processed_event(_event_id: &EthEventId, _accepted: bool) -> Result<(), ()> {
        Ok(())
    }
}

impl TokenManager {
    pub fn initialise_non_avt_tokens_to_account(
        account_id: <TestRuntime as system::Config>::AccountId,
        amount: u128,
    ) {
        TokenManagerBalances::<TestRuntime>::insert((NON_AVT_TOKEN_ID, account_id), amount);
    }
}

pub fn genesis_collators() -> Vec<AccountId> {
    return vec![TestAccount::new([1u8; 32]).account_id(), TestAccount::new([2u8; 32]).account_id()]
}

pub struct ExtBuilder {
    storage: sp_runtime::Storage,
}

impl ExtBuilder {
    pub fn build_default() -> Self {
        let storage = system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();
        Self { storage }
    }

    pub fn with_genesis_config(mut self) -> Self {
        let _ = token_manager::GenesisConfig::<TestRuntime> {
            _phantom: Default::default(),
            lower_account_id: H256::random(),
            avt_token_contract: AVT_TOKEN_CONTRACT,
            lower_schedule_period: 10,
            balances: vec![],
        }
        .assimilate_storage(&mut self.storage);

        self
    }

    pub fn with_validators(mut self) -> Self {
        let genesis_accounts_ids = genesis_collators();
        BasicExternalities::execute_with_storage(&mut self.storage, || {
            for ref k in genesis_accounts_ids.clone() {
                frame_system::Pallet::<TestRuntime>::inc_providers(k);
            }
        });

        self = self.with_balances();

        let _ = session::GenesisConfig::<TestRuntime> {
            keys: genesis_accounts_ids
                .clone()
                .into_iter()
                .enumerate()
                .map(|(i, v)| (v, v, UintAuthorityId((i as u32).into())))
                .collect(),
        }
        .assimilate_storage(&mut self.storage);

        let _ = parachain_staking::GenesisConfig::<TestRuntime> {
            candidates: genesis_accounts_ids.into_iter().map(|c| (c, 1000)).collect(),
            nominations: vec![],
            delay: 2,
            min_collator_stake: 10,
            min_total_nominator_stake: 5,
        }
        .assimilate_storage(&mut self.storage);

        self
    }

    pub fn with_balances(mut self) -> Self {
        let mut balances = vec![
            (account_id_with_100_avt(), AMOUNT_100_TOKEN),
            (account_id2_with_100_avt(), AMOUNT_100_TOKEN),
            (account_id3_with_100_avt(), AMOUNT_100_TOKEN),
        ];
        balances.append(&mut genesis_collators().into_iter().map(|c| (c, 1000)).collect());

        let _ = pallet_balances::GenesisConfig::<TestRuntime> { balances }
            .assimilate_storage(&mut self.storage);
        self
    }

    pub fn as_externality(self) -> sp_io::TestExternalities {
        let keystore = MemoryKeystore::new();

        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(KeystoreExt(Arc::new(keystore)));
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| System::set_block_number(1));
        ext
    }
}

pub fn key_pair_for_account_with_100_avt() -> sr25519::Pair {
    return sr25519::Pair::from_seed(&[69u8; 32])
}

pub fn receiver_topic_with_100_avt() -> Vec<u8> {
    let pair = key_pair_for_account_with_100_avt();
    return pair.public().to_vec()
}

pub fn account_id_with_100_avt() -> <TestRuntime as system::Config>::AccountId {
    return <TestRuntime as system::Config>::AccountId::decode(
        &mut receiver_topic_with_100_avt().as_slice(),
    )
    .unwrap()
}

pub fn account_id2_with_100_avt() -> <TestRuntime as system::Config>::AccountId {
    let pair = sr25519::Pair::from_seed(&[79u8; 32]);
    return <TestRuntime as system::Config>::AccountId::decode(
        &mut pair.public().to_vec().as_slice(),
    )
    .unwrap()
}

pub fn account_id3_with_100_avt() -> <TestRuntime as system::Config>::AccountId {
    let pair = sr25519::Pair::from_seed(&[89u8; 32]);
    return <TestRuntime as system::Config>::AccountId::decode(
        &mut pair.public().to_vec().as_slice(),
    )
    .unwrap()
}

pub fn account_id_with_seed_item(seed_item: u8) -> <TestRuntime as system::Config>::AccountId {
    let key_pair_for_account_with_max_avt = sr25519::Pair::from_seed(&[seed_item; 32]);
    let account_with_max_avt = key_pair_for_account_with_max_avt.public().to_vec();
    return <TestRuntime as system::Config>::AccountId::decode(&mut account_with_max_avt.as_slice())
        .unwrap()
}

pub fn forward_to_next_block() {
    Scheduler::on_finalize(System::block_number());
    System::set_block_number(System::block_number() + 1);
    Scheduler::on_initialize(System::block_number());
}

pub fn fast_forward_to_block(n: u64) {
    println!("fast_forward_to_block: {}", n);
    while System::block_number() < n {
        forward_to_next_block();
    }
}

pub fn get_expected_execution_block() -> u64 {
    return System::block_number() + LowerSchedulePeriod::<TestRuntime>::get() + 1
}

pub struct MockData {
    pub avt_token_lift_event: EthEvent,
    pub non_avt_token_lift_event: EthEvent,
    pub empty_data_lift_event: EthEvent,
    pub lower_claimed_event: EthEvent,
    pub receiver_account_id: <TestRuntime as system::Config>::AccountId,
    pub token_balance_123_tokens: <TestRuntime as Config>::TokenBalance,
}

impl MockData {
    pub fn setup(amount_to_lift: u128, use_receiver_with_existing_amount: bool) -> Self {
        let lift_avt_token_event_topics =
            Self::get_lifted_avt_token_topics(use_receiver_with_existing_amount);
        let lift_non_avt_token_event_topics =
            Self::get_lifted_non_avt_token_topics(use_receiver_with_existing_amount);
        let receiver_account_id =
            Self::get_receiver_account_id_from_topics(&lift_avt_token_event_topics);

        if use_receiver_with_existing_amount {
            TokenManager::initialise_non_avt_tokens_to_account(
                receiver_account_id,
                AMOUNT_100_TOKEN,
            );
        }

        MockData {
            avt_token_lift_event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256::random(),
                },
                event_data: Self::get_event_data(amount_to_lift, &lift_avt_token_event_topics),
            },
            non_avt_token_lift_event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256::random(),
                },
                event_data: Self::get_event_data(amount_to_lift, &lift_non_avt_token_event_topics),
            },
            empty_data_lift_event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::Lifted.signature(),
                    transaction_hash: H256::random(),
                },
                event_data: EventData::EmptyEvent,
            },
            lower_claimed_event: EthEvent {
                event_id: EthEventId {
                    signature: ValidEvents::AvtLowerClaimed.signature(),
                    transaction_hash: H256::random(),
                },
                event_data: EventData::LogLowerClaimed(AvtLowerClaimedData { lower_id: 0 }),
            },
            receiver_account_id,
            token_balance_123_tokens: Self::get_token_balance(AMOUNT_123_TOKEN),
        }
    }

    pub fn setup_lower_request_data() -> (
        Vec<u8>,                                    // from_account
        <TestRuntime as system::Config>::AccountId, // from_account_id
        <TestRuntime as system::Config>::AccountId, // to_account_id
        H160,                                       // t1_recipient
    ) {
        let from_account = receiver_topic_with_100_avt();
        let from_account_id = account_id_with_100_avt();
        TokenManager::initialise_non_avt_tokens_to_account(from_account_id, AMOUNT_100_TOKEN);
        let to_account = TokenManager::lower_account_id();
        let to_account_id =
            <TestRuntime as system::Config>::AccountId::decode(&mut to_account.as_bytes()).unwrap();
        let t1_recipient = H160(hex!("7F792259892d2D07323cF5c449c27eaA50B2Cde3"));

        return (from_account, from_account_id, to_account_id, t1_recipient)
    }

    fn get_event_data(amount: u128, topics: &Vec<Vec<u8>>) -> EventData {
        let data = Some(Self::get_lifted_token_data(amount));
        let event_data =
            EventData::LogLifted(LiftedData::parse_bytes(data, topics.clone()).unwrap());

        if let EventData::LogLifted(d) = event_data.clone() {
            assert_eq!(d.amount, amount);
        }

        return event_data
    }

    fn get_lifted_token_data(amount: u128) -> Vec<u8> {
        let mut data = Vec::new();

        let amount_vec = into_32_be_bytes(&amount.to_le_bytes());
        data.extend(&amount_vec);

        return data
    }

    fn get_lifted_avt_token_topics(use_receiver_with_existing_amount: bool) -> Vec<Vec<u8>> {
        let topic_event_signature = Self::get_topic_32_bytes(10);
        let topic_contract = Self::get_contract_topic(true);
        let topic_receiver = Self::get_receiver_topic(use_receiver_with_existing_amount);

        return vec![topic_event_signature, topic_contract, topic_receiver]
    }

    fn get_lifted_non_avt_token_topics(use_receiver_with_existing_amount: bool) -> Vec<Vec<u8>> {
        let topic_event_signature = Self::get_topic_32_bytes(10);
        let topic_contract = Self::get_contract_topic(false);
        let topic_receiver = Self::get_receiver_topic(use_receiver_with_existing_amount);

        return vec![topic_event_signature, topic_contract, topic_receiver]
    }

    fn get_contract_topic(use_avt_token_contract: bool) -> Vec<u8> {
        if use_avt_token_contract {
            let mut topic = vec![0; 12];
            topic.append(&mut AVT_TOKEN_CONTRACT.clone().as_fixed_bytes_mut().to_vec());
            return topic
        }

        return Self::get_topic_20_bytes(20)
    }

    fn get_receiver_topic(use_receiver_with_existing_amount: bool) -> Vec<u8> {
        if use_receiver_with_existing_amount {
            return receiver_topic_with_100_avt()
        }

        return Self::get_topic_32_bytes(40)
    }

    fn get_topic_32_bytes(n: u8) -> Vec<u8> {
        return vec![n; 32]
    }

    fn get_topic_20_bytes(n: u8) -> Vec<u8> {
        let mut topic = vec![0; 12];
        topic.append(&mut vec![n; 20]);

        return topic
    }

    fn get_receiver_account_id_from_topics(
        topics: &Vec<Vec<u8>>,
    ) -> <TestRuntime as system::Config>::AccountId {
        let receiver_topic = topics[TOPIC_RECEIVER_INDEX].clone();
        return <TestRuntime as system::Config>::AccountId::decode(&mut receiver_topic.as_slice())
            .unwrap()
    }

    pub fn get_token_balance(balance_in_u128: u128) -> <TestRuntime as Config>::TokenBalance {
        return <<TestRuntime as Config>::TokenBalance as TryFrom<u128>>::try_from(balance_in_u128)
            .expect("Balance value overflow")
    }

    pub fn set_avt_balance(
        account_id: <TestRuntime as system::Config>::AccountId,
        amount: u128,
    ) -> bool {
        let amount =
            <BalanceOf<TestRuntime> as TryFrom<u128>>::try_from(amount).expect("amount is valid");
        let imbalance: PositiveImbalanceOf<TestRuntime> =
            <mock::TestRuntime as token_manager::Config>::Currency::deposit_creating(
                &account_id,
                amount,
            );
        if imbalance.peek() == BalanceOf::<TestRuntime>::zero() {
            return false
        }
        drop(imbalance);
        return true
    }
}

// ============================= Signature handling ========================
pub fn sign(signer: &sr25519::Pair, message_to_sign: &[u8]) -> Signature {
    return Signature::from(signer.sign(message_to_sign))
}

pub fn get_account_id(signer: &sr25519::Pair) -> AccountId {
    return AccountId::from(signer.public()).into_account()
}

#[allow(dead_code)]
pub fn verify_signature(signature: Signature, signer: AccountId, signed_data: &[u8]) -> bool {
    return signature.verify(signed_data, &signer)
}

pub fn create_valid_signature_for_signed_transfer(
    relayer: &AccountId,
    from: &AccountId,
    to: &AccountId,
    token_id: H160,
    amount: u128,
    nonce: u64,
    keys: &sr25519::Pair,
) -> Signature {
    let context = SIGNED_TRANSFER_CONTEXT;
    let data_to_sign = (context, relayer, from, to, token_id, amount, nonce);

    return sign(&keys, &data_to_sign.encode())
}
// ============================= Mock correctness tests ========================

#[test]
// Important - do not remove this test
fn avn_test_log_parsing_logic() {
    let mut ext = ExtBuilder::build_default().as_externality();

    ext.execute_with(|| {
        let u128_max_value = u128::max_value();
        let topics = MockData::get_lifted_avt_token_topics(false);
        let event_data = MockData::get_event_data(u128_max_value, &topics);

        if let EventData::LogLifted(d) = event_data.clone() {
            assert_eq!(d.amount, u128_max_value);
        } else {
            // We should never get here, but in case we do force test to fail
            assert!(false);
        }
    });
}

pub struct TestOnIdleHandler;

impl OnIdleHandler<u64, Weight> for TestOnIdleHandler {
    fn run_on_idle_process(_block_number: u64, remaining_weight: Weight) -> Weight {
        set_on_idle_run(true);
        remaining_weight
    }
}
