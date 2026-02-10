use crate::chain_spec::{
    avn_chain_properties, constants::*, AuraId, AuthorityDiscoveryId, AvnId, ChainSpec, ChainType,
    EthPublicKey, Extensions, ImOnlineId,
};

use crate::chain_spec::stable::{
    get_account_id_from_seed, get_authority_keys_from_seed_with_derivation, testnet_genesis,
};
use hex_literal::hex;
use sp_avn_common::primitives::AccountId;
use sp_core::{crypto::UncheckedInto, ecdsa, sr25519, ByteArray, H160};

pub fn staging_testnet_config() -> ChainSpec {
    let staging_parachain_id: u32 = 3000;
    let properties = avn_chain_properties();

    ChainSpec::builder(
        avn_parachain_runtime::WASM_BINARY.expect("WASM binary was not built, please build it!"),
        Extensions {
            relay_chain: RELAY_CHAIN.into(),
            // You MUST set this to the correct network!
            para_id: staging_parachain_id,
        },
    )
    .with_name("AvN Staging Parachain")
    .with_protocol_id("avn-staging-testnet")
    .with_id("staging-parachain")
    .with_properties(properties)
    .with_chain_type(ChainType::Live)
    .with_genesis_config_patch(testnet_genesis(
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
        staging_parachain_id.into(),
        // SUDO account
        get_account_id_from_seed::<sr25519::Public>("avn-sudo"),
        // AVT contract
        H160(hex!("b3594297e1F257AD2A90222F66393645C6622263")),
        // AVN contract
        H160(hex!("2bb59e4f9Cd053779E5d6f6dB2724F5DF5e53ce6")),
        vec![],
        staging_ethereum_public_keys(),
        vec![],
        SMALL_EVENT_CHALLENGE_PERIOD,
        HALF_HOUR_SCHEDULE_PERIOD,
        SMALL_VOTING_PERIOD,
        None,
    ))
    .build()
}

pub fn staging_dev_testnet_config() -> ChainSpec {
    let staging_parachain_id: u32 = 2000;
    let properties = avn_chain_properties();

    let mut endowed_accounts = vec![
        // SUDO account
        (
            hex!["20ef357ca657d8cce8fcfc2e230871347fc68b1451a575eaedb9797616101608"].into(),
            AVT_ENDOWMENT,
        ),
        (get_account_id_from_seed::<sr25519::Public>("Bank"), AVT_ENDOWMENT),
        (get_account_id_from_seed::<sr25519::Public>("gateway-relayer"), AVT_ENDOWMENT),
    ];
    endowed_accounts.append(&mut staging_dev_endowed_collators());

    ChainSpec::builder(
        avn_parachain_runtime::WASM_BINARY.expect("WASM binary was not built, please build it!"),
        Extensions {
            relay_chain: RELAY_CHAIN.into(),
            // You MUST set this to the correct network!
            para_id: staging_parachain_id,
        },
    )
    .with_name("AvN Staging Dev Parachain")
    .with_protocol_id("staging-dev")
    .with_id("avn_staging_dev_testnet")
    .with_properties(properties)
    .with_chain_type(ChainType::Live)
    .with_genesis_config_patch(testnet_genesis(
        // initial collators.
        staging_uat_authorities_keys(),
        // endowed accounts
        endowed_accounts.clone(),
        staging_parachain_id.into(),
        // SUDO account
        hex!["20ef357ca657d8cce8fcfc2e230871347fc68b1451a575eaedb9797616101608"].into(),
        // AVT contract
        H160(hex!("93ba86eCfDDD9CaAAc29bE83aCE5A3188aC47730")),
        // AVN contract
        H160(hex!("0Dd31348e68b6400bf8BdE84a1AaF733D9fCBf9B")),
        // TODO update this if needed with the nft contracts
        vec![],
        staging_dev_ethereum_public_keys(),
        vec![],
        NORMAL_EVENT_CHALLENGE_PERIOD,
        FOUR_HOURS_SCHEDULE_PERIOD,
        NORMAL_VOTING_PERIOD,
        None,
    ))
    .build()
}

#[rustfmt::skip]
fn staging_uat_authorities_keys(
) -> Vec<(AccountId, AuraId, AuthorityDiscoveryId, ImOnlineId, AvnId)> {
	let initial_authorities: Vec<(AccountId, AuraId, AuthorityDiscoveryId, ImOnlineId, AvnId)> = vec![
		(
			// "5DHXA2pDmGMg7299T8E1p7eDpiJe2mzNw6EpBe9GgHKkunkL"
			hex!["360266f4b3459999815e852baacfc9a407c1a622a648329a33f12f87158a156c"].into(),
			// "5DUxNN67NaenKi1LADKGXBC46vVodsZtnNqjbiYpdowGZoa2"
			hex!["3ebaf99202ea4751a02d94a6e53bc5f8810367292a28509d851cb7b7fb24b17c"].unchecked_into(),
			// "5GE2hTvAt9UnVpawoA8jpQoi6nBJZ8h1koicyFFTSPiCiA8Y"
			hex!["b80d91661886153b8296773d53953bf3c295c27d513d2e96ab9920bd2c3ffc39"].unchecked_into(),
			// "5GRkFDth1P6GpEdKoZBB6MUoYKJ2f6rfmdKEdzDyqACRqi44"
			hex!["c0fd2102ce3b51a4876d03777b4e8bf0edd71eba6a56a6b40443e5373ef74409"].unchecked_into(),
			// "5F9icRubb1RM4XgjY47vK2BSxEXTat2vyNuqGVvGEgn6mJyu"
			hex!["8887779fd808805bb4e8265176f554236889376f801005baf31123a0ddb9be63"].unchecked_into(),
		),
		(
			// "5CB8eZjJraPR7eJCLwNoBsutAKwunKwQQS9tWUHYUAG7HbnR"
			hex!["04e6ed6c8781ed042fe630e120da380fb5210e46179904d7987e2e85c701fc0b"].into(),
			// "5DvSXHvN9V3gWfQGfXxmjvwfpjBhbLxo15YJfEvuoLz9yhpL"
			hex!["522ae273ed152fa7f9d6a93a3f52dea9761ede2649820046d302bc206ae65c7d"].unchecked_into(),
			// "5HGjPbVgd6DsJCCPAT4uRNCHJKBinjd2BjFURGoXsAYh8hdq"
			hex!["e65945fd6a660c3b8cf552131a60c9f18f4cd05fc0ef3a7b667a96e78e19ec6b"].unchecked_into(),
			// "5EL8CChoaLZQojgFRsbqvekWu91GWJXSU9EE1wXxp5HsHMvM"
			hex!["643b12aa4d4a0ccd3b1bdd6fe290ceb05dd957320e0a7cf1c05c533215b2b878"].unchecked_into(),
			// "5HYFu7obpyq1Jzxtt3RF3ANTBRmqrDyKYczzXeUjFv9byp4U"
			hex!["f230a82d935663553e848abf54ac83765aef68e25ccd1c50f704349f89a8bc7f"].unchecked_into(),
		),
		(
			// "5GNCn4Tyb3sgCF95XMRURd7dW44gzbU1pihAZQWF1N1ikyVV"
			hex!["be4975ff88596c42528ef74dd41816692489966385c95a0c8c9fdebf6d9b9867"].into(),
			// "5EnmkshjpmYTU2umWYXANyK1tK1u4Tn2GNDcNBr4nHHkapVC"
			hex!["788de71f55193159c7de82af9f191d4bc86bee2cbcc148e60d1efecf66e8d405"].unchecked_into(),
			// "5GxJhgoGpA6XjKYRoNJWvckXJdEWHvzBJKCBattkQk34xPCc"
			hex!["d84bbfa6c3e0a60ef432b58341691b1d1a6f97dc158370f0c8e64b5325dd7b1b"].unchecked_into(),
			// "5HmYGDDo7Q9AgcisaF4Fpbi61QAMpBo4f5XJttVAHVhSiqAo"
			hex!["fc51eea8648d5af55198e603913a7a6f71c5d176c10caf2576e5a8637470e264"].unchecked_into(),
			// "5GGrn8RKxwvAmgNgimfgAF4EQ6Cub4B9YJPKPgUvXejieFeh"
			hex!["ba35e96fecd8bc90bf24806f1cdb3d2bb638c69e7c23cb75f995210b398cad63"].unchecked_into(),
		),
		(
			// "5F6tYYDzZvGk5UN621xEt5zda82AwmUqSb3Q85nmkRa7JSMj"
			hex!["865f2b11569750318e4d1be1c5e3cc901c1552b1a6e47d5f3b635f0e70fdd976"].into(),
			// "5GxVqfVAi8K99tS1h2N7ibtfDeFpy5MDBdNpsHWB33r2KE7x"
			hex!["d8713dae7793dc13ccb1c133fc70455ad527dbe23f9e5535a99d1f9220e19536"].unchecked_into(),
			// "5DjxSHunKNnyU4sKreX4GovTeiTLSsuZBFTeXS99AwsBWB8i"
			hex!["4a2be2490fa305bcbcb97a654a2fae91352e81143bb070ebf24f924a00247256"].unchecked_into(),
			// "5CvxfoqGXffmbDNB8oZpfpzw6Vv3Z9vwcXBMu69PuX6yW1Tz"
			hex!["26542266c298d8e6ad19571fb63135a2fccf01aed16ee09c1b603b493ace7e6f"].unchecked_into(),
			// "5FmkFtKTeCGVuCEkvzE4mA16pEPWHEh11nwWUtiTjFbsLFHb"
			hex!["a401d78fb6c1e1af53fa5db8ba16de70e69a73ea9c1113fc72fdefcdddcdc77a"].unchecked_into(),
		),
		(
			// "5HdTonR7WyEtwARFHfoDWqZBZHQw5W1jzXXnzdgknViqiYyh"
			hex!["f628f84fe150472007ae73e7ee6e88cfc5337f21d23dfbf729a35e3c45273f5f"].into(),
			// "5E7tNMadujg67YBdoJdpi34jXXhTCBPvUQxCLRHTphJcL8aj"
			hex!["5ae596d1aafc413e27da807422f381b089dfb82325a2f6a5a3960f04c2e8b75b"].unchecked_into(),
			// "5FYgSUwPvSscRGxuN8ZWWzf7T9jHHjgQGPBSTbKPZVUyiHiB"
			hex!["9a0acd36eeee60f3244a0791a83ee75455ed6dc1c3a2337147b3da1094e93742"].unchecked_into(),
			// "5F9wCjebDNLFneY6JTnbtifnpSA91ir7me42JiKvCoqHinbs"
			hex!["88b1da9ec96f2f932631c096c53d821314977edc98ce0acf9fc04d027d135847"].unchecked_into(),
			// "5DMKf2VpXEg7gU8HaaeTEnaDhpAhqRwSRtkjzowFZrpQQzWW"
			hex!["38e8abc7fee22accfed02f130334c2ab9e73fd0d381a50fd3794cc46973b6244"].unchecked_into(),
		),
	];
	return initial_authorities;
}

pub(crate) fn staging_dev_endowed_collators() -> Vec<(AccountId, Balance)> {
    return vec![
        (
            hex!["360266f4b3459999815e852baacfc9a407c1a622a648329a33f12f87158a156c"].into(),
            AVT_ENDOWMENT,
        ),
        (
            hex!["04e6ed6c8781ed042fe630e120da380fb5210e46179904d7987e2e85c701fc0b"].into(),
            AVT_ENDOWMENT,
        ),
        (
            hex!["be4975ff88596c42528ef74dd41816692489966385c95a0c8c9fdebf6d9b9867"].into(),
            AVT_ENDOWMENT,
        ),
        (
            hex!["865f2b11569750318e4d1be1c5e3cc901c1552b1a6e47d5f3b635f0e70fdd976"].into(),
            AVT_ENDOWMENT,
        ),
        (
            hex!["f628f84fe150472007ae73e7ee6e88cfc5337f21d23dfbf729a35e3c45273f5f"].into(),
            AVT_ENDOWMENT,
        ),
    ]
}

fn staging_ethereum_public_keys() -> Vec<EthPublicKey> {
    return vec![
        ecdsa::Public::from_slice(&hex![
            "02a2e1cf626313d269e0ab9e3153aeddc6d18ebcf25105e1478cee8307d854f7dc"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "02718ca6f257b752f7222920a8187d1af65c940bc8aee17129d29868e6eb796162"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "03a51c9e4dab3516b44366978f5ec53f627cf506bfc9915530299dc2d793d169d4"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "03cc53fb89f0422a38d2f4edce815cf7f329b14775a248d7b1960f595cd6c7c80c"
        ])
        .unwrap(),
    ]
}

fn staging_dev_ethereum_public_keys() -> Vec<EthPublicKey> {
    return vec![
        ecdsa::Public::from_slice(&hex![
            "0302881488d500e6bb2c667b1c68fee4d8ec0c0ccd39c0ab6796e163345dcb1083"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "03314ec7f7195896c20d896f1632bfe08ef3eb8d487517fcdae7688952e74354de"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "031d86d6fc4315fde389ba45618ed01146141933509b5118f6997c7ead767b689a"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "0245f9ddcd817ba8aeb0c343d7bc8b463051a3258538ef9c2a4c66588b8ec9a6d5"
        ])
        .unwrap(),
        ecdsa::Public::from_slice(&hex![
            "02c916035bfc3ad6f7234816d37307578d0f7d0b848f13b8c15d307c1856394b48"
        ])
        .unwrap(),
    ]
}
