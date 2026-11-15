use crate::buffer::{self, pool, pool::owners, storage};

/// A slot whose writable state has been consumed.
#[derive(Debug)]
pub struct Frozen;

// SAFETY: frozen ownership retains this exact initialized slot and visible range.
unsafe impl<L> buffer::raw::Source for pool::Span<owners::Plain<L>, pool::Owned, Frozen> where
    owners::Plain<L>: crate::buffer::raw::Backing
{
}

impl<L> pool::Span<owners::Plain<L>, pool::Owned, Frozen>
where
    owners::Plain<L>: crate::buffer::raw::Backing,
{
    #[must_use]
    pub fn into_shared(self) -> storage::Shared {
        let len = self.len();
        // SAFETY: consuming the frozen span retains its exact initialized prefix.
        unsafe { pool::raw::into_shared(self, 0, len) }.into()
    }
}

impl<'d, S: pool::state::State> pool::Span<owners::Charged<'d>, pool::Owned, S> {
    #[must_use]
    #[inline]
    pub fn freeze(self) -> buffer::resident::Shared<'d> {
        let len = self.len();
        // SAFETY: consuming the exclusive span retains its initialized prefix.
        unsafe { pool::raw::into_shared(self, 0, len) }.into()
    }
}
