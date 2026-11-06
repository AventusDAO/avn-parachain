// Copyright 2026 Aventus DAO.

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

mod trusted_voting_start {
    use super::*;

    #[test]
    fn is_unset_by_default() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            assert_eq!(TrustedVotingStart::<TestRuntime>::get(), None);
        });
    }

    #[test]
    fn can_be_set_and_cleared() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let start = Some(Perbill::from_percent(50));
            let config = AdminConfig::TrustedVotingStart(start);
            assert_ok!(Watchtower::set_admin_config(RawOrigin::Root.into(), config));
            assert_eq!(TrustedVotingStart::<TestRuntime>::get(), start);
            System::assert_last_event(Event::TrustedVotingStartSet { start }.into());

            let config = AdminConfig::TrustedVotingStart(None);
            assert_ok!(Watchtower::set_admin_config(RawOrigin::Root.into(), config));
            assert_eq!(TrustedVotingStart::<TestRuntime>::get(), None);
            System::assert_last_event(Event::TrustedVotingStartSet { start: None }.into());
        });
    }

    #[test]
    fn needs_root() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let config = AdminConfig::TrustedVotingStart(Some(Perbill::from_percent(50)));
            let signer = TestAccount::new([99u8; 32]).account_id();
            assert_noop!(
                Watchtower::set_admin_config(RuntimeOrigin::signed(signer), config),
                DispatchError::BadOrigin
            );
        });
    }
    #[test]
    fn must_be_above_zero_and_below_one_hundred_percent() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            for start in [Perbill::zero(), Perbill::one()] {
                let config = AdminConfig::TrustedVotingStart(Some(start));
                assert_noop!(
                    Watchtower::set_admin_config(RawOrigin::Root.into(), config),
                    Error::<TestRuntime>::InvalidTrustedVotingStart
                );
            }

            for start in [Perbill::from_parts(1), Perbill::from_parts(999_999_999)] {
                let config = AdminConfig::TrustedVotingStart(Some(start));
                assert_ok!(Watchtower::set_admin_config(RawOrigin::Root.into(), config));
                assert_eq!(TrustedVotingStart::<TestRuntime>::get(), Some(start));
            }
        });
    }
}

mod trusted_owners {
    use super::*;

    fn bounded(owners: Vec<AccountId>) -> BoundedVec<AccountId, MaxTrustedOwners> {
        owners.try_into().expect("within MAX_TRUSTED_OWNERS")
    }

    fn add(owners: Vec<AccountId>) -> DispatchResult {
        Watchtower::add_trusted_owners(RawOrigin::Root.into(), bounded(owners))
    }

    fn remove(owners: Vec<AccountId>) -> DispatchResult {
        Watchtower::remove_trusted_owners(RawOrigin::Root.into(), bounded(owners))
    }

    fn trusted() -> Vec<AccountId> {
        TrustedOwners::<TestRuntime>::get().into_inner()
    }

    fn sorted(mut owners: Vec<AccountId>) -> Vec<AccountId> {
        owners.sort();
        owners
    }

    /// `count` accounts that are not node owners, for filling the list directly.
    fn strangers(count: u32) -> Vec<AccountId> {
        (0..count)
            .map(|i| {
                let mut seed = [0xeeu8; 32];
                seed[..4].copy_from_slice(&i.to_le_bytes());
                TestAccount::new(seed).account_id()
            })
            .collect()
    }

    mod adding {
        use super::*;

        #[test]
        fn stores_the_owners_sorted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let owners = vec![watchtower_owner_3(), watchtower_owner_1()];
                assert_ok!(add(owners.clone()));
                assert_eq!(trusted(), sorted(owners.clone()));
                System::assert_last_event(
                    Event::TrustedOwnersAdded { owners: bounded(owners) }.into(),
                );

                assert_ok!(add(vec![watchtower_owner_2()]));
                assert_eq!(
                    trusted(),
                    sorted(vec![watchtower_owner_1(), watchtower_owner_2(), watchtower_owner_3()])
                );
            });
        }

        #[test]
        fn needs_root() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let owners = bounded(vec![watchtower_owner_1()]);
                assert_noop!(
                    Watchtower::add_trusted_owners(RawOrigin::None.into(), owners.clone()),
                    DispatchError::BadOrigin
                );
                assert_noop!(
                    Watchtower::add_trusted_owners(
                        RuntimeOrigin::signed(watchtower_owner_1()),
                        owners
                    ),
                    DispatchError::BadOrigin
                );
            });
        }

        #[test]
        fn rejects_duplicates() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                assert_noop!(
                    add(vec![watchtower_owner_1(), watchtower_owner_2(), watchtower_owner_1()]),
                    Error::<TestRuntime>::DuplicateTrustedOwner
                );
            });
        }

        #[test]
        fn rejects_an_owner_already_trusted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                assert_ok!(add(vec![watchtower_owner_1()]));
                assert_noop!(
                    add(vec![watchtower_owner_2(), watchtower_owner_1()]),
                    Error::<TestRuntime>::AlreadyTrustedOwner
                );
                // Nothing from the failed call was kept.
                assert_eq!(trusted(), vec![watchtower_owner_1()]);
            });
        }

        #[test]
        fn rejects_an_account_without_nodes() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                // A node is not an owner.
                assert_noop!(
                    add(vec![watchtower_owner_1(), watchtower_1()]),
                    Error::<TestRuntime>::TrustedOwnerNotWatchtowerOwner
                );
            });
        }

        #[test]
        fn stops_at_the_maximum() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                TrustedOwners::<TestRuntime>::put(bounded(sorted(strangers(
                    MAX_TRUSTED_OWNERS - 1,
                ))));

                assert_noop!(
                    add(vec![watchtower_owner_1(), watchtower_owner_2()]),
                    Error::<TestRuntime>::TooManyTrustedOwners
                );
                assert_ok!(add(vec![watchtower_owner_1()]));
                assert_eq!(trusted().len() as u32, MAX_TRUSTED_OWNERS);
            });
        }
    }

    mod removing {
        use super::*;

        #[test]
        fn removes_only_the_given_owners() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                assert_ok!(add(vec![
                    watchtower_owner_1(),
                    watchtower_owner_2(),
                    watchtower_owner_3()
                ]));

                let removed = vec![watchtower_owner_3(), watchtower_owner_1()];
                assert_ok!(remove(removed.clone()));
                assert_eq!(trusted(), vec![watchtower_owner_2()]);
                System::assert_last_event(
                    Event::TrustedOwnersRemoved { owners: bounded(removed) }.into(),
                );

                assert_ok!(remove(vec![watchtower_owner_2()]));
                assert!(trusted().is_empty());
            });
        }

        #[test]
        fn needs_root() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                assert_ok!(add(vec![watchtower_owner_1()]));
                let owners = bounded(vec![watchtower_owner_1()]);
                assert_noop!(
                    Watchtower::remove_trusted_owners(RawOrigin::None.into(), owners.clone()),
                    DispatchError::BadOrigin
                );
                assert_noop!(
                    Watchtower::remove_trusted_owners(
                        RuntimeOrigin::signed(watchtower_owner_1()),
                        owners
                    ),
                    DispatchError::BadOrigin
                );
            });
        }

        #[test]
        fn rejects_duplicates() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                assert_ok!(add(vec![watchtower_owner_1(), watchtower_owner_2()]));
                assert_noop!(
                    remove(vec![watchtower_owner_1(), watchtower_owner_1()]),
                    Error::<TestRuntime>::DuplicateTrustedOwner
                );
            });
        }

        #[test]
        fn rejects_an_owner_not_trusted() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                assert_ok!(add(vec![watchtower_owner_1()]));
                assert_noop!(
                    remove(vec![watchtower_owner_1(), watchtower_owner_2()]),
                    Error::<TestRuntime>::NotTrustedOwner
                );
                // Nothing from the failed call was kept.
                assert_eq!(trusted(), vec![watchtower_owner_1()]);
            });
        }

        #[test]
        fn works_for_an_owner_without_nodes() {
            let mut ext = ExtBuilder::build_default().as_externality();
            ext.execute_with(|| {
                let former_owner = strangers(1)[0];
                TrustedOwners::<TestRuntime>::put(bounded(vec![former_owner]));

                assert_ok!(remove(vec![former_owner]));
                assert!(trusted().is_empty());
            });
        }
    }
}
