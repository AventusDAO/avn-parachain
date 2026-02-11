use crate::{
    chain::ChainClient, eth_signing::sign_digest_from_keystore, keystore_utils::*,
    signing::SignerProvider, timer::Timer,
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
    http_data_codec::decode_from_http_data, EthQueryRequest, EthQueryResponse,
    EthQueryResponseType, EthTransaction, DEFAULT_EXTERNAL_SERVICE_PORT_NUMBER,
};
use sp_core::{sr25519, H160, H256};
use sp_runtime::traits::Block as BlockT;
use std::{marker::PhantomData, net::SocketAddr, sync::Arc, time::Instant};
use tower_http::limit::RequestBodyLimitLayer;

const MAX_BODY_SIZE: usize = 100_000; // 100KB

#[derive(Clone)]
pub struct AppState<Block: BlockT, ClientT: BlockBackend<Block> + UsageProvider<Block>> {
    pub keystore: Arc<LocalKeystore>,
    pub keystore_path: std::path::PathBuf,
    pub avn_port: Option<String>,
    pub chain: Arc<dyn ChainClient>,
    pub signer_provider: Arc<dyn SignerProvider>,
    pub client: Arc<ClientT>,
    pub _block: PhantomData<Block>,
}
fn server_error(msg: impl Into<String>) -> (StatusCode, String) {
    let m = msg.into();
    log::error!("⛓️ 💔 external-service {}", m);
    (StatusCode::INTERNAL_SERVER_ERROR, m)
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
        return Err(server_error("X-Auth token verification failed"));
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
    let _t = Timer::new("eth/send");

    let send_request = EthTransaction::decode(&mut &body[..])
        .map_err(|e| server_error(format!("Error decoding EthTransaction: {e:?}")))?;

    let proof_data = (&send_request.from, &send_request.to, &send_request.data).encode();
    validate_authorisation_token(&state.keystore, &headers, &proof_data)?;

    let to: H160 = send_request.to;
    let data: Vec<u8> = send_request.data;

    let signed_chain = state
        .signer_provider
        .signed_chain_client()
        .await
        .map_err(|e| server_error(format!("SignerProvider: {e:?}")))?;

    let tx_hash = signed_chain
        .send_transaction(to, data)
        .await
        .map_err(|e| server_error(format!("send_transaction: {e:?}")))?;

    Ok(hex::encode(tx_hash))
}

async fn view<Block: BlockT, ClientT>(
    State(state): State<Arc<AppState<Block, ClientT>>>,
    body: AxumBytes,
) -> Result<String, (StatusCode, String)>
where
    ClientT: BlockBackend<Block> + UsageProvider<Block> + Send + Sync + 'static,
{
    let _t = Timer::new("eth/view");

    let view_request = EthTransaction::decode(&mut &body[..])
        .map_err(|e| server_error(format!("Error decoding EthTransaction: {e:?}")))?;

    let to: H160 = view_request.to;
    let input: Vec<u8> = view_request.data;

    let out = state
        .chain
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
    let _t = Timer::new("eth/query");

    let request = EthTransaction::decode(&mut &body[..])
        .map_err(|e| server_error(format!("Error decoding EthTransaction: {e:?}")))?;

    let query_request = EthQueryRequest::decode(&mut &request.data[..])
        .map_err(|e| server_error(format!("Error decoding EthQueryRequest: {e:?}")))?;

    let tx_hash = H256::from_slice(query_request.tx_hash.as_bytes());

    let current_block = state
        .chain
        .block_number()
        .await
        .map_err(|e| server_error(format!("Error getting block number: {e:?}")))?;

    match query_request.response_type {
        EthQueryResponseType::CallData => {
            let input = state
                .chain
                .get_transaction_input(tx_hash)
                .await
                .map_err(|e| server_error(format!("Error getting tx input: {e:?}")))?;

            Ok(to_eth_query_response(input.unwrap_or_default(), current_block, None))
        },

        EthQueryResponseType::TransactionReceipt => {
            let receipt = state
                .chain
                .get_receipt(tx_hash)
                .await
                .map_err(|e| server_error(format!("Error getting receipt: {e:?}")))?;

            if let Some(r) = receipt {
                // IMPORTANT: r.json is already the upstream receipt JSON bytes.
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
        return Ok(hex::encode([0u8; 32]));
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

    sign_digest_from_keystore(&state.keystore_path, &msg_bytes)
        .map_err(|e| server_error(format!("{e:?}")))
}
