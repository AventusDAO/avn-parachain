// This file is part of Substrate.

// Copyright (C) 2019-2022 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A set of constant values used in substrate runtime.

/// AVT, the native token, uses 18 decimals of precision.

pub mod currency {
    use node_primitives::Balance;

    pub const PICO_AVT: Balance = 1_000_000;
    pub const NANO_AVT: Balance = 1_000 * PICO_AVT;
    pub const MICRO_AVT: Balance = 1_000 * NANO_AVT;
    pub const MILLI_AVT: Balance = 1_000 * MICRO_AVT;
    pub const AVT: Balance = 1_000 * MILLI_AVT;

    #[cfg(test)]
    mod test_avt_constants {
        use super::*;

        /// Checks that the avt amounts are correct.
        #[test]
        fn avt_amounts() {
            assert_eq!(AVT, 1_000_000_000_000_000_000, "AVT should be 1_000_000_000_000_000_000");
            assert_eq!(MILLI_AVT, 1_000_000_000_000_000, "mAVT should be 1_000_000_000_000_000");
            assert_eq!(MICRO_AVT, 1_000_000_000_000, "μAVT should be 1_000_000_000_000");
            assert_eq!(NANO_AVT, 1_000_000_000, "nAVT should be 1_000_000_000");
            assert_eq!(PICO_AVT, 1_000_000, "pAVT should be 1_000_000");
        }
    }

    // TODO review this values.
    pub const fn deposit(items: u32, bytes: u32) -> Balance {
        items as Balance * AVT + (bytes as Balance) * 100 * MICRO_AVT
    }
}

/// Time.
pub mod time {
    use node_primitives::{BlockNumber, Moment};

    /// Change this to adjust the block time.
    pub const MILLI_SECS_PER_BLOCK: u64 = 12000;
    pub const SECS_PER_BLOCK: Moment = MILLI_SECS_PER_BLOCK / 1000;

    // These time units are defined in number of blocks.
    pub const MINUTES: BlockNumber = 60 / (SECS_PER_BLOCK as BlockNumber);
    pub const HOURS: BlockNumber = MINUTES * 60;
    pub const DAYS: BlockNumber = HOURS * 24;
}
