// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot. If not, see <http://www.gnu.org/licenses/>.

//! Track configurations for governance.

use super::*;
use alloc::borrow::Cow;
use sp_runtime::str_array as s;

const fn percent(x: i32) -> sp_runtime::FixedI64 {
    sp_runtime::FixedI64::from_rational(x as u128, 100)
}
use pallet_referenda::Curve;
const APP_ROOT: Curve = Curve::make_reciprocal(4, 28, percent(80), percent(50), percent(100));
const SUP_ROOT: Curve = Curve::make_linear(28, 28, percent(0), percent(50));
const APP_GENERAL_ADMIN: Curve =
    Curve::make_reciprocal(4, 28, percent(80), percent(50), percent(100));
const SUP_GENERAL_ADMIN: Curve =
    Curve::make_reciprocal(7, 28, percent(10), percent(0), percent(50));
const APP_REFERENDUM_CANCELLER: Curve = Curve::make_linear(17, 28, percent(50), percent(100));
const SUP_REFERENDUM_CANCELLER: Curve =
    Curve::make_reciprocal(12, 28, percent(1), percent(0), percent(50));
const APP_REFERENDUM_KILLER: Curve = Curve::make_linear(17, 28, percent(50), percent(100));
const SUP_REFERENDUM_KILLER: Curve =
    Curve::make_reciprocal(12, 28, percent(1), percent(0), percent(50));
const APP_WHITELISTED_CALLER: Curve =
    Curve::make_reciprocal(16, 28 * 24, percent(96), percent(50), percent(100));
const SUP_WHITELISTED_CALLER: Curve =
    Curve::make_reciprocal(1, 28, percent(20), percent(5), percent(50));

const TRACKS_DATA: [pallet_referenda::Track<u16, Balance, BlockNumber>; 5] = [
    pallet_referenda::Track {
        id: 0,
        info: pallet_referenda::TrackInfo {
            name: s("root"),
            max_deciding: 1,
            decision_deposit: 1000 * AVT,
            prepare_period: 2 * HOURS,
            decision_period: 28 * DAYS,
            confirm_period: 24 * HOURS,
            min_enactment_period: 24 * HOURS,
            min_approval: APP_ROOT,
            min_support: SUP_ROOT,
        },
    },
    pallet_referenda::Track {
        id: 1,
        info: pallet_referenda::TrackInfo {
            name: s("whitelisted_caller"),
            max_deciding: 100,
            decision_deposit: 1000 * AVT,
            prepare_period: 30 * MINUTES,
            decision_period: 28 * DAYS,
            confirm_period: 10 * MINUTES,
            min_enactment_period: 10 * MINUTES,
            min_approval: APP_WHITELISTED_CALLER,
            min_support: SUP_WHITELISTED_CALLER,
        },
    },
    pallet_referenda::Track {
        id: 2,
        info: pallet_referenda::TrackInfo {
            name: s("general_admin"),
            max_deciding: 10,
            decision_deposit: 500 * AVT,
            prepare_period: 2 * HOURS,
            decision_period: 28 * DAYS,
            confirm_period: 3 * HOURS,
            min_enactment_period: 10 * MINUTES,
            min_approval: APP_GENERAL_ADMIN,
            min_support: SUP_GENERAL_ADMIN,
        },
    },
    pallet_referenda::Track {
        id: 3,
        info: pallet_referenda::TrackInfo {
            name: s("referendum_canceller"),
            max_deciding: 1000,
            decision_deposit: 1000 * AVT,
            prepare_period: 2 * HOURS,
            decision_period: 7 * DAYS,
            confirm_period: 3 * HOURS,
            min_enactment_period: 10 * MINUTES,
            min_approval: APP_REFERENDUM_CANCELLER,
            min_support: SUP_REFERENDUM_CANCELLER,
        },
    },
    pallet_referenda::Track {
        id: 4,
        info: pallet_referenda::TrackInfo {
            name: s("referendum_killer"),
            max_deciding: 1000,
            decision_deposit: 2000 * AVT,
            prepare_period: 2 * HOURS,
            decision_period: 28 * DAYS,
            confirm_period: 3 * HOURS,
            min_enactment_period: 10 * MINUTES,
            min_approval: APP_REFERENDUM_KILLER,
            min_support: SUP_REFERENDUM_KILLER,
        },
    },
];

pub struct TracksInfo;
impl pallet_referenda::TracksInfo<Balance, BlockNumber> for TracksInfo {
    type Id = u16;
    type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;
    fn tracks(
    ) -> impl Iterator<Item = Cow<'static, pallet_referenda::Track<Self::Id, Balance, BlockNumber>>>
    {
        TRACKS_DATA.iter().map(Cow::Borrowed)
    }
    fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
        if let Ok(system_origin) = frame_system::RawOrigin::try_from(id.clone()) {
            match system_origin {
                frame_system::RawOrigin::Root => Ok(0),
                _ => Err(()),
            }
        } else if let Ok(custom_origin) = origins::Origin::try_from(id.clone()) {
            match custom_origin {
                origins::Origin::WhitelistedCaller => Ok(1),
                // General admin
                origins::Origin::GeneralAdmin => Ok(2),
                // Referendum admins
                origins::Origin::ReferendumCanceller => Ok(3),
                origins::Origin::ReferendumKiller => Ok(4),
            }
        } else {
            Err(())
        }
    }
}
