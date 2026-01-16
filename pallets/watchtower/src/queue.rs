use super::*;
use crate::Config;

// Internal queue Id that allows having multiple queues in the future.
pub type QueueId = u8;

// This is an implementation of A ring storage data structure, or ring buffer.
impl<T: Config> Pallet<T> {
    fn cap() -> u32 {
        T::MaxInternalProposalLen::get()
    }

    fn len() -> u64 {
        Tail::<T>::get().saturating_sub(Head::<T>::get())
    }

    fn is_empty() -> bool {
        Self::len() == 0
    }

    fn is_full() -> bool {
        Self::len() >= Self::cap() as u64
    }

    pub fn enqueue(proposal_id: ProposalId) -> Result<(), Error<T>> {
        ensure!(!Self::is_full(), Error::<T>::InnerProposalQueueFull);

        // Put the Id in the ring slot.
        let tail = Tail::<T>::get();
        let slot_index = (tail % Self::cap() as u64) as u32;
        // To use multiple queues in the future, add an extra param for queue id.
        let slot = (QueueId::default(), slot_index);
        // slot should be empty if queue isn’t full.
        ensure!(InternalProposalQueue::<T>::get(slot).is_none(), Error::<T>::QueueCorruptState);

        InternalProposalQueue::<T>::insert(slot, proposal_id);
        Tail::<T>::put(tail + 1);
        Ok(())
    }

    pub fn dequeue() -> Result<ProposalId, Error<T>> {
        ensure!(!Self::is_empty(), Error::<T>::QueueEmpty);

        let head = Head::<T>::get();
        let slot_index = (head % Self::cap() as u64) as u32;
        // To use multiple queues in the future, add an extra param for queue id.
        let slot = (QueueId::default(), slot_index);

        // Take Id from ring slot, then clear the slot.
        let proposal_id =
            InternalProposalQueue::<T>::take(slot).ok_or(Error::<T>::QueueCorruptState)?;

        Head::<T>::put(head + 1);
        Ok(proposal_id)
    }

    pub fn peek_front() -> Result<Option<(ProposalId, Proposal<T>)>, Error<T>> {
        if Self::is_empty() {
            return Ok(None)
        }

        let head = Head::<T>::get();
        let slot_index = (head % Self::cap() as u64) as u32;
        // To use multiple queues in the future, add an extra param for queue id.
        let slot = (QueueId::default(), slot_index);

        let proposal_id =
            InternalProposalQueue::<T>::get(slot).ok_or(Error::<T>::QueueCorruptState)?;
        let item = Proposals::<T>::get(proposal_id).ok_or(Error::<T>::QueueCorruptState)?;

        Ok(Some((proposal_id, item)))
    }
}
