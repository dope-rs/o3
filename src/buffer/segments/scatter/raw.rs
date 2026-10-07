use crate::{
    buffer,
    buffer::segments::{self, Chain, Handle},
    depot::arena::linked::slots,
};

/// A movable traversal checkpoint containing no borrowed addresses.
/// Only an unchanged owner may resume a previously used checkpoint.
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
    /// Since initialization, this checkpoint has only traversed `source`'s
    /// unchanged topology and bytes. `offset` is strictly below its length.
    pub unsafe fn resume<'a, T: buffer::raw::Segment, H: Handle<T>>(
        &mut self,
        source: &'a Chain<T, H>,
        offset: usize,
    ) -> segments::Cursor<'a, T> {
        unsafe { segments::Cursor::resume(source, self, offset) }
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = assert!(size_of::<Position>() == 2 * size_of::<usize>());
