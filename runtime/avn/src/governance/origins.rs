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

//! Custom origins for governance interventions.

pub use pallet_custom_origins::*;

#[frame_support::pallet]
pub mod pallet_custom_origins {
    use frame_support::pallet_prelude::*;

    #[pallet::config]
    pub trait Config: frame_system::Config {}

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[derive(
        PartialEq,
        Eq,
        Clone,
        MaxEncodedLen,
        Encode,
        Decode,
        TypeInfo,
        RuntimeDebug,
        DecodeWithMemTracking,
    )]
    #[pallet::origin]
    pub enum Origin {
        /// Origin for managing the registrar and permissioned HRMP channel operations.
        GeneralAdmin,
        /// Origin able to cancel referenda.
        ReferendumCanceller,
        /// Origin able to kill referenda.
        ReferendumKiller,
        /// Origin able to dispatch a whitelisted call.
        WhitelistedCaller,
    }

    macro_rules! decl_unit_ensures {
		( $name:ident: $success_type:ty = $success:expr ) => {
			pub struct $name;
			impl<O: OriginTrait + From<Origin>> EnsureOrigin<O> for $name
			where
				for <'a> &'a O::PalletsOrigin: TryInto<&'a Origin>,
			{
				type Success = $success_type;
				fn try_origin(o: O) -> Result<Self::Success, O> {
					match o.caller().try_into() {
						Ok(Origin::$name) => return Ok($success),
						_ => (),
					}

					Err(o)
				}
				#[cfg(feature = "runtime-benchmarks")]
				fn try_successful_origin() -> Result<O, ()> {
					Ok(O::from(Origin::$name))
				}
			}
		};
		( $name:ident ) => { decl_unit_ensures! { $name : () = () } };
		( $name:ident: $success_type:ty = $success:expr, $( $rest:tt )* ) => {
			decl_unit_ensures! { $name: $success_type = $success }
			decl_unit_ensures! { $( $rest )* }
		};
		( $name:ident, $( $rest:tt )* ) => {
			decl_unit_ensures! { $name }
			decl_unit_ensures! { $( $rest )* }
		};
		() => {}
	}
    decl_unit_ensures!(GeneralAdmin, ReferendumCanceller, ReferendumKiller, WhitelistedCaller,);
}
