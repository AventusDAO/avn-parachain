use super::*;
use crate::{
    util::{try_process_query_result, unbound_params},
    Author, Config, AVN,
};
use ethabi::{Address, Function, Int, Param, ParamType, Token};
use pallet_avn::{AccountToBytesConverter, EthereumPublicKeyChecker};
use sp_avn_common::{
    eth::EthereumId, recover_public_key_from_ecdsa_signature, EthQueryRequest,
    EthQueryResponseType, EthTransaction, ADDRESS, BYTES, BYTES32, UINT128, UINT256, UINT32,
};
use sp_core::{ecdsa, Get, H256};
use sp_runtime::DispatchError;
use sp_std::vec;

pub fn sign_msg_hash<T: Config<I>, I: 'static>(
    author: &Author<T>,
    msg_hash: &H256,
) -> Result<ecdsa::Signature, DispatchError> {
    let msg_data = msg_hash.as_ref().to_vec();
    let hex_data = hex::encode(&msg_data).into_bytes();
    log::debug!("Sending H256 to sign: {:?}", msg_hash);
    let proof = author.key.sign(&msg_data).ok_or(Error::<T, I>::SigningError)?;
    let confirmation = AVN::<T>::request_ecdsa_signature_from_external_service(hex_data, proof)?;
    Ok(confirmation)
}

pub fn verify_signature<T: Config<I>, I: 'static>(
    msg_hash: H256,
    author: &Author<T>,
    confirmation: &ecdsa::Signature,
) -> Result<(), Error<T, I>> {
    if eth_signature_is_valid::<T, I>(msg_hash, author, confirmation) {
        Ok(())
    } else {
        Err(Error::<T, I>::InvalidECDSASignature)
    }
}

fn eth_signature_is_valid<T: Config<I>, I: 'static>(
    msg_hash: H256,
    validator: &Validator<T::AuthorityId, T::AccountId>,
    signature: &ecdsa::Signature,
) -> bool {
    // verify that the incoming (unverified) pubkey is actually a validator
    if !AVN::<T>::is_validator(&validator.account_id) {
        log::warn!("✋ Account: {:?} is not an authority.", &validator.account_id);
        return false;
    }

    let recovered_public_key = recover_public_key_from_ecdsa_signature(signature, &msg_hash);
    if recovered_public_key.is_err() {
        log::error!(
            "❌ Recovery of public key from ECDSA Signature: {:?} and data: {:?} failed",
            &signature,
            &msg_hash
        );
        return false;
    }

    match <T as pallet_avn::Config>::EthereumPublicKeyChecker::get_validator_for_eth_public_key(
        &recovered_public_key.expect("Checked for error"),
    ) {
        Some(maybe_validator) => maybe_validator == validator.account_id,
        _ => {
            log::error!(
                "❌ ECDSA signature validation failed on data {:?} validator: {:?} signature {:?}.",
                &msg_hash,
                validator,
                signature
            );
            false
        },
    }
}

pub fn send_tx<T: Config<I>, I: 'static>(
    author: &Author<T>,
    tx: &ActiveTransactionData<T::AccountId>,
) -> Result<H256, DispatchError> {
    if author.account_id != tx.data.sender {
        log::error!(
            "❌ Author {:?} is not the sender {:?} of the transaction {:?}",
            author.account_id,
            tx.data.sender,
            tx.request.tx_id
        );
        return Err(Error::<T, I>::AuthorNotSender.into());
    }

    match generate_send_calldata::<T, I>(tx) {
        Ok(calldata) => match send_transaction::<T, I>(calldata, author) {
            Ok(eth_tx_hash) => Ok(eth_tx_hash),
            Err(_) => Err(Error::<T, I>::SendTransactionFailed.into()),
        },
        Err(_) => Err(Error::<T, I>::InvalidSendCalldata.into()),
    }
}

pub fn corroborate<T: Config<I>, I: 'static>(
    tx: &ActiveTransactionData<T::AccountId>,
    author: &Author<T>,
) -> Result<(Option<bool>, Option<bool>), DispatchError> {
    let status = check_tx_status::<T, I>(tx, author)?;
    if status.is_some() {
        let (tx_hash_is_valid, confirmations) = check_tx_hash::<T, I>(tx, author)?;
        if tx_hash_is_valid && confirmations.unwrap_or_default() < T::MinEthBlockConfirmation::get()
        {
            log::warn!("🚨 Transaction {:?} doesn't have the minimum eth confirmations yet, skipping corroboration. Current confirmation: {:?}",
                tx.request.tx_id, confirmations
            );
            return Ok((None, None));
        }

        return Ok((status, Some(tx_hash_is_valid)));
    }

    return Ok((None, None));
}

fn check_tx_status<T: Config<I>, I: 'static>(
    tx: &ActiveTransactionData<T::AccountId>,
    author: &Author<T>,
) -> Result<Option<bool>, DispatchError> {
    if let Ok(calldata) = generate_corroborate_calldata::<T, I>(tx.request.tx_id, tx.data.expiry) {
        if let Ok(result) = call_corroborate_method::<T, I>(calldata, &author.account_id) {
            match result {
                0 => return Ok(None),
                1 => return Ok(Some(true)),
                -1 => return Ok(Some(false)),
                _ => return Err(Error::<T, I>::InvalidCorroborateResult.into()),
            }
        } else {
            return Err(Error::<T, I>::CorroborateCallFailed.into());
        }
    }
    Err(Error::<T, I>::InvalidCorroborateCalldata.into())
}

fn check_tx_hash<T: Config<I>, I: 'static>(
    tx: &ActiveTransactionData<T::AccountId>,
    author: &Author<T>,
) -> Result<(bool, Option<u64>), DispatchError> {
    if tx.data.eth_tx_hash != H256::zero() {
        if let Ok((call_data, num_confirmations)) =
            get_transaction_call_data::<T, I>(tx.data.eth_tx_hash, &author.account_id)
        {
            let expected_call_data = generate_send_calldata::<T, I>(&tx)?;
            return Ok((hex::encode(expected_call_data) == call_data, Some(num_confirmations)));
        } else {
            return Err(Error::<T, I>::ErrorGettingEthereumCallData.into());
        }
    }
    return Ok((TX_HASH_INVALID, None));
}

pub fn encode_confirmations(
    confirmations: &BoundedVec<ecdsa::Signature, ConfirmationsLimit>,
) -> Vec<u8> {
    let mut concatenated_confirmations = Vec::new();
    for conf in confirmations {
        concatenated_confirmations.extend_from_slice(conf.as_ref());
    }
    concatenated_confirmations
}

pub fn generate_send_calldata<T: Config<I>, I: 'static>(
    tx: &ActiveTransactionData<T::AccountId>,
) -> Result<Vec<u8>, Error<T, I>> {
    let concatenated_confirmations = encode_confirmations(&tx.confirmation.confirmations);
    let mut full_params = unbound_params(&tx.data.eth_tx_params);
    full_params.push((BYTES.to_vec(), concatenated_confirmations));

    abi_encode_function(&tx.request.function_name.as_slice(), &full_params)
}

fn generate_corroborate_calldata<T: Config<I>, I: 'static>(
    tx_id: EthereumId,
    expiry: u64,
) -> Result<Vec<u8>, Error<T, I>> {
    let params = vec![
        (UINT32.to_vec(), tx_id.to_string().into_bytes()),
        (UINT256.to_vec(), expiry.to_string().into_bytes()),
    ];

    abi_encode_function(b"corroborate", &params)
}

pub fn generate_encoded_lower_proof<T: Config<I>, I: 'static>(
    lower_req: &LowerProofRequestData,
    confirmations: BoundedVec<ecdsa::Signature, ConfirmationsLimit>,
) -> Vec<u8> {
    let concatenated_confirmations = encode_confirmations(&confirmations);
    let mut compact_lower_data = Vec::new();
    compact_lower_data.extend_from_slice(&lower_req.params.to_vec());
    compact_lower_data.extend_from_slice(&concatenated_confirmations);

    return compact_lower_data;
}

pub fn abi_encode_function<T: Config<I>, I: 'static>(
    function_name: &[u8],
    params: &[(Vec<u8>, Vec<u8>)],
) -> Result<Vec<u8>, Error<T, I>> {
    let inputs = params
        .iter()
        .filter_map(|(type_bytes, _)| {
            to_param_type(type_bytes).map(|kind| Param { name: "".to_string(), kind })
        })
        .collect::<Vec<_>>();

    let tokens: Result<Vec<_>, _> = params
        .iter()
        .map(|(type_bytes, value_bytes)| {
            let param_type =
                to_param_type(type_bytes).ok_or_else(|| Error::<T, I>::ParamTypeEncodingError)?;
            to_token_type(&param_type, value_bytes)
        })
        .collect();

    let function_name_utf8 =
        core::str::from_utf8(function_name).map_err(|_| Error::<T, I>::FunctionNameError)?;
    let function = Function {
        name: function_name_utf8.to_string(),
        inputs,
        outputs: Vec::<Param>::new(),
        constant: false,
    };

    function.encode_input(&tokens?).map_err(|_| Error::<T, I>::FunctionEncodingError)
}

pub fn to_param_type(key: &Vec<u8>) -> Option<ParamType> {
    match key.as_slice() {
        BYTES => Some(ParamType::Bytes),
        BYTES32 => Some(ParamType::FixedBytes(32)),
        UINT32 => Some(ParamType::Uint(32)),
        UINT128 => Some(ParamType::Uint(128)),
        UINT256 => Some(ParamType::Uint(256)),
        ADDRESS => Some(ParamType::Address),

        _ => None,
    }
}

/// Please note: `value` will accept any bytes and its up to the caller to ensure the bytes are
/// valid for `kind`. The compiler will not catch these errors at compile time, but can error at
/// runtime.
pub fn to_token_type<T: Config<I>, I: 'static>(
    kind: &ParamType,
    value: &[u8],
) -> Result<Token, Error<T, I>> {
    match kind {
        ParamType::Bytes => Ok(Token::Bytes(value.to_vec())),
        ParamType::Uint(_) => {
            let dec_str = core::str::from_utf8(value).map_err(|_| Error::<T, I>::InvalidUTF8)?;
            let dec_value = Int::from_dec_str(dec_str).map_err(|_| Error::<T, I>::InvalidUint)?;
            Ok(Token::Uint(dec_value))
        },
        ParamType::FixedBytes(size) => {
            if value.len() != *size {
                return Err(Error::<T, I>::InvalidBytes);
            }
            Ok(Token::FixedBytes(value.to_vec()))
        },
        ParamType::Address => Ok(Token::Address(Address::from_slice(value))),
        _ => Err(Error::<T, I>::InvalidParamData),
    }
}

fn get_transaction_call_data<T: Config<I>, I: 'static>(
    eth_tx_hash: H256,
    author_account_id: &T::AccountId,
) -> Result<(String, u64), DispatchError> {
    let query_request =
        EthQueryRequest { tx_hash: eth_tx_hash, response_type: EthQueryResponseType::CallData };
    make_ethereum_call::<(String, u64), T, I>(
        author_account_id,
        "query",
        query_request.encode(),
        process_query_result::<T, I>,
        None,
        None,
    )
}

fn send_transaction<T: Config<I>, I: 'static>(
    calldata: Vec<u8>,
    author: &Author<T>,
) -> Result<H256, DispatchError> {
    let eth_instance = Instance::<T, I>::get();
    let proof =
        author.key.sign(&(&author.account_id, &eth_instance.bridge_contract, &calldata).encode());
    make_ethereum_call::<H256, T, I>(
        &author.account_id,
        "send",
        calldata,
        process_tx_hash::<T, I>,
        None,
        proof,
    )
}

fn call_corroborate_method<T: Config<I>, I: 'static>(
    calldata: Vec<u8>,
    author_account_id: &T::AccountId,
) -> Result<i8, DispatchError> {
    make_ethereum_call::<i8, T, I>(
        author_account_id,
        "view",
        calldata,
        process_corroborate_result::<T, I>,
        None,
        None,
    )
}

pub fn make_ethereum_call<R, T: Config<I>, I: 'static>(
    author_account_id: &T::AccountId,
    endpoint: &str,
    calldata: Vec<u8>,
    process_result: fn(Vec<u8>) -> Result<R, DispatchError>,
    eth_block: Option<u32>,
    proof_maybe: Option<<T::AuthorityId as RuntimeAppPublic>::Signature>,
) -> Result<R, DispatchError> {
    let sender = T::AccountToBytesConvert::into_bytes(&author_account_id);
    let eth_instance = Instance::<T, I>::get();
    let ethereum_call =
        EthTransaction::new(sender, eth_instance.bridge_contract, calldata).set_block(eth_block);
    let url_path = format!("eth/{}", endpoint);
    let result = AVN::<T>::post_data_to_service(url_path, ethereum_call.encode(), proof_maybe)?;
    process_result(result)
}

fn process_tx_hash<T: Config<I>, I: 'static>(result: Vec<u8>) -> Result<H256, DispatchError> {
    if result.len() != 64 {
        return Err(Error::<T, I>::InvalidHashLength.into());
    }

    let tx_hash_string = core::str::from_utf8(&result).map_err(|_| Error::<T, I>::InvalidUTF8)?;

    let mut data: [u8; 32] = [0; 32];
    hex::decode_to_slice(tx_hash_string, &mut data[..])
        .map_err(|_| Error::<T, I>::InvalidHexString)?;

    Ok(H256::from_slice(&data))
}

fn process_corroborate_result<T: Config<I>, I: 'static>(
    result: Vec<u8>,
) -> Result<i8, DispatchError> {
    let result_bytes = hex::decode(&result).map_err(|_| Error::<T, I>::InvalidBytes)?;

    if result_bytes.len() != 32 {
        return Err(Error::<T, I>::InvalidBytesLength.into());
    }

    Ok(result_bytes[31] as i8)
}

fn process_query_result<T: Config<I>, I: 'static>(
    result: Vec<u8>,
) -> Result<(String, u64), DispatchError> {
    let result_bytes = hex::decode(&result).map_err(|_| Error::<T, I>::InvalidBytes)?;
    let (call_data, eth_tx_confirmations) = try_process_query_result::<Vec<u8>, T, I>(result_bytes)
        .map_err(|e| {
            log::error!("❌ Error processing query result from Ethereum: {:?}", e);
            e
        })?;

    Ok((hex::encode(call_data), eth_tx_confirmations))
}
