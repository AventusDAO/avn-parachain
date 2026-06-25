#![cfg(feature = "runtime-benchmarks")]

use super::*;
use crate::{encode_signed_submit_checkpoint_params, encode_signed_update_chain_handler_params};
use codec::{Decode, Encode};
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite};
use frame_support::{traits::Currency, BoundedVec};
use frame_system::RawOrigin;
use sp_application_crypto::KeyTypeId;
use sp_avn_common::{benchmarking::convert_sr25519_signature, AppChainInterface, Asset, Proof};
use sp_core::H256;
use sp_runtime::{RuntimeAppPublic, Saturating};

pub const BENCH_KEY_TYPE_ID: KeyTypeId = KeyTypeId(*b"test");

mod app_sr25519 {
    use super::BENCH_KEY_TYPE_ID;
    use sp_application_crypto::{app_crypto, sr25519};
    app_crypto!(sr25519, BENCH_KEY_TYPE_ID);
}

type SignerId = app_sr25519::Public;

const SEED: u32 = 0;

pub fn setup_balance<T: Config>(account: &T::AccountId) {
    let min_balance = T::Currency::minimum_balance();
    // Convert default checkpoint fee to the correct balance type
    let default_fee: BalanceOf<T> = T::DefaultCheckpointFee::get();

    // Calculate a large initial balance
    // Use saturating operations to prevent overflow
    let large_multiplier: BalanceOf<T> = 1000u32.into();
    let fee_component = default_fee.saturating_mul(large_multiplier);
    let existential_component = min_balance.saturating_mul(large_multiplier);

    // Add the components together for total initial balance
    let initial_balance = fee_component.saturating_add(existential_component);

    // Set the balance
    T::Currency::make_free_balance_be(account, initial_balance);

    // Ensure the account has enough free balance
    assert!(
        T::Currency::free_balance(account) >= initial_balance,
        "Failed to set up sufficient balance"
    );
}

pub fn ensure_fee_payment_possible<T: Config>(
    chain_id: ChainId,
    account: &T::AccountId,
) -> Result<(), &'static str> {
    let fee = Pallet::<T>::checkpoint_fee(chain_id);
    let balance = T::Currency::free_balance(account);
    if balance < fee {
        return Err("Insufficient balance for fee payment")
    }
    Ok(())
}

fn create_account_id<T: Config>(seed: u32) -> T::AccountId {
    account("account", seed, SEED)
}

fn create_proof<T: Config>(
    signature: sp_core::sr25519::Signature,
    signer: T::AccountId,
    relayer: T::AccountId,
) -> Proof<T::Signature, T::AccountId> {
    Proof { signer, relayer, signature: convert_sr25519_signature::<T::Signature>(signature) }
}

fn setup_chain<T: Config>(handler: &T::AccountId) -> Result<ChainId, &'static str> {
    let chain_id = NextChainId::<T>::get();
    NextChainId::<T>::mutate(|id| *id = id.saturating_add(1));
    ChainHandlers::<T>::insert(handler, chain_id);
    Nonces::<T>::insert(chain_id, 0u64);
    Ok(chain_id)
}

/// Fully register an app chain (handler + asset-registry entry) so it can accrue rewards.
/// `seed` must be unique per chain to avoid duplicate handler/token-location errors.
fn register_appchain_for_bench<T: Config>(
    handler: &T::AccountId,
    seed: u8,
) -> Result<T::AppChainAssetId, &'static str> {
    let name: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(b"Chain".to_vec()).unwrap();
    let symbol: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(b"TKN".to_vec()).unwrap();
    let token = T::Token::from(sp_core::H160::from([seed; 20]));
    let asset_id: T::AppChainAssetId =
        T::AppChainAssetId::decode(&mut &Asset::ForeignAsset(seed as u32).encode()[..])
            .unwrap_or_default();
    Pallet::<T>::register_appchain(
        RawOrigin::Root.into(),
        handler.clone(),
        name,
        symbol,
        token,
        asset_id,
        18u32,
    )
    .map_err(|_| "register_appchain failed")?;
    Ok(asset_id)
}

/// Benchmark-only capability: credit the reward pot with an app-chain token so a payout can be
/// measured. Implemented by the mock and the runtime (where the multi-currency pallet lives), and
/// required via the `benchmarks!` `where_clause`.
pub trait BenchmarkHelper<T: Config> {
    fn fund_reward_pot(asset_id: T::AppChainAssetId, amount: BalanceOf<T>);
}

benchmarks! {
    where_clause { where T: BenchmarkHelper<T> }

    register_chain_handler {
        let caller: T::AccountId = create_account_id::<T>(0);
        let name: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(vec![0u8; 32]).unwrap();
        setup_balance::<T>(&caller);
    }: {
        // Call is deprecated and always returns CallDeprecated; ignore the error.
        let _ = Pallet::<T>::register_chain_handler(RawOrigin::Signed(caller.clone()).into(), name.clone());
    }
    verify {
        assert!(!ChainHandlers::<T>::contains_key(&caller));
    }

    update_chain_handler {
        let old_handler: T::AccountId = create_account_id::<T>(0);
        let new_handler: T::AccountId = create_account_id::<T>(1);
        setup_balance::<T>(&old_handler);
        setup_balance::<T>(&new_handler);
        let chain_id = setup_chain::<T>(&old_handler)?;
    }: _(RawOrigin::Signed(old_handler.clone()), new_handler.clone())
    verify {
        assert!(!ChainHandlers::<T>::contains_key(&old_handler));
        assert!(ChainHandlers::<T>::contains_key(&new_handler));
    }

    submit_checkpoint_with_identity {
        let handler: T::AccountId = create_account_id::<T>(0);
        let chain_id = setup_chain::<T>(&handler)?;
        setup_balance::<T>(&handler);
        ensure_fee_payment_possible::<T>(chain_id, &handler)?;

        let checkpoint = H256::from([0u8; 32]);
        let origin_id = 42u64;
        let initial_checkpoint_id = NextCheckpointId::<T>::get(chain_id);

        let initial_balance = T::Currency::free_balance(&handler);
        let fee = Pallet::<T>::checkpoint_fee(chain_id);
        assert!(initial_balance >= fee, "Insufficient initial balance");
    }: _(RawOrigin::Signed(handler.clone()), checkpoint, origin_id)
    verify {
        assert_eq!(
            Checkpoints::<T>::get(chain_id, initial_checkpoint_id).unwrap(),
            CheckpointData { hash: checkpoint, origin_id }
        );
        assert_eq!(
            OriginIdToCheckpoint::<T>::get(chain_id, origin_id).unwrap(),
            initial_checkpoint_id
        );

        // Verify checkpoint ID increment
        assert_eq!(NextCheckpointId::<T>::get(chain_id), initial_checkpoint_id + 1);
        // Verify fee was paid
        assert!(T::Currency::free_balance(&handler) < initial_balance, "Fee was not deducted");
    }

    signed_register_chain_handler {
        let signer_pair = SignerId::generate_pair(None);
        let handler: T::AccountId = T::AccountId::decode(&mut signer_pair.encode().as_slice())
            .expect("Valid account id");
        let relayer: T::AccountId = create_account_id::<T>(1);
        let name: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(vec![0u8; 32]).unwrap();
        setup_balance::<T>(&handler);
        setup_balance::<T>(&relayer);

        // Build a dummy proof — the call is deprecated and will return CallDeprecated immediately.
        let proof = create_proof::<T>(
            sp_core::sr25519::Signature::default(),
            handler.clone(),
            relayer,
        );
    }: {
        // Call is deprecated and always returns CallDeprecated; ignore the error.
        let _ = Pallet::<T>::signed_register_chain_handler(RawOrigin::Signed(handler.clone()).into(), proof, handler.clone(), name.clone());
    }
    verify {
        assert!(!ChainHandlers::<T>::contains_key(&handler));
    }

    signed_update_chain_handler {
        let old_signer_pair = SignerId::generate_pair(None);
        let old_handler: T::AccountId = T::AccountId::decode(&mut old_signer_pair.encode().as_slice())
            .expect("Valid account id");
        let new_handler: T::AccountId = create_account_id::<T>(1);
        let relayer: T::AccountId = create_account_id::<T>(2);
        setup_balance::<T>(&old_handler);
        setup_balance::<T>(&new_handler);
        setup_balance::<T>(&relayer);
        let chain_id = setup_chain::<T>(&old_handler)?;
        let nonce = Nonces::<T>::get(chain_id);

        let payload = encode_signed_update_chain_handler_params::<T>(
            &relayer,
            &old_handler,
            &new_handler,
            chain_id,
            nonce
        );
        let signature = old_signer_pair.sign(&payload).ok_or("Error signing proof")?;
        let proof = create_proof::<T>(signature.into(), old_handler.clone(), relayer);
    }: _(RawOrigin::Signed(old_handler.clone()), proof, old_handler.clone(), new_handler.clone())
    verify {
        assert!(!ChainHandlers::<T>::contains_key(&old_handler));
        assert!(ChainHandlers::<T>::contains_key(&new_handler));
    }

    signed_submit_checkpoint_with_identity {
        let signer_pair = SignerId::generate_pair(None);
        let handler: T::AccountId = T::AccountId::decode(&mut Encode::encode(&signer_pair).as_slice())
            .expect("valid account id");
        let relayer: T::AccountId = create_account_id::<T>(1);

        setup_balance::<T>(&handler);
        setup_balance::<T>(&relayer);

        let chain_id = setup_chain::<T>(&handler)?;
        ensure_fee_payment_possible::<T>(chain_id, &handler)?;

        let checkpoint = H256::from([0u8; 32]);
        let origin_id = 42u64;
        let nonce = Nonces::<T>::get(chain_id);
        let initial_checkpoint_id = NextCheckpointId::<T>::get(chain_id);

        let payload = encode_signed_submit_checkpoint_params::<T>(
            &relayer,
            &handler,
            &checkpoint,
            chain_id,
            nonce,
            &origin_id
        );
        let signature = signer_pair.sign(&payload).ok_or("Error signing proof")?;
        let proof = create_proof::<T>(signature.into(), handler.clone(), relayer);

        let initial_balance = T::Currency::free_balance(&handler);
        let fee = Pallet::<T>::checkpoint_fee(chain_id);
        assert!(initial_balance >= fee, "Insufficient initial balance");
    }: _(RawOrigin::Signed(handler.clone()), proof, handler.clone(), checkpoint, origin_id)
    verify {
        assert_eq!(
            Checkpoints::<T>::get(chain_id, initial_checkpoint_id).unwrap(),
            CheckpointData { hash: checkpoint, origin_id }
        );
        assert_eq!(
            OriginIdToCheckpoint::<T>::get(chain_id, origin_id).unwrap(),
            initial_checkpoint_id
        );
        assert_eq!(NextCheckpointId::<T>::get(chain_id), initial_checkpoint_id + 1);
        assert!(T::Currency::free_balance(&handler) < initial_balance, "Fee was not deducted");
    }

    register_appchain {
        let handler: T::AccountId = create_account_id::<T>(0);
        let name: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(b"Benchmark Chain".to_vec()).unwrap();
        let symbol: BoundedVec<u8, ConstU32<32>> = BoundedVec::try_from(b"BCH".to_vec()).unwrap();
        let token = T::Token::from(sp_core::H160::from([1u8; 20]));
        let asset_id: T::AppChainAssetId =
            T::AppChainAssetId::decode(&mut &Asset::ForeignAsset(2).encode()[..])
                .unwrap_or_default();
    }: _(RawOrigin::Root, handler.clone(), name, symbol, token, asset_id, 18u32)
    verify {
        assert!(ChainHandlers::<T>::contains_key(&handler));
        let chain_id = ChainHandlers::<T>::get(&handler).expect("handler should be registered");
        assert!(AssetIdToChainId::<T>::get(asset_id).is_some());
    }

    set_checkpoint_fee {
        let chain_id = 0;
        let new_fee = BalanceOf::<T>::from(100u32);
    }: _(RawOrigin::Root, chain_id, new_fee)
    verify {
        assert_eq!(CheckpointFee::<T>::get(chain_id), new_fee);
    }

    set_appchain_period_reward {
        let handler: T::AccountId = create_account_id::<T>(0);
        let asset_id = register_appchain_for_bench::<T>(&handler, 1)?;
        let amount = BalanceOf::<T>::from(1_000u32);
    }: _(RawOrigin::Signed(handler.clone()), asset_id, amount)
    verify {
        assert_eq!(NextRewardAmountPerPeriod::<T>::get(asset_id).map(|(_, a)| a), Some(amount));
    }

    // `n` = number of app chains snapshotted at the period boundary.
    on_new_reward_period {
        let n in 1 .. T::MaxRegisteredAppChains::get();
        for i in 0 .. n {
            let handler: T::AccountId = create_account_id::<T>(i);
            let asset_id = register_appchain_for_bench::<T>(&handler, (i + 1) as u8)?;
            Pallet::<T>::set_appchain_period_reward(
                RawOrigin::Signed(handler).into(),
                asset_id,
                BalanceOf::<T>::from(1_000u32),
            )?;
        }
        let period: sp_avn_common::RewardPeriodIndex = 1;
    }: {
        <Pallet<T> as AppChainInterface>::on_new_reward_period(&period);
    }
    verify {
        assert_eq!(PeriodChainReward::<T>::iter_prefix(period).count(), n as usize);
    }

    // `n` = number of app chains paid for a single (period, node).
    pay_node_period {
        let n in 1 .. T::MaxRegisteredAppChains::get();
        let owner: T::AccountId = create_account_id::<T>(0);
        let node: T::AccountId = create_account_id::<T>(1);
        let period: sp_avn_common::RewardPeriodIndex = 1;
        let amount = BalanceOf::<T>::from(1_000u32);

        for i in 0 .. n {
            let handler: T::AccountId = create_account_id::<T>(100 + i);
            let asset_id = register_appchain_for_bench::<T>(&handler, (i + 1) as u8)?;
            let token = Pallet::<T>::resolve_payout_token(&asset_id).map_err(|_| "token")?;
            PeriodChainReward::<T>::insert(period, asset_id, (token, amount));
            T::fund_reward_pot(asset_id, BalanceOf::<T>::from(1_000_000u32));
        }
        UnpaidByPeriod::<T>::insert(
            period,
            &node,
            RewardRecord { owner: owner.clone(), share: sp_runtime::Perquintill::from_percent(100), auto_stake_expiry: 0u64 },
        );
        UnpaidByNode::<T>::insert(&node, period, ());
        // Mark the period completed so settling the last node reclaims the snapshot (worst case).
        PeriodPayoutCompleted::<T>::insert(period, ());
    }: {
        Pallet::<T>::try_pay_node_period(period, &node)?;
    }
    verify {
        assert!(UnpaidByPeriod::<T>::get(period, &node).is_none());
    }

    // `p` = number of unpaid periods claimed for one node.
    // `c` = number of registered app chains funded in each of those periods. Each period's payout
    // iterates every funded chain, so the worst case scales with both dimensions (`p * c` payouts).
    claim {
        let p in 1 .. T::MaxPeriodsPerPayout::get();
        let c in 1 .. T::MaxRegisteredAppChains::get();
        let owner: T::AccountId = create_account_id::<T>(0);
        let node: T::AccountId = create_account_id::<T>(1);
        let amount = BalanceOf::<T>::from(1_000u32);

        // Register `c` app chains and fund the pot for each so every payout succeeds.
        let mut chains: Vec<(T::AppChainAssetId, T::Token)> = Vec::new();
        for i in 0 .. c {
            let handler: T::AccountId = create_account_id::<T>(100 + i);
            let asset_id = register_appchain_for_bench::<T>(&handler, (i + 1) as u8)?;
            let token = Pallet::<T>::resolve_payout_token(&asset_id).map_err(|_| "token")?;
            // Fund well above total payouts so the pot stays above its existential deposit.
            T::fund_reward_pot(asset_id, BalanceOf::<T>::from(1_000_000_000u32));
            chains.push((asset_id, token));
        }

        for pi in 0 .. p {
            let period = pi as sp_avn_common::RewardPeriodIndex;
            for (asset_id, token) in chains.iter() {
                PeriodChainReward::<T>::insert(period, *asset_id, (*token, amount));
            }
            UnpaidByPeriod::<T>::insert(
                period,
                &node,
                RewardRecord { owner: owner.clone(), share: sp_runtime::Perquintill::from_percent(100), auto_stake_expiry: 0u64 },
            );
            UnpaidByNode::<T>::insert(&node, period, ());
            // Mark completed so each period's snapshot is reclaimed on its final settle (worst case).
            PeriodPayoutCompleted::<T>::insert(period, ());
        }
    }: _(RawOrigin::Signed(owner.clone()), node.clone())
    verify {
        assert!(UnpaidByNode::<T>::iter_prefix(&node).next().is_none());
    }

    // `n` = number of (period, node) payouts swept in one call.
    // `c` = number of registered app chains funded in the period; each payout pays every funded
    // chain, so the worst case scales with both dimensions (`n * c` payments).
    process_outstanding_rewards {
        let n in 1 .. T::MaxPeriodsPerPayout::get();
        let c in 1 .. T::MaxRegisteredAppChains::get();
        let owner: T::AccountId = create_account_id::<T>(0);
        let caller: T::AccountId = create_account_id::<T>(2);
        let period: sp_avn_common::RewardPeriodIndex = 1;
        let amount = BalanceOf::<T>::from(1_000u32);

        // Register `c` app chains, snapshot each for the period, and fund the pot for each.
        for i in 0 .. c {
            let handler: T::AccountId = create_account_id::<T>(100 + i);
            let asset_id = register_appchain_for_bench::<T>(&handler, (i + 1) as u8)?;
            let token = Pallet::<T>::resolve_payout_token(&asset_id).map_err(|_| "token")?;
            PeriodChainReward::<T>::insert(period, asset_id, (token, amount));
            // Fund well above total payouts so the pot stays above its existential deposit.
            T::fund_reward_pot(asset_id, BalanceOf::<T>::from(1_000_000_000u32));
        }
        // Mark completed so the snapshot is reclaimed once the last node is swept (worst case).
        PeriodPayoutCompleted::<T>::insert(period, ());

        for i in 0 .. n {
            let node: T::AccountId = create_account_id::<T>(1_000 + i);
            UnpaidByPeriod::<T>::insert(
                period,
                &node,
                RewardRecord {
                    owner: owner.clone(),
                    share: sp_runtime::Perquintill::from_rational(1u64, n.max(1) as u64),
                    auto_stake_expiry: 0u64,
                },
            );
            UnpaidByNode::<T>::insert(&node, period, ());
        }
    }: _(RawOrigin::Signed(caller))
    verify {
        assert!(UnpaidByPeriod::<T>::iter_prefix(period).next().is_none());
    }

    // `b` = number of nodes recorded for one period (the recording path; a pool exists).
    // The range matches node-manager's `MAX_BATCH_SIZE`.
    on_reward_paid {
        let b in 1 .. 1_000;
        let handler: T::AccountId = create_account_id::<T>(0);
        let owner: T::AccountId = create_account_id::<T>(1);
        let period: sp_avn_common::RewardPeriodIndex = 1;

        let asset_id = register_appchain_for_bench::<T>(&handler, 1)?;
        Pallet::<T>::set_appchain_period_reward(
            RawOrigin::Signed(handler).into(),
            asset_id,
            BalanceOf::<T>::from(1_000u32),
        )?;
        // Snapshot the pool for the period so the hook records rather than short-circuiting.
        <Pallet<T> as AppChainInterface>::on_new_reward_period(&period);
    }: {
        for i in 0 .. b {
            let node: T::AccountId = create_account_id::<T>(1_000 + i);
            <Pallet<T> as AppChainInterface>::on_reward_paid(
                &period,
                &owner,
                &node,
                0u64,
                sp_runtime::Perquintill::from_percent(50),
            );
        }
    }
    verify {
        let last: T::AccountId = create_account_id::<T>(1_000 + b - 1);
        assert!(UnpaidByPeriod::<T>::get(period, &last).is_some());
    }

    disable_appchain {
        let handler: T::AccountId = create_account_id::<T>(0);
        let asset_id = register_appchain_for_bench::<T>(&handler, 1)?;
        Pallet::<T>::set_appchain_period_reward(RawOrigin::Signed(handler.clone()).into(), asset_id, BalanceOf::<T>::from(1_000u32))?;
    }: _(RawOrigin::Signed(handler.clone()), asset_id)
    verify {
        // Disabling removes the entry entirely.
        assert!(NextRewardAmountPerPeriod::<T>::get(asset_id).is_none(), "Entry should be removed after disabling");
    }

    deregister_appchain {
        let handler: T::AccountId = create_account_id::<T>(0);
        let asset_id = register_appchain_for_bench::<T>(&handler, 1)?;
        // Set a rate then disable so the deregister precondition (inactive) is satisfied.
        Pallet::<T>::set_appchain_period_reward(RawOrigin::Signed(handler.clone()).into(), asset_id, BalanceOf::<T>::from(1_000u32))?;
        Pallet::<T>::disable_appchain(RawOrigin::Signed(handler.clone()).into(), asset_id)?;
    }: _(RawOrigin::Root, handler.clone(), asset_id)
    verify {
        assert!(AssetIdToChainId::<T>::get(asset_id).is_none());
        assert!(!ChainHandlers::<T>::contains_key(&handler));
        assert!(NextRewardAmountPerPeriod::<T>::get(asset_id).is_none());
        assert!(!RegisteredAppchains::<T>::get().contains(&asset_id));
    }

    // Worst case: completing a period that was snapshotted across `MaxRegisteredAppChains` chains but
    // had no accruals (`UnpaidByPeriod` empty), so the hook reclaims the entire snapshot.
    on_reward_period_completed {
        let period: sp_avn_common::RewardPeriodIndex = 1;
        let amount = BalanceOf::<T>::from(1_000u32);
        let n = T::MaxRegisteredAppChains::get();
        for i in 0 .. n {
            let handler: T::AccountId = create_account_id::<T>(100 + i);
            let asset_id = register_appchain_for_bench::<T>(&handler, (i + 1) as u8)?;
            let token = Pallet::<T>::resolve_payout_token(&asset_id).map_err(|_| "token")?;
            PeriodChainReward::<T>::insert(period, asset_id, (token, amount));
        }
    }: {
        <Pallet<T> as AppChainInterface>::on_reward_period_completed(&period);
    }
    verify {
        assert!(PeriodChainReward::<T>::iter_prefix(period).next().is_none());
    }
}

impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::TestRuntime);
