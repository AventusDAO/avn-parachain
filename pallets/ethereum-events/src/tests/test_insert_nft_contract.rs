// Copyright 2023 Aventus (UK) Ltd.
#![cfg(test)]

use crate::{mock::*, *};
use frame_support::assert_ok;
use frame_system::RawOrigin;

mod test_set_ethereum_contract {

    use super::*;

    struct Context {
        origin: RuntimeOrigin,
        new_contract_address: H160,
        ethereum_contract: EthereumContracts,
    }

    impl Default for Context {
        fn default() -> Self {
            Context {
                origin: RawOrigin::Root.into(),
                new_contract_address: H160::from([15u8; 20]),
                ethereum_contract: EthereumContracts::NftMarketplace,
            }
        }
    }

    impl Context {
        fn dispatch_map_nft_contract(&self, address: H160) -> DispatchResult {
            return EthereumEvents::insert_nft_contract(self.origin.clone(), address.clone());
        }
    }

    mod successful_cases {
        use super::*;
        #[test]
        fn insert_nft_marketplace_contract_for_reserved_marketplace() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let mut context = Context::default();
                context.ethereum_contract = EthereumContracts::NftMarketplace;
                assert_eq!(
                    false,
                    NftT1Contracts::<TestRuntime>::contains_key(context.new_contract_address)
                );

                assert_ok!(context.dispatch_map_nft_contract(context.new_contract_address));
                assert_eq!(
                    true,
                    NftT1Contracts::<TestRuntime>::contains_key(context.new_contract_address)
                );
            });
        }
    }
}
