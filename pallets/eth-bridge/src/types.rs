use core::fmt;

use crate::*;
use codec::DecodeWithMemTracking;
use sp_avn_common::{
    eth::EthereumId,
    event_discovery::{AdditionalEvents, EthBridgeEventsFilter},
    UINT256, UINT32,
};

// The different types of request this pallet can handle.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum Request {
    Send(SendRequestData),
    LowerProof(LowerProofRequestData),
}

impl Default for Request {
    fn default() -> Self {
        Request::Send(Default::default())
    }
}

impl Request {
    pub fn id_matches(&self, id: &u32) -> bool {
        match self {
            Request::Send(req) => &req.tx_id == id,
            Request::LowerProof(req) => &req.lower_id == id,
        }
    }
}

// Request data for a transaction we are sending to Ethereum
#[derive(Encode, Decode, Clone, PartialEq, Eq, Default, TypeInfo, MaxEncodedLen)]
pub struct SendRequestData {
    pub tx_id: EthereumId,
    pub function_name: BoundedVec<u8, FunctionLimit>,
    pub params: BoundedVec<(BoundedVec<u8, TypeLimit>, BoundedVec<u8, ValueLimit>), ParamsLimit>,
    pub caller_id: BoundedVec<u8, CallerIdLimit>,
}

impl fmt::Debug for SendRequestData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let function_name = String::from_utf8_lossy(&self.function_name).to_string();

        let formatted_params: Vec<(String, String)> = self
            .params
            .iter()
            .map(|(ty, val)| {
                let param_type = String::from_utf8_lossy(&ty);
                match param_type.as_ref() {
                    "address" | "bytes" | "bytes32" => {
                        let hex_val = hex::encode(val);
                        (param_type.to_string(), hex_val)
                    },
                    _ => (param_type.to_string(), String::from_utf8_lossy(val).to_string()),
                }
            })
            .collect();

        f.debug_struct("SendRequestData")
            .field("tx_id", &self.tx_id)
            .field("function_name (hex)", &function_name) // Custom output
            .field("params (hex)", &formatted_params) // Custom output
            .field("caller_id", &String::from_utf8_lossy(&self.caller_id).to_string())
            .finish()
    }
}

impl SendRequestData {
    pub fn extend_params<T: Config<I>, I: 'static>(
        &self,
        expiry: u64,
    ) -> Result<
        BoundedVec<(BoundedVec<u8, TypeLimit>, BoundedVec<u8, ValueLimit>), ParamsLimit>,
        Error<T, I>,
    > {
        let mut extended_params = util::unbound_params(&self.params);
        extended_params.push((UINT256.to_vec(), expiry.to_string().into_bytes()));
        extended_params.push((UINT32.to_vec(), self.tx_id.to_string().into_bytes()));

        Ok(util::bound_params(&extended_params)?)
    }
}

// Request data for a message that requires confirmation for Ethereum
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct LowerProofRequestData {
    pub lower_id: LowerId,
    pub params: LowerParams,
    pub caller_id: BoundedVec<u8, CallerIdLimit>,
}

// Data related to generating confirmations
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, Default, TypeInfo, MaxEncodedLen)]
pub struct ActiveConfirmation {
    pub msg_hash: H256,
    pub confirmations: BoundedVec<ecdsa::Signature, ConfirmationsLimit>,
}

// Persistent storage struct to hold transactions sent to Ethereum
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, Default, TypeInfo, MaxEncodedLen)]
pub struct TransactionData<AccountId> {
    pub function_name: BoundedVec<u8, FunctionLimit>,
    pub params: BoundedVec<(BoundedVec<u8, TypeLimit>, BoundedVec<u8, ValueLimit>), ParamsLimit>,
    pub sender: AccountId,
    pub eth_tx_hash: H256,
    pub tx_succeeded: bool,
}

// Storage item for the active request being processed
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, Default, TypeInfo, MaxEncodedLen)]
pub struct ActiveRequestData<BlockNumber, AccountId> {
    pub request: Request,
    pub confirmation: ActiveConfirmation,
    pub tx_data: Option<ActiveEthTransaction<AccountId>>,
    pub last_updated: BlockNumber,
}

impl<BlockNumber, AccountId> ActiveRequestData<BlockNumber, AccountId> {
    // Function to convert an active request into an active transaction request.
    pub fn as_active_tx<T, I>(self) -> Result<ActiveTransactionData<AccountId>, Error<T, I>> {
        let tx_data = self.tx_data.ok_or(Error::<T, I>::InvalidSendRequest)?;

        match self.request {
            Request::Send(req) => Ok(ActiveTransactionData {
                request: req,
                confirmation: self.confirmation,
                replay_attempt: tx_data.replay_attempt,
                data: tx_data,
            }),
            _ => return Err(Error::<T, I>::InvalidSendRequest),
        }
    }
}

// Active request data specific for a transaction. 'data' is not optional.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct ActiveTransactionData<AccountId> {
    pub request: SendRequestData,
    pub confirmation: ActiveConfirmation,
    pub data: ActiveEthTransaction<AccountId>,
    pub replay_attempt: u16,
}

// Transient data used for an active send transaction request
#[derive(Encode, Decode, Clone, PartialEq, Eq, Default, TypeInfo, MaxEncodedLen)]
pub struct ActiveEthTransaction<AccountId> {
    pub function_name: BoundedVec<u8, FunctionLimit>,
    pub eth_tx_params:
        BoundedVec<(BoundedVec<u8, TypeLimit>, BoundedVec<u8, ValueLimit>), ParamsLimit>,
    pub sender: AccountId,
    pub expiry: u64,
    pub eth_tx_hash: H256,
    pub success_corroborations: BoundedVec<AccountId, ConfirmationsLimit>,
    pub failure_corroborations: BoundedVec<AccountId, ConfirmationsLimit>,
    pub valid_tx_hash_corroborations: BoundedVec<AccountId, ConfirmationsLimit>,
    pub invalid_tx_hash_corroborations: BoundedVec<AccountId, ConfirmationsLimit>,
    pub tx_succeeded: bool,
    pub replay_attempt: u16,
}

impl<AccountId: fmt::Debug> fmt::Debug for ActiveEthTransaction<AccountId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let function_name = String::from_utf8_lossy(&self.function_name).to_string();

        let formatted_tx_params: Vec<(String, String)> = self
            .eth_tx_params
            .iter()
            .map(|(ty, val)| {
                let param_type = String::from_utf8_lossy(&ty);
                match param_type.as_ref() {
                    "address" | "bytes" | "bytes32" => {
                        let hex_val = hex::encode(val);
                        (param_type.to_string(), hex_val)
                    },
                    _ => (param_type.to_string(), String::from_utf8_lossy(val).to_string()),
                }
            })
            .collect();

        f.debug_struct("ActiveEthTransaction")
            .field("function_name (hex)", &function_name)
            .field("eth_tx_params (hex)", &formatted_tx_params)
            .field("sender", &self.sender)
            .field("expiry", &self.expiry)
            .field("eth_tx_hash", &self.eth_tx_hash)
            .field("success_corroborations", &self.success_corroborations.len())
            .field("failure_corroborations", &self.failure_corroborations.len())
            .field("valid_tx_hash_corroborations", &self.valid_tx_hash_corroborations.len())
            .field("invalid_tx_hash_corroborations", &self.invalid_tx_hash_corroborations.len())
            .field("tx_succeeded", &self.tx_succeeded)
            .finish()
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, Default, TypeInfo, MaxEncodedLen)]
pub struct ActiveEthRange {
    pub range: EthBlockRange,
    pub partition: u16,
    pub event_types_filter: EthBridgeEventsFilter,
    pub additional_transactions: AdditionalEvents,
}

impl ActiveEthRange {
    pub fn is_initial_range(&self) -> bool {
        *self == Default::default()
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Debug, Eq, TypeInfo, DecodeWithMemTracking)]
pub enum AdminSettings {
    /// The delay, in blocks, for actions to wait before being executed
    EthereumTransactionLifetimeSeconds(u64),
    /// Set the EthereumTransactionId
    EthereumTransactionId(EthereumId),
    /// Remove the active request and allow the next request to be processed
    RemoveActiveRequest,
    /// Queue an additional ethereum event to be included in the next range
    QueueAdditionalEthereumEvent(EthTransactionId),
    /// Removes all votes on Ethereum Events partitions for the active range.
    RestartEventDiscoveryOnRange,
    /// Set the Ethereum Bridge Instance
    SetEthBridgeInstance(EthBridgeInstance),
}
