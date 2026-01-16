//Copyright 2025 Truth Network.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::DispatchError;

#[test]
fn origin_is_checked_none() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let current_period = MinVotingPeriod::<TestRuntime>::get();
        let new_period = current_period + 1;

        let config = AdminConfig::MinVotingPeriod(new_period);
        assert_noop!(
            Watchtower::set_admin_config(RawOrigin::None.into(), config,),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn origin_is_checked_signed() {
    let mut ext = ExtBuilder::build_default().as_externality();
    ext.execute_with(|| {
        let current_period = MinVotingPeriod::<TestRuntime>::get();
        let new_period = current_period + 1;

        let config = AdminConfig::MinVotingPeriod(new_period);
        let bad_signer = TestAccount::new([99u8; 32]).account_id();
        assert_noop!(
            Watchtower::set_admin_config(RuntimeOrigin::signed(bad_signer.clone()), config,),
            DispatchError::BadOrigin
        );
    });
}

mod min_voting_period {
    use super::*;

    #[test]
    fn can_be_set() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let current_period = MinVotingPeriod::<TestRuntime>::get();
            let new_period = current_period + 1;

            let config = AdminConfig::MinVotingPeriod(new_period);
            assert_ok!(Watchtower::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::MinVotingPeriodSet { new_period }.into());
        });
    }
}

mod admin_account {
    use super::*;

    #[test]
    fn can_be_set() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let current_admin = AdminAccount::<TestRuntime>::get();
            let new_admin = TestAccount::new([79u8; 32]).account_id();

            assert!(current_admin != Some(new_admin.clone()));

            let config = AdminConfig::AdminAccount(Some(new_admin));
            assert_ok!(Watchtower::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::AdminAccountSet { new_admin: Some(new_admin) }.into());
        });
    }

    #[test]
    fn can_be_set_to_none() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let config = AdminConfig::AdminAccount(Some(TestAccount::new([69u8; 32]).account_id()));
            assert_ok!(Watchtower::set_admin_config(RawOrigin::Root.into(), config,));

            let current_admin: Option<AccountId> = AdminAccount::<TestRuntime>::get();
            let new_admin: Option<AccountId> = None;

            assert!(current_admin != new_admin);

            let config = AdminConfig::AdminAccount(new_admin);
            assert_ok!(Watchtower::set_admin_config(RawOrigin::Root.into(), config,));
            System::assert_last_event(Event::AdminAccountSet { new_admin: None }.into());
        });
    }
}
