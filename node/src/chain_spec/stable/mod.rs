mod local;
mod staging;

pub use local::{development_config, local_testnet_config};
pub use staging::{staging_dev_testnet_config, staging_testnet_config};

use crate::chain_spec::{
    constants::*, helpers::*, AuraId, AuthorityDiscoveryId, AvnId, EthPublicKey, ImOnlineId, ParaId,
};
use avn_parachain_runtime::{self as avn_runtime};

use sp_core::{H160, H256};

use hex_literal::hex;
use sp_avn_common::{eth::EthereumNetwork, primitives::AccountId};
use sp_runtime::{traits::ConstU32, BoundedVec};

/// Generate the session keys from individual elements.
///
/// The input must be a tuple of individual keys (a single arg for now since we have just one key).
pub fn avn_session_keys(
    aura_keys: AuraId,
    audi_keys: AuthorityDiscoveryId,
    im_online_keys: ImOnlineId,
    avn_keys: AvnId,
) -> avn_runtime::SessionKeys {
    avn_runtime::SessionKeys {
        aura: aura_keys,
        authority_discovery: audi_keys,
        im_online: im_online_keys,
        avn: avn_keys,
    }
}

pub(crate) fn testnet_genesis(
    candidates: Vec<(AccountId, AuraId, AuthorityDiscoveryId, ImOnlineId, AvnId)>,
    endowed_accounts: Vec<(AccountId, Balance)>,
    id: ParaId,
    sudo_account: AccountId,
    avt_token_contract: H160,
    avn_eth_contract: H160,
    nft_eth_contracts: Vec<(H160, ())>,
    eth_public_keys: Vec<EthPublicKey>,
    lift_tx_hashes: Vec<H256>,
    event_challenge_period: BlockNumber,
    schedule_period: BlockNumber,
    voting_period: BlockNumber,
    default_non_avt_token: Option<H160>,
) -> serde_json::Value {
    let token_balances = if let Some(token) = default_non_avt_token {
        endowed_accounts
            .iter()
            .cloned()
            .map(|(k, a)| (token.clone(), k, a))
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    serde_json::json!({
        "balances": {
            "balances": endowed_accounts.iter().cloned().map(|(account, amount)| (account, amount)).collect::<Vec<_>>(),
        },
        "parachainInfo":{ "parachainId": id },
        "session": {
            "keys": candidates
                .iter()
                .cloned()
                .map(|(acc, aura, audi, imon, avn)| {
                    (
                        acc.clone(),                             // account id
                        acc,                                     // validator id
                        avn_session_keys(aura, audi, imon, avn), // session keys
                    )
                })
                .collect::<Vec<_>>(),
        },
        "polkadotXcm": {
            "safeXcmVersion": Some(SAFE_XCM_VERSION),
        },
        "sudo": { "key": Some(sudo_account) },
        "avn":  {
            "bridgeContractAddress": avn_eth_contract.clone(),
        },
        "ethBridge": {
            "ethTxLifetimeSecs": 60 * 30 as u64, // 30 minutes
            "nextTxId": 1u32,
            "ethBlockRangeSize": 20u32,
            "instance": {
                "network": EthereumNetwork::Sepolia,
                "bridge_contract": avn_eth_contract,
                "name": BoundedVec::<u8, ConstU32<256>>::truncate_from("AVN Bridge".as_bytes().to_vec()),
                "version": BoundedVec::<u8, ConstU32<256>>::truncate_from("1".as_bytes().to_vec().into()),
            },
        },
        "ethereumEvents": {
            "nftT1Contracts": nft_eth_contracts,
            "liftTxHashes": lift_tx_hashes,
            "quorumFactor": QUORUM_FACTOR,
            "eventChallengePeriod": event_challenge_period,
        },
        "validatorsManager": {
            "validators": candidates
                .iter()
                .map(|x| x.0.clone())
                .zip(eth_public_keys.iter().map(|pk| pk.clone()))
                .collect::<Vec<_>>(),
        },
        "parachainStaking": {
            "candidates": candidates
                .iter()
                .cloned()
                .map(|(acc, _, _, _, _)| (acc, COLLATOR_DEPOSIT))
                .collect::<Vec<_>>(),
            "minCollatorStake": COLLATOR_DEPOSIT,
            "minTotalNominatorStake": 10 * AVT,
            "delay": 2,
        },
        "summary": { "schedulePeriod": schedule_period, "votingPeriod": voting_period },
        "tokenManager": {
            "lowerAccountId": H256(hex!(
                "000000000000000000000000000000000000000000000000000000000000dead"
            )),
            // Address of AVT contract
            "avtTokenContract": avt_token_contract,
            "lowerSchedulePeriod": 10,
            "balances": token_balances,
        }
    })
}
