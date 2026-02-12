use crate::{BalanceOf, Config, Error};
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::{scale_info::TypeInfo, traits::Zero, Perbill, Saturating};
use sp_std::{fmt::Debug, marker::PhantomData};

#[derive(
    Encode, Decode, MaxEncodedLen, Clone, PartialEq, Eq, TypeInfo, Copy, DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub enum FeeType<T: Config> {
    FixedFee(FixedFeeConfig<T>),
    PercentageFee(PercentageFeeConfig<T>),
    None,
}

#[derive(
    Encode, Decode, MaxEncodedLen, Clone, PartialEq, Eq, TypeInfo, Copy, DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub enum AdjustmentType<T: Config> {
    TimeBased(Duration<T>),
    TransactionBased(NumberOfTransactions<T>),
    None,
}

#[derive(Encode, Decode, MaxEncodedLen, Clone, PartialEq, Eq, TypeInfo, DecodeWithMemTracking)]
#[scale_info(skip_type_params(T))]
pub enum FeeAdjustmentConfig<T: Config> {
    FixedFee(FixedFeeConfig<T>),
    PercentageFee(PercentageFeeConfig<T>),
    TimeBased(TimeBasedConfig<T>),
    TransactionBased(TransactionBasedConfig<T>),
    None,
}

impl<T: Config> Debug for FeeType<T> {
    fn fmt(&self, f: &mut sp_std::fmt::Formatter<'_>) -> sp_std::fmt::Result {
        match self {
            Self::FixedFee(c) => {
                write!(f, "Fixed fee[{:?}]", c.fee)
            },
            Self::PercentageFee(c) => {
                write!(f, "Percentage fee[{}]", c.percentage)
            },
            Self::None => {
                write!(f, "Unknwon fee type")
            },
        }
    }
}

impl<T: Config> Debug for AdjustmentType<T> {
    fn fmt(&self, f: &mut sp_std::fmt::Formatter<'_>) -> sp_std::fmt::Result {
        match self {
            Self::TimeBased(c) => {
                write!(f, "Time based fee[{:?}", c.duration)
            },
            Self::TransactionBased(c) => {
                write!(f, "Transaction based fee[{:?}", c.number_of_transactions)
            },
            Self::None => {
                write!(f, "Unknwon adjustment type")
            },
        }
    }
}

impl<T: Config> Debug for FeeAdjustmentConfig<T> {
    fn fmt(&self, f: &mut sp_std::fmt::Formatter<'_>) -> sp_std::fmt::Result {
        match self {
            Self::FixedFee(c) => {
                write!(f, "Fixed fee[{:?}]", c.fee)
            },
            Self::PercentageFee(c) => {
                write!(f, "Percentage fee[{}]", c.percentage)
            },
            Self::TimeBased(c) => {
                write!(f, "Time based fee[{:?}, {:?}]", c.end_block_number, c.fee_type)
            },
            Self::TransactionBased(c) => {
                write!(
                    f,
                    "Transaction based fee[{:?}, {:?}, {:?}]",
                    c.account, c.end_count, c.fee_type
                )
            },
            Self::None => {
                write!(f, "Fee config unknown")
            },
        }
    }
}

impl<T: Config> Default for FeeType<T> {
    fn default() -> Self {
        FeeType::None
    }
}

impl<T: Config> Default for AdjustmentType<T> {
    fn default() -> Self {
        AdjustmentType::None
    }
}

impl<T: Config> Default for FeeAdjustmentConfig<T> {
    fn default() -> Self {
        FeeAdjustmentConfig::None
    }
}

impl<T: Config> FeeAdjustmentConfig<T> {
    pub fn is_valid(&self) -> bool {
        return match self {
            FeeAdjustmentConfig::FixedFee(c) => c.is_valid(),
            FeeAdjustmentConfig::PercentageFee(c) => c.is_valid(),
            FeeAdjustmentConfig::TimeBased(c) => c.is_valid(),
            FeeAdjustmentConfig::TransactionBased(c) => c.is_valid(),
            FeeAdjustmentConfig::None => false,
        };
    }

    pub fn is_active(&self) -> bool {
        return match self {
            FeeAdjustmentConfig::FixedFee(_) => true,
            FeeAdjustmentConfig::PercentageFee(_) => true,
            FeeAdjustmentConfig::TimeBased(c) => c.is_active(),
            FeeAdjustmentConfig::TransactionBased(c) => c.is_active(),
            FeeAdjustmentConfig::None => false,
        };
    }

    pub fn get_fee(&self, original_fee: BalanceOf<T>) -> Result<BalanceOf<T>, Error<T>> {
        return match self {
            FeeAdjustmentConfig::FixedFee(c) => c.get_fee(original_fee),
            FeeAdjustmentConfig::PercentageFee(c) => c.get_fee(original_fee),
            FeeAdjustmentConfig::TimeBased(c) => c.get_fee(original_fee),
            FeeAdjustmentConfig::TransactionBased(c) => c.get_fee(original_fee),
            FeeAdjustmentConfig::None => Ok(original_fee),
        };
    }
}

// This is needed to have a named parameter when serialising these types in a UI (like PolkadotJS)
#[derive(
    Encode,
    Decode,
    MaxEncodedLen,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    Copy,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct Duration<T: Config> {
    pub duration: BlockNumberFor<T>,
}

#[derive(
    Encode,
    Decode,
    MaxEncodedLen,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    Copy,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct NumberOfTransactions<T: Config> {
    pub number_of_transactions: T::Nonce,
}

#[derive(
    Encode,
    Decode,
    MaxEncodedLen,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    Copy,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct FixedFeeConfig<T: Config> {
    pub fee: BalanceOf<T>,
}

impl<T: Config> FixedFeeConfig<T> {
    pub fn is_valid(&self) -> bool {
        return !self.fee.is_zero();
    }

    pub fn is_active(&self) -> bool {
        return true;
    }

    pub fn get_fee(&self, original_fee: BalanceOf<T>) -> Result<BalanceOf<T>, Error<T>> {
        if self.is_active() {
            return Ok(self.fee);
        }

        // There is no adjutment to make so return the original fee
        return Ok(original_fee);
    }
}

#[derive(
    Encode,
    Decode,
    MaxEncodedLen,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    Copy,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct PercentageFeeConfig<T: Config> {
    pub percentage: u32,
    #[codec(skip)]
    pub _marker: PhantomData<T>,
}

impl<T: Config> PercentageFeeConfig<T> {
    pub fn is_valid(&self) -> bool {
        return !self.percentage.is_zero() && self.percentage <= 100u32;
    }

    pub fn is_active(&self) -> bool {
        return true;
    }

    pub fn get_fee(&self, original_fee: BalanceOf<T>) -> Result<BalanceOf<T>, Error<T>> {
        if self.is_active() {
            if self.percentage == 100u32 {
                // the fee should be 0 due to the 100% adjustment
                return Ok(BalanceOf::<T>::zero());
            } else {
                return Ok(original_fee
                    .saturating_sub(Perbill::from_percent(self.percentage) * original_fee));
            }
        }

        // There is no adjutment to make so return the original fee
        return Ok(original_fee);
    }
}

#[derive(
    Encode,
    Decode,
    MaxEncodedLen,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct TimeBasedConfig<T: Config> {
    pub fee_type: FeeType<T>,
    end_block_number: BlockNumberFor<T>,
}

impl<T: Config> TimeBasedConfig<T> {
    pub fn is_valid(&self) -> bool {
        return !self.end_block_number.is_zero() && self.fee_type != FeeType::None;
    }

    pub fn is_active(&self) -> bool {
        return self.end_block_number > <frame_system::Pallet<T>>::block_number();
    }

    pub fn get_fee(&self, original_fee: BalanceOf<T>) -> Result<BalanceOf<T>, Error<T>> {
        if self.is_active() {
            return calculate_fee::<T>(original_fee, &self.fee_type);
        }

        // There is no adjutment to make so return the original fee
        return Ok(original_fee);
    }

    pub fn new(fee_type: FeeType<T>, duration: BlockNumberFor<T>) -> Self {
        if duration == BlockNumberFor::<T>::zero() {
            // This is not a valid value so set the end block number to 0
            return TimeBasedConfig::<T> { fee_type, end_block_number: BlockNumberFor::<T>::zero() };
        }

        let end_block_number = <frame_system::Pallet<T>>::block_number().saturating_add(duration);
        return TimeBasedConfig::<T> { fee_type, end_block_number };
    }
}

#[derive(
    Encode,
    Decode,
    MaxEncodedLen,
    Default,
    Clone,
    PartialEq,
    Debug,
    Eq,
    TypeInfo,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct TransactionBasedConfig<T: Config> {
    pub fee_type: FeeType<T>,
    account: T::AccountId,
    end_count: T::Nonce,
}

impl<T: Config> TransactionBasedConfig<T> {
    pub fn is_valid(&self) -> bool {
        return !self.end_count.is_zero() && self.fee_type != FeeType::None;
    }

    pub fn is_active(&self) -> bool {
        return self.end_count >= <frame_system::Pallet<T>>::account(&self.account).nonce;
    }

    pub fn get_fee(&self, original_fee: BalanceOf<T>) -> Result<BalanceOf<T>, Error<T>> {
        if self.is_active() {
            return calculate_fee::<T>(original_fee, &self.fee_type);
        }

        // There is no adjustment to make so return the original fee
        return Ok(original_fee);
    }

    pub fn new(fee_type: FeeType<T>, account: T::AccountId, count: T::Nonce) -> Self {
        let end_count = <frame_system::Pallet<T>>::account(&account).nonce.saturating_add(count);
        return TransactionBasedConfig::<T> { fee_type, account, end_count };
    }
}

// This is used to define the user input when specifying a fee adjustment config
#[derive(
    Encode,
    Decode,
    MaxEncodedLen,
    Default,
    Clone,
    PartialEq,
    Eq,
    TypeInfo,
    Copy,
    DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct AdjustmentInput<T: Config> {
    pub fee_type: FeeType<T>,
    pub adjustment_type: AdjustmentType<T>,
}

impl<T: Config> Debug for AdjustmentInput<T> {
    fn fmt(&self, f: &mut sp_std::fmt::Formatter<'_>) -> sp_std::fmt::Result {
        write!(f, "Adjustment type: {:?}, Fee type: {:?}", self.adjustment_type, self.fee_type)
    }
}

fn calculate_fee<T: Config>(
    original_fee: BalanceOf<T>,
    fee_type: &FeeType<T>,
) -> Result<BalanceOf<T>, Error<T>> {
    return match fee_type {
        FeeType::FixedFee(f) => Ok(f.fee),
        FeeType::PercentageFee(p) => {
            return Ok(
                original_fee.saturating_sub(Perbill::from_percent(p.percentage) * original_fee)
            )
        },
        _ => Err(Error::InvalidFeeType),
    };
}
