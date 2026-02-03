//Copyright 2022 Aventus Network Services (UK) Ltd.

use super::*;
use codec::Decode;
use frame_support::{
    derive_impl,
    pallet_prelude::{DispatchClass, Weight},
    parameter_types,
    traits::EqualPrivilegeOnly,
    PalletId,
};
use frame_system::{self as system, limits::BlockWeights, DefaultConfig, EnsureRoot};
use hex_literal::hex;
use libsecp256k1::{sign, Message, PublicKey, SecretKey};
use pallet_avn::BridgeInterfaceNotification;
use pallet_balances;
use pallet_nft_manager::nft_data::Royalty;
use pallet_session as session;
use sp_avn_common::{eth::EthereumId, hash_string_data_with_ethereum_prefix};
use sp_core::{keccak_256, sr25519, ByteArray, ConstU32, ConstU64, Pair, H160, H256};
use sp_keystore::{testing::MemoryKeystore, KeystoreExt};
use sp_runtime::{
    testing::{TestXt, UintAuthorityId},
    traits::{ConvertInto, IdentityLookup, Verify},
    BuildStorage, MultiSignature, Perbill,
};
pub use std::sync::Arc;
pub const BASE_FEE: u64 = 12;

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);
const MAX_BLOCK_WEIGHT: Weight =
    Weight::from_parts(2_000_000_000_000 as u64, 0).set_proof_size(u64::MAX);
pub const ONE_AVT: u128 = 1_000000_000000_000000u128;
pub const HUNDRED_AVT: u128 = 100 * ONE_AVT;
pub const EXISTENTIAL_DEPOSIT: u64 = 0;
pub const AVT_TOKEN_CONTRACT: H160 = H160(hex!("dB1Cff52f66195f0a5Bd3db91137db98cfc54AE6"));
pub const NON_AVT_TOKEN_CONTRACT: H160 = H160(hex!("2020202020202020202020202020202020202020"));

/// The signature type used by accounts/transactions.
pub type Signature = MultiSignature;
pub type AccountPublic = <Signature as Verify>::Signer;
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

/// A token type
pub type Token = H160;

pub type Extrinsic = TestXt<RuntimeCall, ()>;

use crate::{self as avn_proxy};

impl<LocalCall> frame_system::offchain::CreateTransactionBase<LocalCall> for TestRuntime
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

frame_support::construct_runtime!(
    pub enum TestRuntime
    {
        System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
        Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
        Balances: pallet_balances::{Pallet, Call, Storage, Event<T>},
        NftManager: pallet_nft_manager::{Pallet, Call, Storage, Event<T>},
        AvnProxy: avn_proxy::{Pallet, Call, Storage, Event<T>},
        Avn: pallet_avn::{Pallet, Storage, Event, Config<T>},
        TokenManager: pallet_token_manager::{Pallet, Call, Storage, Event<T>},
        Scheduler: pallet_scheduler::{Pallet, Call, Storage, Event<T>},
        EthBridge: pallet_eth_bridge::{Pallet, Call, Storage, Event<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
        Historical: pallet_session::historical::{Pallet, Storage},
    }
);

impl Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type Public = AccountPublic;
    type Signature = Signature;
    type ProxyConfig = TestAvnProxyConfig;
    type WeightInfo = ();
    type FeeHandler = TokenManager;
    type Token = H160;
}

pub type AvnProxyCall = super::Call<TestRuntime>;
pub type SystemCall = frame_system::Call<TestRuntime>;
pub type BalancesCall = pallet_balances::Call<TestRuntime>;
pub type NftManagerCall = pallet_nft_manager::Call<TestRuntime>;
pub type TokenManagerCall = pallet_token_manager::Call<TestRuntime>;
pub type Hashing = <TestRuntime as system::Config>::Hashing;
type Block = frame_system::mocking::MockBlock<TestRuntime>;

impl sp_runtime::BoundToRuntimeAppPublic for TestRuntime {
    type Public = <mock::TestRuntime as pallet_avn::Config>::AuthorityId;
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for TestRuntime {
    type Nonce = u64;
    type Block = Block;
    type AccountData = pallet_balances::AccountData<u128>;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
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

impl pallet_nft_manager::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type ProcessedEventsChecker = ();
    type Public = AccountPublic;
    type Signature = Signature;
    type WeightInfo = ();
    type BatchBound = ConstU32<10>;
}

#[derive_impl(pallet_avn::config_preludes::TestDefaultConfig as pallet_avn::DefaultConfig)]
impl pallet_avn::Config for TestRuntime {
    type AuthorityId = UintAuthorityId;
}

parameter_types! {
    pub const AvnTreasuryPotId: PalletId = PalletId(*b"Treasury");
    pub static TreasuryGrowthPercentage: Perbill = Perbill::from_percent(75);
}

impl pallet_token_manager::Config for TestRuntime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ProcessedEventsChecker = ();
    type TokenId = sp_core::H160;
    type TokenBalance = u128;
    type Public = AccountPublic;
    type Signature = Signature;
    type AvnTreasuryPotId = AvnTreasuryPotId;
    type TreasuryGrowthPercentage = TreasuryGrowthPercentage;
    type OnGrowthLiftedHandler = ();
    type WeightInfo = ();
    type Scheduler = Scheduler;
    type Preimages = ();
    type PalletsOrigin = OriginCaller;
    type BridgeInterface = EthBridge;
    type OnIdleHandler = ();
    type AccountToBytesConvert = Avn;
}

parameter_types! {
    pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
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
    pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) * RuntimeBlockWeights::get().max_block;
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
    type Preimages = ();
}

impl pallet_eth_bridge::Config for TestRuntime {
    type MaxQueuedTxRequests = frame_support::traits::ConstU32<100>;
    type RuntimeEvent = RuntimeEvent;
    type TimeProvider = Timestamp;
    type MinEthBlockConfirmation = ConstU64<20>;
    type RuntimeCall = RuntimeCall;
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

parameter_types! {
    pub const Period: u64 = 1;
    pub const Offset: u64 = 0;
}

impl session::Config for TestRuntime {
    type SessionManager = ();
    type Keys = UintAuthorityId;
    type ShouldEndSession = session::PeriodicSessions<Period, Offset>;
    type SessionHandler = (Avn,);
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type NextSessionRotation = ();
    type WeightInfo = ();
}

impl pallet_session::historical::Config for TestRuntime {
    type FullIdentification = AccountId;
    type FullIdentificationOf = ConvertInto;
}

// Test Avn proxy configuration logic
// We only allow System::Remark and signed_mint_single_nft calls to be proxied
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, Debug, TypeInfo)]
pub struct TestAvnProxyConfig {}
impl Default for TestAvnProxyConfig {
    fn default() -> Self {
        TestAvnProxyConfig {}
    }
}

impl ProvableProxy<RuntimeCall, Signature, AccountId> for TestAvnProxyConfig {
    fn get_proof(call: &RuntimeCall) -> Option<Proof<Signature, AccountId>> {
        match call {
            RuntimeCall::System(system::Call::remark { remark: _msg }) => {
                let context: ProxyContext = Default::default();
                return Some(context.get_proof())
            },
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_mint_single_nft {
                proof,
                ..
            }) => return Some(proof.clone()),
            RuntimeCall::TokenManager(pallet_token_manager::Call::signed_transfer {
                proof,
                ..
            }) => return Some(proof.clone()),
            _ => None,
        }
    }
}

impl InnerCallValidator for TestAvnProxyConfig {
    type Call = RuntimeCall;

    fn signature_is_valid(call: &Box<Self::Call>) -> bool {
        match **call {
            RuntimeCall::System(..) => return true,
            RuntimeCall::NftManager(..) =>
                return pallet_nft_manager::Pallet::<TestRuntime>::signature_is_valid(call),
            RuntimeCall::TokenManager(..) =>
                return pallet_token_manager::Pallet::<TestRuntime>::signature_is_valid(call),
            _ => false,
        }
    }
}

// ==============================================================================================

pub struct ExtBuilder {
    storage: sp_runtime::Storage,
}

impl ExtBuilder {
    pub fn build_default() -> Self {
        let mut storage = system::GenesisConfig::<TestRuntime>::default().build_storage().unwrap();
        let _ = pallet_token_manager::GenesisConfig::<TestRuntime> {
            _phantom: Default::default(),
            avt_token_contract: AVT_TOKEN_CONTRACT,
            lower_account_id: H256::random(),
            lower_schedule_period: 10,
            balances: vec![(
                NON_AVT_TOKEN_CONTRACT,
                get_default_signer_account_id(),
                100 * ONE_AVT,
            )],
        }
        .assimilate_storage(&mut storage);
        Self { storage }
    }

    pub fn as_externality(self) -> sp_io::TestExternalities {
        let mut ext = sp_io::TestExternalities::from(self.storage);
        ext.register_extension(KeystoreExt(Arc::new(MemoryKeystore::new())));
        // Events do not get emitted on block 0, so we increment the block here
        ext.execute_with(|| System::set_block_number(1));
        ext
    }

    pub fn with_balances(mut self) -> Self {
        let context: ProxyContext = Default::default();

        let _ = pallet_balances::GenesisConfig::<TestRuntime> {
            balances: vec![
                (context.signer.account_id(), HUNDRED_AVT),
                (context.relayer.account_id(), HUNDRED_AVT),
            ],
        }
        .assimilate_storage(&mut self.storage);
        self
    }
}

// ============================= Signature handling ========================

// TODO: Refactor this struct to be reused in all tests
#[derive(Clone, Copy)]
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

    pub fn public_key(&self) -> sr25519::Public {
        return self.key_pair().public()
    }

    pub fn key_pair(&self) -> sr25519::Pair {
        return sr25519::Pair::from_seed(&self.seed)
    }

    pub fn sign(&self, message_to_sign: &[u8]) -> Signature {
        Signature::from(self.key_pair().sign(message_to_sign))
    }

    pub fn ethereum_ecdsa_sign(&self, message_to_sign: &[u8]) -> MultiSignature {
        let secret_key = SecretKey::parse_slice(&self.seed).expect("Invalid private key");
        let message =
            hash_string_data_with_ethereum_prefix(message_to_sign).expect("Invalid message");
        let message = Message::parse(&message);
        let (sig, rec_id) = sign(&message, &secret_key);
        let mut sig_bytes = sig.serialize().to_vec();
        sig_bytes.push(rec_id.serialize() + 27);
        let sig = sp_core::ecdsa::Signature::from_slice(&sig_bytes).expect("Invalid signature");
        sp_runtime::MultiSignature::Ecdsa(sig)
    }

    pub fn derived_account_id(&self) -> AccountId {
        let secret_key = SecretKey::parse_slice(&self.seed).expect("Invalid private key");
        let public_key = PublicKey::from_secret_key(&secret_key);
        let public_key_uncompressed = public_key.serialize();
        let eth_address = H160::from_slice(&keccak_256(&public_key_uncompressed[1..])[12..]);
        let hashed_eth_address = keccak_256(&eth_address[..]);
        let derived_account_public_key = sr25519::Public::from_raw(hashed_eth_address);
        AccountId::decode(&mut derived_account_public_key.encode().as_slice()).unwrap()
    }
}

#[allow(dead_code)]
pub fn verify_signature(signature: Signature, signer: AccountId, signed_data: &[u8]) -> bool {
    return signature.verify(signed_data, &signer)
}

pub fn get_default_signer() -> TestAccount {
    let hex_str = "aa1488619fd87c3ee824d4ae4529ba38acc5227c7a66f414236a7fdfdaccf5d9";
    let bytes = hex::decode(hex_str).expect("Decoding failed");

    // Ensure it's a [u8; 32]
    let seed: [u8; 32] = bytes.try_into().expect("Incorrect length");
    TestAccount::new(seed)
}

pub fn get_default_signer_account_id() -> AccountId {
    get_default_signer().account_id()
}

#[derive(Clone)]
pub struct ProxyContext {
    pub signer: TestAccount,
    pub relayer: TestAccount,
    pub signature: Signature,
}

impl Default for ProxyContext {
    fn default() -> Self {
        let message = &("").encode();
        let signer = TestAccount::new([1u8; 32]);
        ProxyContext {
            signer,
            relayer: TestAccount::new([10u8; 32]),
            signature: signer.sign(message),
        }
    }
}

impl ProxyContext {
    pub fn get_proof(&self) -> Proof<Signature, AccountId> {
        return Proof {
            signer: self.signer.account_id(),
            relayer: self.relayer.account_id(),
            signature: self.signature.clone(),
        }
    }

    pub fn create_valid_inner_call(&self) -> Box<<TestRuntime as Config>::RuntimeCall> {
        return Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![] }))
    }

    pub fn create_invalid_inner_call(&self) -> Box<<TestRuntime as Config>::RuntimeCall> {
        let invalid_receiver = TestAccount::new([8u8; 32]);
        return Box::new(RuntimeCall::Balances(BalancesCall::transfer_allow_death {
            dest: invalid_receiver.account_id(),
            value: Default::default(),
        }))
    }

    pub fn create_proxy_call(&self) -> Box<<TestRuntime as Config>::RuntimeCall> {
        return Box::new(RuntimeCall::AvnProxy(AvnProxyCall::proxy {
            call: self.create_valid_inner_call(),
            payment_info: None,
        }))
    }
}

pub fn proxy_event_emitted(
    relayer: AccountId,
    call_hash: <TestRuntime as system::Config>::Hash,
) -> bool {
    return System::events().iter().any(|a| {
        a.event ==
            RuntimeEvent::AvnProxy(crate::Event::<TestRuntime>::CallDispatched {
                relayer: relayer.clone(),
                hash: call_hash,
            })
    })
}

pub fn inner_call_failed_event_emitted(
    call_relayer: AccountId,
    call_hash: <TestRuntime as system::Config>::Hash,
) -> bool {
    return System::events().iter().any(|a| match &a.event {
        RuntimeEvent::AvnProxy(crate::Event::<TestRuntime>::InnerCallFailed {
            relayer,
            hash,
            ..
        }) =>
            if relayer == &call_relayer && call_hash == *hash {
                return true
            } else {
                return false
            },
        _ => false,
    })
}

#[derive(Clone)]
pub struct SingleNftContext {
    pub unique_external_ref: Vec<u8>,
    pub royalties: Vec<Royalty>,
    pub t1_authority: H160,
}

impl Default for SingleNftContext {
    fn default() -> Self {
        let t1_authority = H160(hex!("0000000000000000000000000000000000000001"));
        let royalties: Vec<Royalty> = vec![];
        let unique_external_ref = String::from("Offchain location of NFT").into_bytes();
        SingleNftContext { unique_external_ref, royalties, t1_authority }
    }
}

pub const SIGNED_MINT_SINGLE_NFT_CONTEXT: &'static [u8] =
    b"authorization for mint single nft operation";

pub fn create_signed_mint_single_nft_call(
    context: &ProxyContext,
) -> Box<<TestRuntime as Config>::RuntimeCall> {
    let single_nft_data: SingleNftContext = Default::default();
    let proof = get_mint_single_nft_proxy_proof(context, &single_nft_data);

    return get_signed_mint_single_nft_call(&single_nft_data, &proof)
}

pub fn create_signed_mint_single_nft_call_ecdsa(
    context: &ProxyContext,
) -> Box<<TestRuntime as Config>::RuntimeCall> {
    let single_nft_data: SingleNftContext = Default::default();
    let proof = get_mint_single_nft_proxy_proof_ecdsa(context, &single_nft_data);

    return get_signed_mint_single_nft_call(&single_nft_data, &proof)
}

pub fn get_signed_mint_single_nft_call(
    single_nft_data: &SingleNftContext,
    proof: &Proof<Signature, AccountId>,
) -> Box<<TestRuntime as Config>::RuntimeCall> {
    return Box::new(crate::mock::RuntimeCall::NftManager(NftManagerCall::signed_mint_single_nft {
        proof: proof.clone(),
        unique_external_ref: single_nft_data.unique_external_ref.clone(),
        royalties: single_nft_data.royalties.clone(),
        t1_authority: single_nft_data.t1_authority,
    }))
}

pub fn get_mint_single_nft_proxy_proof(
    context: &ProxyContext,
    data: &SingleNftContext,
) -> Proof<Signature, AccountId> {
    let data_to_sign = (
        SIGNED_MINT_SINGLE_NFT_CONTEXT,
        context.relayer.account_id(),
        &data.unique_external_ref,
        &data.royalties,
        data.t1_authority,
    );

    let signature = context.signer.sign(&data_to_sign.encode());

    let proof = Proof::<Signature, AccountId> {
        signer: context.signer.account_id(),
        relayer: context.relayer.account_id(),
        signature,
    };

    return proof
}

pub fn get_mint_single_nft_proxy_proof_ecdsa(
    context: &ProxyContext,
    data: &SingleNftContext,
) -> Proof<Signature, AccountId> {
    let data_to_sign = (
        SIGNED_MINT_SINGLE_NFT_CONTEXT,
        context.relayer.account_id(),
        &data.unique_external_ref,
        &data.royalties,
        data.t1_authority,
    );

    let signature = context.signer.ethereum_ecdsa_sign(&data_to_sign.encode());

    let proof = Proof::<Signature, AccountId> {
        signer: context.signer.derived_account_id(),
        relayer: context.relayer.account_id(),
        signature,
    };

    return proof
}

pub fn single_nft_minted_events_emitted() -> bool {
    return single_nft_minted_events_count() > 0
}

pub fn single_nft_minted_events_count() -> usize {
    System::events()
        .into_iter()
        .map(|r| r.event)
        .filter_map(|e| if let RuntimeEvent::NftManager(inner) = e { Some(inner) } else { None })
        .count()
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
