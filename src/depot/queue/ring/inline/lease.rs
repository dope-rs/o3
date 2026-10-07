use inline::index;

use crate::depot::{self, arena::linked::fifo::ends, queue::ring::inline};

/// An exclusive detached slot that can return only to its original owner.
#[must_use]
pub struct Lease<'a, T, const N: usize, P: Copy = ()> {
    pub(in crate::depot::queue::ring::inline) pool: &'a inline::Pool<T, N, P>,
    pub(in crate::depot::queue::ring::inline) index: index::Index<N>,
}

impl<T, const N: usize, P: Copy> Lease<'_, T, N, P> {
    pub fn get(&self) -> &T {
        // SAFETY: removal mints the only lease, retained for this borrow.
        unsafe { &*self.index.get(&self.pool.entries).value.get() }
    }

    pub fn get_mut(&mut self) -> &mut T {
        // SAFETY: neither list owns this slot until its unique lease is consumed.
        unsafe { &mut *self.index.get(&self.pool.entries).value.get() }
    }

    /// Transfers this slot into its owner's pending FIFO without validation.
    pub fn push_back(self, payload: P) {
        // SAFETY: the unique lease owns this payload storage until publication.
        unsafe { (*self.index.get(&self.pool.entries).payload.get()).write(payload) };
        let list = &self.pool.pending;
        self.link(list);
    }

    /// Returns this slot to its owner's available FIFO without validation.
    pub fn release(self) {
        let list = &self.pool.free;
        self.link(list);
    }

    /// Links this detached slot into a list projected from its retained pool.
    fn link(self, list: &ends::Cell<Option<index::Index<N>>>) {
        self.index.get(&self.pool.entries).next.set(None);
        // SAFETY: this lease exclusively owns the detached slot and list belongs
        // to its original pool; its prior tail is linked before publication.
        ends::Cell::push_back(unsafe {
            depot::raw::Proof::new((list, self.index, |tail: Option<index::Index<N>>| {
                if let Some(tail) = tail {
                    tail.get(&self.pool.entries).next.set(Some(self.index));
                }
            }))
        });
    }
}

const _: () = {
    assert!(size_of::<Lease<'static, u64, 64>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Lease<'static, u64, 64>>>() == size_of::<Lease<'static, u64, 64>>());
};
