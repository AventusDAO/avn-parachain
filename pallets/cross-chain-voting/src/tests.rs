#![cfg(test)]

use super::*;
use crate::mock::*;
use frame_support::{assert_noop, assert_ok};
use sp_avn_common::hash_string_data_with_ethereum_prefix;

fn set_balance(who: &AccountId, amount: u128) {
    Balances::make_free_balance_be(who, amount);
    assert_eq!(Balances::free_balance(who), amount);
}

fn payload(action: Action, t1: H160, t2: AccountId, chain_id: u64) -> LinkPayload<AccountId> {
    LinkPayload { action, t1_identity_account: t1, t2_linked_account: t2, chain_id }
}

fn sign_payload_string_format(
    t1_pair: &ecdsa::Pair,
    payload: &LinkPayload<AccountId>,
) -> ecdsa::Signature {
    let msg = payload.signing_bytes();
    let hash =
        hash_string_data_with_ethereum_prefix(&msg).expect("hashing should succeed in tests");

    t1_pair.sign_prehashed(&hash)
}

mod link_account {
    use super::*;

    #[test]
    fn links_account_and_updates_both_maps() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let t2 = test_account(10);
            let p = payload(Action::Link, t1, t2, 1);

            let sig = sign_payload_string_format(&t1_pair, &p);

            assert_ok!(CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p.clone(), sig));

            // map: t2 -> t1
            assert_eq!(CrossChainVoting::get_identity_account(t2), Some(t1));

            // map: t1 -> vec[t2]
            let linked = CrossChainVoting::get_linked_accounts(t1);
            assert_eq!(linked.len(), 1);
            assert_eq!(linked[0], t2);

            System::assert_last_event(
                crate::Event::<TestRuntime>::AccountLinked {
                    t1_identity_account: t1,
                    t2_linked_account: t2,
                }
                .into(),
            );
        })
    }

    #[test]
    fn is_idempotent_for_same_identity_and_account() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);
            let t2 = test_account(10);

            let p = payload(Action::Link, t1, t2, 1);
            let sig = sign_payload_string_format(&t1_pair, &p);

            assert_ok!(CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p.clone(), sig));
            System::reset_events(); // clear so we can check re-adding emits no new events

            // second call should succeed and not duplicate in vec
            let sig2 = sign_payload_string_format(&t1_pair, &p);
            assert_ok!(CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p.clone(), sig2));
            assert!(System::events().is_empty()); // no new event emitted

            let linked = CrossChainVoting::get_linked_accounts(t1);
            assert_eq!(linked.len(), 1);
            assert_eq!(linked[0], t2);
        })
    }

    #[test]
    fn fails_if_caller_is_not_the_t2_linked_account() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let real_t2 = test_account(10);
            let impostor = test_account(11);

            let p = payload(Action::Link, t1, real_t2, 1);
            let sig = sign_payload_string_format(&t1_pair, &p);

            assert_noop!(
                CrossChainVoting::link_account(RuntimeOrigin::signed(impostor), p, sig),
                crate::Error::<TestRuntime>::CallerMustBeLinkedAccount
            );
        })
    }

    #[test]
    fn fails_if_action_is_not_link() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);
            let t2 = test_account(10);

            let p = payload(Action::Unlink, t1, t2, 1);
            // signature doesn't matter here as it should fail before the signature check
            let sig = sign_payload_string_format(&t1_pair, &p);

            assert_noop!(
                CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p, sig),
                crate::Error::<TestRuntime>::InvalidAction
            );
        })
    }

    #[test]
    fn fails_if_the_recovered_t1_signer_does_not_match_the_provided_identity() {
        new_test_ext().execute_with(|| {
            let correct_pair = test_ecdsa_pair(1);
            let correct_t1 = eth_address_from_pair(&correct_pair);

            let incorrect_pair = test_ecdsa_pair(2);
            let incorrect_t1 = eth_address_from_pair(&incorrect_pair);

            let t2 = test_account(10);

            let p = payload(Action::Link, correct_t1, t2, 1);
            let sig = sign_payload_string_format(&incorrect_pair, &p);

            assert_noop!(
                CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p, sig),
                crate::Error::<TestRuntime>::SignerIdentityMismatch
            );

            assert_ne!(incorrect_t1, correct_t1);
        })
    }

    #[test]
    fn fails_if_account_already_linked_to_different_identity() {
        new_test_ext().execute_with(|| {
            let t1a_pair = test_ecdsa_pair(1);
            let t1a = eth_address_from_pair(&t1a_pair);

            let t1b_pair = test_ecdsa_pair(2);
            let t1b = eth_address_from_pair(&t1b_pair);

            let t2 = test_account(10);

            let p1 = payload(Action::Link, t1a, t2, 1);
            let sig1 = sign_payload_string_format(&t1a_pair, &p1);
            assert_ok!(CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p1, sig1));

            let p2 = payload(Action::Link, t1b, t2, 1);
            let sig2 = sign_payload_string_format(&t1b_pair, &p2);

            assert_noop!(
                CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p2, sig2),
                crate::Error::<TestRuntime>::AccountLinkedToDifferentIdentity
            );

            // still linked to t1a
            assert_eq!(CrossChainVoting::get_identity_account(t2), Some(t1a));
        })
    }

    #[test]
    fn fails_when_max_linked_accounts_limit_is_reached() {
        new_test_ext().execute_with(|| {
            // MaxLinkedAccounts for tests is 2
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let t2a = test_account(10);
            let t2b = test_account(11);
            let t2c = test_account(12);

            for t2 in [t2a, t2b] {
                let p = payload(Action::Link, t1, t2, 1);
                let sig = sign_payload_string_format(&t1_pair, &p);
                assert_ok!(CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p, sig));
            }

            // should not allow 3rd link
            let p3 = payload(Action::Link, t1, t2c, 1);
            let sig3 = sign_payload_string_format(&t1_pair, &p3);

            assert_noop!(
                CrossChainVoting::link_account(RuntimeOrigin::signed(t2c), p3, sig3),
                crate::Error::<TestRuntime>::LinkedAccountsLimitReached
            );

            // Check bi-directional links remain as expected
            let linked = CrossChainVoting::get_linked_accounts(t1);
            assert_eq!(linked.len(), 2);
            assert!(linked.contains(&t2a));
            assert!(linked.contains(&t2b));
            assert_eq!(CrossChainVoting::get_identity_account(t2c), None);
        })
    }
}

mod unlink_account {
    use super::*;

    #[test]
    fn unlinks_account_and_updates_both_maps() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let t2 = test_account(10);

            // link first
            let p_link = payload(Action::Link, t1, t2, 1);
            let sig = sign_payload_string_format(&t1_pair, &p_link);
            assert_ok!(CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p_link, sig));

            let p_unlink = payload(Action::Unlink, t1, t2, 1);
            assert_ok!(CrossChainVoting::unlink_account(RuntimeOrigin::signed(t2), p_unlink));

            assert_eq!(CrossChainVoting::get_identity_account(t2), None);
            assert!(CrossChainVoting::get_linked_accounts(t1).is_empty());

            System::assert_last_event(
                crate::Event::<TestRuntime>::AccountUnlinked {
                    t1_identity_account: t1,
                    t2_linked_account: t2,
                }
                .into(),
            );
        })
    }

    #[test]
    fn fails_if_caller_is_not_the_t2_linked_account() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let t2 = test_account(10);
            let impostor = test_account(11);

            let p = payload(Action::Unlink, t1, t2, 1);

            assert_noop!(
                CrossChainVoting::unlink_account(RuntimeOrigin::signed(impostor), p),
                crate::Error::<TestRuntime>::CallerMustBeLinkedAccount
            );
        })
    }

    #[test]
    fn fails_if_action_is_not_unlink() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let t2 = test_account(10);

            let p = payload(Action::Link, t1, t2, 1);

            assert_noop!(
                CrossChainVoting::unlink_account(RuntimeOrigin::signed(t2), p),
                crate::Error::<TestRuntime>::InvalidAction
            );
        })
    }

    #[test]
    fn fails_if_account_not_linked_or_identity_mismatch() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let t2 = test_account(10);

            let p = payload(Action::Unlink, t1, t2, 1);

            assert_noop!(
                CrossChainVoting::unlink_account(RuntimeOrigin::signed(t2), p),
                crate::Error::<TestRuntime>::AccountNotLinkedToIdentity
            );
        })
    }
}

mod total_linked_balance {
    use super::*;

    #[test]
    fn sums_free_balance_of_all_linked_accounts_only() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let a = test_account(10);
            let b = test_account(11);
            let c = test_account(12);

            set_balance(&a, 100);
            set_balance(&b, 250);
            set_balance(&c, 999);

            // link a and b
            for t2 in [a, b] {
                let p = payload(Action::Link, t1, t2, 1);
                let sig = sign_payload_string_format(&t1_pair, &p);
                assert_ok!(CrossChainVoting::link_account(RuntimeOrigin::signed(t2), p, sig));
            }

            // total should be a + b
            let total = crate::Pallet::<TestRuntime>::get_total_linked_balance(t1);
            assert_eq!(total, 350);

            // unlink a
            let p_unlink_a = payload(Action::Unlink, t1, a, 1);
            assert_ok!(CrossChainVoting::unlink_account(RuntimeOrigin::signed(a), p_unlink_a));

            // link c
            let p_link_c = payload(Action::Link, t1, c, 1);
            let sig_c = sign_payload_string_format(&t1_pair, &p_link_c);
            assert_ok!(CrossChainVoting::link_account(RuntimeOrigin::signed(c), p_link_c, sig_c));

            // new total should be b + c
            let total2 = crate::Pallet::<TestRuntime>::get_total_linked_balance(t1);
            assert_eq!(total2, 250 + 999);
        })
    }

    #[test]
    fn returns_zero_when_no_accounts_linked() {
        new_test_ext().execute_with(|| {
            let t1_pair = test_ecdsa_pair(1);
            let t1 = eth_address_from_pair(&t1_pair);

            let total = crate::Pallet::<TestRuntime>::get_total_linked_balance(t1);
            assert_eq!(total, 0);
        })
    }
}
