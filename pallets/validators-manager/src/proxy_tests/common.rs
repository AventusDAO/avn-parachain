//Copyright 2026 Aventus DAO Ltd

#![cfg(test)]

use crate::mock::*;
use crate::*;
use sp_core::sr25519::Pair;

pub fn build_proof(
    signer: &AccountId,
    relayer: &AccountId,
    signature: Signature,
) -> Proof<Signature, AccountId> {
    return Proof { signer: *signer, relayer: *relayer, signature };
}

pub fn get_partial_proof(signer: &AccountId, relayer: &AccountId) -> Proof<Signature, AccountId> {
    return Proof { signer: *signer, relayer: *relayer, signature: Default::default() };
}

#[derive(Clone)]
pub struct Staker {
    pub relayer: AccountId,
    pub controller: TestAccount,
    pub controller_key_pair: Pair,
    pub stash: TestAccount,
    pub stash_key_pair: Pair,
}

impl Default for Staker {
    fn default() -> Self {
        let relayer = TestAccount::new([0u8; 32]).account_id();
        let controller = TestAccount::new([10u8; 32]);
        let stash = TestAccount::new([20u8; 32]);

        Staker {
            relayer,
            controller_key_pair: controller.key_pair(),
            controller,
            stash_key_pair: stash.key_pair(),
            stash,
        }
    }
}
