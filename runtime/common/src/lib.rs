// Copyright 2026 Aventus DAO Ltd
#![cfg_attr(not(feature = "std"), no_std)]

pub mod weights;

use smallvec::smallvec;
use sp_runtime::{generic, traits::BlakeTwo256};

use frame_support::{
    parameter_types,
    weights::{WeightToFeeCoefficient, WeightToFeeCoefficients, WeightToFeePolynomial},
};
pub use sp_runtime::{MultiAddress, Perbill, Permill};

pub use node_primitives::{AccountId, Signature};
use node_primitives::{Balance, BlockNumber};

pub mod constants;
use constants::currency::*;
use weights::ExtrinsicBaseWeight;

/// The address format for describing accounts.
pub type Address = MultiAddress<AccountId, ()>;

/// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;

/// Opaque types. These are used by the CLI to instantiate machinery that don't need to know
/// the specifics of the runtime. They can then be made to be agnostic over specific formats
/// of data like extrinsics, allowing for them to continue syncing the network through upgrades
/// to even the core data structures.
pub mod opaque {
    use super::*;
    use sp_runtime::{
        generic,
        traits::{BlakeTwo256, Hash as HashT},
    };

    pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;
    /// Opaque block header type.
    pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
    /// Opaque block type.
    pub type Block = generic::Block<Header, UncheckedExtrinsic>;
    /// Opaque block identifier type.
    pub type BlockId = generic::BlockId<Block>;
    /// Opaque block hash type.
    pub type Hash = <BlakeTwo256 as HashT>::Output;
}

/// Handles converting a weight scalar to a fee value, based on the scale and granularity of the
/// node's balance type.
///
/// This should typically create a mapping between the following ranges:
///   - `[0, MAXIMUM_BLOCK_WEIGHT]`
///   - `[Balance::min, Balance::max]`
///
/// Yet, it can be used for any other sort of change to weight-fee. Some examples being:
///   - Setting it to `0` will essentially disable the weight fee.
///   - Setting it to `1` will cause the literal `#[weight = x]` values to be charged.
pub struct WeightToFee;
impl WeightToFeePolynomial for WeightToFee {
    type Balance = Balance;
    fn polynomial() -> WeightToFeeCoefficients<Self::Balance> {
        // We adjust the fee conversion so that the Extrinsic Base Weight corresponds to a 1 mAVT
        // fee.
        let p = 1 * MILLI_AVT;
        let q = Balance::from(ExtrinsicBaseWeight::get().ref_time());
        smallvec![WeightToFeeCoefficient {
            degree: 1,
            negative: false,
            coeff_frac: Perbill::from_rational(p % q, q),
            coeff_integer: p / q,
        }]
    }
}

parameter_types! {
    // This value was adjusted so that the length fee of an extrinsic is roughly in line with the weight fees
    // An extrinsic usually has a payload with a few hundred bytes, and its weight fee should be of a few mAVT.
    // In consequence TransactionByteFee should be set at a few MICRO_AVT.
    // The actual value here was chosen to be a round number so that a Token Transfer be around 4mAVT, and an AVT transfer be around 2 mAVT.
    pub const TransactionByteFee: Balance = 5 * MICRO_AVT;
    pub const OperationalFeeMultiplier: u8 = 5;
}
