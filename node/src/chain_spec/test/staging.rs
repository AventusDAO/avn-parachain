use crate::chain_spec::{
    avn_chain_properties, constants::*, ChainSpec, ChainType, EthPublicKey, Extensions,
};

use crate::chain_spec::test::{
    avn_test_runtime_genesis, get_account_id_from_seed,
    get_authority_keys_from_seed_with_derivation,
};
use hex_literal::hex;
use sp_core::{ecdsa, sr25519, ByteArray, H160};

pub fn avn_garde_staging_config() -> ChainSpec {
    let avn_garde_staging_parachain_id: u32 = 3150;
    let properties = avn_chain_properties();

    ChainSpec::builder(
        avn_test_runtime::WASM_BINARY.expect("WASM binary was not built, please build it!"),
        Extensions {
            relay_chain: RELAY_CHAIN.into(),
            // You MUST set this to the correct network!
            para_id: avn_garde_staging_parachain_id,
        },
    )
    .with_name("AvN Garde Staging Parachain")
    .with_protocol_id("avn_garde_staging")
    .with_id("avn_garde_staging")
    .with_properties(properties)
    .with_chain_type(ChainType::Live)
    .with_genesis_config_patch(avn_test_runtime_genesis(
        // initial collators.
        vec![
            get_authority_keys_from_seed_with_derivation("avn-collator-1"),
            get_authority_keys_from_seed_with_derivation("avn-collator-2"),
            get_authority_keys_from_seed_with_derivation("avn-collator-3"),
            get_authority_keys_from_seed_with_derivation("avn-collator-4"),
        ],
        // endowed accounts
        vec![
            (get_account_id_from_seed::<sr25519::Public>("avn-collator-1"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("avn-collator-2"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("avn-collator-3"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("avn-collator-4"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("avn-sudo"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("Bank"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("gateway-relayer"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("nft-marketplace-relayer"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("onboarding-relayer"), AVT_ENDOWMENT),
            (get_account_id_from_seed::<sr25519::Public>("test-account"), AVT_ENDOWMENT),
        ],
        avn_garde_staging_parachain_id.into(),
        // SUDO account
        get_account_id_from_seed::<sr25519::Public>("avn-sudo"),
        // AVT contract
        H160(hex!("b3594297e1F257AD2A90222F66393645C6622263")),
        // AVN contract
        H160(hex!("2bb59e4f9Cd053779E5d6f6dB2724F5DF5e53ce6")),
        vec![],
        avn_garde_ethereum_public_keys(),
        vec![],
        SMALL_EVENT_CHALLENGE_PERIOD,
        HALF_HOUR_SCHEDULE_PERIOD,
        SMALL_VOTING_PERIOD,
        None,
    ))
    .build()
}

fn avn_garde_ethereum_public_keys() -> Vec<EthPublicKey> {
    return vec![
        // 0x5874c84e050185B8317948078c7788a4b6844f07
        ecdsa::Public::from_slice(&hex![
            "020926e3eed6a3595b7c1bdea0e4e4575a19a321360b78951257403adbbbdea1bd"
        ])
        .unwrap(),
        // 0x164067E9600052267C667Ca66b852bCd6C5CEe29
        ecdsa::Public::from_slice(&hex![
            "021ab260b53b4e90b4d8b2dd53565a17fb2878b195f36cd6d4e5fcd3cb43097c00"
        ])
        .unwrap(),
        // 0xCC0D92edFAa9eD0Ad2B254160aD64e9A97925074
        ecdsa::Public::from_slice(&hex![
            "0275d15c4be8c8006865cb6043c05d6359706db7dc9b4585c3cfee51eba96a8799"
        ])
        .unwrap(),
        // 0x777991BBb8e232c13c1425Ec0A7FFCE9C109B909
        ecdsa::Public::from_slice(&hex![
            "032ec3b54b8a2c351770e811aaef70917ce1c8e7fdf9151d365b778679819a8152"
        ])
        .unwrap(),
    ];
}
