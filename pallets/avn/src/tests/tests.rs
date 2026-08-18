use crate::mock::*;
use sp_runtime::testing::UintAuthorityId;

fn setup_last_validator_as_primary() {
    let validators = AVN::validators();
    let eth_index_before: u8 = AVN::get_primary_collator();

    let num_validators_indexed = validators.len() - 1;
    for _ in &validators[..num_validators_indexed] {
        AVN::advance_primary_validator_for_sending().unwrap();
    }

    let eth_index_after: u8 = AVN::get_primary_collator();
    assert_eq!(eth_index_before + num_validators_indexed as u8, eth_index_after);
}

#[cfg(test)]
mod when_advance_primary_validator_for_sending_is_called {
    use super::*;
    #[test]
    fn the_next_validator_for_ethereum_increments_by_one_if_the_current_one_is_not_the_last() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let eth_index_before: u8 = AVN::get_primary_collator();

            let _ = AVN::advance_primary_validator_for_sending();

            let eth_index_after: u8 = AVN::get_primary_collator();
            assert_eq!(eth_index_after, eth_index_before + 1);
        });
    }

    #[test]
    fn the_validator_for_ethereum_wraps_around_if_the_current_one_is_the_last() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let eth_index_before: u8 = AVN::get_primary_collator();
            assert_eq!(eth_index_before, 0);

            setup_last_validator_as_primary();

            AVN::advance_primary_validator_for_sending().unwrap();
            let eth_index_after: u8 = AVN::get_primary_collator();
            assert_eq!(eth_index_before, eth_index_after);
        });
    }
}

#[cfg(test)]
mod calling_is_primary_validator_for_ethereum {
    use super::*;
    #[test]
    fn does_not_change_the_next_primary_validator_for_ethereum() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let expected_primary = 1;
            let eth_index_before: u8 = AVN::get_primary_collator();
            assert_eq!(eth_index_before, 0);

            let result = AVN::is_primary_validator_for_sending(&expected_primary).unwrap();
            assert!(result == true, "Wrong primary validator");

            let eth_index_after: u8 = AVN::get_primary_collator();
            assert_eq!(eth_index_before, eth_index_after);
        });
    }

    #[test]
    fn returns_true_if_the_argument_is_the_same_as_the_next_ethereum_validator() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let mut expected_primary = 1;
            let mut result = AVN::is_primary_validator_for_sending(&expected_primary).unwrap();
            assert!(result == true, "Wrong primary validator");

            let _ = AVN::advance_primary_validator_for_sending();

            expected_primary = 2;
            result = AVN::is_primary_validator_for_sending(&expected_primary).unwrap();
            assert!(result == true, "Wrong primary validator");
        });
    }

    #[test]
    fn returns_false_if_it_is_not() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let non_primary_validator = 4;
            let result = AVN::is_primary_validator_for_sending(&non_primary_validator).unwrap();
            assert!(result != true, "Primary validator is unexpectedly correct");
        });
    }

    #[test]
    fn fails_with_no_validators() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let expected_primary = 1;
            let result = AVN::is_primary_validator_for_sending(&expected_primary);
            assert!(result.is_err());
        });
    }
}

#[cfg(test)]
mod calling_is_primary_validator_for_avn {
    use super::*;
    #[test]
    fn is_expected_on_block_1() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let block_number = 1;
            let expected_primary = 2;
            let result = AVN::is_primary_for_block(block_number, &expected_primary);
            assert!(result.is_ok(), "Getting primary validator failed");
            assert_eq!(result.unwrap(), true);
        });
    }

    #[test]
    fn is_expected_on_block_2() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let block_number = 2;
            let expected_primary = 3;
            let result = AVN::is_primary_for_block(block_number, &expected_primary);
            assert!(result.is_ok(), "Getting primary validator failed");
            assert_eq!(result.unwrap(), true);
        });
    }

    #[test]
    fn is_expected_on_block_3() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let block_number = 3;
            let expected_primary = 1;
            let result = AVN::is_primary_for_block(block_number, &expected_primary);
            assert!(result.is_ok(), "Getting primary validator failed");
            assert_eq!(result.unwrap(), true);
        });
    }

    #[test]
    fn is_expected_on_block_100() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let block_number = 100;
            let expected_primary = 2;
            let result = AVN::is_primary_for_block(block_number, &expected_primary);
            assert!(result.is_ok(), "Getting primary validator failed");
            assert_eq!(result.unwrap(), true);
        });
    }

    #[test]
    fn returns_false_if_it_is_not() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let wrong_validator = 4;
            let block_number = System::block_number();
            let result = AVN::is_primary_for_block(block_number, &wrong_validator).unwrap();
            assert!(result != true, "Primary validator is unexpectedly correct");
        });
    }

    #[test]
    fn fails_with_no_validators() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let expected_primary = 1;
            let block_number = System::block_number();
            let result = AVN::is_primary_for_block(block_number, &expected_primary);
            assert!(result.is_err());
        });
    }
}

/*********************** */

#[test]
fn test_local_authority_keys_empty() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        let current_node_validator = AVN::get_validator_for_current_node();
        assert!(current_node_validator.is_none());
    });
}

#[test]
fn test_local_authority_keys_valid() {
    let mut ext = ExtBuilder::build_default().with_validators().as_externality();
    ext.execute_with(|| {
        UintAuthorityId::set_all_keys(vec![1, 2, 3]);
        let current_node_validator = AVN::get_validator_for_current_node().unwrap();
        assert_eq!(current_node_validator.account_id, 1);
        assert_eq!(current_node_validator.key, UintAuthorityId(1));
    });
}

/**************************** */

// `signature_is_valid` must verify against
// the key that is REGISTERED on-chain for the claimed account, never the key supplied alongside
// the signature. Otherwise an attacker can forge a vote for any real validator's account using a
// key they control (account membership passes, and the signature verifies against their own key).
#[cfg(test)]
mod signature_is_valid_key_binding {
    use super::*;
    use codec::Encode;
    use sp_avn_common::event_types::Validator;
    use sp_runtime::testing::TestSignature;

    // Genesis validators (see mock `VALIDATORS`) are accounts 1, 2, 3, each registered with
    // key `UintAuthorityId(<account_id>)`.
    const REGISTERED_ACCOUNT: u64 = 1;
    const REGISTERED_KEY_ID: u64 = 1;

    // Arbitrary payload that gets signed. `TestSignature(key_id, msg)` verifies iff the verifying
    // key's id == key_id AND msg == the encoding of `data` (what `using_encoded` produces).
    fn signed_payload() -> (Vec<u8>, u64) {
        (b"submit_ethereum_events".to_vec(), 7u64)
    }

    #[test]
    fn accepts_signature_from_the_registered_key() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let data = signed_payload();
            let validator = Validator {
                account_id: REGISTERED_ACCOUNT,
                key: UintAuthorityId(REGISTERED_KEY_ID),
            };
            // Signature genuinely produced by the registered key over the encoded data.
            let signature = TestSignature(REGISTERED_KEY_ID, data.encode());

            assert!(
                AVN::signature_is_valid(&data, &validator, &signature),
                "a signature from the registered key must be accepted"
            );
        });
    }

    #[test]
    fn rejects_forged_vote_using_attacker_key_for_a_real_account() {
        // The core regression: attacker names a real validator account but presents a key they
        // control and a signature made with that key.
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let data = signed_payload();
            let attacker_key_id = 99u64; // NOT the registered key for account 1
            let forged_validator =
                Validator { account_id: REGISTERED_ACCOUNT, key: UintAuthorityId(attacker_key_id) };
            let forged_signature = TestSignature(attacker_key_id, data.encode());

            assert!(
                !AVN::signature_is_valid(&data, &forged_validator, &forged_signature),
                "forged vote with an attacker-controlled key was accepted for a real validator account"
            );
        });
    }

    #[test]
    fn rejects_signature_for_a_non_validator_account() {
        let mut ext = ExtBuilder::build_default().with_validators().as_externality();
        ext.execute_with(|| {
            let data = signed_payload();
            let non_validator_account = 999u64;
            let validator = Validator {
                account_id: non_validator_account,
                key: UintAuthorityId(non_validator_account),
            };
            let signature = TestSignature(non_validator_account, data.encode());

            assert!(
                !AVN::signature_is_valid(&data, &validator, &signature),
                "an account that is not a registered validator must be rejected"
            );
        });
    }
}
