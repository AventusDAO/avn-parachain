use crate::chain_spec::{avn_chain_properties, constants::*, helpers::*, ChainType, Extensions};

use crate::chain_spec::{
    local_ethereum_public_keys,
    test::{avn_test_runtime_genesis, get_account_id_from_seed, ChainSpec},
};
use hex_literal::hex;
use sp_core::{sr25519, H160};

pub fn avn_garde_local_config() -> ChainSpec {
    let parachain_id: u32 = 2000;
    let properties = avn_chain_properties();

    ChainSpec::builder(
        avn_test_runtime::WASM_BINARY.expect("WASM binary was not built, please build it!"),
        Extensions {
            relay_chain: "rococo-local".into(),
            // You MUST set this to the correct network!
            para_id: parachain_id,
        },
    )
    .with_name("AvN Garde Local Parachain")
    .with_protocol_id("avn_garde_local")
    .with_id("avn_garde_local")
    .with_properties(properties)
    .with_chain_type(ChainType::Development)
    .with_genesis_config_patch(avn_test_runtime_genesis(
        // initial collators.
        vec![
            get_authority_keys_from_seed("Eve"),
            get_authority_keys_from_seed("Ferdie"),
            get_authority_keys_from_seed("Dave"),
            get_authority_keys_from_seed("Charlie"),
        ],
        vec![
            (get_account_id_from_seed::<sr25519::Public>("Alice"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Bob"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Charlie"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Dave"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Eve"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Ferdie"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Alice//stash"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Bob//stash"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Charlie//stash"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Dave//stash"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Eve//stash"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Ferdie//stash"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Bank"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("gateway-relayer"), AVT_ENDOWMENT),
            // Use in avn-proxy benchmarks
            (
                get_account_id_from_seed_no_derivation::<sr25519::Public>(
                    "kiss mule sheriff twice make bike twice improve rate quote draw enough",
                ),
                AVT_ENDOWMENT,
            ),
            (get_account_id_from_seed::<sr25519::Public>("nft-marketplace-relayer"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("onboarding-relayer"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("test-account"), AVT_ENDOWMENT),
        ],
        parachain_id.into(),
        // SUDO account
        get_account_id_from_seed::<sr25519::Public>("Ferdie"),
        // AVT contract
        H160(hex!("93ba86eCfDDD9CaAAc29bE83aCE5A3188aC47730")),
        // AVN contract
        H160(hex!("9d6394ea67D297b4Fc777f719F82Ae1F1fc06383")),
        vec![],
        local_ethereum_public_keys(),
        vec![],
        SMALL_EVENT_CHALLENGE_PERIOD,
        HALF_HOUR_SCHEDULE_PERIOD,
        SMALL_VOTING_PERIOD,
        // Non AVT token address
        Some(H160(hex!("ea5da4fd16cc61ffc4235874d6ff05216e3e038e"))),
    ))
    .build()
}
