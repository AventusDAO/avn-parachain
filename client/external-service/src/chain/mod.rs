use sp_core::{H160, H256};

pub type ChainAddress = sp_core::H160;
pub type ChainHash = sp_core::H256;

#[derive(Clone, Debug)]
pub struct ChainLog {
    pub address: ChainAddress,
    pub topics: Vec<ChainHash>,
    pub data: Vec<u8>,
    pub transaction_hash: Option<ChainHash>,
    pub block_number: Option<u64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChainReceipt {
    pub block_number: Option<u64>,
    pub json: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct LogFilter {
    pub from_block: u64,
    pub to_block: u64,
    pub addresses: Vec<ChainAddress>,
    pub topics: [Option<Vec<ChainHash>>; 4],
}

#[async_trait::async_trait]
pub trait ChainClient: Send + Sync {
    async fn block_number(&self) -> anyhow::Result<u64>;
    async fn chain_id(&self) -> anyhow::Result<u64>;
    async fn get_logs(&self, filter: LogFilter) -> anyhow::Result<Vec<ChainLog>>;
    async fn get_receipt(&self, tx_hash: H256) -> anyhow::Result<Option<ChainReceipt>>;
    async fn get_transaction_input(&self, tx_hash: H256) -> anyhow::Result<Option<Vec<u8>>>;
    async fn read_call(&self, to: H160, data: Vec<u8>) -> anyhow::Result<Vec<u8>>;
    async fn send_transaction(&self, to: H160, data: Vec<u8>) -> anyhow::Result<H256>;
}
