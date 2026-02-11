//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::common::*;
use crate::extension_builder::ExtBuilder;
use crate::mock::Call as MockCall;
use crate::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use pallet_balances::Error as BalancesError;
use sp_runtime::DispatchError::BadOrigin;

mod proxy_signed_rebond {
    use super::*;

    #[derive(Clone)]
    struct RebondContext {
        origin: Origin,
        staker: Staker,
        bond_value: BalanceOf<TestRuntime>,
        unbond_value: BalanceOf<TestRuntime>,
        rebond_value: BalanceOf<TestRuntime>,
    }

    impl Default for RebondContext {
        fn default() -> Self {
            let staker: Staker = Default::default();
            RebondContext {
                origin: Origin::signed(staker.relayer),
                staker,
                bond_value: MinUserBond::<TestRuntime>::get() * 2,
                unbond_value: MinUserBond::<TestRuntime>::get(),
                rebond_value: MinUserBond::<TestRuntime>::get(),
            }
        }
    }

    impl RebondContext {
        fn setup(&self) {
            let stash = self.staker.stash.account_id();
            let controller = self.staker.controller.account_id();

            Balances::make_free_balance_be(&stash, self.bond_value);
            assert_ok!(ValidatorManager::bond(
                Origin::signed(stash),
                controller,
                self.bond_value,
                RewardDestination::Stash
            ));

            let nonce = ValidatorManager::proxy_nonce(controller);
            let unbond_call = self.create_call_for_unbond(nonce);
            assert_ok!(AvnProxy::proxy(self.origin.clone(), unbond_call, None));
        }

        fn create_call_for_unbond(&self, sender_nonce: u64) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_unbond(sender_nonce);

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_unbond(proof, self.unbond_value),
            ));
        }

        fn create_proof_for_signed_unbond(&self, sender_nonce: u64) -> Proof<Signature, AccountId> {
            let controller_account_id = &self.staker.controller.account_id();

            let data_to_sign = encode_signed_unbond_params::<TestRuntime>(
                &get_partial_proof(controller_account_id, &self.staker.relayer),
                &self.rebond_value,
                sender_nonce,
            );

            let signature = sign(&self.staker.controller_key_pair, &data_to_sign);
            return build_proof(controller_account_id, &self.staker.relayer, signature);
        }

        fn create_call_for_rebond(&self, sender_nonce: u64) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_rebond(sender_nonce);

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_rebond(proof, self.rebond_value),
            ));
        }

        fn create_call_for_rebond_approved_by_relayer(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let mut proof = self.create_proof_for_signed_rebond(sender_nonce);
            proof.signer = self.staker.relayer;

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_rebond(proof, self.rebond_value),
            ));
        }

        fn create_proof_for_signed_rebond(&self, sender_nonce: u64) -> Proof<Signature, AccountId> {
            let controller_account_id = &self.staker.controller.account_id();

            let data_to_sign = encode_signed_rebond_params::<TestRuntime>(
                &get_partial_proof(controller_account_id, &self.staker.relayer),
                &self.unbond_value,
                sender_nonce,
            );

            let signature = sign(&self.staker.controller_key_pair, &data_to_sign);
            return build_proof(controller_account_id, &self.staker.relayer, signature);
        }
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = &RebondContext::default();
            context.setup();

            let stash_account_id = &context.staker.stash.account_id();
            let controller_account_id = &context.staker.controller.account_id();

            let nonce = ValidatorManager::proxy_nonce(controller_account_id);
            let rebond_call = context.create_call_for_rebond(nonce);

            //Prior to rebonding check that the staker is bonded
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

            // The ledger has a decreased active amount after unbond
            assert_eq!(
                Staking::ledger(&controller_account_id).unwrap().active,
                context.bond_value - context.unbond_value
            );

            // The ledger updated the unlocking
            assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 1);

            assert_ok!(AvnProxy::proxy(context.origin.clone(), rebond_call, None));

            // Proxy nonce has increased
            assert_eq!(ValidatorManager::proxy_nonce(controller_account_id), nonce + 1);

            // The staker is still bonded.
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

            // The ledger has an increased active amount after rebond
            assert_eq!(
                Staking::ledger(&controller_account_id).unwrap().active,
                context.bond_value - context.unbond_value + context.rebond_value
            );

            // Free balance is not affected
            assert_eq!(Balances::free_balance(*stash_account_id), context.bond_value);

            // The ledger updated the unlocking
            assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 0);

            // We still locked up all the money we have before
            assert_eq!(Balances::usable_balance(*stash_account_id), 0u128);
            assert_eq!(
                System::account(stash_account_id).data.misc_frozen,
                context.bond_value - context.unbond_value + context.rebond_value
            );
            assert_eq!(
                System::account(stash_account_id).data.fee_frozen,
                context.bond_value - context.unbond_value + context.rebond_value
            );

            //Transfer will fail because all the balance is locked
            assert_noop!(
                Balances::transfer(Origin::signed(*stash_account_id), context.staker.relayer, 1),
                BalancesError::<TestRuntime>::LiquidityRestrictions
            );
        });
    }

    mod fails_when {
        use super::*;

        #[test]
        fn extrinsic_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &RebondContext::default();
                context.setup();

                let stash_account_id = &context.staker.stash.account_id();
                let controller_account_id = &context.staker.controller.account_id();

                let nonce = ValidatorManager::proxy_nonce(controller_account_id);
                let rebond_call = context.create_call_for_rebond(nonce);

                //Prior to rebonding check that the staker is bonded
                assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

                // The ledger has a decreased active amount after unbond
                assert_eq!(
                    Staking::ledger(&controller_account_id).unwrap().active,
                    context.bond_value - context.unbond_value
                );

                // The ledger updated the unlocking
                assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 1);

                assert_noop!(AvnProxy::proxy(RawOrigin::None.into(), rebond_call, None), BadOrigin);
            });
        }

        // We don't need to test SenderIsNotSigner error through AvnProxy::proxy call
        // as it always uses the proof.signer as the sender

        #[test]
        fn rebond_call_is_unauthorized() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &RebondContext::default();
                context.setup();

                let stash_account_id = &context.staker.stash.account_id();
                let controller_account_id = &context.staker.controller.account_id();

                let nonce = ValidatorManager::proxy_nonce(controller_account_id);
                let rebond_call = context.create_call_for_rebond_approved_by_relayer(nonce);

                //Prior to rebonding check that the staker is bonded
                assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

                // The ledger has an decreased active amount after unbond
                assert_eq!(
                    Staking::ledger(&controller_account_id).unwrap().active,
                    context.bond_value - context.unbond_value
                );

                // The ledger updated the unlocking
                assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 1);

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), rebond_call, None),
                    Error::<TestRuntime>::UnauthorizedSignedRebondTransaction
                );
            });
        }
    }
}
