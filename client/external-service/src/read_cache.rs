// Copyright 2026 Aventus DAO Ltd

//! Shares the result of an expensive read between everyone asking for the same thing.
//!
//! The oracle observation routes read a fixed block (the floor at a pinned block, the venue at
//! the block a target timestamp names), so a successful answer never changes and can be served
//! again. Callers asking while a read is running wait for that read instead of starting another.
//! The read runs on its own task, so it finishes, and its result is kept, even when every
//! caller has gone: an offchain worker whose request timed out finds the result on its next
//! attempt instead of starting over.
//!
//! Failures are not kept. Callers already waiting on a read that fails all get its error; the
//! next caller starts a new read. Only the `capacity` most recently started keys are kept.

use futures::future::{BoxFuture, FutureExt, Shared};
use std::{
    collections::VecDeque,
    future::Future,
    sync::{Arc, Mutex},
};

/// Shared by concurrent callers, so the error is behind an `Arc`.
pub type ReadResult<V> = Result<V, Arc<anyhow::Error>>;

type SharedRead<V> = Shared<BoxFuture<'static, ReadResult<V>>>;

pub struct ReadCache<K, V: Clone> {
    capacity: usize,
    entries: Mutex<VecDeque<(K, SharedRead<V>)>>,
}

impl<K, V> ReadCache<K, V>
where
    K: PartialEq,
    V: Clone + Send + Sync + 'static,
{
    pub fn new(capacity: usize) -> Self {
        Self { capacity: capacity.max(1), entries: Mutex::new(VecDeque::new()) }
    }

    /// The result for `key`: a kept one, the one of a read already running, or that of a new
    /// read started with `read`. Must be called from within a tokio runtime.
    pub async fn get_or_read<F, Fut>(&self, key: K, read: F) -> ReadResult<V>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<V>> + Send + 'static,
    {
        self.entry(key, read).await
    }

    fn entry<F, Fut>(&self, key: K, read: F) -> SharedRead<V>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<V>> + Send + 'static,
    {
        let mut entries = self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(index) = entries.iter().position(|(k, _)| *k == key) {
            if !matches!(entries[index].1.peek(), Some(Err(_))) {
                return entries[index].1.clone()
            }
            entries.remove(index);
        }

        let shared = read().map(|result| result.map_err(Arc::new)).boxed().shared();
        // Drives the read to the end even if every caller drops its copy.
        tokio::spawn(shared.clone());
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back((key, shared.clone()));
        shared
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use tokio::sync::Notify;

    /// A read that counts its runs and returns `value`, or fails when `value` is `None`.
    fn counted(
        runs: &Arc<AtomicUsize>,
        value: Option<u32>,
    ) -> impl FnOnce() -> BoxFuture<'static, anyhow::Result<u32>> {
        let runs = runs.clone();
        move || {
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                value.ok_or_else(|| anyhow!("read failed"))
            }
            .boxed()
        }
    }

    #[tokio::test]
    async fn a_result_is_read_once_and_served_again() {
        let cache = ReadCache::new(4);
        let runs = Arc::new(AtomicUsize::new(0));
        assert_eq!(cache.get_or_read(1, counted(&runs, Some(10))).await.unwrap(), 10);
        assert_eq!(cache.get_or_read(1, counted(&runs, Some(99))).await.unwrap(), 10);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(cache.get_or_read(2, counted(&runs, Some(20))).await.unwrap(), 20);
        assert_eq!(runs.load(Ordering::SeqCst), 2, "another key is another read");
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_read() {
        let cache = Arc::new(ReadCache::new(4));
        let runs = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());

        let callers: Vec<_> = (0..5)
            .map(|_| {
                let (cache, runs, release) = (cache.clone(), runs.clone(), release.clone());
                tokio::spawn(async move {
                    cache
                        .get_or_read(1, move || async move {
                            runs.fetch_add(1, Ordering::SeqCst);
                            release.notified().await;
                            Ok(7)
                        })
                        .await
                })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(20)).await;
        release.notify_one();

        for caller in callers {
            assert_eq!(caller.await.unwrap().unwrap(), 7);
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failure_is_not_kept() {
        let cache = ReadCache::new(4);
        let runs = Arc::new(AtomicUsize::new(0));
        assert!(cache.get_or_read(1, counted(&runs, None)).await.is_err());
        assert_eq!(cache.get_or_read(1, counted(&runs, Some(10))).await.unwrap(), 10);
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_read_finishes_after_its_caller_gives_up() {
        let cache = ReadCache::new(4);
        let runs = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());

        let slow = {
            let (runs, release) = (runs.clone(), release.clone());
            move || async move {
                runs.fetch_add(1, Ordering::SeqCst);
                release.notified().await;
                Ok(7)
            }
        };
        let gave_up =
            tokio::time::timeout(Duration::from_millis(20), cache.get_or_read(1, slow)).await;
        assert!(gave_up.is_err(), "the caller timed out");
        release.notify_one();

        assert_eq!(cache.get_or_read(1, counted(&runs, Some(99))).await.unwrap(), 7);
        assert_eq!(runs.load(Ordering::SeqCst), 1, "the first read was not restarted");
    }

    #[tokio::test]
    async fn the_oldest_key_is_dropped_past_capacity() {
        let cache = ReadCache::new(2);
        let runs = Arc::new(AtomicUsize::new(0));
        for key in [1, 2, 3] {
            cache.get_or_read(key, counted(&runs, Some(key))).await.unwrap();
        }
        assert_eq!(cache.get_or_read(3, counted(&runs, Some(0))).await.unwrap(), 3);
        assert_eq!(cache.get_or_read(1, counted(&runs, Some(11))).await.unwrap(), 11);
        assert_eq!(runs.load(Ordering::SeqCst), 4, "key 1 was read again");
    }
}
