// Copyright 2026 Aventus DAO Ltd

use crate::{
    chain::ChainClient, keystore_utils::*, signing::SignerProvider, timer::OperationTimer,
};
use anyhow::Result;
use axum::{
    body::Bytes as AxumBytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Router,
};
use codec::{Decode, Encode};
use sc_client_api::{client::BlockBackend, UsageProvider};
use sc_keystore::LocalKeystore;
use sp_avn_common::{
    http_data_codec::decode_from_http_data, short_hex, EthQueryRequest, EthQueryResponse,
    EthQueryResponseType, EthTransaction, DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER,
};
use sp_core::{blake2_256, sr25519, H160, H256};
use sp_runtime::traits::Block as BlockT;
use std::{marker::PhantomData, net::SocketAddr, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tower_http::limit::RequestBodyLimitLayer;

const MAX_BODY_SIZE: usize = 100_000; // 100KB

#[derive(Clone)]
pub struct AppState<Block: BlockT, ClientT: BlockBackend<Block> + UsageProvider<Block>> {
    pub keystore: Arc<LocalKeystore>,
    pub keystore_path: std::path::PathBuf,
    pub avn_port: Option<String>,
    pub chain: Option<Arc<dyn ChainClient>>,
    pub signer_provider: Option<Arc<dyn SignerProvider>>,
    pub client: Arc<ClientT>,
    pub send_lock: Arc<Mutex<()>>,
    pub _block: PhantomData<Block>,
}

fn server_error(msg: impl Into<String>) -> (StatusCode, String) {
    let m = msg.into();
    log::error!("⛓️ 💔 external-service {}", m);
    (StatusCode::INTERNAL_SERVER_ERROR, m)
}

fn h160_hex(v: &H160) -> String {
    hex::encode(v.as_bytes())
}

fn request_id<T: Encode>(from: &T, to: &H160, data: &[u8]) -> String {
    let encoded = (from, to, data).encode();
    let fingerprint = blake2_256(&encoded);
    short_hex(&fingerprint)
}

fn validate_authorisation_token(
    keystore: &LocalKeystore,
    headers: &HeaderMap,
    msg_bytes: &[u8],
) -> Result<(), (StatusCode, String)> {
    let token = headers
        .get("X-Auth")
        .ok_or_else(|| server_error("Missing X-Auth token"))?
        .to_str()
        .map_err(|_| server_error("Invalid X-Auth header"))?
        .trim();

    let signature_token = decode_from_http_data::<sr25519::Signature>(token)
        .map_err(|e| server_error(format!("Error decoding X-Auth token: {e:?}")))?;

    if !authenticate_token(keystore, msg_bytes, signature_token) {
        return Err(server_error("X-Auth token verification failed"))
    }

    Ok(())
}

fn to_eth_query_response(data: Vec<u8>, current_block: u64, data_block: Option<u64>) -> String {
    let num_confirmations = current_block.saturating_sub(data_block.unwrap_or_default());
    hex::encode(EthQueryResponse { data: data.encode(), num_confirmations }.encode())
}

pub async fn start<Block: BlockT, ClientT>(state: AppState<Block, ClientT>)
where
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    if let Some(chain) = &state.chain {
        match chain.chain_id().await {
            Ok(id) => log::info!("external-service read chain initialised, chain_id={}", id),
            Err(e) => {
                log::error!("external-service failed to initialise read chain client: {:?}", e);
                return
            },
        }
    } else {
        log::info!("external-service read chain not configured");
    }

    if let Some(signer_provider) = &state.signer_provider {
        match signer_provider.signed_chain_client().await {
            Ok(client) => match client.chain_id().await {
                Ok(id) => log::info!("external-service signed chain initialised, chain_id={}", id),
                Err(e) => {
                    log::error!(
                        "external-service failed to query chain_id from signed client: {:?}",
                        e
                    );
                    return
                },
            },
            Err(e) => {
                log::error!("external-service failed to initialise signed client: {:?}", e);
                return
            },
        }
    } else {
        log::info!("external-service signed chain not configured");
    }

    let port = state
        .avn_port
        .clone()
        .unwrap_or_else(|| DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER.to_string());
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().expect("valid listen addr");

    let app = Router::new()
        .route("/eth/sign_hashed_data", post(sign_hashed_data::<Block, ClientT>))
        .route("/eth/send", post(send::<Block, ClientT>))
        .route("/eth/view", post(view::<Block, ClientT>))
        .route("/eth/query", post(query::<Block, ClientT>))
        .route("/roothash/{from_block}/{to_block}", get(roothash::<Block, ClientT>))
        .route("/latest_finalised_block", get(latest_finalised_block::<Block, ClientT>))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .with_state(Arc::new(state));

    log::info!("external-service listening on {}", addr);
    let _ = axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app).await;
}

async fn send<Block: BlockT, ClientT>(
    State(state): State<Arc<AppState<Block, ClientT>>>,
    headers: HeaderMap,
    body: AxumBytes,
) -> Result<String, (StatusCode, String)>
where
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    let _t = OperationTimer::new("eth/send");

    let send_request = EthTransaction::decode(&mut &body[..])
        .map_err(|e| server_error(format!("Error decoding EthTransaction: {e:?}")))?;

    let proof_data = (&send_request.from, &send_request.to, &send_request.data).encode();
    let to: H160 = send_request.to;
    let data: Vec<u8> = send_request.data;
    let req_id = request_id(&send_request.from, &to, &data);

    log::info!(
        "external-service eth/send start: req_id={}, request_from={:?}, to=0x{}, body_len={}, data_len={}",
        req_id,
        send_request.from,
        h160_hex(&to),
        body.len(),
        data.len(),
    );

    log::debug!(
        "external-service eth/send payload: req_id={}, data=0x{}, proof_data=0x{}",
        req_id,
        hex::encode(&data),
        hex::encode(&proof_data),
    );

    validate_authorisation_token(&state.keystore, &headers, &proof_data)?;

    log::debug!("external-service eth/send auth_ok: req_id={}", req_id);

    let signer_provider = state
        .signer_provider
        .as_ref()
        .ok_or_else(|| server_error("Ethereum signer not configured"))?;

    let signer_eth_address =
        get_eth_address_bytes_from_keystore(&state.keystore_path).map_err(|e| {
            server_error(format!("Failed to read signer eth address from keystore: {e:?}"))
        })?;

    let _guard = state.send_lock.lock().await;

    let signed_chain = signer_provider
        .signed_chain_client()
        .await
        .map_err(|e| server_error(format!("SignerProvider: {e:?}")))?;

    let chain_id = signed_chain
        .chain_id()
        .await
        .map_err(|e| server_error(format!("chain_id: {e:?}")))?;

    log::debug!(
        "external-service eth/send resolved: req_id={}, chain_id={}, eth_signer=0x{}",
        req_id,
        chain_id,
        hex::encode(&signer_eth_address),
    );

    let tx_hash = match signed_chain.send_transaction(to, data.clone()).await {
        Ok(hash) => hash,
        Err(e) => {
            log::error!(
                "💔 external-service eth/send failed: req_id={}, chain_id={}, eth_signer=0x{}, request_from={:?}, to=0x{}, data_len={}, error={:?}",
                req_id,
                chain_id,
                hex::encode(&signer_eth_address),
                send_request.from,
                h160_hex(&to),
                data.len(),
                e
            );

            log::debug!(
                "external-service eth/send failed payload: req_id={}, data=0x{}, proof_data=0x{}",
                req_id,
                hex::encode(&data),
                hex::encode(&proof_data),
            );

            return Err(server_error(format!("send_transaction: {e:?}")))
        },
    };

    log::info!(
        "external-service eth/send submitted: req_id={}, chain_id={}, eth_signer=0x{}, request_from={:?}, to=0x{}, tx_hash=0x{}",
        req_id,
        chain_id,
        hex::encode(&signer_eth_address),
        send_request.from,
        h160_hex(&to),
        hex::encode(tx_hash.as_bytes()),
    );

    Ok(hex::encode(tx_hash))
}

async fn view<Block: BlockT, ClientT>(
    State(state): State<Arc<AppState<Block, ClientT>>>,
    body: AxumBytes,
) -> Result<String, (StatusCode, String)>
where
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    let _t = OperationTimer::new("eth/view");

    let view_request = EthTransaction::decode(&mut &body[..])
        .map_err(|e| server_error(format!("Error decoding EthTransaction: {e:?}")))?;

    let to: H160 = view_request.to;
    let input: Vec<u8> = view_request.data;

    log::debug!(
        "external-service eth/view request: to=0x{}, input_len={}",
        h160_hex(&to),
        input.len(),
    );

    let chain = state
        .chain
        .as_ref()
        .ok_or_else(|| server_error("Ethereum read client not configured"))?;

    let out = chain
        .read_call(to, input)
        .await
        .map_err(|e| server_error(format!("Error calling chain: {e:?}")))?;

    Ok(hex::encode(out))
}

async fn query<Block: BlockT, ClientT>(
    State(state): State<Arc<AppState<Block, ClientT>>>,
    body: AxumBytes,
) -> Result<String, (StatusCode, String)>
where
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    let _t = OperationTimer::new("eth/query");

    let request = EthTransaction::decode(&mut &body[..])
        .map_err(|e| server_error(format!("Error decoding EthTransaction: {e:?}")))?;

    let query_request = EthQueryRequest::decode(&mut &request.data[..])
        .map_err(|e| server_error(format!("Error decoding EthQueryRequest: {e:?}")))?;

    let tx_hash = H256::from_slice(query_request.tx_hash.as_bytes());

    let chain = state
        .chain
        .as_ref()
        .ok_or_else(|| server_error("Ethereum read client not configured"))?;

    let current_block = chain
        .block_number()
        .await
        .map_err(|e| server_error(format!("Error getting block number: {e:?}")))?;

    log::debug!(
        "external-service eth/query request: tx_hash=0x{}, response_type={:?}, current_block={}",
        hex::encode(tx_hash.as_bytes()),
        query_request.response_type,
        current_block,
    );

    match query_request.response_type {
        EthQueryResponseType::CallData => {
            let input = chain
                .get_transaction_input(tx_hash)
                .await
                .map_err(|e| server_error(format!("Error getting tx input: {e:?}")))?;

            Ok(to_eth_query_response(input.unwrap_or_default(), current_block, None))
        },

        EthQueryResponseType::TransactionReceipt => {
            let receipt = chain
                .get_receipt(tx_hash)
                .await
                .map_err(|e| server_error(format!("Error getting receipt: {e:?}")))?;

            if let Some(r) = receipt {
                Ok(to_eth_query_response(r.json, current_block, r.block_number))
            } else {
                Ok("".to_string())
            }
        },
    }
}

async fn roothash<Block: BlockT, ClientT>(
    State(state): State<Arc<AppState<Block, ClientT>>>,
    Path((from_block, to_block)): Path<(u32, u32)>,
) -> Result<String, (StatusCode, String)>
where
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    use client_extrinsic_utils::summary_utils::{generate_tree_root, get_extrinsics};

    let extrinsics_start = Instant::now();
    let extrinsics = get_extrinsics::<Block, ClientT>(&state.client, from_block, to_block)
        .map_err(|e| server_error(format!("{e:?}")))?;
    log::info!("⏲️ get_extrinsics [{from_block},{to_block}] {:?}", extrinsics_start.elapsed());

    if extrinsics.is_empty() {
        return Ok(hex::encode([0u8; 32]))
    }

    let root_start = Instant::now();
    let root = generate_tree_root(extrinsics).map_err(|e| server_error(format!("{e:?}")))?;
    log::info!("⏲️ generate_tree_root [{from_block},{to_block}] {:?}", root_start.elapsed());

    Ok(hex::encode(root))
}

async fn latest_finalised_block<Block: BlockT, ClientT>(
    State(state): State<Arc<AppState<Block, ClientT>>>,
) -> Result<String, (StatusCode, String)>
where
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    use client_extrinsic_utils::extrinsic_utils::get_latest_finalised_block;
    let n = get_latest_finalised_block(&state.client);
    Ok(hex::encode(n.encode()))
}

async fn sign_hashed_data<Block: BlockT, ClientT>(
    State(state): State<Arc<AppState<Block, ClientT>>>,
    headers: HeaderMap,
    body: AxumBytes,
) -> Result<String, (StatusCode, String)>
where
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    let msg_bytes = hex::decode(&body)
        .map_err(|e| server_error(format!("Error decoding digest hex: {e:?}")))?;

    validate_authorisation_token(&state.keystore, &headers, &msg_bytes)?;

    let digest: &[u8; 32] = msg_bytes
        .as_slice()
        .try_into()
        .map_err(|_| server_error("digest must be 32 bytes"))?;

    log::debug!("external-service eth/sign_hashed_data request: digest=0x{}", hex::encode(digest),);

    let signer_provider = state
        .signer_provider
        .as_ref()
        .ok_or_else(|| server_error("Ethereum signer not configured"))?;

    signer_provider
        .sign_digest(digest)
        .await
        .map_err(|e| server_error(format!("{e:?}")))
}
