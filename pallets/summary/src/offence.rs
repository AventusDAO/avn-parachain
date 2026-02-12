// Copyright 2026 Aventus DAO Ltd

use sp_runtime::Perbill;
use sp_staking::{
    offence::{Kind, Offence},
    SessionIndex,
};

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use pallet_session::{historical::IdentificationTuple, Config as SessionConfig};
use sp_core::Get;
use sp_runtime::{scale_info::TypeInfo, traits::Convert};
use sp_staking::offence::ReportOffence;
use sp_std::prelude::*;
#[derive(
    PartialEq, Eq, Clone, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub enum SummaryOffenceType {
    InvalidSignatureSubmitted,
    InvalidVoteSubmitted,
    SlotNotAdvanced,
    NoSummaryCreated,
    RejectedValidRoot,
    CreatedInvalidRoot,
    ApprovedInvalidRoot,
}
use crate::Event;

#[derive(PartialEq, Clone, Debug, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub struct SummaryOffence<Offender> {
    /// The current session index in which we report the validators that submitted an invalid
    /// ethereum log.
    pub session_index: SessionIndex,
    /// The size of the validator set in current session/era.
    pub validator_set_count: u32,
    /// Authorities that validated the invalid log.
    pub offenders: Vec<Offender>,
    /// The different types of the offence
    pub offence_type: SummaryOffenceType,
}

impl<Offender: Clone> Offence<Offender> for SummaryOffence<Offender> {
    const ID: Kind = *b"summary::offence";
    type TimeSlot = SessionIndex;

    fn offenders(&self) -> Vec<Offender> {
        self.offenders.clone()
    }

    fn session_index(&self) -> SessionIndex {
        self.session_index
    }

    fn validator_set_count(&self) -> u32 {
        self.validator_set_count
    }

    fn time_slot(&self) -> Self::TimeSlot {
        self.session_index
    }

    fn slash_fraction(&self, _offenders: u32) -> Perbill {
        // We don't implement fraction slashes at the moment.
        Perbill::from_percent(100)
    }
}

pub fn create_offenders_identification<T: crate::Config<I>, I: 'static>(
    offenders_accounts: &Vec<T::AccountId>,
) -> Vec<IdentificationTuple<T>> {
    let offenders = offenders_accounts
        .into_iter()
        .filter_map(|id| <T as SessionConfig>::ValidatorIdOf::convert(id.clone()))
        .filter_map(|id| T::FullIdentificationOf::convert(id.clone()).map(|full_id| (id, full_id)))
        .collect::<Vec<IdentificationTuple<T>>>();
    return offenders;
}

pub fn create_and_report_summary_offence<T: crate::Config<I>, I: 'static>(
    reporter: &T::AccountId,
    offenders_accounts: &Vec<T::AccountId>,
    offence_type: SummaryOffenceType,
) {
    let offenders = create_offenders_identification::<T, I>(offenders_accounts);

    if !offenders.is_empty() {
        let invalid_event_offence = SummaryOffence {
            session_index: <pallet_session::Pallet<T>>::current_index(),
            validator_set_count: crate::AVN::<T>::validators().len() as u32,
            offenders: offenders.clone(),
            offence_type: offence_type.clone(),
        };

        if !T::ReportSummaryOffence::is_known_offence(
            &invalid_event_offence.offenders(),
            &invalid_event_offence.time_slot(),
        ) {
            let reporters = vec![reporter.clone()];
            if let Err(e) =
                T::ReportSummaryOffence::report_offence(reporters, invalid_event_offence)
            {
                log::info!(
                    target: "pallet-summary",
                    "ℹ️ Instance({}) Error while reporting offence: {:?}. Stored in deferred",
                    T::InstanceId::get(), e
                );
            }
            <crate::Pallet<T, I>>::deposit_event(Event::<T, I>::SummaryOffenceReported {
                offence_type,
                offenders,
            });
        }
    }
}
