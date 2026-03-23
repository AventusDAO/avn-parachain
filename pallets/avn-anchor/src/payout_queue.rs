use crate::{
    pallet::{PayoutQueueHead, PayoutQueueItems, PayoutQueueTail},
    Config,
};
use sp_avn_common::RewardPeriodIndex;

/// An unbounded FIFO queue of reward period indices pending app-chain payout.
///
/// Storage is split across three pallet storage items declared in `lib.rs`:
/// - `PayoutQueueHead`: index of the next entry to dequeue.
/// - `PayoutQueueTail`: index where the next enqueued entry will be written.
/// - `PayoutQueueItems`: map from position index to `RewardPeriodIndex`.
///
/// Enqueue and dequeue are both O(1). Dequeued entries are removed via `take`.
/// The indices are `u64` and will not overflow in practice.
pub struct PayoutQueue<T>(core::marker::PhantomData<T>);

impl<T: Config> PayoutQueue<T> {
    /// Add `period_index` to the back of the queue.
    pub fn enqueue(period_index: RewardPeriodIndex) {
        let tail = PayoutQueueTail::<T>::get();
        PayoutQueueItems::<T>::insert(tail, period_index);
        PayoutQueueTail::<T>::put(tail.saturating_add(1));
    }

    /// Remove and return the front entry, or `None` if the queue is empty.
    pub fn dequeue() -> Option<RewardPeriodIndex> {
        let head = PayoutQueueHead::<T>::get();
        if head == PayoutQueueTail::<T>::get() {
            return None
        }
        let period = PayoutQueueItems::<T>::take(head);
        PayoutQueueHead::<T>::put(head.saturating_add(1));
        period
    }

    pub fn is_empty() -> bool {
        PayoutQueueHead::<T>::get() == PayoutQueueTail::<T>::get()
    }
}
