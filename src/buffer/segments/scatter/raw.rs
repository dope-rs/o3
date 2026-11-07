use std::num::NonZero;

use crate::{
    buffer,
    buffer::segments::{self, Chain, Handle},
    depot::arena::linked::slots,
};

/// A movable traversal checkpoint containing no borrowed addresses.
/// Prefix removal requires rebasing before the same chain is resumed.
#[derive(Clone, Copy, Debug)]
pub struct Position {
    pub(in crate::buffer) index: slots::Index,
    pub(in crate::buffer) start: usize,
}

impl Position {
    pub const fn new() -> Self {
        Self {
            index: slots::Index::NONE,
            start: 0,
        }
    }

    /// # Safety
    /// This checkpoint is fresh, or belongs to this exact `source` whose topology
    /// and bytes changed only through completed prefix rebases. `offset < source.len()`.
    pub unsafe fn resume<'a, T: buffer::raw::Segment, H: Handle<T>>(
        &mut self,
        source: &'a Chain<T, H>,
        offset: usize,
    ) -> segments::Cursor<'a, T> {
        unsafe { segments::Cursor::resume(source, self, offset) }
    }

    /// # Safety
    /// Before removal, this checkpoint was valid for the same live chain.
    /// Since its last initialization, resume, or rebase, exactly `removed` bytes of
    /// whole leading nodes were removed; surviving node identities, forward links
    /// and bytes are unchanged.
    pub unsafe fn rebase_prefix(&mut self, removed: NonZero<usize>) {
        match self.start.checked_sub(removed.get()) {
            Some(start) => self.start = start,
            None => *self = Self::new(),
        }
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = assert!(size_of::<Position>() == 2 * size_of::<usize>());
