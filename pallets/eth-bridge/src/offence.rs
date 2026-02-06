use codec::{Decode, DecodeWithMemTracking, Encode};
use pallet_session::{historical::IdentificationTuple, Config as SessionConfig};
use sp_runtime::{scale_info::TypeInfo, traits::Convert, Perbill};
use sp_staking::{
    offence::{Kind, Offence, ReportOffence},
    SessionIndex,
};
use sp_std::{prelude::*, vec};

use crate::Event;

#[derive(PartialEq, Eq, Clone, Debug, Encode, Decode, TypeInfo, DecodeWithMemTracking)]
pub enum EthBridgeOffenceType {
    ChallengeAttemptedOnSuccessfulTransaction,
    ChallengeAttemptedOnUnsuccessfulTransaction,
    InvalidEthereumRangeData,
}

#[derive(PartialEq, Clone, Debug, Encode, Decode, TypeInfo)]
pub struct EthBridgeOffence<Offender> {
    pub session_index: SessionIndex,
    pub offenders: Vec<Offender>,
    pub offence_type: EthBridgeOffenceType,
    pub validator_set_count: u32,
}

impl<Offender: Clone> Offence<Offender> for EthBridgeOffence<Offender> {
    const ID: Kind = *b"bridge:::offence";
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

    fn slash_fraction(&self, offenders_count: u32) -> Perbill {
        Perbill::from_rational(3 * offenders_count, self.validator_set_count).square()
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
    return offenders
}

pub fn create_and_report_bridge_offence<T: crate::Config<I>, I: 'static>(
    reporter: &T::AccountId,
    offenders_accounts: &Vec<T::AccountId>,
    offence_type: EthBridgeOffenceType,
) {
    let offenders = create_offenders_identification::<T, I>(offenders_accounts);

    if !offenders.is_empty() {
        let invalid_event_offence = EthBridgeOffence {
            session_index: <pallet_session::Pallet<T>>::current_index(),
            validator_set_count: crate::AVN::<T>::validators().len() as u32,
            offenders: offenders.clone(),
            offence_type: offence_type.clone(),
        };

        if !T::ReportCorroborationOffence::is_known_offence(
            &invalid_event_offence.offenders(),
            &invalid_event_offence.time_slot(),
        ) {
            let reporters = vec![reporter.clone()];
            if let Err(e) =
                T::ReportCorroborationOffence::report_offence(reporters, invalid_event_offence)
            {
                log::info!(target: "pallet-eth-bridge", "ℹ️ Error while reporting offence: {:?}. Stored in deferred",e);
            }

            <crate::Pallet<T, I>>::deposit_event(Event::<T, I>::CorroborationOffenceReported {
                offence_type,
                offenders,
            });
        }
    }
}
