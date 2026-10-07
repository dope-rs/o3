use std::num::NonZero;

use pool::state;

use crate::buffer::{pool, pool::owners};

/// A nonempty completed prefix bound to its original output lease.
#[must_use = "commit publishes the completed prefix"]
pub struct Complete<'a, L = pool::Layout> {
    pub(in crate::buffer::pool) target:
        &'a mut pool::Span<owners::Plain<L>, pool::Owned, state::Initialized>,
    pub(in crate::buffer::pool) len: NonZero<usize>,
}

impl<'a, L> Complete<'a, L> {
    /// # Safety
    /// The foreign completion proves len fits this exact retained output suffix.
    pub(in crate::buffer) unsafe fn new(
        target: &'a mut pool::Span<owners::Plain<L>, pool::Owned, state::Initialized>,
        len: NonZero<usize>,
    ) -> Self {
        Self { target, len }
    }

    pub fn commit(self) -> &'a mut pool::Span<owners::Plain<L>, pool::Owned, state::Initialized> {
        self.target.len += self.len.get() as u32;
        self.target
    }
}

const _: () = {
    assert!(size_of::<Complete<'_>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Complete<'_>>>() == size_of::<Complete<'_>>());
};
