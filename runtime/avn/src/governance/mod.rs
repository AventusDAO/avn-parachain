pub use super::*;

pub mod origins;
use frame_support::traits::EitherOf;
pub use origins::{
    pallet_custom_origins, ReferendumCanceller, ReferendumKiller, WhitelistedCaller,
};

pub mod tracks;
use pallet_token_manager;
pub use tracks::TracksInfo;

parameter_types! {
    pub const VoteLockingPeriod: BlockNumber = 28 * DAYS;
}

impl pallet_conviction_voting::Config for Runtime {
    type WeightInfo = pallet_conviction_voting::weights::SubstrateWeight<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type VoteLockingPeriod = VoteLockingPeriod;
    type MaxVotes = ConstU32<512>;
    type MaxTurnout =
        frame_support::traits::tokens::currency::ActiveIssuanceOf<Balances, Self::AccountId>;
    type Polls = Referenda;
    type BlockNumberProvider = System;
    type VotingHooks = ();
}

parameter_types! {
    pub const AlarmInterval: BlockNumber = 1;
    pub const SubmissionDeposit: Balance = 50 * AVT;
    pub const UndecidingTimeout: BlockNumber = 14 * DAYS;
}

impl pallet_custom_origins::Config for Runtime {}

pub struct ToTreasury<R>(sp_std::marker::PhantomData<R>);
impl<R> OnUnbalanced<NegativeImbalance<R>> for ToTreasury<R>
where
    R: pallet_balances::Config + pallet_token_manager::Config,
    <R as frame_system::Config>::AccountId: From<AccountId>,
    <R as frame_system::Config>::AccountId: Into<AccountId>,
    <R as frame_system::Config>::RuntimeEvent: From<pallet_balances::Event<R>>,
{
    fn on_nonzero_unbalanced(amount: NegativeImbalance<R>) {
        let treasury_address = <pallet_token_manager::Pallet<R>>::compute_treasury_account_id();
        <pallet_balances::Pallet<R>>::resolve_creating(&treasury_address, amount);
    }
}

impl pallet_whitelist::Config for Runtime {
    type WeightInfo = pallet_whitelist::weights::SubstrateWeight<Runtime>;
    type RuntimeCall = RuntimeCall;
    type RuntimeEvent = RuntimeEvent;
    type WhitelistOrigin = EnsureRoot<Self::AccountId>;
    type DispatchWhitelistedOrigin = EitherOf<EnsureRoot<Self::AccountId>, WhitelistedCaller>;
    type Preimages = Preimage;
}

impl pallet_referenda::Config for Runtime {
    type WeightInfo = pallet_referenda::weights::SubstrateWeight<Runtime>;
    type RuntimeCall = RuntimeCall;
    type RuntimeEvent = RuntimeEvent;
    type Scheduler = Scheduler;
    type Currency = Balances;
    type SubmitOrigin = frame_system::EnsureSigned<AccountId>;
    type CancelOrigin = EitherOf<EnsureRoot<AccountId>, ReferendumCanceller>;
    type KillOrigin = EitherOf<EnsureRoot<AccountId>, ReferendumKiller>;
    type Slash = ToTreasury<Runtime>;
    type Votes = pallet_conviction_voting::VotesOf<Runtime>;
    type Tally = pallet_conviction_voting::TallyOf<Runtime>;
    type SubmissionDeposit = SubmissionDeposit;
    type MaxQueued = ConstU32<100>;
    type UndecidingTimeout = UndecidingTimeout;
    type AlarmInterval = AlarmInterval;
    type Tracks = TracksInfo;
    type Preimages = Preimage;
    type BlockNumberProvider = System;
}
