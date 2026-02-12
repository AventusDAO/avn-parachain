// Copyright 2022 Aventus (UK) Ltd.
#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::traits::BadOrigin;

pub fn event_emitted(old_contract: H160, new_contract: H160) -> bool {
    return System::events().iter().any(|a| {
        return a.event
            == RuntimeEvent::Avn(Event::AvnBridgeContractUpdated { old_contract, new_contract });
    });
}

mod test_set_bridge_contract {

    use super::*;

    struct Context {
        origin: RuntimeOrigin,
        new_contract_address: H160,
    }

    impl Default for Context {
        fn default() -> Self {
            Context { origin: RawOrigin::Root.into(), new_contract_address: H160::from([15u8; 20]) }
        }
    }

    impl Context {
        fn dispatch_set_bridge_contract(&self, address: H160) -> DispatchResult {
            return AVN::set_bridge_contract(self.origin.clone(), address.clone());
        }
    }

    mod successful_cases {
        use super::*;
        #[test]
        fn update_contract() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                let new_address = context.new_contract_address;
                assert_ne!(new_address, AVN::get_bridge_contract_address());
                assert_ok!(context.dispatch_set_bridge_contract(new_address));
                assert_eq!(new_address, AVN::get_bridge_contract_address());
                assert_eq!(true, event_emitted(CUSTOM_BRIDGE_CONTRACT, new_address));
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn zero_contract_should_fail() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();
                let invalid_contract_address = H160::zero();
                assert_noop!(
                    context.dispatch_set_bridge_contract(invalid_contract_address),
                    Error::<TestRuntime>::InvalidContractAddress
                );
            });
        }

        #[test]
        fn origin_is_not_root() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context: Context = Context {
                    origin: RuntimeOrigin::signed(Default::default()),
                    ..Default::default()
                };
                let new_address = context.new_contract_address;
                assert_noop!(context.dispatch_set_bridge_contract(new_address), BadOrigin);
            });
        }

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context: Context =
                    Context { origin: RawOrigin::None.into(), ..Default::default() };
                let new_address = context.new_contract_address;
                assert_noop!(context.dispatch_set_bridge_contract(new_address), BadOrigin);
            });
        }
    }
}
