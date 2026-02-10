use crate::chain_spec::{avn_chain_properties, constants::*, ChainSpec, ChainType, Extensions};

use crate::chain_spec::{
    local_ethereum_public_keys,
    stable::{
        get_account_id_from_seed, get_account_id_from_seed_no_derivation,
        get_authority_keys_from_seed, testnet_genesis,
    },
};
use hex_literal::hex;
use sp_core::{sr25519, H160};

pub fn development_config() -> ChainSpec {
    let dev_rococo_parachain_id: u32 = 2060;
    let properties = avn_chain_properties();

    ChainSpec::builder(
        avn_parachain_runtime::WASM_BINARY.expect("WASM binary was not built, please build it!"),
        Extensions {
            relay_chain: RELAY_CHAIN.into(),
            // You MUST set this to the correct network!
            para_id: dev_rococo_parachain_id,
        },
    )
    .with_name("Development")
    .with_protocol_id("template-dev")
    .with_id("dev")
    .with_properties(properties)
    .with_chain_type(ChainType::Development)
    .with_genesis_config_patch(testnet_genesis(
        // initial collators.
        vec![get_authority_keys_from_seed("Alice//stash"), get_authority_keys_from_seed("Ferdie")],
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
        dev_rococo_parachain_id.into(),
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

pub fn local_testnet_config() -> ChainSpec {
    let properties = avn_chain_properties();
    ChainSpec::builder(
        avn_parachain_runtime::WASM_BINARY.expect("WASM binary was not built, please build it!"),
        Extensions {
            relay_chain: RELAY_CHAIN.into(),
            // You MUST set this to the correct network!
            para_id: 2000,
        },
    )
    .with_name("AvN Local Parachain")
    .with_protocol_id("avn-local")
    .with_id("local-testnet")
    .with_properties(properties)
    .with_chain_type(ChainType::Local)
    .with_genesis_config_patch(testnet_genesis(
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
        2000.into(),
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
