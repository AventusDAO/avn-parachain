#[cfg(any(test, feature = "runtime-benchmarks"))]
use crate as pallet_cross_chain_voting;
use codec::Decode;
use frame_support::{derive_impl, parameter_types};
use frame_system::{self as system};
use libsecp256k1::PublicKey;
use pallet_balances;
use sp_core::{ecdsa, sr25519, Pair, H160};
use sp_runtime::{
    traits::{IdentityLookup, Verify},
    BuildStorage,
};

pub type Signature = sr25519::Signature;
pub type AccountId = <Signature as Verify>::Signer;

type Block = frame_system::mocking::MockBlock<TestRuntime>;

frame_support::construct_runtime!(
    pub enum TestRuntime {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Event<T>},
        CrossChainVoting: pallet_cross_chain_voting::{Pallet, Call, Storage, Event<T>},
    }
);

parameter_types! {
    pub const ExistentialDeposit: u64 = 0;
    pub const MaxLinkedAccounts: u32 = 2;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Nonce = u64;
    type AccountData = pallet_balances::AccountData<u128>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig as pallet_balances::DefaultConfig)]
impl pallet_balances::Config for TestRuntime {
    type Balance = u128;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
}

impl pallet_cross_chain_voting::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type MaxLinkedAccounts = MaxLinkedAccounts;
    type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let t = frame_system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

pub fn test_account(i: u8) -> AccountId {
    let seed = [i; 32];
    let pair = sr25519::Pair::from_seed(&seed);
    AccountId::decode(&mut pair.public().to_vec().as_slice()).unwrap()
}

pub fn test_ecdsa_pair(i: u8) -> ecdsa::Pair {
    let seed = [i; 32];
    ecdsa::Pair::from_seed(&seed)
}

pub fn eth_address_from_pair(pair: &ecdsa::Pair) -> H160 {
    let compressed = pair.public().0;
    let pubkey = PublicKey::parse_compressed(&compressed).expect("valid compressed pubkey");
    let uncompressed = pubkey.serialize();
    let hash = sp_io::hashing::keccak_256(&uncompressed[1..]);
    H160::from_slice(&hash[12..])
}
