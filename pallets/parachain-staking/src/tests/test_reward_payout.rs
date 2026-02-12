#[cfg(test)]
use crate::mock::{
    pay_gas_for_transaction, roll_one_block, roll_to_era_begin, set_author, AccountId, Balances,
    ExtBuilder, ParachainStaking, TestAccount, BASE_FEE, TX_LEN,
};
use crate::{assert_eq_events, assert_event_emitted, Event};
use frame_support::traits::Currency;
use sp_runtime::{traits::Zero, Perbill};

fn collator_1() -> AccountId {
    return TestAccount::new(1u64).account_id();
}

fn collator_2() -> AccountId {
    return TestAccount::new(2u64).account_id();
}

fn tx_sender() -> AccountId {
    return TestAccount::new(3u64).account_id();
}

fn nominator() -> AccountId {
    return TestAccount::new(4u64).account_id();
}

const ERA_BLOCKS_HAVE_BEEN_AUTHORED: u32 = 1;
const TIP: u128 = 5;
const COLLATOR1_POINTS: u32 = 4;
const COLLATOR2_POINTS: u32 = 2;
const NOMINATOR4_STAKE: u128 = 500;
const COLLATOR1_OWN_STAKE: u128 = 1000;
const COLLATOR1_TOTAL_STAKE: u128 = COLLATOR1_OWN_STAKE + NOMINATOR4_STAKE;
const COLLATOR2_OWN_STAKE: u128 = 500;
const TOTAL_STAKE: u128 = COLLATOR1_OWN_STAKE + COLLATOR2_OWN_STAKE + NOMINATOR4_STAKE;
const TOTAL_POINTS_FOR_ERA: u32 = COLLATOR1_POINTS + COLLATOR2_POINTS;

fn expected_tx_fee() -> u128 {
    return (BASE_FEE + TX_LEN as u64) as u128;
}

#[test]
fn end_to_end_happy_path() {
    let reward_pot_account_id = ParachainStaking::compute_reward_pot_account_id();
    let collator_1 = collator_1();
    let collator_2 = collator_2();

    ExtBuilder::default()
        .with_balances(vec![
            (collator_1, 10000),
            (collator_2, 10000),
            (tx_sender(), 10000),
            (nominator(), 10000),
        ])
        .with_candidates(vec![(collator_1, COLLATOR1_OWN_STAKE), (collator_2, COLLATOR2_OWN_STAKE)])
        .with_nominations(vec![(nominator(), collator_1, NOMINATOR4_STAKE)])
        .build()
        .execute_with(|| {
            // Move to the begining of era 2.
            roll_to_era_begin(2);

            // To earn rewards:
            //   - Collators have to earn points for producing blocks
            //   - Reward pot must have some funds
            //   - 2 eras must have passed

            // Show that reward pot is empty
            assert_eq!(Balances::free_balance(&reward_pot_account_id), 0);
            assert_eq!(ParachainStaking::locked_era_payout(), 0);

            // Show a list of events we expect when rolling to era 2. Note the absence of rewards.
            let expected_events = vec![
                Event::CollatorChosen {
                    era: 2,
                    collator_account: collator_1,
                    total_exposed_amount: COLLATOR1_TOTAL_STAKE,
                },
                Event::CollatorChosen {
                    era: 2,
                    collator_account: collator_2,
                    total_exposed_amount: COLLATOR2_OWN_STAKE,
                },
                Event::NewEra {
                    starting_block: 5,
                    era: 2,
                    selected_collators_number: 2,
                    total_balance: TOTAL_STAKE,
                },
            ];
            assert_eq_events!(expected_events);

            // We now set the conditions to start generating rewards
            //-------------------------------------------------------

            // Sending a transaction (with tip) to generate income for the chain
            pay_gas_for_transaction(&tx_sender(), TIP);

            let reward_pot_balance_before_reward_payout =
                Balances::free_balance(&reward_pot_account_id);

            // Show that transaction fee + tip make up an income
            assert_eq!(reward_pot_balance_before_reward_payout, expected_tx_fee() + TIP);

            // Assign block author points to collators 1 & 2.
            // Because it takes 2 eras before we can pay collators, we set the block authorship
            // points for era 1.
            set_author(ERA_BLOCKS_HAVE_BEEN_AUTHORED, collator_1, COLLATOR1_POINTS);
            set_author(ERA_BLOCKS_HAVE_BEEN_AUTHORED, collator_2, COLLATOR2_POINTS);

            // We expect reward payouts on era 3 because all 3 conditions for earning rewards are
            // met.
            roll_to_era_begin(3);

            // We now do the relevant checks
            //-------------------------------------------------------

            let collator1_points_percentage =
                Perbill::from_rational(COLLATOR1_POINTS, TOTAL_POINTS_FOR_ERA);
            let collator1_total_reward =
                collator1_points_percentage * reward_pot_balance_before_reward_payout;
            let expected_collator1_reward =
                (collator1_total_reward * COLLATOR1_OWN_STAKE) / COLLATOR1_TOTAL_STAKE;
            let expected_nominator_reward =
                (collator1_total_reward * NOMINATOR4_STAKE) / COLLATOR1_TOTAL_STAKE;

            assert_event_emitted!(Event::Rewarded {
                account: collator_1,
                rewards: expected_collator1_reward
            });
            assert_event_emitted!(Event::Rewarded {
                account: nominator(),
                rewards: expected_nominator_reward
            });

            // Show that reward pot balance has decreased by "total reward payment amount"
            assert_eq!(
                Balances::free_balance(&reward_pot_account_id),
                reward_pot_balance_before_reward_payout
                    - (expected_collator1_reward + expected_nominator_reward)
            );

            // Show that locked era payout has reserved enough to pay collator2
            assert_eq!(
                ParachainStaking::locked_era_payout(),
                reward_pot_balance_before_reward_payout
                    - (expected_collator1_reward + expected_nominator_reward)
            );

            // Move to the next block to trigger paying out the next collator
            roll_one_block();

            let collator2_points_percentage =
                Perbill::from_rational(COLLATOR2_POINTS, TOTAL_POINTS_FOR_ERA);
            let expected_collator2_reward =
                collator2_points_percentage * reward_pot_balance_before_reward_payout;
            assert_event_emitted!(Event::Rewarded {
                account: collator_2,
                rewards: expected_collator2_reward
            });

            // Show that reward pot balance and locked era balance are 0 because everything has been
            // paid out for all collators
            assert_eq!(Balances::free_balance(&reward_pot_account_id), 0);
            assert_eq!(ParachainStaking::locked_era_payout(), 0);

            // check that distributing rewards clears awarded points
            assert!(
                ParachainStaking::awarded_pts(ERA_BLOCKS_HAVE_BEEN_AUTHORED, collator_1).is_zero()
            );
        });
}

// This function will setup the payments so both collators get the same reward
fn set_reward_pot_and_trigger_payout(block_author_era: u32, destination_era: u64) -> (u128, u128) {
    pay_gas_for_transaction(&tx_sender(), TIP);

    set_author(block_author_era, collator_1(), COLLATOR1_POINTS);
    set_author(block_author_era, collator_2(), COLLATOR2_POINTS);

    roll_to_era_begin(destination_era);

    let expected_total_reward = expected_tx_fee() + TIP;

    let collator1_points_percentage =
        Perbill::from_rational(COLLATOR1_POINTS, TOTAL_POINTS_FOR_ERA);
    let collator1_total_reward = collator1_points_percentage * expected_total_reward;

    let collator2_points_percentage =
        Perbill::from_rational(COLLATOR2_POINTS, TOTAL_POINTS_FOR_ERA);
    let collator2_total_reward = collator2_points_percentage * expected_total_reward;

    return (collator1_total_reward, collator2_total_reward);
}

mod compute_total_reward_to_pay {
    use super::*;

    #[test]
    fn works_as_expected() {
        let reward_pot_account_id = ParachainStaking::compute_reward_pot_account_id();
        let collator_1 = collator_1();
        let collator_2 = collator_2();

        ExtBuilder::default()
            .with_balances(vec![(collator_1, 10000), (collator_2, 10000), (tx_sender(), 10000)])
            .with_candidates(vec![
                (collator_1, COLLATOR1_OWN_STAKE),
                (collator_2, COLLATOR2_OWN_STAKE),
            ])
            .build()
            .execute_with(|| {
                roll_to_era_begin(2);

                // Initial state
                assert_eq!(ParachainStaking::locked_era_payout(), 0);

                let expected_total_reward = expected_tx_fee() + TIP;
                let (collator1_reward, collator2_reward) =
                    set_reward_pot_and_trigger_payout(ERA_BLOCKS_HAVE_BEEN_AUTHORED, 3);

                // Reward pot balance has decreased
                assert_eq!(
                    Balances::free_balance(&reward_pot_account_id),
                    expected_total_reward - collator1_reward
                );

                // Locked era payout has reserved enough to pay remaining collators
                assert_eq!(ParachainStaking::locked_era_payout(), collator2_reward);

                // Send another transaction and generate income
                pay_gas_for_transaction(&tx_sender(), TIP * 2);

                // Show that reward pot balance has increased due to the new transaction paying a
                // fee
                assert_eq!(
                    Balances::free_balance(&reward_pot_account_id),
                    collator2_reward + expected_tx_fee() + TIP * 2
                );

                // Locked era payout did not change
                assert_eq!(ParachainStaking::locked_era_payout(), collator2_reward);

                // Compute new reward to distribute. This will lock the new amount.
                let new_total_reward_to_pay = ParachainStaking::compute_total_reward_to_pay();

                // Locked era payout has increased
                assert_eq!(
                    ParachainStaking::locked_era_payout(),
                    collator2_reward + expected_tx_fee() + TIP * 2
                );

                // We can only pay out new income. collator2_reward has already been locked for
                // payment
                assert_eq!(new_total_reward_to_pay, expected_tx_fee() + TIP * 2);
            });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn locked_payout_is_greater_than_total_income() {
            let reward_pot_account_id = ParachainStaking::compute_reward_pot_account_id();
            let collator_1 = collator_1();
            let collator_2 = collator_2();

            ExtBuilder::default()
                .with_balances(vec![(collator_1, 10000), (collator_2, 10000), (tx_sender(), 10000)])
                .with_candidates(vec![
                    (collator_1, COLLATOR1_OWN_STAKE),
                    (collator_2, COLLATOR2_OWN_STAKE),
                ])
                .build()
                .execute_with(|| {
                    roll_to_era_begin(2);

                    // Initial state
                    assert_eq!(ParachainStaking::locked_era_payout(), 0);

                    let (_collator1_reward, collator2_reward) =
                        set_reward_pot_and_trigger_payout(ERA_BLOCKS_HAVE_BEEN_AUTHORED, 3);

                    // Locked era payout and reward pot have the same amount
                    assert_eq!(
                        ParachainStaking::locked_era_payout(),
                        Balances::free_balance(&reward_pot_account_id)
                    );

                    // Reduce reward pot balance
                    let bad_reward_pot_balance = collator2_reward - 1;
                    Balances::make_free_balance_be(&reward_pot_account_id, bad_reward_pot_balance);

                    // attempt to compute total reward to pay
                    let new_total_reward_to_pay = ParachainStaking::compute_total_reward_to_pay();

                    // due to underflow, we return 0 and emit an event
                    assert_eq!(new_total_reward_to_pay, 0);
                    assert_event_emitted!(Event::NotEnoughFundsForEraPayment {
                        reward_pot_balance: bad_reward_pot_balance
                    });
                });
        }
    }
}

mod reward_payout_fails_when {
    use super::*;

    #[test]
    fn reward_pot_does_not_have_enough_funds() {
        let reward_pot_account_id = ParachainStaking::compute_reward_pot_account_id();
        let collator_1 = collator_1();
        let collator_2 = collator_2();

        ExtBuilder::default()
            .with_balances(vec![(collator_1, 10000), (collator_2, 10000), (tx_sender(), 10000)])
            .with_candidates(vec![
                (collator_1, COLLATOR1_OWN_STAKE),
                (collator_2, COLLATOR2_OWN_STAKE),
            ])
            .build()
            .execute_with(|| {
                roll_to_era_begin(2);

                // Initial state
                assert_eq!(ParachainStaking::locked_era_payout(), 0);

                let (_collator1_reward, collator2_reward) =
                    set_reward_pot_and_trigger_payout(ERA_BLOCKS_HAVE_BEEN_AUTHORED, 3);

                // Reward pot balance has reserved enough to pay collator 2
                assert_eq!(Balances::free_balance(&reward_pot_account_id), collator2_reward);
                // Locked era payout matches reward pot balance
                assert_eq!(ParachainStaking::locked_era_payout(), collator2_reward);

                // Reduce reward pot balance
                let bad_reward_pot_balance = collator2_reward - 1;
                Balances::make_free_balance_be(&reward_pot_account_id, bad_reward_pot_balance);

                // Move to the next block to trigger paying out the next collator
                roll_one_block();

                // Payment fails
                assert_event_emitted!(Event::ErrorPayingStakingReward {
                    payee: collator_2,
                    rewards: collator2_reward,
                });

                // reward pot balance didn't change
                assert_eq!(Balances::free_balance(&reward_pot_account_id), bad_reward_pot_balance);
                // locked era balance did not change
                assert_eq!(ParachainStaking::locked_era_payout(), collator2_reward);
            });
    }
}
