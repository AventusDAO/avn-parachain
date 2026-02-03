use crate::*;
use sp_runtime::Saturating;
use sp_runtime::traits::{UniqueSaturatedInto, SaturatedConversion};

#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
/// The current era index and transition information
pub struct RewardPeriodInfo<BlockNumber> {
    /// Current era index
    pub current: RewardPeriodIndex,
    /// The first block of the current era
    pub first: BlockNumber,
    /// The length of the current era in number of blocks
    pub length: u32,
    /// The minimum number of uptime reports required to earn full reward
    pub uptime_threshold: u32,
}

impl<
        B: Copy
            + sp_std::ops::Add<Output = B>
            + sp_std::ops::Sub<Output = B>
            + From<u32>
            + PartialOrd
            + Saturating,
    > RewardPeriodInfo<B>
{
    pub fn new(
        current: RewardPeriodIndex,
        first: B,
        length: u32,
        uptime_threshold: u32,
    ) -> RewardPeriodInfo<B> {
        RewardPeriodInfo { current, first, length, uptime_threshold }
    }

    /// Check if the reward period should be updated
    pub fn should_update(&self, now: B) -> bool {
        now.saturating_sub(self.first) >= self.length.into()
    }

    /// New reward period
    pub fn update(&self, now: B, uptime_threshold: u32) -> Self {
        let current = self.current.saturating_add(1u64);
        let first = now;
        Self { current, first, length: self.length, uptime_threshold }
    }
}

impl<
        B: Copy
            + sp_std::ops::Add<Output = B>
            + sp_std::ops::Sub<Output = B>
            + From<u32>
            + PartialOrd
            + Saturating,
    > Default for RewardPeriodInfo<B>
{
    fn default() -> RewardPeriodInfo<B> {
        RewardPeriodInfo::new(0u64, 0u32.into(), 20u32, u32::MAX)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct RewardPotInfo<Balance> {
    /// The total reward to pay out
    pub total_reward: Balance,
    /// The minimum number of uptime reports required to earn full reward
    pub uptime_threshold: u32,
}

impl<Balance: Copy> RewardPotInfo<Balance> {
    pub fn new(total_reward: Balance, uptime_threshold: u32) -> RewardPotInfo<Balance> {
        RewardPotInfo { total_reward, uptime_threshold }
    }
}

#[derive(
    Copy, Clone, PartialEq, Default, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen,
)]
pub struct UptimeInfo<BlockNumber> {
    /// Number of uptime reported
    pub count: u64,
    /// Block number when the uptime was last reported
    pub last_reported: BlockNumber,
}

impl<BlockNumber: Copy> UptimeInfo<BlockNumber> {
    pub fn new(count: u64, last_reported: BlockNumber) -> UptimeInfo<BlockNumber> {
        UptimeInfo { count, last_reported }
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct PaymentPointer<AccountId> {
    pub period_index: RewardPeriodIndex,
    pub node: AccountId,
}

impl<AccountId: Clone + FullCodec + MaxEncodedLen + TypeInfo> PaymentPointer<AccountId> {
    /// Return the *final* storage key for NodeUptime<(period, node)>.
    /// This positions iteration beyond (period,node), preventing double payments.
    pub fn get_final_key<T: Config<AccountId = AccountId>>(&self) -> Vec<u8> {
        crate::pallet::NodeUptime::<T>::storage_double_map_final_key(
            self.period_index,
            self.node.clone(),
        )
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct NodeInfo<SignerId, AccountId> {
    /// The node owner
    pub owner: AccountId,
    /// The node signing key
    pub signing_key: SignerId,
    /// serial number of the node
    pub serial_number: u32,
    /// Expiry block number for auto stake
    pub auto_stake_expiry: u64,
}

impl<
        AccountId: Clone + FullCodec + MaxEncodedLen + TypeInfo,
        SignerId: Clone + FullCodec + MaxEncodedLen + TypeInfo,
    > NodeInfo<SignerId, AccountId>
{
    pub fn new(
        owner: AccountId,
        signing_key: SignerId,
        serial_number: u32,
        auto_stake_expiry: u64,
    ) -> NodeInfo<SignerId, AccountId> {
        NodeInfo { owner, signing_key, serial_number, auto_stake_expiry }
    }
}

#[derive(Encode, Decode, TypeInfo, Debug, Clone, PartialEq)]
pub enum AdminConfig<AccountId, Balance> {
    NodeRegistrar(AccountId),
    RewardPeriod(u32),
    BatchSize(u32),
    Heartbeat(u32),
    RewardAmount(Balance),
    RewardToggle(bool),
    MinUptimeThreshold(Perbill),
    AutoStakeDuration(u64),
}

#[derive(Clone, Copy)]
pub struct Multiplier {
    pub integer: u128,
    pub fraction: Perbill,
}

impl Multiplier {
    pub fn apply_to_balance<T: Config>(&self, amount: BalanceOf<T>) -> BalanceOf<T> {
        let a: u128 = amount.unique_saturated_into();
        let base = a.saturating_mul(self.integer);
        let frac = self.fraction * a;
        (base.saturating_add(frac)).saturated_into()
    }

    pub fn apply_to_u128(&self, amount: u128) -> u128 {
        amount.saturating_mul(self.integer).saturating_add(self.fraction * amount)
    }
}