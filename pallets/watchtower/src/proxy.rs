use crate::*;

const SIGNED_SUBMIT_EXTERNAL_PROPOSAL_CONTEXT: &'static [u8] = b"wt_submit_external_proposal";
const SIGNED_SUBMIT_VOTE_CONTEXT: &'static [u8] = b"wt_submit_vote";

impl<T: Config> Pallet<T> {
    // external_ref will ensure signature re-use is not possible but we also add a lifetime (block
    // number) to be on the safe side.
    pub fn encode_signed_submit_external_proposal_params(
        relayer: &T::AccountId,
        proposal: &ProposalRequest,
        block_number: &BlockNumberFor<T>,
    ) -> Vec<u8> {
        (SIGNED_SUBMIT_EXTERNAL_PROPOSAL_CONTEXT, relayer.clone(), proposal, block_number).encode()
    }

    // Voters and only vote once per proposal so no nonce needed here.
    pub fn encode_signed_submit_vote_params(
        relayer: &T::AccountId,
        proposal_id: &ProposalId,
        in_favor: &bool,
        block_number: &BlockNumberFor<T>,
    ) -> Vec<u8> {
        (SIGNED_SUBMIT_VOTE_CONTEXT, relayer.clone(), proposal_id, in_favor, block_number).encode()
    }

    pub fn offchain_signature_is_valid<D: Encode>(
        data: &D,
        signer: &T::SignerId,
        signature: &<T::SignerId as RuntimeAppPublic>::Signature,
    ) -> bool {
        let signature_valid =
            data.using_encoded(|encoded_data| signer.verify(&encoded_data, &signature));

        log::trace!(
            "🪲 Validating ocw signature: [ data {:?} - account {:?} - signature {:?} ] Result: {}",
            data.encode(),
            signer.encode(),
            signature,
            signature_valid
        );
        return signature_valid
    }

    pub fn get_encoded_call_param(
        call: &<T as Config>::RuntimeCall,
    ) -> Option<(&Proof<T::Signature, T::AccountId>, Vec<u8>)> {
        let call = match call.is_sub_type() {
            Some(call) => call,
            None => return None,
        };

        match call {
            Call::signed_submit_external_proposal { ref proof, ref block_number, ref proposal } => {
                let encoded_data = Self::encode_signed_submit_external_proposal_params(
                    &proof.relayer,
                    proposal,
                    block_number,
                );

                Some((proof, encoded_data))
            },
            Call::signed_vote { ref proof, ref proposal_id, ref in_favor, ref block_number } => {
                let encoded_data = Self::encode_signed_submit_vote_params(
                    &proof.relayer,
                    proposal_id,
                    in_favor,
                    block_number,
                );

                Some((proof, encoded_data))
            },
            _ => None,
        }
    }
}
