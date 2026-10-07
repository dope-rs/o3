use std::{cell, mem};

use inline::{index, slot};

use crate::{
    base::marker::ThreadBound,
    depot::{self, arena::linked::fifo::ends, queue::ring::inline},
};

/// One inline slot owner with available and pending FIFO lists.
pub struct Pool<T, const N: usize, P: Copy = ()> {
    _thread: ThreadBound,
    pub(in crate::depot::queue::ring::inline) entries: [slot::Slot<T, N, P>; N],
    pub(in crate::depot::queue::ring::inline) free: ends::Cell<Option<index::Index<N>>>,
    pub(in crate::depot::queue::ring::inline) pending: ends::Cell<Option<index::Index<N>>>,
}

impl<T, const N: usize, P: Copy> Pool<T, N, P> {
    pub fn new(values: [T; N]) -> Self {
        let (head, tail) = match index::Index::bounds() {
            Some((head, tail)) => (Some(head), Some(tail)),
            None => (None, None),
        };
        let mut next = head.and_then(index::Index::next);
        let entries = values.map(|value| {
            let slot = slot::Slot {
                value: cell::UnsafeCell::new(value),
                payload: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
                next: cell::Cell::new(next),
            };
            next = next.and_then(index::Index::next);
            slot
        });
        Self {
            _thread: ThreadBound::NEW,
            entries,
            // SAFETY: bounds and next constructed this complete exclusive chain.
            free: unsafe { ends::Cell::from_parts(head, tail) },
            pending: ends::Cell::empty(),
        }
    }

    /// Acquires one exclusive slot, preserving this owner's lifetime.
    pub fn try_acquire(&self) -> Result<inline::Lease<'_, T, N, P>, inline::Full> {
        self.pop::<false>().ok_or(inline::Full { capacity: N })
    }

    pub fn pop_front(&self) -> Option<(inline::Lease<'_, T, N, P>, P)> {
        let lease = self.pop::<true>()?;
        // SAFETY: pending membership owns the payload initialized by push_back.
        let payload = unsafe { (*lease.index.get(&self.entries).payload.get()).assume_init_read() };
        Some((lease, payload))
    }

    fn pop<const PENDING: bool>(&self) -> Option<inline::Lease<'_, T, N, P>> {
        let list = if PENDING { &self.pending } else { &self.free };
        // SAFETY: this exact list owns its front; the callback returns its stored
        // successor and transfers the detached slot to one original-owner lease.
        ends::Cell::pop_front(unsafe {
            depot::raw::Proof::new((list, (), |head: index::Index<N>, ()| {
                (
                    head.get(&self.entries).next.get(),
                    inline::Lease {
                        pool: self,
                        index: head,
                    },
                )
            }))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
