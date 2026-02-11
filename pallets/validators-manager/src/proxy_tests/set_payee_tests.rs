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

mod proxy_signed_set_payee {
    use super::*;

    #[derive(Clone)]
    struct SetPayeeContext {
        origin: Origin,
        staker: Staker,
        new_payee: TestAccount,
        value: BalanceOf<TestRuntime>,
    }

    impl Default for SetPayeeContext {
        fn default() -> Self {
            let staker: Staker = Default::default();
            let new_payee = TestAccount::new([30u8; 32]);
            SetPayeeContext {
                origin: Origin::signed(staker.relayer),
                staker,
                new_payee,
                value: MinUserBond::<TestRuntime>::get(),
            }
        }
    }

    impl SetPayeeContext {
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

        fn create_call_for_set_payee(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_set_payee(sender_nonce);

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_set_payee(
                    proof,
                    RewardDestination::Account(self.new_payee.account_id()),
                ),
            ));
        }

        fn create_call_for_set_payee_approved_by_relayer(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let mut proof = self.create_proof_for_signed_set_payee(sender_nonce);
            proof.signer = self.staker.relayer;

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_set_payee(
                    proof,
                    RewardDestination::Account(self.new_payee.account_id()),
                ),
            ));
        }

        fn create_proof_for_signed_set_payee(
            &self,
            sender_nonce: u64,
        ) -> Proof<Signature, AccountId> {
            let controller_account_id = &self.staker.controller.account_id();

            let data_to_sign = encode_signed_set_payee_params::<TestRuntime>(
                &get_partial_proof(controller_account_id, &self.staker.relayer),
                &RewardDestination::Account(self.new_payee.account_id()),
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
            let context = &SetPayeeContext::default();
            context.setup();

            let stash_account_id = &context.staker.stash.account_id();
            let controller_account_id = &context.staker.controller.account_id();
            let new_payee_account_id = &context.new_payee.account_id();

            let nonce = ValidatorManager::proxy_nonce(controller_account_id);
            let set_payee_call = context.create_call_for_set_payee(nonce);

            // The controller account is added to the ledger for the staker
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

            // The RewardDestination is set to Stash account
            assert_eq!(Staking::payee(stash_account_id), RewardDestination::Stash);

            assert_ok!(AvnProxy::proxy(context.origin.clone(), set_payee_call, None));

            // Proxy nonce has increased
            assert_eq!(ValidatorManager::proxy_nonce(controller_account_id), nonce + 1);

            // The RewardDestination is set to a payee account
            assert_eq!(
                Staking::payee(stash_account_id),
                RewardDestination::Account(*new_payee_account_id)
            );

            // TODO: SYS-1960 Add assertion to make sure that rewards are paid into the new_payee_account_id account instead of stash

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
                let context = &SetPayeeContext::default();
                context.setup();
                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());
                let set_payee_call = context.create_call_for_set_payee(nonce);

                let stash_account_id = &context.staker.stash.account_id();
                let controller_account_id = &context.staker.controller.account_id();
                let new_payee_account_id = &context.new_payee.account_id();

                // The controller account is added to the ledger for the staker
                assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

                // The RewardDestination is set to Stash account
                assert_eq!(Staking::payee(stash_account_id), RewardDestination::Stash);

                assert_noop!(
                    AvnProxy::proxy(RawOrigin::None.into(), set_payee_call, None),
                    BadOrigin
                );

                // The RewardDestination is not set to a payee account
                assert_ne!(
                    Staking::payee(stash_account_id),
                    RewardDestination::Account(*new_payee_account_id)
                );
            });
        }

        // We don't need to test SenderIsNotSigner error through AvnProxy::proxy call
        // as it always uses the proof.signer as the sender

        #[test]
        fn set_payee_call_is_unauthorized() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &SetPayeeContext::default();
                context.setup();
                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());

                // Create a set payee call with a proof that is signed by the relayer rather than the staker himself.
                let set_payee_call = context.create_call_for_set_payee_approved_by_relayer(nonce);

                let stash_account_id = &context.staker.stash.account_id();
                let controller_account_id = &context.staker.controller.account_id();
                let new_payee_account_id = &context.new_payee.account_id();

                // The controller account is added to the ledger for the staker
                assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

                // The RewardDestination is set to Stash account
                assert_eq!(Staking::payee(stash_account_id), RewardDestination::Stash);

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), set_payee_call, None),
                    Error::<TestRuntime>::UnauthorizedSignedSetPayeeTransaction
                );

                // The RewardDestination is not set to a payee account
                assert_ne!(
                    Staking::payee(stash_account_id),
                    RewardDestination::Account(*new_payee_account_id)
                );
            });
        }
    }
}
