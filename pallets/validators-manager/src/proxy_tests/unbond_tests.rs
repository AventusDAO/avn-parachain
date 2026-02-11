//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::common::*;
use crate::extension_builder::ExtBuilder;
use crate::mock::staking::StakingLedger;
use crate::mock::Call as MockCall;
use crate::mock::Event as MockEvent;
use crate::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use pallet_balances::Error as BalancesError;
use sp_runtime::DispatchError::BadOrigin;

mod proxy_signed_unbond {
    use super::*;

    #[derive(Clone)]
    struct UnbondContext {
        origin: Origin,
        staker: Staker,
        value: BalanceOf<TestRuntime>,
    }

    impl Default for UnbondContext {
        fn default() -> Self {
            let staker: Staker = Default::default();
            UnbondContext {
                origin: Origin::signed(staker.relayer),
                staker,
                value: MinUserBond::<TestRuntime>::get(),
            }
        }
    }

    impl UnbondContext {
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

        fn create_call_for_unbond(&self, sender_nonce: u64) -> Box<<TestRuntime as Config>::Call> {
            let proof = self.create_proof_for_signed_unbond(sender_nonce);

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_unbond(proof, self.value),
            ));
        }

        fn create_call_for_unbond_approved_by_relayer(
            &self,
            sender_nonce: u64,
        ) -> Box<<TestRuntime as Config>::Call> {
            let mut proof = self.create_proof_for_signed_unbond(sender_nonce);
            proof.signer = self.staker.relayer;

            return Box::new(MockCall::ValidatorManager(
                super::super::Call::<TestRuntime>::signed_unbond(proof, self.value),
            ));
        }

        fn create_proof_for_signed_unbond(&self, sender_nonce: u64) -> Proof<Signature, AccountId> {
            let controller_account_id = &self.staker.controller.account_id();

            let data_to_sign = encode_signed_unbond_params::<TestRuntime>(
                &get_partial_proof(controller_account_id, &self.staker.relayer),
                &self.value,
                sender_nonce,
            );

            let signature = sign(&self.staker.controller_key_pair, &data_to_sign);
            return build_proof(controller_account_id, &self.staker.relayer, signature);
        }

        pub fn unbonded_event_emitted(&self) -> bool {
            return System::events().iter().any(|e| {
                e.event
                    == MockEvent::pallet_staking(
                        crate::mock::staking::Event::<TestRuntime>::Unbonded(
                            self.staker.stash.account_id(),
                            self.value,
                        ),
                    )
            });
        }
    }

    #[test]
    fn succeeds_with_good_parameters() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let context = &UnbondContext::default();
            context.setup();

            let stash_account_id = &context.staker.stash.account_id();
            let controller_account_id = &context.staker.controller.account_id();

            let nonce = ValidatorManager::proxy_nonce(controller_account_id);
            let unbond_call = context.create_call_for_unbond(nonce);

            //Prior to unbonding check that the staker is bonded
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

            // The ledger has a record for the controller account
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

            assert_ok!(AvnProxy::proxy(context.origin.clone(), unbond_call, None));

            //Event is emitted
            assert!(context.unbonded_event_emitted());

            // Proxy nonce has increased
            assert_eq!(ValidatorManager::proxy_nonce(controller_account_id), nonce + 1);

            // The staker is still bonded.
            assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

            // But the ledger has no active amount after unbond
            assert_eq!(Staking::ledger(&controller_account_id).unwrap().active, 0u128);

            // Free balance is not affected
            assert_eq!(Balances::free_balance(*stash_account_id), context.value);

            // The ledger updated the unlocking
            assert_eq!(Staking::ledger(&controller_account_id).unwrap().unlocking.len(), 1);

            // We still locked up all the money we have before
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
                let context = &UnbondContext::default();
                context.setup();

                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());
                let unbond_call = context.create_call_for_unbond(nonce);
                let stash_account_id = &context.staker.stash.account_id();
                let controller_account_id = &context.staker.controller.account_id();

                //Prior to unbonding check that the staker is bonded
                assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

                // The ledger has a record for the controller account
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

                assert_noop!(AvnProxy::proxy(RawOrigin::None.into(), unbond_call, None), BadOrigin);
            });
        }

        // We don't need to test SenderIsNotSigner error through AvnProxy::proxy call
        // as it always uses the proof.signer as the sender

        #[test]
        fn unbond_call_is_unauthorized() {
            let mut ext = ExtBuilder::build_default().with_validators().as_externality();
            ext.execute_with(|| {
                let context = &UnbondContext::default();
                context.setup();
                let nonce = ValidatorManager::proxy_nonce(context.staker.controller.account_id());

                // Create a unbond call with a proof that is signed by the relayer rather than the staker himself.
                let unbond_call = context.create_call_for_unbond_approved_by_relayer(nonce);

                let stash_account_id = &context.staker.stash.account_id();
                let controller_account_id = &context.staker.controller.account_id();

                //Prior to unbonding check that the staker is bonded
                assert_eq!(Staking::bonded(stash_account_id).unwrap(), *controller_account_id);

                // The ledger has a record for the controller account
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

                assert_noop!(
                    AvnProxy::proxy(context.origin.clone(), unbond_call, None),
                    Error::<TestRuntime>::UnauthorizedSignedUnbondTransaction
                );
            });
        }
    }
}
