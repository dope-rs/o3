use crate::buffer::{self, pool, pool::owners, storage};

/// A slot whose writable state has been consumed.
#[derive(Debug)]
pub struct Frozen;

// SAFETY: frozen ownership retains this exact initialized slot and visible range.
unsafe impl<L> buffer::raw::Source for pool::Span<owners::Plain<L>, pool::Owned, Frozen> {}

impl<L> pool::Span<owners::Plain<L>, pool::Owned, Frozen> {
    #[must_use]
    pub fn into_shared(self) -> storage::Shared {
        pool::raw::Init::into_shared(self).into()
    }
}
