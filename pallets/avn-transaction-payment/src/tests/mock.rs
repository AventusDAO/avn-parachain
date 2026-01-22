use crate::{
    self as pallet_avn_transaction_payment, system::limits, AvnCurrencyAdapter, KnownSenders,
    NativeRateProvider,
};
use codec::{Decode, Encode};
use frame_support::{
    derive_impl,
    pallet_prelude::DispatchClass,
    parameter_types,
    traits::{ConstU8, Currency, ExistenceRequirement, Imbalance, OnFinalize, OnInitialize},
    weights::{Weight, WeightToFee as WeightToFeeT},
    PalletId,
};
use frame_system::{self as system, DefaultConfig};
use pallet_avn_transaction_payment::BalanceOf;
use pallet_balances;
use sp_core::{sr25519, Pair, U256};
use sp_runtime::{
    traits::{AccountIdConversion, IdentityLookup, Verify, Zero},
    BuildStorage, FixedPointNumber, Perbill, SaturatedConversion,
};

pub type AccountId = <Signature as Verify>::Signer;
pub type Signature = sr25519::Signature;

type Block = frame_system::mocking::MockBlock<TestRuntime>;

pub const EXISTENTIAL_DEPOSIT: u64 = 0;
pub const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);
pub const MAX_BLOCK_WEIGHT: Weight = Weight::from_parts(1024 as u64, u64::MAX);
pub const BASE_FEE_U64: u64 = 12;
pub const BASE_FEE: u128 = BASE_FEE_U64 as u128;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Event<T>},
        TransactionPayment: pallet_transaction_payment::{Pallet, Storage, Event<T>, Config<T>},
        AvnTransactionPayment: pallet_avn_transaction_payment::{Pallet, Call, Storage, Event<T>},
        Authorship: pallet_authorship::{Pallet, Storage},
    }
);

parameter_types! {
    pub BlockLength: limits::BlockLength = limits::BlockLength::max_with_normal_ratio(1024, NORMAL_DISPATCH_RATIO);
    pub RuntimeBlockWeights: limits::BlockWeights = limits::BlockWeights::builder()
        .base_block(Weight::from_parts(10 as u64,0))
        .for_class(DispatchClass::all(), |weights| {
            weights.base_extrinsic = Weight::from_parts(BASE_FEE_U64, 0);
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

pub struct TestRateProvider;
impl NativeRateProvider for TestRateProvider {
    fn native_rate_usd() -> Option<u128> {
        Some(25_000_000u128)
    }
}

pub struct PayCollatorAndBurn;
impl crate::FeeDistributor<TestRuntime> for PayCollatorAndBurn {
    fn distribute_fees(
        fee_pot: &AccountId,
        total_fees: BalanceOf<TestRuntime>,
        used_weight: u128,
        max_weight: u128,
    ) {
        if total_fees.is_zero() {
            return;
        }

        let burn_pot: AccountId = PalletId(sp_avn_common::BURN_POT_ID).into_account_truncating();

        let collator_ratio =
            crate::Pallet::<TestRuntime>::collator_ratio_from_weights(used_weight, max_weight);

        let collator_share: BalanceOf<TestRuntime> = collator_ratio.saturating_mul_int(total_fees);
        let mut burn_share: BalanceOf<TestRuntime> = total_fees.saturating_sub(collator_share);

        // Pay collator; if no author, burn everything.
        if !collator_share.is_zero() {
            match pallet_authorship::Pallet::<TestRuntime>::author() {
                Some(author) => {
                    let _ = <Balances as Currency<AccountId>>::transfer(
                        fee_pot,
                        &author,
                        collator_share,
                        ExistenceRequirement::KeepAlive,
                    );
                },
                None => {
                    burn_share = total_fees;
                },
            }
        }

        // Send rest to burn pot
        if !burn_share.is_zero() {
            let _ = <Balances as Currency<AccountId>>::transfer(
                fee_pot,
                &burn_pot,
                burn_share,
                ExistenceRequirement::AllowDeath,
            );
        }
    }
}

impl pallet_avn_transaction_payment::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type KnownUserOrigin = frame_system::EnsureRoot<AccountId>;
    type WeightInfo = pallet_avn_transaction_payment::default_weights::SubstrateWeight<TestRuntime>;
    type NativeRateProvider = TestRateProvider;
    type FeeDistributor = PayCollatorAndBurn;
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

parameter_types! {
    pub static WeightToFee: u128 = 1u128;
    pub static TransactionByteFee: u128 = 1u128;
}

pub struct DealWithFeesForTest;
impl frame_support::traits::OnUnbalanced<pallet_balances::NegativeImbalance<TestRuntime>>
    for DealWithFeesForTest
{
    fn on_unbalanceds<B>(
        mut fees_then_tips: impl Iterator<Item = pallet_balances::NegativeImbalance<TestRuntime>>,
    ) {
        if let Some(mut fees) = fees_then_tips.next() {
            if let Some(tips) = fees_then_tips.next() {
                tips.merge_into(&mut fees);
            }

            let fee_pot = pallet_avn_transaction_payment::Pallet::<TestRuntime>::fee_pot_account();
            pallet_balances::Pallet::<TestRuntime>::resolve_creating(&fee_pot, fees);
        }
    }
}

impl pallet_transaction_payment::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = AvnCurrencyAdapter<Balances, DealWithFeesForTest>;
    type LengthToFee = TransactionByteFee;
    type WeightToFee = WeightToFee;
    type FeeMultiplierUpdate = ();
    type OperationalFeeMultiplier = ConstU8<5>;
}

use frame_support::traits::FindAuthor;
use sp_runtime::ConsensusEngineId;

pub struct TestFindAuthor;
impl FindAuthor<AccountId> for TestFindAuthor {
    fn find_author<'a, I>(_digests: I) -> Option<AccountId>
    where
        I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
    {
        Some(test_collator())
    }
}

impl pallet_authorship::Config for TestRuntime {
    type FindAuthor = TestFindAuthor;
    type EventHandler = ();
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

impl AvnTransactionPayment {
    pub fn is_known_sender(account_id: <TestRuntime as frame_system::Config>::AccountId) -> bool {
        KnownSenders::<TestRuntime>::contains_key(account_id)
    }
}

#[derive(Clone, Copy)]
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

/// The global collator used in all tests.
pub fn test_collator() -> AccountId {
    TestAccount::new(42).account_id()
}

/// Rolls forward one block. Returns the new block number.
pub(crate) fn roll_one_block() -> u64 {
    Balances::on_finalize(System::block_number());
    System::on_finalize(System::block_number());
    System::set_block_number(System::block_number() + 1);
    System::on_initialize(System::block_number());
    Balances::on_initialize(System::block_number());
    System::block_number()
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
    let t = frame_system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

pub fn event_emitted(event: &RuntimeEvent) -> bool {
    return System::events().iter().any(|a| a.event == *event)
}
