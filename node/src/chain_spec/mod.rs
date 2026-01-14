mod stable;
pub use stable::*;

#[cfg(feature = "avn-test-runtime")]
mod test;

#[cfg(feature = "avn-test-runtime")]
pub use test::{avn_garde_local_config, avn_garde_staging_config};

use hex_literal::hex;
use sc_chain_spec::{ChainSpecExtension, ChainSpecGroup};
use serde::{Deserialize, Serialize};
use sp_core::{ecdsa, ByteArray};

/// Specialized `ChainSpec` for the normal parachain runtime.
pub type ChainSpec = sc_service::GenericChainSpec<Extensions>;

pub(crate) type EthPublicKey = ecdsa::Public;

pub(crate) use cumulus_primitives_core::ParaId;
pub(crate) use pallet_avn::sr25519::AuthorityId as AvnId;
pub(crate) use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
pub(crate) use sc_service::ChainType;
pub(crate) use sp_authority_discovery::AuthorityId as AuthorityDiscoveryId;
pub(crate) use sp_consensus_aura::sr25519::AuthorityId as AuraId;

pub(crate) mod constants {
    pub use sp_avn_common::{
        constants::{currency::AVT, time::*},
        primitives::{Balance, BlockNumber}
    };

    pub(crate) const SMALL_EVENT_CHALLENGE_PERIOD: BlockNumber = 5 * MINUTES;
    pub(crate) const NORMAL_EVENT_CHALLENGE_PERIOD: BlockNumber = 20 * MINUTES;

    pub(crate) const SMALL_VOTING_PERIOD: BlockNumber = 20 * MINUTES;
    pub(crate) const NORMAL_VOTING_PERIOD: BlockNumber = 30 * MINUTES;

    pub(crate) const HALF_HOUR_SCHEDULE_PERIOD: BlockNumber = 30 * MINUTES;
    pub(crate) const FOUR_HOURS_SCHEDULE_PERIOD: BlockNumber = 4 * HOURS;

    pub(crate) const AVT_ENDOWMENT: Balance = 10_000 * AVT;
    pub(crate) const COLLATOR_DEPOSIT: Balance = 2_000 * AVT;

    pub const QUORUM_FACTOR: u32 = 3;
}

pub(crate) mod helpers {

    use crate::chain_spec::{AuraId, AuthorityDiscoveryId, AvnId, ImOnlineId};
    use sp_avn_common::primitives::{AccountId, Signature};
    use sp_core::{sr25519, Pair, Public};
    use sp_runtime::traits::{IdentifyAccount, Verify};

    pub type AccountPublic = <Signature as Verify>::Signer;

    /// The default XCM version to set in genesis config.
    pub const SAFE_XCM_VERSION: u32 = xcm::prelude::XCM_VERSION;

    /// Helper function to generate a crypto pair from seed
    pub fn get_public_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
        TPublic::Pair::from_string(&format!("//{}", seed), None)
            .expect("static values are valid; qed")
            .public()
    }

    /// Helper function to generate a crypto pair from seed without any derivation
    pub fn get_public_from_seed_no_derivation<TPublic: Public>(
        seed: &str,
    ) -> <TPublic::Pair as Pair>::Public {
        TPublic::Pair::from_string(&format!("{}", seed), None)
            .expect("static values are valid; qed")
            .public()
    }

    /// Generate collator keys from seed.
    ///
    /// This function's return type must always match the session keys of the chain in tuple format.
    pub fn get_collator_session_keys_from_seed(
        seed: &str,
    ) -> (AuraId, AuthorityDiscoveryId, ImOnlineId, AvnId) {
        (
            get_public_from_seed::<AuraId>(seed),
            get_public_from_seed::<AuthorityDiscoveryId>(seed),
            get_public_from_seed::<ImOnlineId>(seed),
            get_public_from_seed::<AvnId>(seed),
        )
    }

    fn get_seed_with_extra_derivation(seed: &str, extra_derivation: &str) -> String {
        let mut derived_seed = seed.to_owned();
        derived_seed.push_str("//");
        derived_seed.push_str(extra_derivation);

        return derived_seed
    }

    /// Generate collator keys from seed using the name of the key as extra derivation path.
    ///
    /// This function's return type must always match the session keys of the chain in tuple format.
    pub fn get_collator_session_keys_from_seed_with_extra_derivation(
        seed: &str,
    ) -> (AuraId, AuthorityDiscoveryId, ImOnlineId, AvnId) {
        (
            get_public_from_seed::<AuraId>(get_seed_with_extra_derivation(seed, "aura").as_str()),
            get_public_from_seed::<AuthorityDiscoveryId>(
                get_seed_with_extra_derivation(seed, "audi").as_str(),
            ),
            get_public_from_seed::<ImOnlineId>(
                get_seed_with_extra_derivation(seed, "imon").as_str(),
            ),
            get_public_from_seed::<AvnId>(get_seed_with_extra_derivation(seed, "avnk").as_str()),
        )
    }

    /// Helper function to generate an account ID from seed
    pub fn get_account_id_from_seed<TPublic: Public>(seed: &str) -> AccountId
    where
        AccountPublic: From<<TPublic::Pair as Pair>::Public>,
    {
        AccountPublic::from(get_public_from_seed::<TPublic>(seed)).into_account()
    }

    /// Helper function to generate an account ID from seed
    pub fn get_account_id_from_seed_no_derivation<TPublic: Public>(seed: &str) -> AccountId
    where
        AccountPublic: From<<TPublic::Pair as Pair>::Public>,
    {
        AccountPublic::from(get_public_from_seed_no_derivation::<TPublic>(seed)).into_account()
    }

    /// Helper function to return the authority keys for a seed
    pub fn get_authority_keys_from_seed(
        seed: &str,
    ) -> (AccountId, AuraId, AuthorityDiscoveryId, ImOnlineId, AvnId) {
        let (aura, audi, imon, avn) = get_collator_session_keys_from_seed(seed);
        return (get_account_id_from_seed::<sr25519::Public>(seed), aura, audi, imon, avn)
    }

    /// Helper function to return the authority keys for a seed with extra derivation
    pub fn get_authority_keys_from_seed_with_derivation(
        seed: &str,
    ) -> (AccountId, AuraId, AuthorityDiscoveryId, ImOnlineId, AvnId) {
        let (aura, audi, imon, avn) =
            get_collator_session_keys_from_seed_with_extra_derivation(seed);
        return (get_account_id_from_seed::<sr25519::Public>(seed), aura, audi, imon, avn)
    }
}

/// The extensions for the [`ChainSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ChainSpecGroup, ChainSpecExtension)]
pub struct Extensions {
    /// The relay chain of the Parachain.
    #[serde(alias = "relayChain", alias = "RelayChain")]
    pub relay_chain: String,
    /// The id of the Parachain.
    #[serde(alias = "paraId", alias = "ParaId")]
    pub para_id: u32,
}

impl Extensions {
    /// Try to get the extension from the given `ChainSpec`.
    pub fn try_get(chain_spec: &dyn sc_service::ChainSpec) -> Option<&Self> {
        sc_chain_spec::get_extension(chain_spec.extensions())
    }
}

/// Sets currency to AVT for an AvN chain
pub(crate) fn avn_chain_properties() -> sc_chain_spec::Properties {
    // Give your base currency a unit name and decimal places
    let mut properties = sc_chain_spec::Properties::new();
    properties.insert("tokenSymbol".into(), "AVT".into());
    properties.insert("tokenDecimals".into(), 18.into());
    properties.insert("ss58Format".into(), 42.into());
    // TODO: Replace with this when we switch to using custom prefixes
    // properties.insert("ss58Format".into(), 65.into());
    return properties
}

fn local_ethereum_public_keys() -> Vec<EthPublicKey> {
    /*
        The following test public keys are generated with 12 word mnemonic:
        decade mask flag steak negative eagle crunch sea evoke rack drive print

        Derivation			Address										Public key																Private key
        m/44'/60'/0'/0/0	0xFB509cFaCE208C271f9fBEAC5ac39cEC245e3587	0x03471b4c1012dddf4d494c506a098c7b1b719b20bbb177b1174f2166f953c29503	0x35f071b8e86c710b71c5cdbf95a9a146e152e64e7b13e3d15d278cab5f1ec517
        m/44'/60'/0'/0/1	0x5ebf86c1749bbF45Bf0dAb36B1E4D81836e03953	0x0292a73ad9488b934fd04cb31a0f50634841f7105a5b4a8538e4bfa06aa477bed6	0xbaf9b03f2e80afc21ffe0bbb908d25cc8e99be3e60f8485d8a7c8fd813dc8606
        m/44'/60'/0'/0/2	0x97cD8BC20EF2f9bE4eaf2EA52906d71C4aeaa766	0x03c5527886d8e09ad1fededd3231f890685d2d5345385d54181269f80c8926ff8e	0x4f751d57b6595fcfe851774a7dd281b17061bd9d9e4b0551f870013aff6df790
        m/44'/60'/0'/0/3	0xDef5375191e70257ea8cdc1246Dfdd295046b477	0x020e7593c534411f6f0e2fb91340751ada34ee5986f70b300443be17844416b28b	0xfa9bc1b70b739012ba34d0530dcbd09cbbd5ada78a52ec8d5c7ea30758eb559e
        m/44'/60'/0'/0/4	0xa06964bAC7b3A9cC488241e7419f617e469fBf24	0x02fde5665a2cb42863fb312fb527f2b02110997fc6865df583ca4324be137b7894	0x09a7205005ad40afa7c590b5fa6d26fabe23f07790f758ac06a7c9fbc56d06ae
        m/44'/60'/0'/0/5	0x54A26b5082F3b26d5472E080c54372aB5c1D867F	0x031f8860a4f05ec62077a97d37af60f0229b775b98946efcb92998522abefc1b6c	0xfaf4f3166d415b7f356655434d808962442471e98e11d973bd5270a71e39a13b
    */
    return vec![
        ecdsa::Public::from_slice(&hex![
            "03471b4c1012dddf4d494c506a098c7b1b719b20bbb177b1174f2166f953c29503"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "0292a73ad9488b934fd04cb31a0f50634841f7105a5b4a8538e4bfa06aa477bed6"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "03c5527886d8e09ad1fededd3231f890685d2d5345385d54181269f80c8926ff8e"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "020e7593c534411f6f0e2fb91340751ada34ee5986f70b300443be17844416b28b"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "02fde5665a2cb42863fb312fb527f2b02110997fc6865df583ca4324be137b7894"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "031f8860a4f05ec62077a97d37af60f0229b775b98946efcb92998522abefc1b6c"
        ])
        .unwrap(),
    ]
}
