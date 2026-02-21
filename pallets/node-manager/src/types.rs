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
    pub fn new(current: RewardPeriodIndex, first: B, length: u32, uptime_threshold: u32) -> Self {
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
    pub fn new(total_reward: Balance, uptime_threshold: u32, reward_end_time: Duration) -> Self {
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
    pub fn new(count: u64, weight: u128, last_reported: BlockNumber) -> Self {
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
    Copy,
    Clone,
    PartialEq,
    Eq,
    RuntimeDebug,
    TypeInfo,
    MaxEncodedLen,
    Default,
)]
pub enum UnstakeRestriction<Balance> {
    /// Default state. Unstaking is not permitted.
    #[default]
    Locked,
    /// There are no restrictions on unstaking
    Free,
    /// A periodic unlock allowance applies until `expires_sec`, after which the node is
    /// treated identically to `Free`.
    Periodic {
        /// Amount unlocked per `unstake_period` (snapshot_amount x `MaxUnstakePercentage`).
        per_period_allowance: Balance,
        /// Timestamp at which all restrictions are fully lifted.
        expires_sec: Duration,
    },
}

impl<Balance: Copy> UnstakeRestriction<Balance> {
    pub fn per_period_allowance(&self) -> Option<Balance> {
        match self {
            UnstakeRestriction::Periodic { per_period_allowance, .. } =>
                Some(*per_period_allowance),
            _ => None,
        }
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
        Balance: Clone + FullCodec + MaxEncodedLen + TypeInfo + Zero + AtLeast32BitUnsigned + Debug + Copy,
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

    pub fn try_snapshot_stake(
        &mut self,
        now_sec: Duration,
        max_pct: Perbill,
        restriction_duration: Duration,
    ) {
        // Already resolved — nothing to do.
        if !matches!(self.stake.restriction, UnstakeRestriction::Locked) {
            return
        }
        // Expiry not yet reached — stay Locked.
        if now_sec < self.auto_stake_expiry {
            return
        }

        self.stake.restriction = if self.stake.amount.is_zero() {
            // No stake was present at expiry. User is free to operate without restriction.
            UnstakeRestriction::Free
        } else {
            // Snapshot the stake present at expiry and set up periodic unlock.
            UnstakeRestriction::Periodic {
                per_period_allowance: max_pct * self.stake.amount,
                expires_sec: self.auto_stake_expiry.saturating_add(restriction_duration),
            }
        };
    }

    pub fn available_to_unstake(
        &self,
        now_sec: Duration,
        unstake_period: Duration,
    ) -> Result<(Balance, Option<Duration>), DispatchError> {
        if self.stake.amount.is_zero() || unstake_period == 0 {
            return Ok((Zero::zero(), self.stake.next_unstake_time_sec))
        }

        match &self.stake.restriction {
            UnstakeRestriction::Locked => Ok((Zero::zero(), None)),
            UnstakeRestriction::Free => Ok((self.stake.amount, None)),
            UnstakeRestriction::Periodic { per_period_allowance, expires_sec } => {
                // All restrictions lifted — treat as Free.
                if now_sec >= *expires_sec {
                    return Ok((self.stake.amount, None))
                }

                // Determine the boundary of the current unstake period.
                let next_unstake =
                    self.stake.next_unstake_time_sec.unwrap_or(self.auto_stake_expiry);

                // Still within the current period return already free allowance only.
                if now_sec < next_unstake {
                    return Ok((
                        self.stake.unlocked_stake.min(self.stake.amount),
                        Some(next_unstake),
                    ))
                }

                let elapsed = now_sec.saturating_sub(next_unstake);
                let periods = 1u64.saturating_add(elapsed / unstake_period);
                let newly_unlocked = per_period_allowance.saturating_mul((periods as u32).into());
                let available = self
                    .stake
                    .unlocked_stake
                    .checked_add(&newly_unlocked)
                    .ok_or(ArithmeticError::Overflow)?
                    .min(self.stake.amount);

                let next = next_unstake
                    .checked_add(periods.saturating_mul(unstake_period))
                    .ok_or(ArithmeticError::Overflow)?;

                Ok((available, Some(next)))
            },
        }
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
    /// Unstake restriction state.
    pub restriction: UnstakeRestriction<Balance>,
}

impl<Balance: Copy + Debug> StakeInfo<Balance> {
    pub fn new(
        amount: Balance,
        unlocked_stake: Balance,
        next_unstake_time_sec: Option<Duration>,
        restriction: UnstakeRestriction<Balance>,
    ) -> Self {
        StakeInfo { amount, unlocked_stake, next_unstake_time_sec, restriction }
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
    pub total_heartbeats: u64,
    /// Total weight of the total heartbeats reported for reward period
    pub total_weight: u128,
}

impl TotalUptimeInfo {
    pub fn new(total_heartbeats: u64, total_weight: u128) -> TotalUptimeInfo {
        TotalUptimeInfo { total_heartbeats, total_weight }
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
