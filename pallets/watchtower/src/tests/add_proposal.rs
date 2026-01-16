//Copyright 2025 Truth Network.

#![cfg(test)]

use crate::{mock::*, *};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_core::Get;

mod adding_external_proposal {
    use super::*;

    #[test]
    fn works() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let uri = b"https://voting.testnet.aventus.io/proposals/0xd36e3f7e478529129c8ed64dfad7b772b9854aa957d869b0a6e812bb499233c7".to_vec();
            let external_proposal = context.build_external_request(uri);
            assert_ok!(Watchtower::submit_external_proposal(RawOrigin::Root.into(), external_proposal));

            //Verify state and events
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert_eq!(Proposals::<TestRuntime>::contains_key(&proposal_id), true);
            assert_eq!(ProposalStatus::<TestRuntime>::get(&proposal_id), ProposalStatusEnum::Active);
            // External proposals are not added to active proposals
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get().is_none(), true);

            System::assert_last_event(Event::ProposalSubmitted { proposal_id, external_ref: context.external_ref, status: ProposalStatusEnum::Active }.into());
        });
    }

    #[test]
    fn works_with_multiple_proposals() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let uri = b"https://voting.testnet.aventus.io/proposals/0xd36e3f7e478529129c8ed64dfad7b772b9854aa957d869b0a6e812bb499233c7".to_vec();
            let external_proposal = context.build_external_request(uri.clone());
            assert_ok!(Watchtower::submit_external_proposal(RawOrigin::Root.into(), external_proposal));

            let second_external_ref = H256::repeat_byte(2);
            let second_context = Context { external_ref: second_external_ref, ..context.clone() };
            let second_proposal = second_context.build_external_request(uri);
            assert_ok!(Watchtower::submit_external_proposal(RawOrigin::Root.into(), second_proposal));

            //Verify state and events
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert_eq!(Proposals::<TestRuntime>::contains_key(&proposal_id), true);
            assert_eq!(ProposalStatus::<TestRuntime>::get(&proposal_id), ProposalStatusEnum::Active);
            // External proposals are not added to active proposals
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get().is_none(), true);

            let second_proposal_id = ExternalRef::<TestRuntime>::get(&second_context.external_ref);
            assert_eq!(Proposals::<TestRuntime>::contains_key(&second_proposal_id), true);
            assert_eq!(ProposalStatus::<TestRuntime>::get(&second_proposal_id), ProposalStatusEnum::Active);
            // External proposals are not added to active proposals
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get().is_none(), true);

            System::assert_last_event(Event::ProposalSubmitted {
                proposal_id: second_proposal_id,
                external_ref: second_context.external_ref,
                status: ProposalStatusEnum::Active }.into()
            );
        });
    }
}

mod internally_adding_proposal {
    use super::*;
    use test_case::test_case;

    #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
    #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
    fn works(payload: RawPayload, source: ProposalSource) {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_request(payload, source.clone());
            assert!(ActiveInternalProposal::<TestRuntime>::get().is_none());

            assert_ok!(<Watchtower as WatchtowerInterface>::submit_proposal(None, proposal));

            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert!(Proposals::<TestRuntime>::contains_key(&proposal_id));
            assert_eq!(
                ProposalStatus::<TestRuntime>::get(&proposal_id),
                ProposalStatusEnum::Active
            );

            if source == ProposalSource::Internal(ProposalType::Summary) {
                // Internal proposals are added to active proposals
                assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(proposal_id));
            } else {
                // External proposals are not added to active proposals
                assert_eq!(ActiveInternalProposal::<TestRuntime>::get().is_none(), true);
            }

            System::assert_last_event(
                Event::ProposalSubmitted {
                    proposal_id,
                    external_ref: context.external_ref,
                    status: ProposalStatusEnum::Active,
                }
                .into(),
            );
        });
    }

    #[test]
    fn works_with_multiple_proposals() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let payload = b"Test payload. This can be an encoded byte of an object".to_vec();
            let internal_proposal = context.build_internal_request(payload.clone());
            assert!(ActiveInternalProposal::<TestRuntime>::get().is_none());
            assert_ok!(<Watchtower as WatchtowerInterface>::submit_proposal(
                None,
                internal_proposal
            ));

            let second_external_ref = H256::repeat_byte(2);
            let second_context = Context { external_ref: second_external_ref, ..context.clone() };
            let second_proposal = second_context.build_internal_request(payload);
            assert_ok!(<Watchtower as WatchtowerInterface>::submit_proposal(None, second_proposal));

            // Verify first proposal - active
            let proposal_id = ExternalRef::<TestRuntime>::get(&context.external_ref);
            assert!(Proposals::<TestRuntime>::contains_key(&proposal_id));
            assert_eq!(
                ProposalStatus::<TestRuntime>::get(&proposal_id),
                ProposalStatusEnum::Active
            );
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(proposal_id));

            // Verify second proposal - queued
            let second_proposal_id = ExternalRef::<TestRuntime>::get(&second_context.external_ref);
            assert!(Proposals::<TestRuntime>::contains_key(&second_proposal_id));
            assert_eq!(
                ProposalStatus::<TestRuntime>::get(&second_proposal_id),
                ProposalStatusEnum::Queued
            );

            // Active proposal stays the same
            assert_eq!(ActiveInternalProposal::<TestRuntime>::get(), Some(proposal_id));

            System::assert_last_event(
                Event::ProposalSubmitted {
                    proposal_id: second_proposal_id,
                    external_ref: second_context.external_ref,
                    status: ProposalStatusEnum::Queued,
                }
                .into(),
            );
        });
    }
}

mod adding_proposal_fails_when {
    use super::*;
    use test_case::test_case;

    #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
    #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
    fn created_at_is_in_the_future(payload: RawPayload, source: ProposalSource) {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let mut context = Context::default();
            context.created_at = 10u32; // Future block
            let internal_proposal = context.build_request(payload, source);

            assert_noop!(
                Watchtower::submit_proposal(None, internal_proposal.clone()),
                Error::<TestRuntime>::InvalidProposal
            );
        });
    }

    #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
    #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
    fn external_ref_is_duplicated(payload: RawPayload, source: ProposalSource) {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_request(payload.clone(), source.clone());

            if source == ProposalSource::External {
                // First submission should work
                assert_ok!(Watchtower::submit_external_proposal(
                    RawOrigin::Root.into(),
                    proposal.clone()
                ));
                // Second submission with the same external ref should fail
                assert_noop!(
                    Watchtower::submit_external_proposal(RawOrigin::Root.into(), proposal.clone()),
                    Error::<TestRuntime>::DuplicateExternalRef
                );
            } else {
                assert_ok!(Watchtower::submit_proposal(None, proposal.clone()));
                // Second submission with the same external ref should fail
                assert_noop!(
                    Watchtower::submit_proposal(None, proposal.clone()),
                    Error::<TestRuntime>::DuplicateExternalRef
                );
            }
        });
    }

    #[test]
    fn source_is_not_valid() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let payload = b"Test".to_vec();
            let internal_proposal = context.build_internal_request(payload);
            assert_noop!(
                Watchtower::submit_external_proposal(
                    RawOrigin::Root.into(),
                    internal_proposal.clone()
                ),
                Error::<TestRuntime>::InvalidProposalSource
            );
        });
    }

    #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
    #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
    fn title_too_long(payload: RawPayload, source: ProposalSource) {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let mut context = Context::default();
            let max_title_len: u32 =
                <<TestRuntime as crate::Config>::MaxTitleLen as Get<u32>>::get();
            context.title = vec![b'A'; max_title_len as usize + 1];
            let proposal = context.build_request(payload.clone(), source.clone());

            if source == ProposalSource::External {
                assert_noop!(
                    Watchtower::submit_external_proposal(RawOrigin::Root.into(), proposal.clone()),
                    Error::<TestRuntime>::InvalidTitle
                );
            } else {
                assert_noop!(
                    Watchtower::submit_proposal(None, proposal.clone()),
                    Error::<TestRuntime>::InvalidTitle
                );
            }
        });
    }

    #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
    #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
    fn title_empty(payload: RawPayload, source: ProposalSource) {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let mut context = Context::default();
            context.title = vec![];
            let proposal = context.build_request(payload.clone(), source.clone());

            if source == ProposalSource::External {
                assert_noop!(
                    Watchtower::submit_external_proposal(RawOrigin::Root.into(), proposal.clone()),
                    Error::<TestRuntime>::InvalidProposal
                );
            } else {
                assert_noop!(
                    Watchtower::submit_proposal(None, proposal.clone()),
                    Error::<TestRuntime>::InvalidProposal
                );
            }
        });
    }

    #[test_case(RawPayload::Uri(b"test".to_vec()), ProposalSource::External; "external")]
    #[test_case(RawPayload::Inline(b"test".to_vec()), ProposalSource::Internal(ProposalType::Summary); "internal")]
    fn external_ref_empty(payload: RawPayload, source: ProposalSource) {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let mut context = Context::default();
            context.external_ref = H256::zero();
            let proposal = context.build_request(payload.clone(), source.clone());

            if source == ProposalSource::External {
                assert_noop!(
                    Watchtower::submit_external_proposal(RawOrigin::Root.into(), proposal.clone()),
                    Error::<TestRuntime>::InvalidProposal
                );
            } else {
                assert_noop!(
                    Watchtower::submit_proposal(None, proposal.clone()),
                    Error::<TestRuntime>::InvalidProposal
                );
            }
        });
    }

    #[test_case(RawPayload::Uri(vec![]), ProposalSource::External; "external")]
    #[test_case(RawPayload::Inline(vec![]), ProposalSource::Internal(ProposalType::Summary); "internal")]
    fn empty_bytes(payload: RawPayload, source: ProposalSource) {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let proposal = context.build_request(payload.clone(), source.clone());

            if source == ProposalSource::External {
                assert_noop!(
                    Watchtower::submit_external_proposal(RawOrigin::Root.into(), proposal.clone()),
                    Error::<TestRuntime>::InvalidProposal
                );
            } else {
                assert_noop!(
                    Watchtower::submit_proposal(None, proposal.clone()),
                    Error::<TestRuntime>::InvalidProposal
                );
            }
        });
    }

    #[test]
    fn inline_payload_too_long() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let max_inline_len: u32 =
                <<TestRuntime as crate::Config>::MaxInlineLen as Get<u32>>::get();
            let payload = vec![b'A'; max_inline_len as usize + 1];
            let internal_proposal = context.build_internal_request(payload.clone());

            assert_noop!(
                Watchtower::submit_proposal(None, internal_proposal.clone()),
                Error::<TestRuntime>::InvalidInlinePayload
            );
        });
    }

    #[test]
    fn uri_too_long() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let max_uri_len: u32 = <<TestRuntime as crate::Config>::MaxUriLen as Get<u32>>::get();
            let uri = vec![b'A'; max_uri_len as usize + 1];
            let external_proposal = context.build_external_request(uri.clone());

            assert_noop!(
                Watchtower::submit_proposal(None, external_proposal.clone()),
                Error::<TestRuntime>::InvalidUri
            );
        });
    }

    #[test]
    fn proposal_queue_is_full() {
        let mut ext = ExtBuilder::build_default().as_externality();
        ext.execute_with(|| {
            let context = Context::default();
            let max_queue_size: u32 =
                <<TestRuntime as crate::Config>::MaxInternalProposalLen as Get<u32>>::get();
            for _i in 0..=max_queue_size {
                let external_ref = H256::random();
                let queue_context = Context { external_ref, ..context.clone() };
                let proposal = queue_context.build_internal_request(b"Test".to_vec());
                assert_ok!(<Watchtower as WatchtowerInterface>::submit_proposal(None, proposal));
            }

            // The queue should be full now, next proposal should fail
            let external_ref = H256::random();
            let final_context = Context { external_ref, ..context.clone() };
            let internal_proposal = final_context.build_internal_request(b"Test".to_vec());

            assert_noop!(
                Watchtower::submit_proposal(None, internal_proposal.clone()),
                Error::<TestRuntime>::InnerProposalQueueFull
            );
        });
    }
}
