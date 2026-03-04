//! Transaction pool filter: rejects extrinsics before they enter the pool.
//! Used by the node to wrap the pool with `FilteredPool`.

use async_trait::async_trait;
use codec::Encode;
use sc_transaction_pool_api::{
    error::Error as PoolError, ChainEvent, ImportNotificationStream, MaintainedTransactionPool,
    PoolStatus, ReadyTransactions, TransactionFor, TransactionPool, TransactionSource,
    TransactionStatusStreamFor, TxHash, TxInvalidityReportMap,
};
use sp_runtime::traits::Block as BlockT;
use std::{collections::HashMap, pin::Pin, sync::Arc};

// Re-export FilterResult from parent module
pub use crate::FilterResult;

impl FilterResult {
    /// Returns true if the extrinsic should be rejected (banned).
    pub fn is_banned(&self) -> bool {
        !matches!(self, FilterResult::Allowed)
    }
}

/// Filter that decides if an extrinsic (as raw bytes) is allowed in the pool.
pub trait ExtrinsicFilter: Send + Sync + 'static {
    /// Check if an extrinsic is allowed. Returns rich result for logging.
    fn check(&self, xt: &sp_core::Bytes) -> FilterResult;
}

/// Wraps a transaction pool and applies an [`ExtrinsicFilter`] before submissions.
pub struct FilteredPool<Pool> {
    inner: Arc<Pool>,
    filter: Arc<dyn ExtrinsicFilter>,
}

impl<Pool> FilteredPool<Pool> {
    /// Create a new filtered pool.
    pub fn new(inner: Arc<Pool>, filter: Arc<dyn ExtrinsicFilter>) -> Self {
        Self { inner, filter }
    }

    fn check_allowed(&self, xt: &impl Encode) -> Result<(), PoolError> {
        let result = self.filter.check(&xt.encode().into());
        if result.is_banned() {
            return Err(PoolError::InvalidTransaction(
                sp_runtime::transaction_validity::InvalidTransaction::Call,
            ))
        }
        Ok(())
    }
}

impl<Pool> Clone for FilteredPool<Pool> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone(), filter: self.filter.clone() }
    }
}

#[async_trait]
impl<Pool> TransactionPool for FilteredPool<Pool>
where
    Pool: TransactionPool,
    Pool::Error: 'static,
{
    type Block = Pool::Block;
    type Hash = Pool::Hash;
    type InPoolTransaction = Pool::InPoolTransaction;
    type Error = Pool::Error;

    async fn submit_at(
        &self,
        at: <Self::Block as BlockT>::Hash,
        source: TransactionSource,
        xts: Vec<TransactionFor<Self>>,
    ) -> Result<Vec<Result<TxHash<Self>, Self::Error>>, Self::Error> {
        let len = xts.len();
        let mut allowed_xts = Vec::with_capacity(len);
        let mut allowed_indices = Vec::with_capacity(len);
        let mut results: Vec<Option<Result<TxHash<Self>, Self::Error>>> =
            (0..xts.len()).map(|_| None).collect();

        for (i, xt) in xts.into_iter().enumerate() {
            match self.check_allowed(&xt) {
                Ok(_) => {
                    allowed_xts.push(xt);
                    allowed_indices.push(i);
                },
                Err(e) => results[i] = Some(Err(e.into())),
            }
        }

        if allowed_xts.is_empty() {
            let mut final_result = Vec::with_capacity(len);
            for r in results.into_iter() {
                match r {
                    Some(res) => final_result.push(res),
                    None => return Err(PoolError::Unactionable.into()),
                }
            }
            return Ok(final_result)
        }

        let inner_results = self.inner.submit_at(at, source, allowed_xts).await?;

        if inner_results.len() != allowed_indices.len() {
            return Err(PoolError::Unactionable.into())
        }

        for (result, index) in inner_results.into_iter().zip(allowed_indices) {
            results[index] = Some(result);
        }

        let mut final_result = Vec::with_capacity(len);
        for r in results.into_iter() {
            match r {
                Some(res) => final_result.push(res),
                None => return Err(PoolError::Unactionable.into()),
            }
        }
        Ok(final_result)
    }

    async fn submit_one(
        &self,
        at: <Self::Block as BlockT>::Hash,
        source: TransactionSource,
        xt: TransactionFor<Self>,
    ) -> Result<TxHash<Self>, Self::Error> {
        if let Err(e) = self.check_allowed(&xt) {
            return Err(e.into())
        }
        self.inner.submit_one(at, source, xt).await
    }

    async fn submit_and_watch(
        &self,
        at: <Self::Block as BlockT>::Hash,
        source: TransactionSource,
        xt: TransactionFor<Self>,
    ) -> Result<Pin<Box<TransactionStatusStreamFor<Self>>>, Self::Error> {
        if let Err(e) = self.check_allowed(&xt) {
            return Err(e.into())
        }
        self.inner.submit_and_watch(at, source, xt).await
    }

    async fn ready_at(
        &self,
        at: <Self::Block as BlockT>::Hash,
    ) -> Box<dyn ReadyTransactions<Item = Arc<Self::InPoolTransaction>> + Send> {
        self.inner.ready_at(at).await
    }

    fn ready(&self) -> Box<dyn ReadyTransactions<Item = Arc<Self::InPoolTransaction>> + Send> {
        self.inner.ready()
    }

    fn report_invalid(
        &self,
        at: Option<<Self::Block as BlockT>::Hash>,
        invalid_tx_errors: TxInvalidityReportMap<TxHash<Self>>,
    ) -> Vec<Arc<Self::InPoolTransaction>> {
        self.inner.report_invalid(at, invalid_tx_errors)
    }

    fn status(&self) -> PoolStatus {
        self.inner.status()
    }

    fn import_notification_stream(&self) -> ImportNotificationStream<TxHash<Self>> {
        self.inner.import_notification_stream()
    }

    fn on_broadcasted(&self, propagations: HashMap<TxHash<Self>, Vec<String>>) {
        self.inner.on_broadcasted(propagations)
    }

    fn hash_of(&self, xt: &TransactionFor<Self>) -> TxHash<Self> {
        self.inner.hash_of(xt)
    }

    fn ready_transaction(&self, hash: &TxHash<Self>) -> Option<Arc<Self::InPoolTransaction>> {
        self.inner.ready_transaction(hash)
    }

    async fn ready_at_with_timeout(
        &self,
        at: <Self::Block as BlockT>::Hash,
        timeout: std::time::Duration,
    ) -> Box<dyn ReadyTransactions<Item = Arc<Self::InPoolTransaction>> + Send> {
        self.inner.ready_at_with_timeout(at, timeout).await
    }

    fn futures(&self) -> Vec<Self::InPoolTransaction> {
        self.inner.futures()
    }
}

#[async_trait]
impl<Pool> MaintainedTransactionPool for FilteredPool<Pool>
where
    Pool: MaintainedTransactionPool,
    Pool::Error: 'static,
{
    async fn maintain(&self, event: ChainEvent<Self::Block>) {
        self.inner.maintain(event).await
    }
}

impl<Pool> sc_transaction_pool_api::LocalTransactionPool for FilteredPool<Pool>
where
    Pool: sc_transaction_pool_api::LocalTransactionPool,
{
    type Block = Pool::Block;
    type Hash = Pool::Hash;
    type Error = Pool::Error;

    fn submit_local(
        &self,
        at: <Self::Block as BlockT>::Hash,
        xt: sc_transaction_pool_api::LocalTransactionFor<Self>,
    ) -> Result<Self::Hash, Self::Error> {
        if let Err(e) = self.check_allowed(&xt) {
            return Err(e.into())
        }
        self.inner.submit_local(at, xt)
    }
}
