use std::{cell, marker};

/// Shared surplus for one structural accounting domain.
pub struct Pool<const N: usize = 1, Tag = (), W: crate::mem::raw::Word = usize> {
    pub(in crate::mem::raw) shared: cell::Cell<[W; N]>,
    pub(in crate::mem::raw) domain: crate::mem::raw::Domain<Tag>,
    pub(in crate::mem::raw) limit: [W; N],
}

impl<const N: usize, Tag, W: crate::mem::raw::Word> Pool<N, Tag, W> {
    /// Creates a shared account before its exact lane allocation is bound.
    pub const fn new(shared: [W; N]) -> Self {
        const { assert!(N != 0) };
        Self {
            shared: cell::Cell::new(shared),
            limit: shared,
            domain: marker::PhantomData,
        }
    }

    /// # Safety
    /// Keep both addresses and the exclusive pairing for all usable derived access; forgotten grants stay charged in that epoch.
    /// Restart only under exclusive access to this pool and all bound states, rebuilding both; reserve plus initial surplus must fit `W` on every axis.
    pub unsafe fn lane<'p, 's>(
        &'p self,
        state: &'s crate::mem::raw::State<N, Tag, W>,
    ) -> crate::mem::raw::Lane<'p, 's, N, Tag, W> {
        crate::mem::raw::Lane { pool: self, state }
    }

    pub fn shared(&self) -> [W; N] {
        self.shared.get()
    }
}
