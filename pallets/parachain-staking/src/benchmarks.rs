// Copyright 2019-2022 PureStake Inc.
// This file is part of Moonbeam.

// Moonbeam is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonbeam is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonbeam.  If not, see <http://www.gnu.org/licenses/>.

#![cfg(feature = "runtime-benchmarks")]

//! Benchmarking
use super::*;
use crate::{
    encode_signed_bond_extra_params, encode_signed_candidate_bond_extra_params,
    encode_signed_execute_candidate_unbond_params, encode_signed_execute_leave_nominators_params,
    encode_signed_execute_nomination_request_params, encode_signed_nominate_params,
    encode_signed_schedule_candidate_unbond_params, encode_signed_schedule_leave_nominators_params,
    encode_signed_schedule_nominator_unbond_params,
    encode_signed_schedule_revoke_nomination_params, AdminSettings, AwardedPts, BalanceOf, Call,
    CandidateBondLessRequest, Config, Delay, Era, MinCollatorStake, MinTotalNominatorStake,
    NominationAction, Pallet, Points, Proof, ScheduledRequest,
};
use codec::{Decode, Encode};
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use sp_runtime::traits::Zero;

use frame_support::traits::{Currency, Get, OnFinalize, OnInitialize};
use frame_system::{self as system, EventRecord, RawOrigin};
use hex_literal::hex;
use pallet_authorship::EventHandler;
use rand::{RngCore, SeedableRng};
use sp_application_crypto::KeyTypeId;
use sp_core::{bounded::BoundedVec, ecdsa, ConstU32};
use sp_runtime::{traits::StaticLookup, RuntimeAppPublic, WeakBoundedVec};
use sp_std::{collections::btree_map::BTreeMap, vec, vec::Vec};

pub const BENCH_KEY_TYPE_ID: KeyTypeId = KeyTypeId(*b"test");

mod app_sr25519 {
    use super::BENCH_KEY_TYPE_ID;
    use sp_application_crypto::{app_crypto, sr25519};
    app_crypto!(sr25519, BENCH_KEY_TYPE_ID);
}

type SignerId = app_sr25519::Public;

/// Minimum collator candidate stake
fn min_candidate_stk<T: Config>() -> BalanceOf<T> {
    <MinCollatorStake<T>>::get()
}

/// Minimum nominator stake
fn min_nominator_stk<T: Config>() -> BalanceOf<T> {
    <MinTotalNominatorStake<T>>::get()
}

fn fund_account<T: Config>(account: &T::AccountId, extra: BalanceOf<T>) -> BalanceOf<T> {
    let min_candidate_stk = min_candidate_stk::<T>();
    let total = min_candidate_stk + extra;
    T::Currency::make_free_balance_be(&account, total);
    T::Currency::issue(total);

    return total
}

/// Create a funded user.
/// Extra + min_candidate_stk is total minted funds
/// Returns tuple (id, balance)
fn create_funded_user<T: Config>(
    string: &'static str,
    n: u32,
    extra: BalanceOf<T>,
) -> (T::AccountId, BalanceOf<T>) {
    const SEED: u32 = 0;
    let user = account(string, n, SEED);
    let total_funded = fund_account::<T>(&user, extra);
    (user, total_funded)
}

fn create_funded_nominator<T: Config>(
    string: &'static str,
    n: u32,
    extra: BalanceOf<T>,
    collator: T::AccountId,
    min_bond: bool,
    collator_nominator_count: u32,
) -> Result<T::AccountId, &'static str> {
    let (user, total) = create_funded_user::<T>(string, n, extra);
    let bond = if min_bond { min_nominator_stk::<T>() } else { total };
    Pallet::<T>::nominate(
        RawOrigin::Signed(user.clone()).into(),
        collator,
        bond,
        collator_nominator_count,
        0u32, // first nomination for all calls
    )?;
    Ok(user)
}

fn create_funded_collator<T: Config>(
    string: &'static str,
    n: u32,
    extra: BalanceOf<T>,
    min_bond: bool,
    candidate_count: u32,
) -> Result<T::AccountId, &'static str> {
    let (user, total) = create_funded_user::<T>(string, n, extra);
    let bond = if min_bond { min_candidate_stk::<T>() } else { total };

    set_session_key::<T>(&user, n)?;
    Pallet::<T>::join_candidates(RawOrigin::Signed(user.clone()).into(), bond, candidate_count)?;

    add_collator_to_avn::<T>(&user)?;

    Ok(user)
}

fn set_account_as_collator<T: Config>(
    account: &T::AccountId,
    additional_bond: BalanceOf<T>,
    candidate_count: u32,
) -> Result<(), &'static str> {
    set_session_key::<T>(account, candidate_count)?;
    let total_bond = additional_bond + min_candidate_stk::<T>();
    Pallet::<T>::join_candidates(
        RawOrigin::Signed(account.clone()).into(),
        total_bond,
        candidate_count,
    )?;

    add_collator_to_avn::<T>(account)?;

    Ok(())
}

fn add_collator_to_avn<T: Config>(
    collator: &T::AccountId,
) -> Result<Validator<T::AuthorityId, T::AccountId>, &'static str> {
    let key = <T as avn::Config>::AuthorityId::generate_pair(None);
    let validator: Validator<T::AuthorityId, T::AccountId> =
        Validator::new(collator.clone(), key.into());

    let current_collators = avn::Validators::<T>::get();
    let new_collators: Vec<_> = current_collators
        .iter()
        .chain(vec![validator.clone()].iter())
        .cloned()
        .collect();

    avn::Validators::<T>::put(WeakBoundedVec::force_from(
        new_collators,
        Some("Too many validators for session"),
    ));

    Ok(validator)
}

fn set_session_key<T: Config>(user: &T::AccountId, index: u32) -> Result<(), &'static str> {
    frame_system::Pallet::<T>::inc_providers(user);

    let keys = {
        let mut keys = [0u8; 128];
        let mut rng = rand::rngs::StdRng::seed_from_u64(index as u64);
        rng.fill_bytes(&mut keys);
        keys
    };

    let keys: T::Keys = Decode::decode(&mut &keys[..]).unwrap();

    pallet_session::Pallet::<T>::set_keys(
        RawOrigin::<T::AccountId>::Signed(user.clone()).into(),
        keys,
        Vec::new(),
    )?;

    Ok(())
}

// Simulate staking on finalize by manually setting points
fn parachain_staking_on_finalize<T: Config>(author: T::AccountId) {
    let now = <Era<T>>::get().current;
    let score_plus_20 = <AwardedPts<T>>::get(now, &author).saturating_add(20);
    <AwardedPts<T>>::insert(now, author, score_plus_20);
    <Points<T>>::mutate(now, |x| *x = x.saturating_add(20));
}

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    assert_last_nth_event::<T>(generic_event, 1);
}

fn assert_last_nth_event<T: Config>(generic_event: <T as Config>::RuntimeEvent, n: u32) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(n as usize)];
    assert_eq!(event, &system_event);
}

/// Run to end block and author
fn roll_to_and_author<T: Config>(era_delay: u32, author: T::AccountId) {
    let total_eras = era_delay + 1u32;
    let era_length: BlockNumberFor<T> = Pallet::<T>::era().length.into();
    let mut now = <frame_system::Pallet<T>>::block_number() + 1u32.into();
    let end = Pallet::<T>::era().first + (era_length * total_eras.into());
    while now < end {
        parachain_staking_on_finalize::<T>(author.clone());
        <frame_system::Pallet<T> as OnFinalize<BlockNumberFor<T>>>::on_finalize(
            <frame_system::Pallet<T>>::block_number(),
        );
        <frame_system::Pallet<T>>::set_block_number(
            <frame_system::Pallet<T>>::block_number() + 1u32.into(),
        );
        <frame_system::Pallet<T> as OnInitialize<BlockNumberFor<T>>>::on_initialize(
            <frame_system::Pallet<T>>::block_number(),
        );
        <pallet::Pallet<T> as OnInitialize<BlockNumberFor<T>>>::on_initialize(
            <frame_system::Pallet<T>>::block_number(),
        );
        now += 1u32.into();
    }
}

fn get_collator_count<T: Config>() -> u32 {
    return Pallet::<T>::selected_candidates().len() as u32
}

fn get_allowed_max_collators<T: Config>(max_collators: u32) -> u32 {
    let mut actual_max_collators: u32 =
        max_collators.min(<<T as Config>::MaxCandidates as Get<u32>>::get() - 1);

    let existing_collators = Pallet::<T>::selected_candidates().len() as u32;
    if actual_max_collators > existing_collators {
        actual_max_collators -= existing_collators;
    }

    return actual_max_collators
}

fn setup_nomination<T: Config>(
    max_collators: u32,
    max_nominators: u32,
    bond: BalanceOf<T>,
    caller: &T::AccountId,
) -> Result<(T::AccountId, Vec<T::AccountId>, Vec<T::AccountId>), &'static str> {
    // Worst Case is full of nominations before calling `nominate`
    let mut collators: Vec<T::AccountId> = Vec::new();
    let initial_collators_count = get_collator_count::<T>();

    // Initialize MaxNominationsPerNominator collator candidates
    for i in 2..get_allowed_max_collators::<T>(max_collators) {
        let seed = USER_SEED - i;
        let collator = create_funded_collator::<T>(
            "collator",
            seed,
            0u32.into(),
            true,
            collators.len() as u32 + initial_collators_count + 1u32,
        )?;
        collators.push(collator.clone());
    }

    let extra = if (bond * (collators.len() as u32 + 1u32).into()) > min_candidate_stk::<T>() {
        (bond * (collators.len() as u32 + 1u32).into()) - min_candidate_stk::<T>()
    } else {
        0u32.into()
    };

    fund_account::<T>(caller, extra.into());

    // Nomination count
    let mut del_del_count = 0u32;
    // Nominate MaxNominationsPerNominators collator candidates
    for col in collators.clone() {
        Pallet::<T>::nominate(
            RawOrigin::Signed(caller.clone()).into(),
            col,
            bond,
            0u32,
            del_del_count,
        )?;
        del_del_count += 1u32;
    }

    // Last collator to be nominated
    let collator: T::AccountId = create_funded_collator::<T>(
        "collator",
        USER_SEED,
        0u32.into(),
        true,
        collators.len() as u32 + initial_collators_count + 1u32,
    )?;

    // Worst Case Complexity is insertion into an almost full collator
    let mut nominators: Vec<T::AccountId> = Vec::new();
    for i in 1..max_nominators {
        let seed = USER_SEED + i;
        let nominator = create_funded_nominator::<T>(
            "nominator",
            seed,
            0u32.into(),
            collator.clone(),
            true,
            nominators.len() as u32,
        )?;
        nominators.push(nominator);
    }

    return Ok((collator, collators, nominators))
}

fn get_proof<T: Config>(
    relayer: &T::AccountId,
    signer: &T::AccountId,
    signature: sp_core::sr25519::Signature,
) -> Proof<T::Signature, T::AccountId> {
    return Proof {
        signer: signer.clone(),
        relayer: relayer.clone(),
        signature: T::Signature::decode(&mut &signature[..]).unwrap(),
    }
}

fn get_caller<T: Config, F>(
    encoder: F,
) -> Result<(T::AccountId, Proof<T::Signature, T::AccountId>), &'static str>
where
    F: Fn(T::AccountId, u64) -> Vec<u8>,
{
    let key = SignerId::generate_pair(None);
    let caller: T::AccountId =
        T::AccountId::decode(&mut Encode::encode(&key).as_slice()).expect("valid account id");
    let sender_nonce = Pallet::<T>::proxy_nonce(&caller);
    let encoded_data = encoder(caller.clone(), sender_nonce);
    let signature = key.sign(&encoded_data).ok_or("Error signing proof")?;
    let proof = get_proof::<T>(&caller, &caller, signature.into());

    return Ok((caller, proof))
}

fn setup_leave_nominator_state<T: Config>(
    num_of_collators: u32,
    caller: &T::AccountId,
) -> Result<u32, &'static str> {
    // Worst Case is full of nominations before execute exit
    let mut collators: Vec<T::AccountId> = Vec::new();
    let initial_candidate_count = get_collator_count::<T>();
    // Initialize MaxNominationsPerNominator collator candidates
    for i in 1..get_allowed_max_collators::<T>(num_of_collators) {
        let seed = USER_SEED - i;
        let collator = create_funded_collator::<T>(
            "leave_collator",
            seed,
            0u32.into(),
            true,
            collators.len() as u32 + initial_candidate_count + 1u32,
        )?;
        collators.push(collator.clone());
    }
    let bond = <MinTotalNominatorStake<T>>::get();
    let need = bond * (collators.len() as u32).into();
    let default_minted = min_candidate_stk::<T>();

    if need > default_minted {
        fund_account::<T>(&caller, need - default_minted);
    };

    // Nomination count
    let mut nomination_count = 0u32;
    let author = collators[0].clone();
    // Nominate MaxNominationsPerNominators collator candidates
    for col in collators {
        Pallet::<T>::nominate(
            RawOrigin::Signed(caller.clone()).into(),
            col,
            bond,
            0u32,
            nomination_count,
        )?;
        nomination_count += 1u32;
    }
    Pallet::<T>::schedule_leave_nominators(RawOrigin::Signed(caller.clone()).into())?;
    roll_to_and_author::<T>(2, author);

    return Ok(nomination_count)
}

fn generate_signature<T: pallet_avn::Config>(
) -> <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature {
    let encoded_data = 0.encode();
    let authority_id = T::AuthorityId::generate_pair(None);
    return authority_id.sign(&encoded_data).expect("able to make signature")
}

const USER_SEED: u32 = 999666;

benchmarks! {
    // ROOT DISPATCHABLES

    set_total_selected {
        Pallet::<T>::set_blocks_per_era(RawOrigin::Root.into(), 100u32)?;
    }: _(RawOrigin::Root, 100u32)
    verify {
        assert_eq!(Pallet::<T>::total_selected(), 100u32);
    }

    set_blocks_per_era {}: _(RawOrigin::Root, 1200u32)
    verify {
        assert_eq!(Pallet::<T>::era().length, 1200u32);
    }

    // USER DISPATCHABLES

    join_candidates {
        let x in 3..get_allowed_max_collators::<T>(<<T as Config>::MaxCandidates as Get<u32>>::get());
        // Worst Case Complexity is insertion into an ordered list so \exists full list before call
        let mut candidate_count = get_collator_count::<T>();
        for i in 2..x {
            let seed = USER_SEED - i;
            let collator = create_funded_collator::<T>(
                "collator",
                seed,
                0u32.into(),
                true,
                candidate_count
            )?;
            candidate_count += 1u32;
        }
        let (caller, min_candidate_stk) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        set_session_key::<T>(&caller, candidate_count)?;
    }: _(RawOrigin::Signed(caller.clone()), min_candidate_stk, candidate_count)
    verify {
        assert!(Pallet::<T>::is_candidate(&caller));
    }


    // This call schedules the collator's exit and removes them from the candidate pool
    // -> it retains the self-bond and nominator bonds
    schedule_leave_candidates {
        let x in 3..get_allowed_max_collators::<T>(<<T as Config>::MaxCandidates as Get<u32>>::get());
        // Worst Case Complexity is removal from an ordered list so \exists full list before call
        let mut candidate_count = get_collator_count::<T>();
        for i in 2..x {
            let seed = USER_SEED - i;
            let collator = create_funded_collator::<T>(
                "collator",
                seed,
                0u32.into(),
                true,
                candidate_count
            )?;
            candidate_count += 1u32;
        }
        let caller: T::AccountId = create_funded_collator::<T>(
            "caller",
            USER_SEED,
            0u32.into(),
            true,
            candidate_count,
        )?;
        candidate_count += 1u32;
    }: _(RawOrigin::Signed(caller.clone()), candidate_count)
    verify {
        assert!(Pallet::<T>::candidate_info(&caller).unwrap().is_leaving());
    }

    execute_leave_candidates {
        // x is total number of nominations for the candidate
        let x in 2..(<<T as Config>::MaxTopNominationsPerCandidate as Get<u32>>::get()
            + <<T as Config>::MaxBottomNominationsPerCandidate as Get<u32>>::get());

        let mut candidate_count = get_collator_count::<T>();
        // Make sure we have enough candidates first before we can leave
        for c in 1..=<<T as Config>::MinSelectedCandidates as Get<u32>>::get() {
            let candidate: T::AccountId = create_funded_collator::<T>(
                "setup_candidate",
                USER_SEED - c,
                0u32.into(),
                true,
                candidate_count,
            )?;
            candidate_count += candidate_count + 1u32;
        }

        let candidate: T::AccountId = create_funded_collator::<T>(
            "unique_caller",
            USER_SEED - 100,
            0u32.into(),
            true,
            candidate_count,
        )?;
        // 2nd nomination required for all nominators to ensure NominatorState updated not removed
        let second_candidate: T::AccountId = create_funded_collator::<T>(
            "unique__caller",
            USER_SEED - 99,
            0u32.into(),
            true,
            candidate_count + 1u32,
        )?;
        let mut nominators: Vec<T::AccountId> = Vec::new();
        let mut col_del_count = 0u32;
        for i in 1..x {
            let seed = USER_SEED + i;
            let nominator = create_funded_nominator::<T>(
                "nominator",
                seed,
                min_nominator_stk::<T>(),
                candidate.clone(),
                true,
                col_del_count,
            )?;
            Pallet::<T>::nominate(
                RawOrigin::Signed(nominator.clone()).into(),
                second_candidate.clone(),
                min_nominator_stk::<T>(),
                col_del_count,
                1u32,
            )?;
            Pallet::<T>::schedule_revoke_nomination(
                RawOrigin::Signed(nominator.clone()).into(),
                candidate.clone()
            )?;
            nominators.push(nominator);
            col_del_count += 1u32;
        }

        Pallet::<T>::schedule_leave_candidates(
            RawOrigin::Signed(candidate.clone()).into(), candidate_count
        )?;

        roll_to_and_author::<T>(2, candidate.clone());

    }: _(RawOrigin::Signed(candidate.clone()), candidate.clone(), col_del_count)
    verify {
        assert!(Pallet::<T>::candidate_info(&candidate).is_none());
        assert!(Pallet::<T>::candidate_info(&second_candidate).is_some());
        for nominator in nominators {
            assert!(Pallet::<T>::is_nominator(&nominator));
        }
    }

    cancel_leave_candidates {
        let x in 3..get_allowed_max_collators::<T>(<<T as Config>::MaxCandidates as Get<u32>>::get());
        // Worst Case Complexity is removal from an ordered list so \exists full list before call
        let mut candidate_count = get_collator_count::<T>();
        for i in 2..x {
            let seed = USER_SEED - i;
            let collator = create_funded_collator::<T>(
                "collator",
                seed,
                0u32.into(),
                true,
                candidate_count
            )?;
            candidate_count += 1u32;
        }
        let caller: T::AccountId = create_funded_collator::<T>(
            "caller",
            USER_SEED,
            0u32.into(),
            true,
            candidate_count,
        )?;
        candidate_count += 1u32;
        Pallet::<T>::schedule_leave_candidates(
            RawOrigin::Signed(caller.clone()).into(),
            candidate_count
        )?;
        candidate_count -= 1u32;
    }: _(RawOrigin::Signed(caller.clone()), candidate_count)
    verify {
        assert!(Pallet::<T>::candidate_info(&caller).unwrap().is_active());
    }

    go_offline {
        let caller: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
    }: _(RawOrigin::Signed(caller.clone()))
    verify {
        assert!(!Pallet::<T>::candidate_info(&caller).unwrap().is_active());
    }

    go_online {
        let caller: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        Pallet::<T>::go_offline(RawOrigin::Signed(caller.clone()).into())?;
    }: _(RawOrigin::Signed(caller.clone()))
    verify {
        assert!(Pallet::<T>::candidate_info(&caller).unwrap().is_active());
    }

    candidate_bond_extra {
        let more = min_candidate_stk::<T>();
        let caller: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            more,
            true,
            get_collator_count::<T>(),
        )?;
    }: _(RawOrigin::Signed(caller.clone()), more)
    verify {
        let expected_bond = more * 2u32.into();
        assert_eq!(
            Pallet::<T>::candidate_info(&caller).expect("caller was created, qed").bond,
            expected_bond,
        );
    }

    signed_candidate_bond_extra {
        let more = min_candidate_stk::<T>();
        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_candidate_bond_extra_params::<T>(relayer, &more, nonce))?;
        fund_account::<T>(&caller, more * 2u32.into());
        set_account_as_collator::<T>(&caller, BalanceOf::<T>::zero(), get_collator_count::<T>())?;
    }: _(RawOrigin::Signed(caller.clone()), proof, more)
    verify {
        let expected_bond = more * 2u32.into();
        assert_eq!(
            Pallet::<T>::candidate_info(&caller).expect("caller was created, qed").bond,
            expected_bond,
        );
    }

    schedule_candidate_unbond {
        let min_candidate_stk = min_candidate_stk::<T>();
        let caller: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            min_candidate_stk,
            false,
            get_collator_count::<T>(),
        )?;
    }: _(RawOrigin::Signed(caller.clone()), min_candidate_stk)
    verify {
        let state = Pallet::<T>::candidate_info(&caller).expect("request bonded less so exists");
        assert_eq!(
            state.request,
            Some(CandidateBondLessRequest {
                amount: min_candidate_stk,
                when_executable: 3,
            })
        );
    }

    signed_schedule_candidate_unbond {
        let min_candidate_stk = min_candidate_stk::<T>();
        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_schedule_candidate_unbond_params::<T>(relayer, &min_candidate_stk, nonce))?;
        fund_account::<T>(&caller, min_candidate_stk * 2u32.into());
        set_account_as_collator::<T>(&caller, min_candidate_stk, get_collator_count::<T>())?;
    }: _(RawOrigin::Signed(caller.clone()), proof, min_candidate_stk)
    verify {
        let state = Pallet::<T>::candidate_info(&caller).expect("request bonded less so exists");
        assert_eq!(
            state.request,
            Some(CandidateBondLessRequest {
                amount: min_candidate_stk,
                when_executable: 3,
            })
        );
    }

    execute_candidate_unbond {
        let min_candidate_stk = min_candidate_stk::<T>();
        let caller: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            min_candidate_stk,
            false,
            get_collator_count::<T>(),
        )?;
        Pallet::<T>::schedule_candidate_unbond(
            RawOrigin::Signed(caller.clone()).into(),
            min_candidate_stk
        )?;
        roll_to_and_author::<T>(2, caller.clone());
    }: {
        Pallet::<T>::execute_candidate_unbond(
            RawOrigin::Signed(caller.clone()).into(),
            caller.clone()
        )?;
    } verify {
        assert_eq!(
            Pallet::<T>::candidate_info(&caller).expect("caller was created, qed").bond,
            min_candidate_stk,
        );
    }

    signed_execute_candidate_unbond {
        let min_candidate_stk = min_candidate_stk::<T>();
        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_execute_candidate_unbond_params::<T>(relayer.clone(), &relayer, nonce))?;
        fund_account::<T>(&caller, min_candidate_stk * 2u32.into());
        set_account_as_collator::<T>(&caller, min_candidate_stk, get_collator_count::<T>())?;

        Pallet::<T>::schedule_candidate_unbond(
            RawOrigin::Signed(caller.clone()).into(),
            min_candidate_stk
        )?;

        roll_to_and_author::<T>(2, caller.clone());
    }: {
        Pallet::<T>::signed_execute_candidate_unbond(
            RawOrigin::Signed(caller.clone()).into(),
            proof,
            caller.clone()
        )?;
    } verify {
        assert_eq!(
            Pallet::<T>::candidate_info(&caller).expect("caller was created, qed").bond,
            min_candidate_stk,
        );
    }

    cancel_candidate_unbond {
        let min_candidate_stk = min_candidate_stk::<T>();
        let caller: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            min_candidate_stk,
            false,
            get_collator_count::<T>(),
        )?;
        Pallet::<T>::schedule_candidate_unbond(
            RawOrigin::Signed(caller.clone()).into(),
            min_candidate_stk
        )?;
    }: {
        Pallet::<T>::cancel_candidate_unbond(
            RawOrigin::Signed(caller.clone()).into(),
        )?;
    } verify {
        assert!(
            Pallet::<T>::candidate_info(&caller).unwrap().request.is_none()
        );
    }

    nominate {
        let x in 3..<<T as Config>::MaxNominationsPerNominator as Get<u32>>::get();
        let y in 2..<<T as Config>::MaxTopNominationsPerCandidate as Get<u32>>::get();
        let bond = <MinTotalNominatorStake<T>>::get();
        let (caller, _) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());

        let (collator, collators, nominators) = setup_nomination::<T>(x, y, bond, &caller)?;
    }: _(RawOrigin::Signed(caller.clone()), collator, bond, nominators.len() as u32, (collators.len() as u32 + 1u32))
    verify {
        assert!(Pallet::<T>::is_nominator(&caller));
    }

    signed_nominate {
        let x in 3..<<T as Config>::MaxNominationsPerNominator as Get<u32>>::get();
        let y in 2..<<T as Config>::MaxTopNominationsPerCandidate as Get<u32>>::get();

        let bond = <MinTotalNominatorStake<T>>::get() * x.into();
        let (test_setup_nominator, _) = create_funded_user::<T>("test_setup_nominator", USER_SEED, 0u32.into());
        let (collator, collators, _) = setup_nomination::<T>(x, y, bond, &test_setup_nominator)?;

        set_session_key::<T>(&collator, y)?;

        let mut targets: Vec<<T::Lookup as StaticLookup>::Source> = collators.into_iter().map(|c| T::Lookup::unlookup(c)).collect::<_>();
        targets.push(T::Lookup::unlookup(collator));

        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_nominate_params::<T>(relayer, &targets, &bond, nonce))?;
        fund_account::<T>(&caller, bond * 2u32.into());
    }: _(RawOrigin::Signed(caller.clone()), proof, targets, bond)
    verify {
        assert!(Pallet::<T>::is_nominator(&caller));
    }

    schedule_leave_nominators {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, _) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        let bond = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;
    }: _(RawOrigin::Signed(caller.clone()))
    verify {
        assert!(
            Pallet::<T>::nomination_scheduled_requests(&collator)
                .iter()
                .any(|r| r.nominator == caller && matches!(r.action, NominationAction::Revoke(_)))
        );
    }

    signed_schedule_leave_nominators {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;

        let bond = <MinTotalNominatorStake<T>>::get();
        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_schedule_leave_nominators_params::<T>(relayer, nonce))?;
        fund_account::<T>(&caller, bond * 2u32.into());

        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;
    }: _(RawOrigin::Signed(caller.clone()), proof)
    verify {
        assert!(
            Pallet::<T>::nomination_scheduled_requests(&collator)
                .iter()
                .any(|r| r.nominator == caller && matches!(r.action, NominationAction::Revoke(_)))
        );
    }

    execute_leave_nominators {
        let x in 2..<<T as Config>::MaxNominationsPerNominator as Get<u32>>::get();

        // Fund the nominator
        let (caller, _) = create_funded_user::<T>("caller", USER_SEED, min_nominator_stk::<T>());
        let nomination_count = setup_leave_nominator_state::<T>(x, &caller)?;

    }: _(RawOrigin::Signed(caller.clone()), caller.clone(), nomination_count)
    verify {
        assert!(Pallet::<T>::nominator_state(&caller).is_none());
    }

    signed_execute_leave_nominators {
        let x in 2..<<T as Config>::MaxNominationsPerNominator as Get<u32>>::get();
        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_execute_leave_nominators_params::<T>(relayer.clone(), &relayer, nonce))?;
        fund_account::<T>(&caller, min_nominator_stk::<T>());

        setup_leave_nominator_state::<T>(x, &caller)?;
    }: _(RawOrigin::Signed(caller.clone()), proof, caller.clone())
    verify {
        assert!(Pallet::<T>::nominator_state(&caller).is_none());
    }

    cancel_leave_nominators {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, _) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        let bond = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;
        Pallet::<T>::schedule_leave_nominators(RawOrigin::Signed(caller.clone()).into())?;
        let total_amount_to_withdraw = Pallet::<T>::nominator_state(&caller).expect("caller was created, qed").less_total;
    }: _(RawOrigin::Signed(caller.clone()))
    verify {
        // After cancelling the request, there shouldn't be any amount pending withdrawal
        assert_eq!(
            Pallet::<T>::nominator_state(&caller).expect("caller was created, qed").less_total, total_amount_to_withdraw - bond
        );
    }

    schedule_revoke_nomination {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, _) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        let bond = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;
    }: _(RawOrigin::Signed(caller.clone()), collator.clone())
    verify {
        assert_eq!(
            Pallet::<T>::nomination_scheduled_requests(&collator),
            vec![ScheduledRequest {
                nominator: caller,
                when_executable: 3,
                action: NominationAction::Revoke(bond),
            }],
        );
    }

    signed_schedule_revoke_nomination{
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;

        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_schedule_revoke_nomination_params::<T>(relayer.clone(), &collator, nonce))?;
        fund_account::<T>(&caller, min_nominator_stk::<T>());

        let bond = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;
    }: _(RawOrigin::Signed(caller.clone()), proof, collator.clone())
    verify {
        assert_eq!(
            Pallet::<T>::nomination_scheduled_requests(&collator),
            vec![ScheduledRequest {
                nominator: caller,
                when_executable: 3,
                action: NominationAction::Revoke(bond),
            }],
        );
    }

    bond_extra {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, _) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        let bond = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::nominate(
            RawOrigin::Signed(caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;
    }: _(RawOrigin::Signed(caller.clone()), collator.clone(), bond)
    verify {
        let expected_bond = bond * 2u32.into();
        assert_eq!(
            Pallet::<T>::nominator_state(&caller).expect("caller was created, qed").total,
            expected_bond,
        );
    }

    signed_bond_extra {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;

        let bond = <MinTotalNominatorStake<T>>::get() * 10u32.into();
        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_bond_extra_params::<T>(relayer, &bond, nonce))?;
        fund_account::<T>(&caller, bond * 2u32.into());

        Pallet::<T>::nominate(
            RawOrigin::Signed(caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;

        roll_to_and_author::<T>(2, collator.clone());

    }: _(RawOrigin::Signed(caller.clone()), proof, bond)
    verify {
        let expected_bond = bond * 2u32.into();
        assert_eq!(
            Pallet::<T>::nominator_state(&caller).expect("caller was created, qed").total,
            expected_bond,
        );
    }

    schedule_nominator_unbond {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, total) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            total,
            0u32,
            0u32
        )?;
        let bond_less = <MinTotalNominatorStake<T>>::get();
    }: _(RawOrigin::Signed(caller.clone()), collator.clone(), bond_less)
    verify {
        let state = Pallet::<T>::nominator_state(&caller)
            .expect("just request bonded less so exists");
        assert_eq!(
            Pallet::<T>::nomination_scheduled_requests(&collator),
            vec![ScheduledRequest {
                nominator: caller,
                when_executable: 3,
                action: NominationAction::Decrease(bond_less),
            }],
        );
    }

    signed_schedule_nominator_unbond {
        let num_collators = get_collator_count::<T>() + 1;
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            num_collators
        )?;

        let bond_less = <MinTotalNominatorStake<T>>::get();
        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_schedule_nominator_unbond_params::<T>(relayer, &bond_less, nonce))?;
        fund_account::<T>(&caller, bond_less * (num_collators * 3u32).into());

        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            bond_less * num_collators.into() * 2u32.into(),
            0u32,
            0u32
        )?;

    }: _(RawOrigin::Signed(caller.clone()), proof, bond_less)
    verify {
        let state = Pallet::<T>::nominator_state(&caller)
            .expect("just request bonded less so exists");
        assert_eq!(
            Pallet::<T>::nomination_scheduled_requests(&collator),
            vec![ScheduledRequest {
                nominator: caller,
                when_executable: 3,
                action: NominationAction::Decrease(bond_less),
            }],
        );
    }

    execute_revoke_nomination {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, _) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        let bond = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;
        Pallet::<T>::schedule_revoke_nomination(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone()
        )?;
        roll_to_and_author::<T>(2, collator.clone());
    }: {
        Pallet::<T>::execute_nomination_request(
            RawOrigin::Signed(caller.clone()).into(),
            caller.clone(),
            collator.clone()
        )?;
    } verify {
        assert!(
            !Pallet::<T>::is_nominator(&caller)
        );
    }

    execute_nominator_unbond {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, total) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            total,
            0u32,
            0u32
        )?;
        let bond_less = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::schedule_nominator_unbond(
            RawOrigin::Signed(caller.clone()).into(),
            collator.clone(),
            bond_less
        )?;
        roll_to_and_author::<T>(2, collator.clone());
    }: {
        Pallet::<T>::execute_nomination_request(
            RawOrigin::Signed(caller.clone()).into(),
            caller.clone(),
            collator.clone()
        )?;
    } verify {
        let expected = total - bond_less;
        assert_eq!(
            Pallet::<T>::nominator_state(&caller).expect("caller was created, qed").total,
            expected,
        );
    }

    signed_execute_nominator_unbond {
        let num_collators = get_collator_count::<T>() + 1;
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;

        let amount = min_nominator_stk::<T>();
        let (caller, proof) = get_caller::<T, _>(|relayer, nonce| encode_signed_execute_nomination_request_params::<T>(relayer.clone(), &relayer, nonce))?;
        fund_account::<T>(&caller, amount * (num_collators * 3u32).into());

        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            amount * 2u32.into(),
            0u32,
            0u32
        )?;

        Pallet::<T>::schedule_nominator_unbond(
            RawOrigin::Signed(caller.clone()).into(),
            collator.clone(),
            amount
        )?;

        roll_to_and_author::<T>(2, collator.clone());
    }: {
        Pallet::<T>::signed_execute_nomination_request(
            RawOrigin::Signed(caller.clone()).into(),
            proof,
            caller.clone()
        )?;
    } verify {
        let expected = amount; // bonded 2*amount and unbonded 1*amount
        assert_eq!(
            Pallet::<T>::nominator_state(&caller).expect("caller was created, qed").total,
            expected,
        );
    }

    cancel_revoke_nomination {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, _) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        let bond = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            bond,
            0u32,
            0u32
        )?;
        Pallet::<T>::schedule_revoke_nomination(
            RawOrigin::Signed(caller.clone()).into(),
            collator.clone()
        )?;
    }: {
        Pallet::<T>::cancel_nomination_request(
            RawOrigin::Signed(caller.clone()).into(),
            collator.clone()
        )?;
    } verify {
        assert!(
            !Pallet::<T>::nomination_scheduled_requests(&collator)
            .iter()
            .any(|x| &x.nominator == &caller)
        );
    }

    cancel_nominator_unbond {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let (caller, total) = create_funded_user::<T>("caller", USER_SEED, 0u32.into());
        Pallet::<T>::nominate(RawOrigin::Signed(
            caller.clone()).into(),
            collator.clone(),
            total,
            0u32,
            0u32
        )?;
        let bond_less = <MinTotalNominatorStake<T>>::get();
        Pallet::<T>::schedule_nominator_unbond(
            RawOrigin::Signed(caller.clone()).into(),
            collator.clone(),
            bond_less
        )?;
        roll_to_and_author::<T>(2, collator.clone());
    }: {
        Pallet::<T>::cancel_nomination_request(
            RawOrigin::Signed(caller.clone()).into(),
            collator.clone()
        )?;
    } verify {
        assert!(
            !Pallet::<T>::nomination_scheduled_requests(&collator)
                .iter()
                .any(|x| &x.nominator == &caller)
        );
    }

    // ON_INITIALIZE

    era_transition_on_initialize {
        // TOTAL SELECTED COLLATORS PER ERA
        let x in 8..20;
        // NOMINATIONS
        let y in 0..(<<T as Config>::MaxTopNominationsPerCandidate as Get<u32>>::get() * 100);
        let max_nominators_per_collator =
            <<T as Config>::MaxTopNominationsPerCandidate as Get<u32>>::get();
        let max_nominations = x * max_nominators_per_collator;
        // y should depend on x but cannot directly, we overwrite y here if necessary to bound it
        let total_nominations: u32 = if max_nominations < y { max_nominations } else { y };
        // INITIALIZE RUNTIME STATE
        // To set total selected to 40, must first increase era length to at least 40
        // to avoid hitting EraLengthMustBeAtLeastTotalSelectedCollators
        if Pallet::<T>::era().length < 100 {
            Pallet::<T>::set_blocks_per_era(RawOrigin::Root.into(), 100u32)?;
        }

        if Pallet::<T>::total_selected() < 100u32 {
            Pallet::<T>::set_total_selected(RawOrigin::Root.into(), 100u32)?;
        }

        // INITIALIZE COLLATOR STATE
        let mut collators: Vec<T::AccountId> = Vec::new();
        let mut collator_count = Pallet::<T>::selected_candidates().len() as u32;
        for i in 0..x {
            let seed = USER_SEED - i;
            let collator = create_funded_collator::<T>(
                "collator",
                seed,
                min_candidate_stk::<T>() * 1_000_000u32.into(),
                true,
                collator_count
            )?;
            collators.push(collator);
            collator_count += 1u32;
        }
        // STORE starting balances for all collators
        let collator_starting_balances: Vec<(
            T::AccountId,
            <<T as Config>::Currency as Currency<T::AccountId>>::Balance
        )> = collators.iter().map(|x| (x.clone(), T::Currency::free_balance(&x))).collect();
        // INITIALIZE NOMINATIONS
        let mut col_del_count: BTreeMap<T::AccountId, u32> = BTreeMap::new();
        collators.iter().for_each(|x| {
            col_del_count.insert(x.clone(), 0u32);
        });
        let mut nominators: Vec<T::AccountId> = Vec::new();
        let mut remaining_nominations = if total_nominations > max_nominators_per_collator {
            for j in 1..(max_nominators_per_collator + 1) {
                let seed = USER_SEED + j;
                let nominator = create_funded_nominator::<T>(
                    "nominator",
                    seed,
                    min_candidate_stk::<T>() * 1_000_000u32.into(),
                    collators[0].clone(),
                    true,
                    nominators.len() as u32,
                )?;
                nominators.push(nominator);
            }
            total_nominations - max_nominators_per_collator
        } else {
            for j in 1..(total_nominations + 1) {
                let seed = USER_SEED + j;
                let nominator = create_funded_nominator::<T>(
                    "nominator",
                    seed,
                    min_candidate_stk::<T>() * 1_000_000u32.into(),
                    collators[0].clone(),
                    true,
                    nominators.len() as u32,
                )?;
                nominators.push(nominator);
            }
            0u32
        };
        col_del_count.insert(collators[0].clone(), nominators.len() as u32);
        // FILL remaining nominations
        if remaining_nominations > 0 {
            for (col, n_count) in col_del_count.iter_mut() {
                if n_count < &mut (nominators.len() as u32) {
                    // assumes nominators.len() <= MaxTopNominationsPerCandidate
                    let mut open_spots = nominators.len() as u32 - *n_count;
                    while open_spots > 0 && remaining_nominations > 0 {
                        let caller = nominators[open_spots as usize - 1usize].clone();
                        if let Ok(_) = Pallet::<T>::nominate(RawOrigin::Signed(
                            caller.clone()).into(),
                            col.clone(),
                            <MinTotalNominatorStake<T>>::get(),
                            *n_count,
                            collators.len() as u32, // overestimate
                        ) {
                            *n_count += 1;
                            remaining_nominations -= 1;
                        }
                        open_spots -= 1;
                    }
                }
                if remaining_nominations == 0 {
                    break;
                }
            }
        }
        // STORE starting balances for all nominators
        let nominator_starting_balances: Vec<(
            T::AccountId,
            <<T as Config>::Currency as Currency<T::AccountId>>::Balance
        )> = nominators.iter().map(|x| (x.clone(), T::Currency::free_balance(&x))).collect();
        // PREPARE RUN_TO_BLOCK LOOP
        let before_running_era_index = Pallet::<T>::era().current;
        let era_length: BlockNumberFor<T> = Pallet::<T>::era().length.into();
        let reward_delay = <<T as Config>::RewardPaymentDelay as Get<u32>>::get() + 2u32;
        let mut now = <frame_system::Pallet<T>>::block_number() + 1u32.into();
        let mut counter = 0usize;
        let end = Pallet::<T>::era().first + (era_length * reward_delay.into());
        // SET collators as authors for blocks from now - end
        while now < end {
            // Set some rewards to payout
            T::Currency::make_free_balance_be(&Pallet::<T>::compute_reward_pot_account_id(), min_candidate_stk::<T>() * 1_000_000u32.into());

            let author = collators[counter % collators.len()].clone();
            parachain_staking_on_finalize::<T>(author);
            <frame_system::Pallet<T> as OnFinalize<BlockNumberFor<T>>>::on_finalize(<frame_system::Pallet<T>>::block_number());
            <frame_system::Pallet<T>>::set_block_number(
                <frame_system::Pallet<T>>::block_number() + 1u32.into()
            );
            <frame_system::Pallet<T> as OnInitialize<BlockNumberFor<T>>>::on_initialize(<frame_system::Pallet<T>>::block_number());
            <pallet::Pallet<T> as OnInitialize<BlockNumberFor<T>>>::on_initialize(<frame_system::Pallet<T>>::block_number());
            now += 1u32.into();
            counter += 1usize;
        }
        parachain_staking_on_finalize::<T>(collators[counter % collators.len()].clone());

        // Set some rewards to payout
        T::Currency::make_free_balance_be(&Pallet::<T>::compute_reward_pot_account_id(), min_candidate_stk::<T>() * 1_000_000u32.into());

        <frame_system::Pallet<T> as OnFinalize<BlockNumberFor<T>>>::on_finalize(<frame_system::Pallet<T>>::block_number());
        <frame_system::Pallet<T>>::set_block_number(
            <frame_system::Pallet<T>>::block_number() + 1u32.into()
        );
        <frame_system::Pallet<T> as OnInitialize<BlockNumberFor<T>>>::on_initialize(<frame_system::Pallet<T>>::block_number());
    }: { <pallet::Pallet<T> as OnInitialize<BlockNumberFor<T>>>::on_initialize(<frame_system::Pallet<T>>::block_number()); }
    verify {
        // Collators have been paid
        for (col, initial) in collator_starting_balances {
            assert!(T::Currency::free_balance(&col) > initial);
        }
        // Nominators have been paid
        for (nom, initial) in nominator_starting_balances {
            assert!(T::Currency::free_balance(&nom) > initial, "Free balance: {:?} should be greater than initial balance: {:?}", T::Currency::free_balance(&nom), initial);
        }
        // Era transitions
        assert_eq!(Pallet::<T>::era().current, before_running_era_index + reward_delay);
    }

    pay_one_collator_reward {
        // y controls number of nominations, its maximum per collator is the max top nominations
        let y in 0..<<T as Config>::MaxTopNominationsPerCandidate as Get<u32>>::get();

        // must come after 'let foo in 0..` statements for macro
        use crate::{
            DelayedPayout, DelayedPayouts, AtStake, CollatorSnapshot, Bond, Points,
            AwardedPts,
        };

        let before_running_era_index = Pallet::<T>::era().current;
        let initial_stake_amount = min_candidate_stk::<T>() * 1_000_000u32.into();

        let mut total_staked = 0u32.into();

        // initialize our single collator
        let sole_collator = create_funded_collator::<T>(
            "collator",
            0,
            initial_stake_amount,
            true,
            get_collator_count::<T>(),
        )?;
        total_staked += initial_stake_amount;

        // generate funded collator accounts
        let mut nominators: Vec<T::AccountId> = Vec::new();
        for i in 0..y {
            let seed = USER_SEED + i;
            let nominator = create_funded_nominator::<T>(
                "nominator",
                seed,
                initial_stake_amount,
                sole_collator.clone(),
                true,
                nominators.len() as u32,
            )?;
            nominators.push(nominator);
            total_staked += initial_stake_amount;
        }

        // rather than roll through eras in order to initialize the storage we want, we set it
        // directly and then call pay_one_collator_reward directly.

        let era_for_payout = 5;
        <DelayedPayouts<T>>::insert(&era_for_payout, DelayedPayout {
            total_staking_reward: total_staked,
        });

        let mut nominations: BoundedVec<Bond<T::AccountId, BalanceOf<T>>, MaxNominations> = BoundedVec::default();
        for nominator in &nominators {
            nominations.try_push(Bond {
                owner: nominator.clone(),
                amount: 100u32.into(),
            }).unwrap();
        }

        <AtStake<T>>::insert(era_for_payout, &sole_collator, CollatorSnapshot {
            bond: 1_000u32.into(),
            nominations,
            total: 1_000_000u32.into(),
        });

        <Points<T>>::insert(era_for_payout, 100);
        <AwardedPts<T>>::insert(era_for_payout, &sole_collator, 20);
        fund_account::<T>(&Pallet::<T>::compute_reward_pot_account_id(), min_candidate_stk::<T>() * 1_000_000_000u32.into());

    }: {
        let era_for_payout = 5;
        // TODO: this is an extra read right here (we should whitelist it?)
        let payout_info = Pallet::<T>::delayed_payouts(era_for_payout).expect("payout expected");
        let result = Pallet::<T>::pay_one_collator_reward(era_for_payout, payout_info);
        assert!(result.0.is_some()); // TODO: how to keep this in scope so it can be done in verify block?
    }
    verify {
        // collator should have been paid
        assert!(
            T::Currency::free_balance(&sole_collator) > initial_stake_amount,
            "collator should have been paid in pay_one_collator_reward"
        );
        // nominators should have been paid
        for nominator in &nominators {
            assert!(
                T::Currency::free_balance(&nominator) > initial_stake_amount,
                "nominator should have been paid in pay_one_collator_reward"
            );
        }
    }

    base_on_initialize {
        let collator: T::AccountId = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            get_collator_count::<T>()
        )?;
        let start = <frame_system::Pallet<T>>::block_number();
        parachain_staking_on_finalize::<T>(collator.clone());
        <frame_system::Pallet<T> as OnFinalize<BlockNumberFor<T>>>::on_finalize(start);
        <frame_system::Pallet<T>>::set_block_number(
            start + 1u32.into()
        );
        let end = <frame_system::Pallet<T>>::block_number();
        <frame_system::Pallet<T> as OnInitialize<BlockNumberFor<T>>>::on_initialize(end);
    }: { <pallet::Pallet<T> as OnInitialize<BlockNumberFor<T>>>::on_initialize(end); }
    verify {
        // Era transitions
        assert_eq!(start + 1u32.into(), end);
    }

    select_top_candidates {
        // Setup collators first
        let mut candidate_count = get_collator_count::<T>();
        for i in 2..get_allowed_max_collators::<T>(100) {
            let seed = USER_SEED - i;
            let collator = create_funded_collator::<T>(
                "collator",
                seed,
                0u32.into(),
                true,
                candidate_count
            )?;
            candidate_count += 1u32;
        }
    }: { Pallet::<T>::select_top_candidates(1u32) }
    verify {
        assert_eq!(Pallet::<T>::selected_candidates().len() as u32, T::MinSelectedCandidates::get());
    }

    // worse case is paying a non-existing candidate account.
    note_author {
        let candidate_count = get_collator_count::<T>();
        let author = create_funded_collator::<T>(
            "collator",
            USER_SEED,
            0u32.into(),
            true,
            candidate_count
        )?;

        let new_block: BlockNumberFor<T> = 10u32.into();
        let now = <Era<T>>::get().current;

        frame_system::Pallet::<T>::set_block_number(new_block);
        assert_eq!(0u32, <AwardedPts<T>>::get(now, author.clone()));
        assert_eq!(0u32, <Points<T>>::get(now));
    }: {
        <Pallet::<T> as EventHandler<_, _>>::note_author(author.clone())
    } verify {
        assert_eq!(frame_system::Pallet::<T>::block_number(), new_block);
        assert_eq!(20u32, <AwardedPts<T>>::get(now, author));
        assert_eq!(20u32, <Points<T>>::get(now));
    }

    set_admin_setting {
        let new_delay_value = <Delay<T>>::get() - 1;
        let new_delay_setting = AdminSettings::<BalanceOf<T>>::Delay(new_delay_value);
    }: _(RawOrigin::Root, new_delay_setting)
    verify {
        assert_eq!(new_delay_value, <Delay<T>>::get());
    }
}

#[cfg(test)]
mod tests {
    use sp_io::TestExternalities;

    pub fn new_test_ext() -> TestExternalities {
        use sp_keystore::{testing::MemoryKeystore as KeyStore, KeystoreExt, KeystorePtr};
        use sp_std::sync::Arc;

        let mut ext = crate::mock::ExtBuilder::default().build();
        ext.register_extension(KeystoreExt(Arc::new(KeyStore::new()) as KeystorePtr));
        ext
    }
}

impl_benchmark_test_suite!(Pallet, crate::benchmarks::tests::new_test_ext(), crate::mock::Test);
