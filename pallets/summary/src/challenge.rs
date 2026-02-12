#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_system::{offchain::SubmitTransaction, pallet_prelude::BlockNumberFor};
use sp_application_crypto::RuntimeAppPublic;
use sp_avn_common::event_types::Validator;
use sp_runtime::{
    scale_info::TypeInfo,
    traits::Member,
    transaction_validity::{
        InvalidTransaction, TransactionPriority, TransactionValidity, ValidTransaction,
    },
};
use sp_std::{fmt::Debug, prelude::*};

use super::{Config, OcwLock};
use crate::{Call, CurrentSlot, CurrentSlotsValidator, Pallet as Summary, AVN};

pub const CHALLENGE_CONTEXT: &'static [u8] = b"root_challenge";
pub const UNKNOWN_CHALLENGE_REASON: u8 = 10;

pub type SlotNumber = u32;

#[derive(
    Encode, Decode, Clone, PartialEq, Debug, Eq, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub enum SummaryChallengeReason {
    /// The slot has not been advanced
    SlotNotAdvanced(SlotNumber),

    /// Default challenge reason
    Unknown,
}

#[derive(
    Encode, Decode, Default, Clone, PartialEq, Debug, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub struct SummaryChallenge<AccountId: Member> {
    pub challenge_reason: SummaryChallengeReason,
    pub challenger: AccountId,
    pub challengee: AccountId,
}

impl<AccountId: Member> SummaryChallenge<AccountId> {
    pub fn new(
        challenge_reason: SummaryChallengeReason,
        challenger: AccountId,
        challengee: AccountId,
    ) -> Self {
        return SummaryChallenge::<AccountId> { challenge_reason, challenger, challengee };
    }

    /// Validates the challenge and returns true if it's correct.
    pub fn is_valid<T: Config<I>, I: 'static>(
        &self,
        current_slot_number: BlockNumberFor<T>,
        current_block_number: BlockNumberFor<T>,
        challengee: &T::AccountId,
    ) -> bool {
        match self.challenge_reason {
            SummaryChallengeReason::SlotNotAdvanced(slot_number_to_challenge) => {
                let current_slot_validator = CurrentSlotsValidator::<T, I>::get();
                if current_slot_validator.is_none() {
                    return false;
                }

                return BlockNumberFor::<T>::from(slot_number_to_challenge) == current_slot_number
                    && Summary::<T, I>::grace_period_elapsed(current_block_number)
                    && *challengee == current_slot_validator.expect("checked for none");
            },
            _ => false,
        }
    }
}

impl Default for SummaryChallengeReason {
    fn default() -> Self {
        SummaryChallengeReason::Unknown
    }
}

pub fn add_challenge_validate_unsigned<T: Config<I>, I: 'static>(
    challenge: &SummaryChallenge<T::AccountId>,
    validator: &Validator<T::AuthorityId, T::AccountId>,
    signature: &<T::AuthorityId as RuntimeAppPublic>::Signature,
) -> TransactionValidity {
    if challenge.challenge_reason == SummaryChallengeReason::Unknown {
        return InvalidTransaction::Custom(UNKNOWN_CHALLENGE_REASON).into();
    }

    if !AVN::<T>::signature_is_valid(&(CHALLENGE_CONTEXT, challenge), &validator, signature) {
        return InvalidTransaction::BadProof.into();
    };

    return ValidTransaction::with_tag_prefix("summary_challenge")
        .priority(TransactionPriority::max_value())
        .and_provides(vec![(CHALLENGE_CONTEXT, challenge, validator).encode()])
        .longevity(64_u64)
        .propagate(true)
        .build();
}

pub fn challenge_slot_if_required<T: Config<I>, I: 'static>(
    offchain_worker_block_number: BlockNumberFor<T>,
    this_validator: &Validator<T::AuthorityId, T::AccountId>,
) {
    let slot_number: BlockNumberFor<T> = CurrentSlot::<T, I>::get();
    let slot_as_u32 = AVN::<T>::convert_block_number_to_u32(slot_number);
    if let Err(_) = slot_as_u32 {
        log::error!("💔 Error converting block number: {:?} into u32", slot_number);
        return;
    }

    let current_slot_validator = CurrentSlotsValidator::<T, I>::get();
    if current_slot_validator.is_none() {
        log::error!("💔 Current slot validator is not found for slot: {:?}", slot_number);
        return;
    }

    let challenge = SummaryChallenge::new(
        SummaryChallengeReason::SlotNotAdvanced(slot_as_u32.expect("Checked for error")),
        this_validator.account_id.clone(),
        current_slot_validator.expect("Checked for none"),
    );

    if can_challenge::<T, I>(&challenge, this_validator, offchain_worker_block_number) {
        let _ = send_challenge_transaction::<T, I>(&challenge, this_validator);
    }
}

fn can_challenge<T: Config<I>, I: 'static>(
    challenge: &SummaryChallenge<T::AccountId>,
    this_validator: &Validator<T::AuthorityId, T::AccountId>,
    ocw_block_number: BlockNumberFor<T>,
) -> bool {
    if OcwLock::is_locked::<frame_system::Pallet<T>>(&challenge_lock_name::<T, I>(challenge)) {
        return false;
    }

    let is_chosen_validator =
        AVN::<T>::is_primary_for_block(ocw_block_number, &this_validator.account_id)
            .unwrap_or_else(|_| false);

    let grace_period_elapsed = Summary::<T, I>::grace_period_elapsed(ocw_block_number);

    return is_chosen_validator && grace_period_elapsed;
}

fn send_challenge_transaction<T: Config<I>, I: 'static>(
    challenge: &SummaryChallenge<T::AccountId>,
    this_validator: &Validator<T::AuthorityId, T::AccountId>,
) -> Result<(), ()> {
    let signature = this_validator.key.sign(&(CHALLENGE_CONTEXT, challenge).encode());

    if signature.is_none() {
        log::error!("💔 Error signing challenge: {:?}", &challenge);
        return Err(());
    };

    let xt = T::create_inherent(
        Call::add_challenge {
            challenge: challenge.clone(),
            validator: this_validator.clone(),
            signature: signature.expect("We have a signature"),
        }
        .into(),
    );
    if let Err(e) = SubmitTransaction::<T, Call<T, I>>::submit_transaction(xt) {
        log::error!("💔 Error sending `challenge transaction`: {:?}. Error: {:?}", &challenge, e);
        return Err(());
    }

    let challenge_lock_name = challenge_lock_name::<T, I>(challenge);
    let mut lock = AVN::<T>::get_ocw_locker(&challenge_lock_name);

    // Add a lock to record the fact that we have sent a challenge.
    if let Ok(guard) = lock.try_lock() {
        guard.forget();
    } else {
        log::warn!("ℹ️  Error adding a lock for `challenge transaction`: {:?}.", &challenge);
    };

    Ok(())
}

pub fn challenge_lock_name<T: Config<I>, I: 'static>(
    challenge: &SummaryChallenge<T::AccountId>,
) -> Vec<u8> {
    let mut name = b"challenge_summary::slot::".to_vec();
    name.extend_from_slice(&mut challenge.encode());
    name
}
