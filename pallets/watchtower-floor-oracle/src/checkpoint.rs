// Copyright 2026 Aventus DAO Ltd

//! Checkpoint votes: the collator offchain worker that feeds [`Pallet::submit_checkpoint_vote`].
//!
//! Every block, each collator looks at the open checkpoint rounds, asks its local external
//! service which block of that chain to vote (`checkpoint/block/{chain_id}`), and submits one
//! vote per round. The service rounds the chain's head down to a fixed interval after a
//! confirmation depth, so collators reading the chain seconds apart vote the same number and
//! the round reaches quorum. A chain no collator can reach never gets votes; its rounds time
//! out and restart (`CheckpointRoundRestarted`) until the endpoint is fixed.
//!
//! Only validators vote: the node must hold the AVN key of a current validator. Watchtower
//! nodes have none and return early.

use crate::{ocw::OcwChannel, *};
use sp_avn_common::http_data_codec::decode_from_http_data;

const CHECKPOINT_OCW: OcwChannel = OcwChannel {
    name: "Checkpoint oracle",
    id: b"checkpoint_oracle::ocw",
    db_prefix: b"checkpoint_oracle::ocw::",
};

impl<T: Config> Pallet<T> {
    /// Path of the service request for the block to vote on `chain_id`. Shared with the tests
    /// so they cannot drift.
    pub fn checkpoint_service_path(chain_id: ExternalChainId) -> String {
        format!("checkpoint/block/{}", chain_id)
    }

    /// `GET` of [`Pallet::checkpoint_service_path`] on the local external service. The body is
    /// the http-data encoding of a `u32` block number.
    pub fn fetch_checkpoint_block(chain_id: ExternalChainId) -> Result<u32, DispatchError> {
        let body = AVN::<T>::get_data_from_service(Self::checkpoint_service_path(chain_id))?;
        let text = core::str::from_utf8(&body)
            .map_err(|_| DispatchError::Other("checkpoint block is not utf8"))?;
        decode_from_http_data::<u32>(text)
            .map_err(|_| DispatchError::Other("checkpoint block does not decode"))
    }

    /// Votes once per open round. Only nodes holding a current validator's AVN key take part.
    pub fn checkpoint_offchain_worker(now: BlockNumberFor<T>) {
        let rounds = Self::active_rounds();
        if rounds.is_empty() {
            return
        }
        if Self::ocw_already_run(&CHECKPOINT_OCW, now).is_err() {
            return
        }
        let validator = match AVN::<T>::get_validator_for_current_node() {
            Some(validator) => validator,
            None => return,
        };

        for (chain_id, round_id) in rounds {
            if Self::author_has_voted(chain_id, round_id, &validator.account_id) {
                continue
            }
            let key = (b"vote", chain_id, round_id, &validator.account_id).encode();
            if Self::submission_in_progress(&CHECKPOINT_OCW, &key, now) {
                continue
            }

            let block = match Self::fetch_checkpoint_block(chain_id) {
                Ok(block) => block,
                Err(e) => {
                    log::error!(
                        "💔 Checkpoint oracle: cannot fetch a block for chain {}: {:?}",
                        chain_id,
                        e
                    );
                    continue
                },
            };
            if block == 0 || !Self::is_at_or_above_checkpoint(chain_id, block) {
                log::error!(
                    "💔 Checkpoint oracle: service block {} for chain {} is unusable (below the current checkpoint or zero)",
                    block,
                    chain_id
                );
                continue
            }

            let payload =
                Self::vote_signing_payload(chain_id, round_id, &validator.account_id, block);
            let signature = match validator.key.sign(&payload) {
                Some(signature) => signature,
                None => {
                    log::error!("💔 Checkpoint oracle: cannot sign vote for chain {}", chain_id);
                    continue
                },
            };
            let call = <T as CreateBare<Call<T>>>::create_bare(
                Call::<T>::submit_checkpoint_vote {
                    chain_id,
                    round_id,
                    author: validator.clone(),
                    block_number: block,
                    signature,
                }
                .into(),
            );
            match SubmitTransaction::<T, Call<T>>::submit_transaction(call) {
                Ok(()) => {
                    log::info!(
                        "Checkpoint oracle: voted block {} for chain {} round {}",
                        block,
                        chain_id,
                        round_id
                    );
                    Self::record_submission(&CHECKPOINT_OCW, &key, now)
                },
                Err(e) => log::error!(
                    "💔 Checkpoint oracle: cannot submit vote for chain {}: {:?}",
                    chain_id,
                    e
                ),
            }
        }
    }
}
