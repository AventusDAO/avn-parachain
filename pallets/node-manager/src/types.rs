use crate::*;
use sp_runtime::{
    traits::{AtLeast32BitUnsigned, Zero},
    FixedPointNumber, FixedU128, Saturating,
};

// This is used to scale a single heartbeat so we can preserve precision when applying the reward
// weight.
const HEARTBEAT_BASE_WEIGHT: u128 = 1_000_000;

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
    /// The last timestamp of the previous reward period, used to calculate gensis bonus
    pub reward_end_time: u64,
}

impl<Balance: Copy> RewardPotInfo<Balance> {
    pub fn new(
        total_reward: Balance,
        uptime_threshold: u32,
        reward_end_time: u64,
    ) -> RewardPotInfo<Balance> {
        RewardPotInfo { total_reward, uptime_threshold, reward_end_time }
    }
}

#[derive(
    Copy, Clone, PartialEq, Default, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking,
)]
pub struct UptimeInfo<BlockNumber> {
    /// Number of uptime reported
    pub count: u64,
    /// The weight of the node (including genesis bonus and stake multiplier)
    pub weight: u128,
    /// Block number when the uptime was last reported
    pub last_reported: BlockNumber,
}

impl<BlockNumber: Copy> UptimeInfo<BlockNumber> {
    pub fn new(count: u64, weight: u128, last_reported: BlockNumber) -> UptimeInfo<BlockNumber> {
        UptimeInfo { count, weight, last_reported }
    }
}

#[derive(Encode, Decode, DecodeWithMemTracking, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
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

#[derive(Encode, Decode, DecodeWithMemTracking, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
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

#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Debug, Clone, PartialEq)]
pub enum AdminConfig<AccountId, Balance> {
    NodeRegistrar(AccountId),
    RewardPeriod(u32),
    BatchSize(u32),
    Heartbeat(u32),
    RewardAmount(Balance),
    RewardToggle(bool),
    MinUptimeThreshold(Perbill),
    AutoStakeDuration(u64),
    MaxUnstakePercentage(Perbill),
    UnstakePeriod(u64),
}

#[derive(
    Copy, Clone, PartialEq, Default, Eq, Encode, Decode, DecodeWithMemTracking, RuntimeDebug, TypeInfo, MaxEncodedLen,
)]
pub struct TotalUptimeInfo {
    /// Total number of uptime reported for reward period
    // TODO NS: rename _total_heartbeats
    pub _total_heartbeats: u64,
    /// Total weight of the total heartbeats reported for reward period
    pub total_weight: u128,
}

impl TotalUptimeInfo {
    pub fn new(_total_heartbeats: u64, total_weight: u128) -> TotalUptimeInfo {
        TotalUptimeInfo { _total_heartbeats, total_weight }
    }
}

#[derive(Clone, Copy)]
pub struct RewardWeight {
    pub genesis_bonus: Perbill,
    pub stake_multiplier: FixedU128,
}

impl RewardWeight {
    pub fn to_heartbeat_weight(&self) -> u128 {
        let scaled_stake_weight = self.stake_multiplier.saturating_mul_int(HEARTBEAT_BASE_WEIGHT);
        // apply the bonus last to preserve precision.
        self.genesis_bonus * scaled_stake_weight
    }
}

#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, Default)]
pub struct OwnerStakeInfo<Balance> {
    /// The amount staked
    pub amount: Balance,
    /// The last reward period this stake was updated
    pub last_period_updated: RewardPeriodIndex,
    /// The expiry timestamp the owner can start unstaking
    pub auto_stake_expiry: u64,
    /// The unstake state for this owner
    pub state: UnstakeState<Balance>,
}

#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, Default)]
pub struct UnstakeState<Balance> {
    /// Allowance carried over (how much they can withdraw right now).
    pub max_unstake_allowance: Balance,
    /// The timestamp (seconds) that represents the next unstaking period.
    pub next_unstake_time_sec: u64,
}

impl<Balance: Copy + AtLeast32BitUnsigned + Zero> UnstakeState<Balance> {
    pub fn new(
        max_unstake_allowance: Balance,
        next_unstake_time_sec: u64,
    ) -> UnstakeState<Balance> {
        UnstakeState { max_unstake_allowance, next_unstake_time_sec }
    }
}

impl<Balance: Copy + AtLeast32BitUnsigned + Zero + Saturating> OwnerStakeInfo<Balance> {
    pub fn new(
        amount: Balance,
        last_period_updated: RewardPeriodIndex,
        auto_stake_expiry: u64,
        state: UnstakeState<Balance>,
    ) -> OwnerStakeInfo<Balance> {
        OwnerStakeInfo { amount, last_period_updated, auto_stake_expiry, state }
    }

    pub fn can_unstake(&self, now_sec: u64) -> bool {
        now_sec >= self.auto_stake_expiry
    }

    pub fn available_to_unstake(
        &self,
        now_sec: u64,
        unstake_period: u64,
        max_unstake_percentage: Perbill,
    ) -> (Balance, u64) {
        if !self.can_unstake(now_sec) || self.amount.is_zero() {
            return (Zero::zero(), 0)
        }

        // This should not happen because we set this when stake is added
        if self.state.next_unstake_time_sec == 0 {
            return (Zero::zero(), 0)
        }

        let elapsed = now_sec.saturating_sub(self.state.next_unstake_time_sec);
        let periods = elapsed / unstake_period;
        if periods == 0 {
            // No new stake unlocked yet
            return (self.state.max_unstake_allowance.min(self.amount), 0)
        }

        // Increase for whole periods only.
        let per_period = max_unstake_percentage * self.amount;
        let newly_unlocked_stake = per_period.saturating_mul((periods as u32).into());

        let available = self
            .state
            .max_unstake_allowance
            .saturating_add(newly_unlocked_stake)
            .min(self.amount);

        // Return available to unstake and how many periods we advanced (so caller can persist).
        (available, periods)
    }
}
