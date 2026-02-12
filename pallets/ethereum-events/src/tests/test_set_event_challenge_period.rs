// Copyright 2021 Aventus (UK) Ltd.
#![cfg(test)]

use crate::{
    mock::{RuntimeEvent as Event, *},
    *,
};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::traits::BadOrigin;

mod test_set_event_challenge_period {
    use super::*;

    struct Context {
        origin: RuntimeOrigin,
        new_event_challenge_period: BlockNumber,
    }

    impl Default for Context {
        fn default() -> Self {
            Context { origin: RawOrigin::Root.into(), new_event_challenge_period: 1200 }
        }
    }

    impl Context {
        fn dispatch_set_event_challenge_period(&self) -> DispatchResult {
            return EthereumEvents::set_event_challenge_period(
                self.origin.clone(),
                self.new_event_challenge_period.clone(),
            );
        }

        fn event_challenge_period_updated_emitted(&self) -> bool {
            return System::events().iter().any(|a| {
                a.event
                    == Event::EthereumEvents(
                        crate::Event::<TestRuntime>::EventChallengePeriodUpdated {
                            block: self.new_event_challenge_period,
                        },
                    )
            });
        }
    }

    mod success_implies {
        use super::*;

        #[test]
        fn event_challenge_period_is_updated() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();

                assert_ne!(
                    context.new_event_challenge_period,
                    EthereumEvents::event_challenge_period()
                );

                assert_ok!(context.dispatch_set_event_challenge_period());

                assert_eq!(
                    context.new_event_challenge_period,
                    EthereumEvents::event_challenge_period()
                );
            });
        }

        #[test]
        fn event_is_emitted() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context = Context::default();

                assert_eq!(false, context.event_challenge_period_updated_emitted());

                assert_ok!(context.dispatch_set_event_challenge_period());

                assert_eq!(true, context.event_challenge_period_updated_emitted());
            });
        }
    }

    mod fails_when {
        use super::*;

        #[test]
        fn origin_is_not_root() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context: Context =
                    Context { origin: RuntimeOrigin::signed(account_id_0()), ..Default::default() };

                assert_noop!(context.dispatch_set_event_challenge_period(), BadOrigin);

                assert_ne!(
                    context.new_event_challenge_period,
                    EthereumEvents::event_challenge_period()
                );
                assert_eq!(false, context.event_challenge_period_updated_emitted());
            });
        }

        #[test]
        fn origin_is_unsigned() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let context: Context =
                    Context { origin: RawOrigin::None.into(), ..Default::default() };

                assert_noop!(context.dispatch_set_event_challenge_period(), BadOrigin);

                assert_ne!(
                    context.new_event_challenge_period,
                    EthereumEvents::event_challenge_period()
                );
                assert_eq!(false, context.event_challenge_period_updated_emitted());
            });
        }

        #[test]
        fn event_challenge_perid_is_invalid() {
            let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
            ext.execute_with(|| {
                let mut context = Context::default();
                context.new_event_challenge_period = (MINIMUM_EVENT_CHALLENGE_PERIOD - 1).into();

                assert_noop!(
                    context.dispatch_set_event_challenge_period(),
                    Error::<TestRuntime>::InvalidEventChallengePeriod
                );

                assert_ne!(
                    context.new_event_challenge_period,
                    EthereumEvents::event_challenge_period()
                );
                assert_eq!(false, context.event_challenge_period_updated_emitted());
            });
        }
    }
}
