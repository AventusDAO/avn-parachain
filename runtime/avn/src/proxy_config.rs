use super::{
    AccountId, Box, Decode, Encode, InnerCallValidator, Proof, ProvableProxy, Runtime, RuntimeCall,
    RuntimeDebug, Signature, TypeInfo,
};
use codec::DecodeWithMemTracking;

// Avn proxy configuration logic
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Encode,
    Decode,
    RuntimeDebug,
    TypeInfo,
    DecodeWithMemTracking,
)]
pub struct AvnProxyConfig {}
impl Default for AvnProxyConfig {
    fn default() -> Self {
        AvnProxyConfig {}
    }
}

impl ProvableProxy<RuntimeCall, Signature, AccountId> for AvnProxyConfig {
    fn get_proof(call: &RuntimeCall) -> Option<Proof<Signature, AccountId>> {
        match call {
            RuntimeCall::EthereumEvents(
                pallet_ethereum_events::Call::signed_add_ethereum_log {
                    proof,
                    event_type: _,
                    tx_hash: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::TokenManager(pallet_token_manager::pallet::Call::signed_transfer {
                proof,
                from: _,
                to: _,
                token_id: _,
                amount: _,
            }) => return Some(proof.clone()),
            RuntimeCall::TokenManager(
                pallet_token_manager::pallet::Call::schedule_signed_lower {
                    proof,
                    from: _,
                    token_id: _,
                    amount: _,
                    t1_recipient: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_mint_single_nft {
                proof,
                unique_external_ref: _,
                royalties: _,
                t1_authority: _,
            }) => return Some(proof.clone()),
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_list_nft_open_for_sale {
                proof,
                nft_id: _,
                market: _,
            }) => return Some(proof.clone()),
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_transfer_fiat_nft {
                proof,
                nft_id: _,
                t2_transfer_to_public_key: _,
            }) => return Some(proof.clone()),
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_cancel_list_fiat_nft {
                proof,
                nft_id: _,
            }) => return Some(proof.clone()),
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_create_batch {
                proof,
                total_supply: _,
                royalties: _,
                t1_authority: _,
            }) => return Some(proof.clone()),
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_mint_batch_nft {
                proof,
                batch_id: _,
                index: _,
                owner: _,
                unique_external_ref: _,
            }) => return Some(proof.clone()),
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_list_batch_for_sale {
                proof,
                batch_id: _,
                market: _,
            }) => return Some(proof.clone()),
            RuntimeCall::NftManager(pallet_nft_manager::Call::signed_end_batch_sale {
                proof,
                batch_id: _,
            }) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_candidate_bond_extra {
                    proof,
                    extra_amount: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_execute_candidate_unbond {
                    proof,
                    candidate: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_schedule_candidate_unbond { proof, less: _ },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(pallet_parachain_staking::Call::signed_nominate {
                proof,
                targets: _,
                amount: _,
            }) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_schedule_leave_nominators { proof },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_execute_leave_nominators {
                    proof,
                    nominator: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_schedule_revoke_nomination {
                    proof,
                    collator: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(pallet_parachain_staking::Call::signed_bond_extra {
                proof,
                extra_amount: _,
            }) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_schedule_nominator_unbond { proof, less: _ },
            ) => return Some(proof.clone()),
            RuntimeCall::ParachainStaking(
                pallet_parachain_staking::Call::signed_execute_nomination_request {
                    proof,
                    nominator: _,
                },
            ) => return Some(proof.clone()),
            RuntimeCall::AvnAnchor(pallet_avn_anchor::Call::signed_register_chain_handler {
                proof,
                ..
            }) => return Some(proof.clone()),
            RuntimeCall::AvnAnchor(pallet_avn_anchor::Call::signed_update_chain_handler {
                proof,
                ..
            }) => return Some(proof.clone()),
            RuntimeCall::AvnAnchor(
                pallet_avn_anchor::Call::signed_submit_checkpoint_with_identity { proof, .. },
            ) => return Some(proof.clone()),
            _ => None,
        }
    }
}

impl InnerCallValidator for AvnProxyConfig {
    type Call = RuntimeCall;

    fn signature_is_valid(call: &Box<Self::Call>) -> bool {
        match **call {
            RuntimeCall::EthereumEvents(..) => {
                return pallet_ethereum_events::Pallet::<Runtime>::signature_is_valid(call)
            },
            RuntimeCall::TokenManager(..) => {
                return pallet_token_manager::Pallet::<Runtime>::signature_is_valid(call)
            },
            RuntimeCall::NftManager(..) => {
                return pallet_nft_manager::Pallet::<Runtime>::signature_is_valid(call)
            },
            RuntimeCall::ParachainStaking(..) => {
                return pallet_parachain_staking::Pallet::<Runtime>::signature_is_valid(call)
            },
            RuntimeCall::AvnAnchor(..) => {
                return pallet_avn_anchor::Pallet::<Runtime>::signature_is_valid(call)
            },
            _ => false,
        }
    }
}
