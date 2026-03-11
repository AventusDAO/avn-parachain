// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
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

#![cfg(all(test, not(feature = "runtime-benchmarks")))]

use crate::{
    migrations::v1::{
        self,
        weights::{self, WeightInfo as _},
    },
    mock::{
        AccountId, AllPalletsWithSystem, BlockNumber, ExtBuilder, MigratorServiceWeight, System,
        TestRuntime as T, Timestamp,
    },
};
use frame_support::{migrations::MultiStepMigrator, traits::OnRuntimeUpgrade};
use pallet_migrations::WeightInfo as _;
use sp_avn_common::{RootId, RootRange};
use sp_runtime::{BoundedVec, WeakBoundedVec};

pub fn run_to_block(n: u64) {
    System::run_to_block_with::<AllPalletsWithSystem>(
        n,
        frame_system::RunToBlockHooks::default()
            .before_finalize(|_| {
                // Satisfy the timestamp pallet.
                Timestamp::set_timestamp(0);
            })
            .after_initialize(|_| {
                // Done by Executive:
                <T as frame_system::Config>::MultiBlockMigrator::step();
            }),
    );
}

#[test]
fn lazy_migration_works() {
    ExtBuilder::build_default().with_validators().as_externality().execute_with(|| {
        frame_support::__private::sp_tracing::try_init_simple();
        // Insert some values into the old storage map.
        for i in 0..1024 {
            let root_id = RootId::<BlockNumber> {
                range: RootRange { from_block: i, to_block: i + 1 },
                ingress_counter: 0,
            };

            let old_data = v1::v0::VotingSessionDataV0 {
                voting_session_id: WeakBoundedVec::force_from(vec![i as u8, 64], None),
                ..Default::default()
            };

            v1::v0::VotesRepository::<T, ()>::insert(root_id, old_data);
        }

        // Give it enough weight do do exactly 16 iterations:
        let limit = <T as pallet_migrations::Config>::WeightInfo::progress_mbms_none() +
            pallet_migrations::Pallet::<T>::exec_migration_max_weight() +
            weights::SubstrateWeight::<T>::step() * 16;
        MigratorServiceWeight::set(&limit);

        System::set_block_number(1);
        AllPalletsWithSystem::on_runtime_upgrade(); // onboard MBMs

        run_to_block(150);

        // Check that everything is decodable now:
        for i in 0..1024 {
            let root_id = RootId::<BlockNumber> {
                range: RootRange { from_block: i, to_block: i + 1 },
                ingress_counter: 0,
            };
            let new_data: crate::VotingSessionData<AccountId, BlockNumber> =
                crate::VotingSessionData {
                    voting_session_id: BoundedVec::truncate_from(vec![i as u8, 64]),
                    ..Default::default()
                };
            assert_eq!(crate::VotesRepository::<T>::get(root_id), new_data);
        }
    });
}
