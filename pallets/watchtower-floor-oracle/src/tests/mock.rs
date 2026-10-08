// Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::{self as pallet_watchtower_floor_oracle, *};
use frame_support::{derive_impl, parameter_types, WeakBoundedVec};
use frame_system::{self as system};
use sp_core::ConstU64;
use sp_runtime::{
    testing::{TestSignature, TestXt, UintAuthorityId},
    BuildStorage,
};

pub type Block = frame_system::mocking::MockBlock<TestRuntime>;
pub type Extrinsic = TestXt<RuntimeCall, ()>;
pub type AccountId = u64;
pub type BlockNumber = u64;

pub const ROUND_TIMEOUT_BLOCKS: BlockNumber = 10;
pub const VALIDATORS: [AccountId; 6] = [1, 2, 3, 4, 5, 6];

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances,
        Timestamp: pallet_timestamp,
        Avn: pallet_avn::{Pallet, Storage, Event},
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
    pub const RoundTimeoutBlocks: BlockNumber = ROUND_TIMEOUT_BLOCKS;
}

impl Config for TestRuntime {
    type RuntimeCall = RuntimeCall;
    type TimeProvider = Timestamp;
    type Quorum = Avn;
    type RoundTimeoutBlocks = RoundTimeoutBlocks;
    type WeightInfo = ();
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Nonce = u64;
    type Block = Block;
    type AccountData = pallet_balances::AccountData<u64>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for TestRuntime {
    type AccountStore = System;
}

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

pub fn author(account_id: AccountId) -> Author<TestRuntime> {
    Author::<TestRuntime> { account_id, key: UintAuthorityId(account_id) }
}

pub fn set_validators(accounts: &[AccountId]) {
    let validators: Vec<Author<TestRuntime>> = accounts.iter().map(|a| author(*a)).collect();
    pallet_avn::Validators::<TestRuntime>::put(WeakBoundedVec::force_from(
        validators,
        Some("Too many validators"),
    ));
}

/// A vote as a collator would build it.
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
        account_id: AccountId,
        block_number: u32,
    ) -> Self {
        Self { chain_id, round_id, author: author(account_id), block_number }
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

pub struct ExtBuilder {
    storage: sp_runtime::Storage,
}

impl ExtBuilder {
    pub fn build_default() -> Self {
        let storage =
            frame_system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();
        Self { storage }
    }

    /// Externalities at block 1 with the six default validators and the clock at 1 second.
    pub fn as_externality(self) -> sp_io::TestExternalities {
        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.execute_with(|| {
            System::set_block_number(1);
            Timestamp::set_timestamp(1_000);
            set_validators(&VALIDATORS);
        });
        ext
    }
}
