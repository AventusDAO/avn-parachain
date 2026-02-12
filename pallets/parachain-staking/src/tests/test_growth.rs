use crate::{
    assert_event_emitted,
    mock::{
        get_default_block_per_era, roll_one_block, roll_to_era_begin, set_author, set_reward_pot,
        AccountId, Balances, ErasPerGrowthPeriod, ExtBuilder, ParachainStaking, RewardPaymentDelay,
        RuntimeOrigin, System, Test, TestAccount,
    },
    BalanceOf, CollatorScore, EraIndex, Error, Event, Growth, GrowthInfo, GrowthPeriod,
    GrowthPeriodInfo, ProcessedGrowthPeriods,
};
use codec::{Decode, Encode};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::Perbill;
use std::collections::HashMap;

const DEFAULT_POINTS: u32 = 5;

pub type Reward = u128;
pub type Stake = u128;

#[derive(Clone, Encode, Decode, Debug)]
pub struct GrowthData {
    pub reward: Reward,
    pub stake: Stake,
}
impl GrowthData {
    pub fn new(reward: Reward, stake: Stake) -> Self {
        GrowthData { reward, stake }
    }
}

fn to_acc_id(id: u64) -> AccountId {
    return TestAccount::new(id).account_id();
}

fn roll_one_growth_period(current_era_index: EraIndex) -> u32 {
    roll_to_era_begin((current_era_index + ErasPerGrowthPeriod::get()).into());
    return ParachainStaking::era().current;
}

fn roll_one_era_and_try_paying_collators(current_era: EraIndex) -> EraIndex {
    // This will change era and trigger first collator payout (if any due)
    roll_to_era_begin((current_era + 1).into());
    // move one more block to finish paying out the second collator (if any due)
    roll_one_block();

    return ParachainStaking::era().current;
}

fn set_equal_points_for_collators(era: EraIndex, collator_1: AccountId, collator_2: AccountId) {
    set_author(era, collator_1, DEFAULT_POINTS);
    set_author(era, collator_2, DEFAULT_POINTS);
}

fn increase_collator_nomination(
    collator_1: AccountId,
    collator_2: AccountId,
    increase_amount: u128,
) {
    assert_ok!(ParachainStaking::candidate_bond_extra(
        RuntimeOrigin::signed(collator_1),
        increase_amount
    ));
    assert_ok!(ParachainStaking::candidate_bond_extra(
        RuntimeOrigin::signed(collator_2),
        increase_amount
    ));
}

fn get_expected_block_number(growth_index: u64) -> u64 {
    return get_default_block_per_era() as u64 * ErasPerGrowthPeriod::get() as u64 * growth_index;
}

fn increase_reward_pot_by(amount: u128) -> u128 {
    let current_balance = ParachainStaking::reward_pot();
    let new_balance = current_balance + amount;
    set_reward_pot(new_balance);
    return new_balance;
}

fn roll_foreward_and_pay_stakers(
    max_era: u32,
    collator_1: AccountId,
    collator_2: AccountId,
    collator1_stake: u128,
    collator2_stake: u128,
) -> HashMap<EraIndex, GrowthData> {
    let mut era_data: HashMap<EraIndex, GrowthData> =
        HashMap::from([(1, GrowthData::new(0u128, 30u128))]);
    let mut era_index = ParachainStaking::era().current;
    let mut total_stake = collator1_stake + collator2_stake;

    let initial_reward = 6u128;

    // SETUP: Run through era to generate realistic data. Change staked amount, reward for each era.
    for n in 1..=max_era - RewardPaymentDelay::get() {
        <frame_system::Pallet<Test>>::reset_events();

        if n == 1 {
            // No collator payouts on first era
            set_equal_points_for_collators(era_index, collator_1, collator_2);
            set_reward_pot(initial_reward);
            era_index = roll_one_era_and_try_paying_collators(era_index);

            assert_event_emitted!(Event::NewEra {
                starting_block: (get_default_block_per_era() as u64).into(),
                era: 2,
                selected_collators_number: 2,
                total_balance: total_stake,
            });

            era_data.insert(era_index, GrowthData::new(initial_reward, total_stake as u128));
        }

        // Both collators will be paid from now on because we will be in era 3
        let reward_amount = (10 * n) as u128;
        let bond_increase_amount = (10 + n) as u128;

        increase_collator_nomination(collator_1, collator_2, bond_increase_amount);
        set_equal_points_for_collators(era_index, collator_1, collator_2);
        let new_reward_pot_amount = increase_reward_pot_by(reward_amount);

        era_index = roll_one_era_and_try_paying_collators(era_index);

        total_stake += bond_increase_amount * 2;
        assert_event_emitted!(Event::NewEra {
            starting_block: (get_default_block_per_era() as u64 * (era_index as u64 - 1u64)).into(),
            era: era_index,
            selected_collators_number: 2,
            total_balance: total_stake,
        });

        let expected_reward = new_reward_pot_amount / 2;
        assert_event_emitted!(Event::Rewarded { account: collator_1, rewards: expected_reward });
        assert_event_emitted!(Event::Rewarded { account: collator_2, rewards: expected_reward });

        era_data.insert(era_index, GrowthData::new(new_reward_pot_amount, total_stake as u128));
    }

    return era_data;
}

#[test]
fn initial_growth_state_is_ok() {
    ExtBuilder::default().build().execute_with(|| {
        let default_growth: GrowthInfo<AccountId, BalanceOf<Test>> = GrowthInfo::new(1u32);

        // Growth period starts from 0
        let growth_period_info = ParachainStaking::growth_period_info();
        assert_eq!(growth_period_info.start_era_index, 0u32);
        assert_eq!(growth_period_info.index, 0u32);

        // The first growth is empty
        let initial_growth = ParachainStaking::growth(0);
        assert_eq!(initial_growth.number_of_accumulations, default_growth.number_of_accumulations);
        assert_eq!(initial_growth.total_stake_accumulated, default_growth.total_stake_accumulated);
        assert_eq!(initial_growth.total_staker_reward, default_growth.total_staker_reward);
        assert_eq!(initial_growth.total_points, default_growth.total_points);
        assert_eq!(initial_growth.collator_scores, default_growth.collator_scores);
    });
}

#[test]
fn growth_period_indices_updated_correctly() {
    let collator_1 = to_acc_id(1u64);
    let collator_2 = to_acc_id(2u64);
    ExtBuilder::default()
        .with_balances(vec![(collator_1, 100), (collator_2, 100)])
        .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
        .build()
        .execute_with(|| {
            let initial_growth_period_index = 0;
            let mut era_index = ParachainStaking::era().current;

            for n in 1..5 {
                set_author(era_index, collator_1, 5);
                set_author(era_index, collator_2, 5);
                era_index = roll_one_growth_period(era_index);

                let growth_period_info = ParachainStaking::growth_period_info();
                assert_eq!(
                    growth_period_info.start_era_index,
                    era_index - RewardPaymentDelay::get(),
                    "Start era index for n={} does not match expected",
                    n
                );
                assert_eq!(
                    growth_period_info.index,
                    initial_growth_period_index + n,
                    "index for n={} does not match expected",
                    n
                );
                assert_eq!(
                    System::block_number(),
                    get_expected_block_number(growth_period_info.index.into()),
                    "Block number for n={} does not match expected",
                    n
                );
            }
        });
}

mod growth_disabled {
    use super::*;
    use crate::mock::disable_growth;

    #[test]
    fn growth_period_indices_are_not_updated() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        ExtBuilder::default()
            .with_balances(vec![(collator_1, 100), (collator_2, 100)])
            .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
            .build()
            .execute_with(|| {
                disable_growth();

                let initial_growth_period_index = 0;
                let mut era_index = ParachainStaking::era().current;

                set_author(era_index, collator_1, 5);
                set_author(era_index, collator_2, 5);
                era_index = roll_one_growth_period(era_index);

                let growth_period_info = ParachainStaking::growth_period_info();
                // ensure indice is not updated
                assert_eq!(initial_growth_period_index, growth_period_info.start_era_index);
            });
    }

    #[test]
    fn growth_info_is_not_updated() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let collator1_stake = 20;
        let collator2_stake = 10;
        let reward_payment_delay = RewardPaymentDelay::get();
        ExtBuilder::default()
            .with_balances(vec![(collator_1, 10000), (collator_2, 10000)])
            .with_candidates(vec![(collator_1, collator1_stake), (collator_2, collator2_stake)])
            .build()
            .execute_with(|| {
                disable_growth();

                let num_era_to_roll_foreward = reward_payment_delay + 1;

                // Setup data by rolling forward and letting the system generate staking rewards.
                // This is not "faked" data.
                let raw_era_data: HashMap<EraIndex, GrowthData> = roll_foreward_and_pay_stakers(
                    num_era_to_roll_foreward,
                    collator_1,
                    collator_2,
                    collator1_stake,
                    collator2_stake,
                );

                let growth = ParachainStaking::growth(1);
                // Ensure info is not updated
                assert_eq!(growth.number_of_accumulations, 0);
                assert_eq!(growth.total_points, 0);
                assert_eq!(growth.total_stake_accumulated, 0);
                assert_eq!(growth.total_staker_reward, 0);
                assert_eq!(growth.collator_scores.len(), 0);
            });
    }
}

mod growth_info_recorded_correctly {
    use super::*;

    #[test]
    fn for_one_single_period() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let collator1_stake = 20;
        let collator2_stake = 10;
        let reward_payment_delay = RewardPaymentDelay::get();
        ExtBuilder::default()
            .with_balances(vec![(collator_1, 10000), (collator_2, 10000)])
            .with_candidates(vec![(collator_1, collator1_stake), (collator_2, collator2_stake)])
            .build()
            .execute_with(|| {
                let num_era_to_roll_foreward = reward_payment_delay + 1;

                // Setup data by rolling forward and letting the system generate staking rewards.
                // This is not "faked" data.
                let raw_era_data: HashMap<EraIndex, GrowthData> = roll_foreward_and_pay_stakers(
                    num_era_to_roll_foreward,
                    collator_1,
                    collator_2,
                    collator1_stake,
                    collator2_stake,
                );

                // Verification: On era (RewardPaymentDelay + 1) we should have the first growth
                // period created, and since we only rolled that many times
                // (num_era_to_roll_foreward = reward_payment_delay + 1),
                // we only expect a single entry growth info (no accumulation)

                // Check that we have the expected number of records added
                assert_eq!(ParachainStaking::growth_period_info().index, 1);

                let growth = ParachainStaking::growth(1);
                assert_eq!(growth.number_of_accumulations, 1);
                assert_eq!(growth.total_points, DEFAULT_POINTS * 2); // 2 collators

                // Check total stake matches era 1's stake because payouts are delayed by 2 eras
                assert_eq!(growth.total_stake_accumulated, raw_era_data.get(&1).unwrap().stake);

                // This is a bit tricky because `Reward` is not backdated. We pay whatever was
                // in the reward pot at the time of payout therefore it is the
                // sum of rewards for the non backdated eras that make up this GrowthPeriod)
                let current_era_index = ParachainStaking::era().current;
                assert_eq!(
                    growth.total_staker_reward,
                    raw_era_data.get(&current_era_index).unwrap().reward
                );

                // Check that scores are recorded for both collators
                assert_eq!(growth.collator_scores.len(), 2usize);
            });
    }

    #[test]
    fn for_one_accumulated_period() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let collator1_stake = 20;
        let collator2_stake = 10;
        ExtBuilder::default()
            .with_balances(vec![(collator_1, 10000), (collator_2, 10000)])
            .with_candidates(vec![(collator_1, collator1_stake), (collator_2, collator2_stake)])
            .build()
            .execute_with(|| {
                let num_era_to_roll_foreward =
                    RewardPaymentDelay::get() + ErasPerGrowthPeriod::get();

                // Setup data by rolling forward and letting the system generate staking rewards.
                // This is not "faked" data.
                let raw_era_data: HashMap<EraIndex, GrowthData> = roll_foreward_and_pay_stakers(
                    num_era_to_roll_foreward,
                    collator_1,
                    collator_2,
                    collator1_stake,
                    collator2_stake,
                );

                // Verification: On era (RewardPaymentDelay + 1) we should have the first growth
                // period created, and for the next 'ErasPerGrowthPeriod' we
                // accumulate the data, thats why we rolled
                // (RewardPaymentDelay::get() + ErasPerGrowthPeriod::get()) eras.

                // Check that we have the expected number of records added
                let expected_number_of_growth_records = 1;
                assert_eq!(
                    ParachainStaking::growth_period_info().index,
                    expected_number_of_growth_records
                );

                let growth = ParachainStaking::growth(1);

                // Check that we accumulated the correct number of times
                assert_eq!(growth.number_of_accumulations, ErasPerGrowthPeriod::get());
                assert_eq!(growth.total_points, DEFAULT_POINTS * ErasPerGrowthPeriod::get() * 2); // 2 collators

                // Check that total stake matches eras 1 and 2 because payouts are delayed by 2 eras
                assert_eq!(
                    growth.total_stake_accumulated,
                    raw_era_data.get(&1).unwrap().stake + raw_era_data.get(&2).unwrap().stake
                );

                // Because `Reward` is not backdated, get the sum of reward for the current eras we
                // snapshoted this GrowthPeriod
                assert_eq!(
                    growth.total_staker_reward,
                    raw_era_data.get(&3).unwrap().reward + raw_era_data.get(&4).unwrap().reward
                );
            });
    }

    #[test]
    fn for_multiple_periods() {
        let collator_1 = to_acc_id(1u64);
        let collator_2 = to_acc_id(2u64);
        let collator1_stake = 20;
        let collator2_stake = 10;
        let reward_payment_delay = RewardPaymentDelay::get();
        ExtBuilder::default()
            .with_balances(vec![(collator_1, 10000), (collator_2, 10000)])
            .with_candidates(vec![(collator_1, collator1_stake), (collator_2, collator2_stake)])
            .build()
            .execute_with(|| {
                assert_eq!(
                    ErasPerGrowthPeriod::get(),
                    2,
                    "This test will only work if ErasPerGrowthPeriod is set to 2"
                );

                let num_era_to_roll_foreward = 16;

                // Setup data by rolling forward and letting the system generate staking rewards.
                // This is not "faked" data.
                let raw_era_data: HashMap<EraIndex, GrowthData> = roll_foreward_and_pay_stakers(
                    num_era_to_roll_foreward,
                    collator_1,
                    collator_2,
                    collator1_stake,
                    collator2_stake,
                );

                // Check that we have the expected number of records added
                let expected_number_of_growth_records =
                    (raw_era_data.len() as u32 - reward_payment_delay) / reward_payment_delay;
                assert_eq!(
                    ParachainStaking::growth_period_info().index,
                    expected_number_of_growth_records
                );

                // payout era is always 'RewardPaymentDelay' (in this case 2 eras) behind the
                // current era.
                let mut payout_era = (1, 2);

                // current era starts at the actual era this growth period has been created (3) and
                // ends at the last era accumulated by this growth period (4)
                let mut current_era = (3, 4);

                for n in 1..=expected_number_of_growth_records {
                    let growth = ParachainStaking::growth(n);

                    assert_eq!(
                        growth.number_of_accumulations,
                        ErasPerGrowthPeriod::get(),
                        "total stake for n={} does not match expected value",
                        n
                    );

                    assert_eq!(
                        growth.total_points,
                        DEFAULT_POINTS * 2 * ErasPerGrowthPeriod::get(), // 2 collators
                        "total points for n={} does not match expected value",
                        n
                    );

                    // This assumes that ErasPerGrowthPeriod = 2
                    assert_eq!(
                        growth.total_stake_accumulated,
                        raw_era_data.get(&payout_era.0).unwrap().stake
                            + raw_era_data.get(&payout_era.1).unwrap().stake,
                        "total accumulation for n={} does not match expected value",
                        n
                    );

                    // This is a bit tricky because `Reward` is not backdated. We pay whatever was
                    // in the reward pot at the time of payout therefore it is the
                    // sum of rewards for the non backdated eras that make up this GrowthPeriod)
                    assert_eq!(
                        growth.total_staker_reward,
                        raw_era_data.get(&current_era.0).unwrap().reward
                            + raw_era_data.get(&current_era.1).unwrap().reward,
                        "total reward for n={} does not match expected value",
                        n
                    );

                    // Check that scores are recorded for both collators
                    assert_eq!(growth.collator_scores.len(), 2usize);

                    payout_era = (payout_era.1 + 1, payout_era.1 + 2);
                    current_era = (current_era.1 + 1, current_era.1 + 2);
                }
            });
    }
}

mod growth_amount {
    use sp_core::ConstU32;
    use sp_runtime::BoundedVec;

    use super::*;

    const PERIOD_INDEX: u32 = 1;
    const TOTAL_STAKE: u128 = 50;
    const TOTAL_REWARD: u128 = 100;
    const COLLATOR_BALANCE: u128 = 100;
    const COLLATOR1_POINTS: u32 = 20;
    const COLLATOR2_POINTS: u32 = 10;
    // const CollatorMaxScores: ConstU32<10000> = 10000;

    fn set_growth_data(
        total_staked: u128,
        staking_reward: u128,
        total_points: u32,
        collator_scores: BoundedVec<CollatorScore<AccountId>, ConstU32<10000>>,
    ) {
        <GrowthPeriod<Test>>::put(GrowthPeriodInfo { start_era_index: 1, index: 1 });

        let mut new_payout_info = GrowthInfo::new(1);
        new_payout_info.number_of_accumulations = 2u32;
        new_payout_info.total_stake_accumulated = total_staked;
        new_payout_info.total_staker_reward = staking_reward;
        new_payout_info.total_points = total_points;
        new_payout_info.collator_scores = collator_scores;

        <Growth<Test>>::insert(1, new_payout_info);
    }

    mod is_paid_correctly {
        use super::*;

        #[test]
        fn with_good_values() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let previous_collator_3 = to_acc_id(3u64);
            let previous_collator3_points = 10;
            let total_points = COLLATOR1_POINTS + previous_collator3_points;
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, COLLATOR_BALANCE),
                    (collator_2, COLLATOR_BALANCE),
                    (previous_collator_3, COLLATOR_BALANCE),
                ])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    set_growth_data(
                        TOTAL_STAKE,
                        TOTAL_REWARD,
                        total_points,
                        BoundedVec::truncate_from(vec![
                            CollatorScore::new(collator_1, COLLATOR1_POINTS),
                            CollatorScore::new(previous_collator_3, previous_collator3_points),
                        ]),
                    );

                    // Initial state
                    assert_eq!(Balances::free_balance(&collator_1), COLLATOR_BALANCE);
                    assert_eq!(Balances::free_balance(&collator_2), COLLATOR_BALANCE);
                    assert_eq!(Balances::free_balance(&previous_collator_3), COLLATOR_BALANCE);
                    assert_eq!(true, <Growth<Test>>::contains_key(PERIOD_INDEX));
                    assert_eq!(false, <ProcessedGrowthPeriods<Test>>::contains_key(PERIOD_INDEX));

                    let payout_amount = 1111111111111111111;
                    let current_total_issuance = pallet_balances::Pallet::<Test>::total_issuance();

                    assert_ok!(ParachainStaking::payout_collators(payout_amount, PERIOD_INDEX));

                    // Collator 1 should get 2/3 of the lifted amount because they have 20 points
                    // (2/3 of total_points)
                    let expected_collator_1_payment =
                        Perbill::from_rational::<u32>(2, 3) * payout_amount;
                    assert_eq!(
                        Balances::free_balance(&collator_1),
                        COLLATOR_BALANCE + expected_collator_1_payment
                    );

                    // Collator 2's balance did not change, even though they are a "current"
                    // collator
                    assert_eq!(Balances::free_balance(&collator_2), COLLATOR_BALANCE);

                    // Previous Collator 3 should get 1/3 of the lifted amount because they have 10
                    // points (1/3 of total_points)
                    let expected_previous_collator_3_payment =
                        Perbill::from_rational::<u32>(1, 3) * payout_amount;
                    assert_eq!(
                        Balances::free_balance(&previous_collator_3),
                        COLLATOR_BALANCE + expected_previous_collator_3_payment
                    );

                    // Check correct events emitted
                    assert_event_emitted!(Event::CollatorPaid {
                        account: collator_1,
                        amount: expected_collator_1_payment,
                        period: PERIOD_INDEX,
                    });

                    assert_event_emitted!(Event::CollatorPaid {
                        account: previous_collator_3,
                        amount: expected_previous_collator_3_payment,
                        period: PERIOD_INDEX,
                    });

                    // Check processed growth has been removed to reduce state size
                    assert_eq!(false, <Growth<Test>>::contains_key(PERIOD_INDEX));

                    // Check processed growths has been updated
                    assert_eq!(true, <ProcessedGrowthPeriods<Test>>::contains_key(PERIOD_INDEX));

                    // Check total issuance has increased by the full amount lifted - this is very
                    // important
                    assert_eq!(
                        pallet_balances::Pallet::<Test>::total_issuance(),
                        current_total_issuance + payout_amount
                    );
                });
        }

        #[test]
        fn even_if_growth_info_is_not_found() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let collator_3 = to_acc_id(3u64);
            let collator_4 = to_acc_id(4u64);
            ExtBuilder::default()
                .with_balances(vec![
                    (collator_1, COLLATOR_BALANCE),
                    (collator_2, COLLATOR_BALANCE),
                    (collator_3, COLLATOR_BALANCE),
                    (collator_4, COLLATOR_BALANCE),
                ])
                .with_candidates(vec![
                    (collator_1, 10),
                    (collator_2, 10),
                    (collator_3, 10),
                    (collator_4, 10),
                ])
                .build()
                .execute_with(|| {
                    // Initial state
                    assert_eq!(Balances::free_balance(&collator_1), COLLATOR_BALANCE);
                    assert_eq!(Balances::free_balance(&collator_2), COLLATOR_BALANCE);
                    assert_eq!(Balances::free_balance(&collator_3), COLLATOR_BALANCE);
                    assert_eq!(Balances::free_balance(&collator_4), COLLATOR_BALANCE);

                    //There is no growth record
                    assert_eq!(false, <Growth<Test>>::contains_key(PERIOD_INDEX));
                    assert_eq!(false, <ProcessedGrowthPeriods<Test>>::contains_key(PERIOD_INDEX));

                    let payout_amount = 33333333333333;
                    let current_total_issuance = pallet_balances::Pallet::<Test>::total_issuance();

                    assert_ok!(ParachainStaking::payout_collators(payout_amount, PERIOD_INDEX));

                    // Each collator gets the same share because we have no way of knowing how many
                    // points they earned (400 / 4)
                    let expected_collator_payment =
                        Perbill::from_rational::<u32>(1, 4) * payout_amount;
                    assert_eq!(
                        Balances::free_balance(&collator_1),
                        COLLATOR_BALANCE + expected_collator_payment
                    );
                    assert_eq!(
                        Balances::free_balance(&collator_2),
                        COLLATOR_BALANCE + expected_collator_payment
                    );
                    assert_eq!(
                        Balances::free_balance(&collator_3),
                        COLLATOR_BALANCE + expected_collator_payment
                    );
                    assert_eq!(
                        Balances::free_balance(&collator_4),
                        COLLATOR_BALANCE + expected_collator_payment
                    );

                    // Check correct events emitted
                    assert_event_emitted!(Event::CollatorPaid {
                        account: collator_1,
                        amount: expected_collator_payment,
                        period: PERIOD_INDEX,
                    });

                    assert_event_emitted!(Event::CollatorPaid {
                        account: collator_2,
                        amount: expected_collator_payment,
                        period: PERIOD_INDEX,
                    });

                    assert_event_emitted!(Event::CollatorPaid {
                        account: collator_3,
                        amount: expected_collator_payment,
                        period: PERIOD_INDEX,
                    });

                    assert_event_emitted!(Event::CollatorPaid {
                        account: collator_4,
                        amount: expected_collator_payment,
                        period: PERIOD_INDEX,
                    });

                    // Check processed growths has been updated
                    assert_eq!(true, <ProcessedGrowthPeriods<Test>>::contains_key(PERIOD_INDEX));

                    // Check total issuance has increased by the full amount lifted - this is very
                    // important
                    assert_eq!(
                        pallet_balances::Pallet::<Test>::total_issuance(),
                        current_total_issuance + payout_amount
                    );
                });
        }
    }

    mod fails_to_be_paid {
        use super::*;

        #[test]
        fn when_payment_is_replayed() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let total_points = COLLATOR1_POINTS + COLLATOR2_POINTS;
            ExtBuilder::default()
                .with_balances(vec![(collator_1, COLLATOR_BALANCE), (collator_2, COLLATOR_BALANCE)])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    set_growth_data(
                        TOTAL_STAKE,
                        TOTAL_REWARD,
                        total_points,
                        BoundedVec::truncate_from(vec![
                            CollatorScore::new(collator_1, COLLATOR1_POINTS),
                            CollatorScore::new(collator_2, COLLATOR2_POINTS),
                        ]),
                    );

                    assert_eq!(true, <Growth<Test>>::contains_key(PERIOD_INDEX));
                    assert_eq!(false, <ProcessedGrowthPeriods<Test>>::contains_key(PERIOD_INDEX));

                    let amount = 300;
                    assert_ok!(ParachainStaking::payout_collators(amount, PERIOD_INDEX));

                    // Second attempt to pay fails
                    assert_noop!(
                        ParachainStaking::payout_collators(amount, PERIOD_INDEX),
                        Error::<Test>::GrowthAlreadyProcessed
                    );
                });
        }

        #[test]
        fn when_payment_overflows() {
            let collator_1 = to_acc_id(1u64);
            let collator_2 = to_acc_id(2u64);
            let collator_balance = u128::max_value() / 2;
            let total_points = COLLATOR1_POINTS + COLLATOR2_POINTS;
            ExtBuilder::default()
                .with_balances(vec![(collator_1, collator_balance), (collator_2, collator_balance)])
                .with_candidates(vec![(collator_1, 10), (collator_2, 10)])
                .build()
                .execute_with(|| {
                    set_growth_data(
                        TOTAL_STAKE,
                        TOTAL_REWARD,
                        total_points,
                        BoundedVec::truncate_from(vec![
                            CollatorScore::new(collator_1, COLLATOR1_POINTS),
                            CollatorScore::new(collator_2, COLLATOR2_POINTS),
                        ]),
                    );

                    assert_eq!(true, <Growth<Test>>::contains_key(PERIOD_INDEX));
                    assert_eq!(false, <ProcessedGrowthPeriods<Test>>::contains_key(PERIOD_INDEX));

                    let amount = u128::max_value();
                    // Payout fails due to overflow
                    assert_noop!(
                        ParachainStaking::payout_collators(amount, PERIOD_INDEX),
                        Error::<Test>::ErrorPayingCollator
                    );
                });
        }
    }
}
