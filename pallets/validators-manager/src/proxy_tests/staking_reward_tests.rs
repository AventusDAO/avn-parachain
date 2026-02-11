//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::common::*;
use crate::extension_builder::ExtBuilder;
use crate::mock::Call as MockCall;
use crate::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use pallet_staking::Error as StakingError;
use sp_runtime::{assert_eq_error_rate, DispatchError::BadOrigin};

mod proxy_signed_nominate {
    use super::*;

    #[derive(Clone)]
    struct PayoutContext {
        origin: Origin,
        staker: Staker,
        total_payout: BalanceOf<TestRuntime>,
        user_stake_amount: BalanceOf<TestRuntime>,
        targets: Vec<<<TestRuntime as system::Config>::Lookup as StaticLookup>::Source>,
    }

    impl Default for PayoutContext {
        fn default() -> Self {
            let staker: Staker = Default::default();
            PayoutContext {
                origin: Origin::signed(staker.relayer),
                staker,
                total_payout: 50_000_000_000_000_000_000,
                user_stake_amount: USER_STAKE,
                targets: genesis_config_initial_validators().to_vec(),
            }
        }
    }

    impl PayoutContext {
        fn setup_staker(&self) {
            let stash = self.staker.stash.account_id();
            let controller = self.staker.controller.account_id();

            Balances::make_free_balance_be(&stash, self.user_stake_amount);
            assert_ok!(ValidatorManager::bond(
                Origin::signed(stash),
                controller,
                self.user_stake_amount,
                RewardDestination::Stash
            ));
        }

        fn setup_nominate(&self) {
            let nonce = ValidatorManager::proxy_nonce(self.staker.controller.account_id());
            let nominate_call = self.create_call_for_nominate(nonce);

            // Nominate
            assert_ok!(AvnProxy::proxy(self.origin.clone(), nominate_call, None));
        }

        fn with_commission(&self, commission: u32) {
            update_commission(&validator_id_1(), Perbill::from_percent(commission));
            update_commission(&validator_id_2(), Perbill::from_percent(commission));
            update_commission(&validator_id_3(), Perbill::from_percent(commission));
            update_commission(&validator_id_4(), Perbill::from_percent(commission));
            update_commission(&validator_id_5(), Perbill::from_percent(commission));
        }

        fn create_call_for_payout(
            &self,
            sender_nonce: u64,
            era: EraIndex,
        ) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_payout(sender_nonce, era);
            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_payout_stakers(proof, era),
            ));
        }

        fn create_call_for_payout_approved_by_relayer(
            &self,
            sender_nonce: u64,
            era: EraIndex,
        ) -> Box<<TestRuntime as Config>::Call> {
            let mut proof = self.create_proof_for_signed_payout(sender_nonce, era);
            proof.signer = self.staker.relayer;

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_payout_stakers(proof, era),
            ));
        }

        fn create_proof_for_signed_payout(
            &self,
            sender_nonce: u64,
            era: EraIndex,
        ) -> Proof<Signature, AccountId> {
            let controller_account_id = &self.staker.controller.account_id();
            let data_to_sign = encode_signed_payout_stakers_params::<TestRuntime>(
                &get_partial_proof(controller_account_id, &self.staker.relayer),
                &era,
                sender_nonce,
            );

            let signature = sign(&self.staker.controller_key_pair, &data_to_sign);
            return build_proof(controller_account_id, &self.staker.relayer, signature);
        }

        fn create_call_for_nominate(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_nominate(sender_nonce);
            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_nominate(proof, self.targets.clone()),
            ));
        }

        fn create_proof_for_signed_nominate(
            &self,
            sender_nonce: u64,
        ) -> Proof<Signature, AccountId> {
            let controller_account_id = &self.staker.controller.account_id();
            let data_to_sign = encode_signed_nominate_params::<TestRuntime>(
                &get_partial_proof(controller_account_id, &self.staker.relayer),
                &self.targets,
                sender_nonce,
            );

            let signature = sign(&self.staker.controller_key_pair, &data_to_sign);
            return build_proof(controller_account_id, &self.staker.relayer, signature);
        }

        pub fn payout_event_emitted(&self, era_index: EraIndex) -> bool {
            return System::events().iter().any(|e| {
                e.event
                    == mock::Event::ValidatorManager(
                        crate::Event::<TestRuntime>::PayoutCompleted(
                            era_index,
                            self.targets.len() as u32,
                        ),
                    )
            });
        }
    }

    mod succeeds_when {
        use super::*;

        #[test]
        fn staker_rewards_work() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &PayoutContext::default();
                let stash_account_id = &context.staker.stash.account_id();

                context.setup_staker();
                context.setup_nominate();

                // The staker is now a nominator
                assert_eq!(
                    Staking::nominators(context.staker.stash.account_id()).unwrap().targets,
                    genesis_config_initial_validators()
                );

                // There is no payout
                assert_eq!(ValidatorManager::locked_era_payout(), 0);

                // Advance the era
                advance_era();

                // Assign equal points to all the validators
                Staking::reward_by_ids(vec![
                    (validator_id_1(), 10),
                    (validator_id_2(), 10),
                    (validator_id_3(), 10),
                    (validator_id_4(), 10),
                    (validator_id_5(), 10),
                ]);

                // Set the pot so we can payout
                Balances::make_free_balance_be(
                    &ValidatorManager::account_id(),
                    context.total_payout,
                );

                let staker_balance = Balances::total_balance(stash_account_id);

                // Advance the era to trigger payout calculation
                advance_era();

                // Show that the full amount is being paid to the stakers
                assert_eq!(ValidatorManager::locked_era_payout(), context.total_payout);

                // Payout all stakers for the previous era
                let era_index = Staking::active_era().unwrap().index;
                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());
                let payout_call = context.create_call_for_payout(nonce, era_index - 1);
                assert_ok!(AvnProxy::proxy(context.origin.clone(), payout_call, None));

                //Event is emitted
                assert!(context.payout_event_emitted(era_index - 1));

                let earning_per_validator = context.total_payout / 5;
                let staker_reward =
                    get_expected_staker_reward(era_index, validator_id_1(), earning_per_validator)
                        + get_expected_staker_reward(
                            era_index,
                            validator_id_2(),
                            earning_per_validator,
                        )
                        + get_expected_staker_reward(
                            era_index,
                            validator_id_3(),
                            earning_per_validator,
                        )
                        + get_expected_staker_reward(
                            era_index,
                            validator_id_4(),
                            earning_per_validator,
                        )
                        + get_expected_staker_reward(
                            era_index,
                            validator_id_5(),
                            earning_per_validator,
                        );

                // If there were no rounding issues, locked_era_payout would be 0 but here we allow for pico AVT errors
                assert!(ValidatorManager::locked_era_payout() <= 100000000000);
                assert_eq_error_rate!(
                    staker_balance + staker_reward,
                    Balances::total_balance(stash_account_id),
                    2
                );
            });
        }

        #[test]
        fn validator_rewards_work_without_commission() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &PayoutContext::default();

                // Advance the era
                advance_era();

                // There is no payout
                assert_eq!(ValidatorManager::locked_era_payout(), 0);

                // Set the pot so we can payout
                Balances::make_free_balance_be(
                    &ValidatorManager::account_id(),
                    context.total_payout,
                );

                // Assign equal points to all the validators
                Staking::reward_by_ids(vec![
                    (validator_id_1(), 10),
                    (validator_id_2(), 10),
                    (validator_id_3(), 10),
                    (validator_id_4(), 10),
                    (validator_id_5(), 10),
                ]);

                // Advance the era to trigger payout calculation
                advance_era();

                // Show that the full amount is being paid to the stakers
                assert_eq!(ValidatorManager::locked_era_payout(), context.total_payout);

                // Record balances before payout is triggered
                let validator1_balance_before = Balances::total_balance(&validator_id_1());
                let validator2_balance_before = Balances::total_balance(&validator_id_2());
                let validator3_balance_before = Balances::total_balance(&validator_id_3());
                let validator4_balance_before = Balances::total_balance(&validator_id_4());
                let validator5_balance_before = Balances::total_balance(&validator_id_5());

                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());

                // Payout all stakers for the previous era
                let era_index = Staking::active_era().unwrap().index;
                let payout_call = context.create_call_for_payout(nonce, era_index - 1);
                assert_ok!(AvnProxy::proxy(context.origin.clone(), payout_call, None));

                //Event is emitted
                assert!(context.payout_event_emitted(era_index - 1));

                let earning_per_validator = context.total_payout / 5;

                let validator1_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_1(),
                    earning_per_validator,
                );
                let validator2_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_2(),
                    earning_per_validator,
                );
                let validator3_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_3(),
                    earning_per_validator,
                );
                let validator4_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_4(),
                    earning_per_validator,
                );
                let validator5_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_5(),
                    earning_per_validator,
                );

                // If there were no rounding issues, locked_era_payout would be 0 but here we allow for pico AVT errors
                assert!(ValidatorManager::locked_era_payout() <= 100000000000);

                // Check the balances
                assert_eq_error_rate!(
                    validator1_balance_before + validator1_reward,
                    Balances::total_balance(&validator_id_1()),
                    2
                );
                assert_eq_error_rate!(
                    validator2_balance_before + validator2_reward,
                    Balances::total_balance(&validator_id_2()),
                    2
                );
                assert_eq_error_rate!(
                    validator3_balance_before + validator3_reward,
                    Balances::total_balance(&validator_id_3()),
                    2
                );
                assert_eq_error_rate!(
                    validator4_balance_before + validator4_reward,
                    Balances::total_balance(&validator_id_4()),
                    2
                );
                assert_eq_error_rate!(
                    validator5_balance_before + validator5_reward,
                    Balances::total_balance(&validator_id_5()),
                    2
                );
            });
        }

        #[test]
        fn validator_rewards_work_with_commission() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &PayoutContext::default();
                context.with_commission(25);

                // Advance the era
                advance_era();

                // There is no payout
                assert_eq!(ValidatorManager::locked_era_payout(), 0);

                // Set the pot so we can payout
                Balances::make_free_balance_be(
                    &ValidatorManager::account_id(),
                    context.total_payout,
                );

                // Assign equal points to all the validators
                Staking::reward_by_ids(vec![
                    (validator_id_1(), 10),
                    (validator_id_2(), 10),
                    (validator_id_3(), 10),
                    (validator_id_4(), 10),
                    (validator_id_5(), 10),
                ]);

                // Advance the era to trigger payout calculation
                advance_era();

                // Show that the full amount is being paid to the stakers
                assert_eq!(ValidatorManager::locked_era_payout(), context.total_payout);

                // Record balances before payout is triggered
                let validator1_balance_before = Balances::total_balance(&validator_id_1());
                let validator2_balance_before = Balances::total_balance(&validator_id_2());
                let validator3_balance_before = Balances::total_balance(&validator_id_3());
                let validator4_balance_before = Balances::total_balance(&validator_id_4());
                let validator5_balance_before = Balances::total_balance(&validator_id_5());

                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());

                // Payout all stakers for the previous era
                let era_index = Staking::active_era().unwrap().index;
                let payout_call = context.create_call_for_payout(nonce, era_index - 1);
                assert_ok!(AvnProxy::proxy(context.origin.clone(), payout_call, None));

                //Event is emitted
                assert!(context.payout_event_emitted(era_index - 1));

                let earning_per_validator = context.total_payout / 5;

                let validator1_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_1(),
                    earning_per_validator,
                );
                let validator2_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_2(),
                    earning_per_validator,
                );
                let validator3_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_3(),
                    earning_per_validator,
                );
                let validator4_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_4(),
                    earning_per_validator,
                );
                let validator5_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_5(),
                    earning_per_validator,
                );

                // If there were no rounding issues, locked_era_payout would be 0 but here we allow for pico AVT errors
                assert!(ValidatorManager::locked_era_payout() <= 100000000000);

                // Check the balances
                assert_eq_error_rate!(
                    validator1_balance_before + validator1_reward,
                    Balances::total_balance(&validator_id_1()),
                    2
                );
                assert_eq_error_rate!(
                    validator2_balance_before + validator2_reward,
                    Balances::total_balance(&validator_id_2()),
                    2
                );
                assert_eq_error_rate!(
                    validator3_balance_before + validator3_reward,
                    Balances::total_balance(&validator_id_3()),
                    2
                );
                assert_eq_error_rate!(
                    validator4_balance_before + validator4_reward,
                    Balances::total_balance(&validator_id_4()),
                    2
                );
                assert_eq_error_rate!(
                    validator5_balance_before + validator5_reward,
                    Balances::total_balance(&validator_id_5()),
                    2
                );
            });
        }

        #[test]
        fn validator_rewards_work_with_different_commission() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &PayoutContext::default();

                update_commission(&validator_id_1(), Perbill::from_percent(20));
                update_commission(&validator_id_2(), Perbill::from_percent(5));
                update_commission(&validator_id_3(), Perbill::from_percent(0));
                update_commission(&validator_id_4(), Perbill::from_percent(10));
                update_commission(&validator_id_5(), Perbill::from_percent(25));

                // Advance the era
                advance_era();

                // There is no payout
                assert_eq!(ValidatorManager::locked_era_payout(), 0);

                // Set the pot so we can payout
                Balances::make_free_balance_be(
                    &ValidatorManager::account_id(),
                    context.total_payout,
                );

                // Assign equal points to all the validators
                Staking::reward_by_ids(vec![
                    (validator_id_1(), 10),
                    (validator_id_2(), 10),
                    (validator_id_3(), 10),
                    (validator_id_4(), 10),
                    (validator_id_5(), 10),
                ]);

                // Advance the era to trigger payout calculation
                advance_era();

                // Show that the full amount is being paid to the stakers
                assert_eq!(ValidatorManager::locked_era_payout(), context.total_payout);

                // Record balances before payout is triggered
                let validator1_balance_before = Balances::total_balance(&validator_id_1());
                let validator2_balance_before = Balances::total_balance(&validator_id_2());
                let validator3_balance_before = Balances::total_balance(&validator_id_3());
                let validator4_balance_before = Balances::total_balance(&validator_id_4());
                let validator5_balance_before = Balances::total_balance(&validator_id_5());

                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());

                // payout all stakers for the previous era
                let era_index = Staking::active_era().unwrap().index;
                let payout_call = context.create_call_for_payout(nonce, era_index - 1);
                assert_ok!(AvnProxy::proxy(context.origin.clone(), payout_call, None));

                //Event is emitted
                assert!(context.payout_event_emitted(era_index - 1));

                let earning_per_validator = context.total_payout / 5;

                let validator1_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_1(),
                    earning_per_validator,
                );
                let validator2_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_2(),
                    earning_per_validator,
                );
                let validator3_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_3(),
                    earning_per_validator,
                );
                let validator4_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_4(),
                    earning_per_validator,
                );
                let validator5_reward = get_expected_validator_reward(
                    era_index,
                    validator_id_5(),
                    earning_per_validator,
                );

                // If there were no rounding issues, locked_era_payout would be 0 but here we allow for pico AVT errors
                assert!(ValidatorManager::locked_era_payout() <= 100000000000);

                // Check the balances
                assert_eq_error_rate!(
                    validator1_balance_before + validator1_reward,
                    Balances::total_balance(&validator_id_1()),
                    2
                );
                assert_eq_error_rate!(
                    validator2_balance_before + validator2_reward,
                    Balances::total_balance(&validator_id_2()),
                    2
                );
                assert_eq_error_rate!(
                    validator3_balance_before + validator3_reward,
                    Balances::total_balance(&validator_id_3()),
                    2
                );
                assert_eq_error_rate!(
                    validator4_balance_before + validator4_reward,
                    Balances::total_balance(&validator_id_4()),
                    2
                );
                assert_eq_error_rate!(
                    validator5_balance_before + validator5_reward,
                    Balances::total_balance(&validator_id_5()),
                    2
                );
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &PayoutContext::default();

                context.setup_staker();
                context.setup_nominate();

                // The staker is now a nominator
                assert_eq!(
                    Staking::nominators(context.staker.stash.account_id()).unwrap().targets,
                    genesis_config_initial_validators()
                );

                // There is no payout
                assert_eq!(ValidatorManager::locked_era_payout(), 0);

                // advance the era
                advance_era();

                // Assign equal points to all the validators
                Staking::reward_by_ids(vec![
                    (validator_id_1(), 10),
                    (validator_id_2(), 10),
                    (validator_id_3(), 10),
                    (validator_id_4(), 10),
                    (validator_id_5(), 10),
                ]);

                // Set the pot so we can payout
                Balances::make_free_balance_be(
                    &ValidatorManager::account_id(),
                    context.total_payout,
                );

                // advance the era to trigger payout calculation
                advance_era();

                // Show that the full amount is being paid to the stakers
                assert_eq!(ValidatorManager::locked_era_payout(), context.total_payout);

                // payout all stakers for the previous era
                let era_index = Staking::active_era().unwrap().index;
                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());
                let payout_call = context.create_call_for_payout(nonce, era_index - 1);

                assert_noop!(AvnProxy::proxy(RawOrigin::None.into(), payout_call, None), BadOrigin);
            });
        }

        // We don't need to test SenderIsNotSigner error through AvnProxy::proxy call
        // as it always uses the proof.signer as the sender

        #[test]
        fn payout_stakers_call_is_unauthorized() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &PayoutContext::default();

                context.setup_staker();
                context.setup_nominate();

                // The staker is now a nominator
                assert_eq!(
                    Staking::nominators(context.staker.stash.account_id()).unwrap().targets,
                    genesis_config_initial_validators()
                );

                // There is no payout
                assert_eq!(ValidatorManager::locked_era_payout(), 0);

                // advance the era
                advance_era();

                // Assign equal points to all the validators
                Staking::reward_by_ids(vec![
                    (validator_id_1(), 10),
                    (validator_id_2(), 10),
                    (validator_id_3(), 10),
                    (validator_id_4(), 10),
                    (validator_id_5(), 10),
                ]);

                // Set the pot so we can payout
                Balances::make_free_balance_be(
                    &ValidatorManager::account_id(),
                    context.total_payout,
                );

                // advance the era to trigger payout calculation
                advance_era();

                // Show that the full amount is being paid to the stakers
                assert_eq!(ValidatorManager::locked_era_payout(), context.total_payout);

                // payout all stakers for the previous era
                let era_index = Staking::active_era().unwrap().index;
                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());
                let payout_call =
                    context.create_call_for_payout_approved_by_relayer(nonce, era_index - 1);

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), payout_call, None),
                    Error::<TestRuntime>::UnauthorizedSignedPayoutStakersTransaction
                );
            });
        }

        #[test]
        fn era_is_invalid() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &PayoutContext::default();

                context.setup_staker();
                context.setup_nominate();

                // The staker is now a nominator
                assert_eq!(
                    Staking::nominators(context.staker.stash.account_id()).unwrap().targets,
                    genesis_config_initial_validators()
                );

                // There is no payout
                assert_eq!(ValidatorManager::locked_era_payout(), 0);

                // advance the era
                advance_era();

                // Assign equal points to all the validators
                Staking::reward_by_ids(vec![
                    (validator_id_1(), 10),
                    (validator_id_2(), 10),
                    (validator_id_3(), 10),
                    (validator_id_4(), 10),
                    (validator_id_5(), 10),
                ]);

                // Set the pot so we can payout
                Balances::make_free_balance_be(
                    &ValidatorManager::account_id(),
                    context.total_payout,
                );

                // advance the era to trigger payout calculation
                advance_era();

                // Show that the full amount is being paid to the stakers
                assert_eq!(ValidatorManager::locked_era_payout(), context.total_payout);

                // payout all stakers for the previous era
                let era_index = Staking::active_era().unwrap().index;
                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());
                let payout_call = context.create_call_for_payout(nonce, era_index);

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), payout_call, None),
                    StakingError::<TestRuntime>::InvalidEraToReward
                );
            });
        }
    }
}

// Phragmen assigns the exposure for each era and its hard to predict so instead we read them from storage
pub fn get_expected_staker_reward(
    era_index: EraIndex,
    validator: AccountId,
    total_payout: u128,
) -> u128 {
    let validator_prefs = Staking::eras_validator_prefs(&era_index, &validator);
    let exposure = Staking::eras_stakers(era_index, validator);

    let user_stake = exposure.others[0].value;

    let leftover_payout = total_payout - (validator_prefs.commission * total_payout);
    let staker_exposure_part = Perbill::from_rational_approximation(user_stake, exposure.total);
    return staker_exposure_part * leftover_payout;
}

// Phragmen assigns the exposure for each era and its hard to predict so instead we read them from storage
pub fn get_expected_validator_reward(
    era_index: EraIndex,
    validator: AccountId,
    total_payout: u128,
) -> u128 {
    let validator_prefs = Staking::eras_validator_prefs(&era_index, &validator);
    let exposure = Staking::eras_stakers(era_index, validator);

    let commission_earning = validator_prefs.commission * total_payout;
    let leftover_payout = total_payout - commission_earning;
    let exposure_part = Perbill::from_rational_approximation(exposure.own, exposure.total);
    return commission_earning + (exposure_part * leftover_payout);
}

fn update_commission(validator: &AccountId, commission: Perbill) {
    pallet_staking::Validators::<TestRuntime>::mutate(validator, |p| p.commission = commission);
}
