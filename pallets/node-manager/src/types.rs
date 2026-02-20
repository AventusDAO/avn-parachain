use crate::*;
use sp_runtime::{
    traits::{AtLeast32BitUnsigned, Zero},
    ArithmeticError, FixedPointNumber, FixedU128, Saturating,
};
use sp_std::fmt::Debug;
// This is used to scale a single heartbeat so we can preserve precision when applying the reward
// weight.
pub const HEARTBEAT_BASE_WEIGHT: u128 = 100_000_000;
pub type Duration = u64;

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
    pub reward_end_time: Duration,
}

impl<Balance: Copy> RewardPotInfo<Balance> {
    pub fn new(
        total_reward: Balance,
        uptime_threshold: u32,
        reward_end_time: Duration,
    ) -> RewardPotInfo<Balance> {
        RewardPotInfo { total_reward, uptime_threshold, reward_end_time }
    }
}

#[derive(
    Copy,
    Clone,
    PartialEq,
    Default,
    Eq,
    Encode,
    Decode,
    RuntimeDebug,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
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

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    MaxEncodedLen,
)]
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

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    MaxEncodedLen,
)]
pub struct NodeInfo<SignerId, AccountId, Balance> {
    /// The node owner
    pub owner: AccountId,
    /// The node signing key
    pub signing_key: SignerId,
    /// serial number of the node
    pub serial_number: u32,
    /// Expiry block number for auto stake
    pub auto_stake_expiry: Duration,
    /// The stake information for this node
    pub stake: StakeInfo<Balance>,
}

impl<
        AccountId: Clone + FullCodec + MaxEncodedLen + TypeInfo,
        SignerId: Clone + FullCodec + MaxEncodedLen + TypeInfo,
        Balance: Clone + FullCodec + MaxEncodedLen + TypeInfo,
    > NodeInfo<SignerId, AccountId, Balance>
{
    pub fn new(
        owner: AccountId,
        signing_key: SignerId,
        serial_number: u32,
        auto_stake_expiry: Duration,
        stake: StakeInfo<Balance>,
    ) -> NodeInfo<SignerId, AccountId, Balance> {
        NodeInfo { owner, signing_key, serial_number, auto_stake_expiry, stake }
    }

    pub fn can_unstake(&self, now_sec: Duration) -> bool {
        now_sec >= self.auto_stake_expiry
    }
}

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    RuntimeDebug,
    TypeInfo,
    MaxEncodedLen,
    Default,
)]
pub struct StakeInfo<Balance> {
    /// The amount staked
    pub amount: Balance,
    /// Allowance carried over (how much they can withdraw right now).
    pub unlocked_stake: Balance,
    /// The timestamp (seconds) that represents the next unstaking period.
    pub next_unstake_time_sec: Option<Duration>,
    /// The max amount that can be unstaked in a period.
    pub max_unstake_per_period: Option<Balance>,
    /// The timestamp where all staking restrictions are lifted and user can unstake all without
    /// limit.
    pub staking_restriction_expiry_sec: Option<Duration>,
}

impl<Balance: Copy + AtLeast32BitUnsigned + Zero + Saturating + Debug> StakeInfo<Balance> {
    pub fn new(
        amount: Balance,
        unlocked_stake: Balance,
        next_unstake_time_sec: Option<Duration>,
        max_unstake_per_period: Option<Balance>,
        staking_restriction_expiry_sec: Option<Duration>,
    ) -> StakeInfo<Balance> {
        StakeInfo {
            amount,
            unlocked_stake,
            next_unstake_time_sec,
            max_unstake_per_period,
            staking_restriction_expiry_sec,
        }
    }

    pub fn available_to_unstake(
        &self,
        now_sec: Duration,
        auto_stake_expiry: Duration,
        unstake_period: Duration,
    ) -> Result<(Balance, Option<Duration>), DispatchError> {
        if self.amount.is_zero() || now_sec < auto_stake_expiry || unstake_period == 0 {
            return Ok((Zero::zero(), self.next_unstake_time_sec))
        }

        // If all restrictions are lifted, user can unstake everything.
        if let Some(restriction_expiry) = self.staking_restriction_expiry_sec {
            if now_sec >= restriction_expiry {
                return Ok((self.amount, None))
            }
        }

        // If this is first time, initialize boundary at expiry
        let mut next_unstake: Duration = self.next_unstake_time_sec.unwrap_or(auto_stake_expiry);

        if now_sec < next_unstake {
            // Not yet time for the next unstake period, so return current allowances.
            return Ok((self.unlocked_stake.min(self.amount), Some(next_unstake)))
        }

        if self.max_unstake_per_period.is_none() {
            // We have gone over next unstake but max_unstake_per_period is not set, which means
            // there was no stake on expiry so allow them to unstake.
            return Ok((self.unlocked_stake.min(self.amount), None))
        }

        let elapsed = now_sec.saturating_sub(next_unstake);
        let periods = 1u64.saturating_add(elapsed / unstake_period);

        // We know max_unstake_per_period is set if we get here but avoid a panic.
        let newly_unlocked_stake = self
            .max_unstake_per_period
            .unwrap_or(Zero::zero())
            .saturating_mul((periods as u32).into());

        let available = self
            .unlocked_stake
            .checked_add(&newly_unlocked_stake)
            .ok_or(ArithmeticError::Overflow)?
            .min(self.amount);
        next_unstake = next_unstake
            .checked_add(periods.saturating_mul(unstake_period))
            .ok_or(ArithmeticError::Overflow)?;

        // Return available to unstake and the next unstake time (so caller can persist).
        Ok((available, Some(next_unstake)))
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
    AutoStakeDuration(Duration),
    MaxUnstakePercentage(Perbill),
    UnstakePeriod(Duration),
    RestrictedUnstakeDuration(Duration),
    AppChainFee(Perbill),
}

#[derive(
    Copy,
    Clone,
    PartialEq,
    Default,
    Eq,
    Encode,
    Decode,
    DecodeWithMemTracking,
    RuntimeDebug,
    TypeInfo,
    MaxEncodedLen,
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
    pub genesis_bonus: FixedU128,
    pub stake_multiplier: FixedU128,
}

impl RewardWeight {
    pub fn to_heartbeat_weight(&self) -> u128 {
        let scaled_stake_weight = self.stake_multiplier.saturating_mul_int(HEARTBEAT_BASE_WEIGHT);
        // apply the bonus last to preserve precision.
        self.genesis_bonus.saturating_mul_int(scaled_stake_weight)
    }
}

pub enum StakeOperation {
    Add,
    Remove,
}
