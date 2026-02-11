//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::common::*;
use crate::extension_builder::ExtBuilder;
use crate::mock::Call as MockCall;
use crate::mock::Event as MockEvent;
use crate::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use pallet_balances::Error as BalancesError;
use sp_runtime::DispatchError::BadOrigin;

mod proxy_signed_withdraw_unbonded {
    use super::*;

    #[derive(Clone)]
    struct WithdrawUnbondedContext {
        origin: Origin,
        staker: Staker,
        bond_value: BalanceOf<TestRuntime>,
        unbonded_value: BalanceOf<TestRuntime>,
        num_slashing_spans: u32,
    }

    impl Default for WithdrawUnbondedContext {
        fn default() -> Self {
            let staker: Staker = Default::default();
            WithdrawUnbondedContext {
                origin: Origin::signed(staker.relayer),
                staker,
                bond_value: MinUserBond::<TestRuntime>::get() * 2,
                unbonded_value: MinUserBond::<TestRuntime>::get(),
                num_slashing_spans: 1,
            }
        }
    }

    impl WithdrawUnbondedContext {
        fn setup(&self) {
            let stash = self.staker.stash.account_id();
            let controller = self.staker.controller.account_id();

            // Set staker balance to bond value
            Balances::make_free_balance_be(&stash, self.bond_value);

            // Bond all tokens
            assert_ok!(ValidatorManager::bond(
                Origin::signed(stash),
                controller,
                self.bond_value,
                RewardDestination::Stash
            ));

            // Unbond some tokens
            let nonce = ValidatorManager::proxy_nonce(controller);
            let unbond_call = self.create_call_for_unbond(nonce);
            assert_ok!(AvnProxy::proxy(self.origin.clone(), unbond_call, None));
        }

        fn create_call_for_unbond(&self, sender_nonce: u64) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_unbond(sender_nonce);

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_unbond(proof, self.unbonded_value),
            ));
        }

        fn create_proof_for_signed_unbond(&self, sender_nonce: u64) -> Proof<Signature, AccountId> {
            let controller_account_id = &self.staker.controller.account_id();

            let data_to_sign = encode_signed_unbond_params::<TestRuntime>(
                &get_partial_proof(controller_account_id, &self.staker.relayer),
                &self.unbonded_value,
                sender_nonce,
            );

            let signature = sign(&self.staker.controller_key_pair, &data_to_sign);
            return build_proof(controller_account_id, &self.staker.relayer, signature);
        }

        fn create_call_for_withdraw_unbonded(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_withdraw_unbonded(sender_nonce);

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_withdraw_unbonded(
                    proof,
                    self.num_slashing_spans,
                ),
            ));
        }

        fn create_call_for_withdraw_unbonded_approved_by_relayer(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let mut proof = self.create_proof_for_signed_withdraw_unbonded(sender_nonce);
            proof.signer = self.staker.relayer;

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_withdraw_unbonded(
                    proof,
                    self.num_slashing_spans,
                ),
            ));
        }

        fn create_proof_for_signed_withdraw_unbonded(
            &self,
            sender_nonce: u64,
        ) -> Proof<Signature, AccountId> {
            let controller_account_id = &self.staker.controller.account_id();

            let data_to_sign = encode_signed_withdraw_unbonded_params::<TestRuntime>(
                &get_partial_proof(controller_account_id, &self.staker.relayer),
                &self.num_slashing_spans,
                sender_nonce,
            );

            let signature = sign(&self.staker.controller_key_pair, &data_to_sign);
            return build_proof(controller_account_id, &self.staker.relayer, signature);
        }

        pub fn withdrawn_event_emitted(&self) -> bool {
            return System::events().iter().any(|e| {
                e.event
                    == MockEvent::pallet_staking(
                        crate::mock::staking::Event::<TestRuntime>::Withdrawn(
                            self.staker.stash.account_id(),
                            self.unbonded_value,
                        ),
                    )
            });
        }
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = &WithdrawUnbondedContext::default();
            context.setup();

            let stash_account_id = &context.staker.stash.account_id();
            let controller_account_id = &context.staker.controller.account_id();

            let nonce = ValidatorManager::proxy_nonce(controller_account_id);
            let withdraw_unbonded_call = context.create_call_for_withdraw_unbonded(nonce);

            // Prior to withdraw_unbonded check that the staker is bonded
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

            // The ledger has an decreased active amount after unbond
            assert_eq!(
                Staking::ledger(&controller_account_id).unwrap().active,
                context.bond_value - context.unbonded_value
            );

            // The ledger updated the unlocking
            assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 1);

            // Still cannot spend the unbonded tokens
            assert_eq!(Balances::usable_balance(*stash_account_id), 0);
            assert_noop!(
                Balances::transfer(Origin::signed(*stash_account_id), context.staker.relayer, 1),
                BalancesError::<TestRuntime>::LiquidityRestrictions
            );

            advance_era_to_pass_bonding_period();

            assert_ok!(AvnProxy::proxy(context.origin.clone(), withdraw_unbonded_call, None));

            // Event is emitted
            assert!(context.withdrawn_event_emitted());

            // Proxy nonce has increased
            assert_eq!(ValidatorManager::proxy_nonce(controller_account_id), nonce + 1);

            // The staker is still bonded.
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

            // Both total amount and active amount in the ledger were decreased by the withdraw_unbonded amount
            assert_eq!(
                Staking::ledger(&controller_account_id).unwrap().total,
                context.bond_value - context.unbonded_value
            );
            assert_eq!(
                Staking::ledger(&controller_account_id).unwrap().active,
                context.bond_value - context.unbonded_value
            );

            // The unlocking list in ledger became empty
            assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 0);

            // Free balance is not affected
            assert_eq!(Balances::free_balance(*stash_account_id), context.bond_value);

            // The unbonded value is unlocked
            assert_eq!(Balances::usable_balance(*stash_account_id), context.unbonded_value);
            assert_eq!(
                System::account(stash_account_id).data.misc_frozen,
                context.bond_value - context.unbonded_value
            );
            assert_eq!(
                System::account(stash_account_id).data.fee_frozen,
                context.bond_value - context.unbonded_value
            );

            // Transfer more than unbonded tokens will fail
            assert_noop!(
                Balances::transfer(
                    Origin::signed(*stash_account_id),
                    context.staker.relayer,
                    context.unbonded_value + 1
                ),
                BalancesError::<TestRuntime>::LiquidityRestrictions
            );

            // Transfer equal or less than the unlocked withdrawn unboned amount is successful
            assert_ok!(Balances::transfer(
                Origin::signed(*stash_account_id),
                context.staker.relayer,
                context.unbonded_value
            ));
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &WithdrawUnbondedContext::default();
                context.setup();

                let stash_account_id = &context.staker.stash.account_id();
                let controller_account_id = &context.staker.controller.account_id();

                let nonce = ValidatorManager::proxy_nonce(controller_account_id);
                let withdraw_unbonded_call = context.create_call_for_withdraw_unbonded(nonce);

                // Prior to withdraw_unbonded check that the staker is bonded
                assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

                // The ledger has an decreased active amount after unbond
                assert_eq!(
                    Staking::ledger(&controller_account_id).unwrap().active,
                    context.bond_value - context.unbonded_value
                );

                // The ledger updated the unlocking
                assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 1);

                // Still cannot spend the unbonded tokens
                assert_eq!(Balances::usable_balance(*stash_account_id), 0);
                assert_noop!(
                    Balances::transfer(
                        Origin::signed(*stash_account_id),
                        context.staker.relayer,
                        1
                    ),
                    BalancesError::<TestRuntime>::LiquidityRestrictions
                );

                advance_era_to_pass_bonding_period();

                assert_noop!(
                    AvnProxy::proxy(RawOrigin::None.into(), withdraw_unbonded_call, None),
                    BadOrigin
                );
            });
        }

        // We don't need to test SenderIsNotSigner error through AvnProxy::proxy call
        // as it always uses the proof.signer as the sender

        #[test]
        fn withdraw_unbonded_call_is_unauthorized() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &WithdrawUnbondedContext::default();
                context.setup();

                let stash_account_id = &context.staker.stash.account_id();
                let controller_account_id = &context.staker.controller.account_id();

                let nonce = ValidatorManager::proxy_nonce(controller_account_id);
                let withdraw_unbonded_call =
                    context.create_call_for_withdraw_unbonded_approved_by_relayer(nonce);

                // Prior to withdraw_unbonded check that the staker is bonded
                assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

                // The ledger has an decreased active amount after unbond
                assert_eq!(
                    Staking::ledger(&controller_account_id).unwrap().active,
                    context.bond_value - context.unbonded_value
                );

                // The ledger updated the unlocking
                assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 1);

                // Still cannot spend the unbonded tokens
                assert_eq!(Balances::usable_balance(*stash_account_id), 0);
                assert_noop!(
                    Balances::transfer(
                        Origin::signed(*stash_account_id),
                        context.staker.relayer,
                        1
                    ),
                    BalancesError::<TestRuntime>::LiquidityRestrictions
                );

                advance_era_to_pass_bonding_period();

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), withdraw_unbonded_call, None),
                    Error::<TestRuntime>::UnauthorizedSignedWithdrawUnbondedTransaction
                );
            });
        }
    }
}
