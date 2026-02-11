//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::common::*;
use crate::extension_builder::ExtBuilder;
use crate::mock::staking::Exposure;
use crate::mock::Call as MockCall;
use crate::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use sp_runtime::{assert_eq_error_rate, DispatchError::BadOrigin, Perbill};

mod proxy_signed_nominate {
    use super::*;

    #[derive(Clone)]
    struct NominateContext {
        origin: Origin,
        staker: Staker,
        amount: BalanceOf<TestRuntime>,
        targets: Vec<<<TestRuntime as system::Config>::Lookup as StaticLookup>::Source>,
    }

    impl Default for NominateContext {
        fn default() -> Self {
            let staker: Staker = Default::default();
            NominateContext {
                origin: Origin::signed(staker.relayer),
                staker,
                targets: genesis_config_initial_validators().to_vec(),
                amount: MinUserBond::<TestRuntime>::get(),
            }
        }
    }

    impl NominateContext {
        fn setup(&self) {
            let stash = self.staker.stash.account_id();
            let controller = self.staker.controller.account_id();

            Balances::make_free_balance_be(&stash, self.amount);
            assert_ok!(ValidatorManager::bond(
                Origin::signed(stash),
                controller,
                self.amount,
                RewardDestination::Stash
            ));
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

        pub fn nominated_event_emitted(&self) -> bool {
            return System::events().iter().any(|e| {
                e.event
                    == mock::Event::ValidatorManager(crate::Event::<TestRuntime>::Nominated(
                        self.staker.stash.account_id(),
                        self.amount,
                        self.targets.len() as u32,
                    ))
            });
        }
    }

    fn sum_staker_exposure(era_index: EraIndex, staker: AccountId) -> u128 {
        let mut exposures: Vec<Exposure<AccountId, u128>> = vec![];
        exposures.push(Staking::eras_stakers(era_index, validator_id_1()));
        exposures.push(Staking::eras_stakers(era_index, validator_id_2()));
        exposures.push(Staking::eras_stakers(era_index, validator_id_3()));
        exposures.push(Staking::eras_stakers(era_index, validator_id_4()));
        exposures.push(Staking::eras_stakers(era_index, validator_id_5()));

        let mut sum = 0;
        exposures.into_iter().for_each(|e| {
            if !e.others.is_empty() {
                sum += e.others.iter().find(|o| o.who == staker).unwrap().value;
            }
        });

        return sum;
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = &NominateContext::default();
            context.setup();

            let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());
            let nominate_call = context.create_call_for_nominate(nonce);
            let stash_account_id = &context.staker.stash.account_id();

            //Prio to nominating check that the staker is not a nominator
            assert_eq!(Staking::nominators(stash_account_id), None);
            assert_ok!(AvnProxy::proxy(context.origin.clone(), nominate_call, None));

            //The staker is now a nominator
            assert_eq!(
                Staking::nominators(stash_account_id).unwrap().targets,
                genesis_config_initial_validators()
            );

            //Event is emitted
            assert!(context.nominated_event_emitted());

            let mut era_index = Staking::active_era().unwrap().index;

            // The nomination is not active yet
            let exposure = Staking::eras_stakers(era_index, validator_id_1());
            assert_eq_error_rate!(exposure.own, VALIDATOR_STAKE, 1000);
            assert_eq_error_rate!(exposure.total, VALIDATOR_STAKE, 1000);

            assert_eq!(sum_staker_exposure(era_index, *stash_account_id), 0);

            // advance the era
            advance_era();

            era_index = Staking::active_era().unwrap().index;

            // The exposure is set
            let new_exposure = Staking::eras_stakers(era_index, validator_id_2());
            assert_eq_error_rate!(new_exposure.own, VALIDATOR_STAKE, 1000);
            assert_eq_error_rate!(
                sum_staker_exposure(era_index, *stash_account_id),
                MinUserBond::<TestRuntime>::get(),
                2000
            );
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &NominateContext::default();
                context.setup();

                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());
                let nominate_call = context.create_call_for_nominate(nonce);

                assert_noop!(
                    AvnProxy::proxy(RawOrigin::None.into(), nominate_call, None),
                    BadOrigin
                );
            });
        }

        #[test]
        fn sender_is_not_controller_account() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let mut context = &mut NominateContext::default();
                context.setup();

                context.staker.controller = TestAccount::new([30u8; 32]);
                context.staker.controller_key_pair = context.staker.controller.key_pair();

                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());

                let nominate_call = context.create_call_for_nominate(nonce);

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), nominate_call, None),
                    Error::<TestRuntime>::NotController
                );
            });
        }

        #[test]
        fn sender_does_not_have_enough_fund() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &mut NominateContext::default();
                context.setup();
                let controller_account_id = context.staker.controller.account_id();

                let nonce = ValidatorManager::proxy_nonce(controller_account_id);
                let nominate_call = context.create_call_for_nominate(nonce);

                // Increased the minimum user bond, so the previously bonded amount is not valid to nominate anymore
                <MinUserBond::<TestRuntime>>::put(
                    ValidatorManager::min_user_bond() + 1,
                );

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), nominate_call, None),
                    Error::<TestRuntime>::InsufficientFundsToNominateBond
                );
            });
        }

        #[test]
        fn sender_is_already_a_validator() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &NominateContext::default();
                context.setup();
                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());

                pallet_staking::Validators::<TestRuntime>::insert(
                    &context.staker.stash.account_id(),
                    ValidatorPrefs {
                        commission: Perbill::from_percent(10).clone(),
                        blocked: false,
                    },
                );

                let nominate_call = context.create_call_for_nominate(nonce);

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), nominate_call, None),
                    Error::<TestRuntime>::AlreadyValidating
                );
            });
        }
    }
}
