//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::common::*;
use crate::extension_builder::ExtBuilder;
use crate::mock::staking::StakingLedger;
use crate::mock::Call as MockCall;
use crate::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use pallet_balances::Error as BalancesError;
use sp_runtime::DispatchError::BadOrigin;

mod proxy_signed_set_controller {
    use super::*;

    #[derive(Clone)]
    struct SetControllerContext {
        origin: Origin,
        staker: Staker,
        new_controller: TestAccount,
        value: BalanceOf<TestRuntime>,
    }

    impl Default for SetControllerContext {
        fn default() -> Self {
            let staker: Staker = Default::default();
            let new_controller = TestAccount::new([30u8; 32]);
            SetControllerContext {
                origin: Origin::signed(staker.relayer),
                staker,
                new_controller,
                value: MinUserBond::<TestRuntime>::get(),
            }
        }
    }

    impl SetControllerContext {
        fn setup(&self) {
            let stash = self.staker.stash.account_id();
            let controller = self.staker.controller.account_id();

            Balances::make_free_balance_be(&stash, self.value);
            assert_ok!(ValidatorManager::bond(
                Origin::signed(stash),
                controller,
                self.value,
                RewardDestination::Stash
            ));
        }

        fn create_call_for_set_controller(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_set_controller(sender_nonce);

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_set_controller(
                    proof,
                    self.new_controller.account_id(),
                ),
            ));
        }

        fn create_call_for_set_controller_approved_by_relayer(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let mut proof = self.create_proof_for_signed_set_controller(sender_nonce);
            proof.signer = self.staker.relayer;

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_set_controller(
                    proof,
                    self.staker.controller.account_id(),
                ),
            ));
        }

        fn create_proof_for_signed_set_controller(
            &self,
            sender_nonce: u64,
        ) -> Proof<Signature, AccountId> {
            let stash_account_id = &self.staker.stash.account_id();

            let data_to_sign = encode_signed_set_controller_params::<TestRuntime>(
                &get_partial_proof(stash_account_id, &self.staker.relayer),
                &self.new_controller.account_id(),
                sender_nonce,
            );

            let signature = sign(&self.staker.stash_key_pair, &data_to_sign);
            return build_proof(stash_account_id, &self.staker.relayer, signature);
        }
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = &SetControllerContext::default();
            context.setup();

            let stash_account_id = &context.staker.stash.account_id();
            let controller_account_id = &context.staker.controller.account_id();
            let new_controller_account_id = &context.new_controller.account_id();

            let nonce = ValidatorManager::proxy_nonce(stash_account_id);
            let set_controller_call = context.create_call_for_set_controller(nonce);

            // The old and new controller account ids are different
            assert_ne!(controller_account_id, new_controller_account_id);

            // The old controller account is added to the ledger for the staker
            assert!(pallet_staking::Ledger::<TestRuntime>::contains_key(&controller_account_id));
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);
            assert_eq!(
                Staking::ledger(&controller_account_id),
                Some(StakingLedger {
                    stash: *stash_account_id,
                    total: context.value,
                    active: context.value,
                    unlocking: vec![],
                    claimed_rewards: vec![]
                })
            );

            assert_ok!(AvnProxy::proxy(context.origin.clone(), set_controller_call, None));

            // Proxy nonce has increased
            assert_eq!(ValidatorManager::proxy_nonce(stash_account_id), nonce + 1);

            // The new controller account is added to the ledger for the staker
            assert!(pallet_staking::Ledger::<TestRuntime>::contains_key(
                &new_controller_account_id
            ));
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *new_controller_account_id);

            // The ledger for the new controller account is as expected. Total and active have the same value
            assert_eq!(
                Staking::ledger(&new_controller_account_id),
                Some(StakingLedger {
                    stash: *stash_account_id,
                    total: context.value,
                    active: context.value,
                    unlocking: vec![],
                    claimed_rewards: vec![]
                })
            );

            // Free balance is not affected
            assert_eq!(Balances::free_balance(*stash_account_id), context.value);

            // We have locked up all the money we have
            assert_eq!(Balances::usable_balance(*stash_account_id), 0u128);
            assert_eq!(System::account(stash_account_id).data.misc_frozen, context.value);
            assert_eq!(System::account(stash_account_id).data.fee_frozen, context.value);

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
                let context = &SetControllerContext::default();
                context.setup();
                let nonce = ValidatorManager::proxy_nonce(context.staker.stash.account_id());
                let set_controller_call = context.create_call_for_set_controller(nonce);

                assert_noop!(
                    AvnProxy::proxy(RawOrigin::None.into(), set_controller_call, None),
                    BadOrigin
                );
            });
        }

        // We don't need to test SenderIsNotSigner error through AvnProxy::proxy call
        // as it always uses the proof.signer as the sender

        #[test]
        fn set_controller_call_is_unauthorized() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &SetControllerContext::default();
                context.setup();
                let nonce = ValidatorManager::proxy_nonce(context.staker.stash.account_id());

                // Create a set controller call with a proof that is signed by the relayer rather than the staker himself.
                let set_controller_call =
                    context.create_call_for_set_controller_approved_by_relayer(nonce);

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), set_controller_call, None),
                    Error::<TestRuntime>::UnauthorizedSignedSetControllerTransaction
                );
            });
        }
    }
}
