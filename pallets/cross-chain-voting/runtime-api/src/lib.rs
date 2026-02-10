#![cfg_attr(not(feature = "std"), no_std)]

use sp_avn_common::primitives::{AccountId, Balance};
use sp_core::H160;
use sp_std::vec::Vec;

sp_api::decl_runtime_apis! {
    pub trait CrossChainVotingApi {
        fn get_total_linked_balance(t1_identity_account: H160) -> Balance;
        fn get_linked_accounts(t1_identity_account: H160) -> Vec<AccountId>;
        fn get_identity_account(t2_linked_account: AccountId) -> Option<H160>;
    }
}
