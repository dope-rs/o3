use std::num::NonZero;

use crate::{buffer, buffer::segments::scatter};
/// Exact-progress traversal over one immutable scattered source.
///
/// # Safety
/// Progress, cap, remaining length, and `'data` must all be exact.
pub unsafe trait Scatter<'data> {
    fn remaining(&self) -> usize;

    /// Advances to an admitted byte offset in this exact cursor.
    fn seek(mut self, offset: usize) -> Option<Self>
    where
        Self: Sized,
    {
        if offset > self.remaining() {
            return None;
        }
        scatter::Take::new(&mut self, offset).finish();
        Some(self)
    }

    /// Borrows at most the requested number of remaining bytes.
    fn take_bytes(&mut self, limit: usize) -> scatter::Take<'_, 'data, Self>
    where
        Self: Sized,
    {
        scatter::Take::new(self, limit)
    }

    /// Advances from the exact non-empty state represented by `left`.
    ///
    /// # Safety
    /// `left == self.remaining()`, `cap <= left`, `cap <= MAX`; output advances `1..=cap` bytes.
    unsafe fn advance<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
        left: NonZero<usize>,
    ) -> buffer::raw::Chunk<'data, MAX>;
}

// SAFETY: the exclusive borrow delegates every observation and transition to
// the same exact cursor while preventing independent advancement of its state.
unsafe impl<'data, C: Scatter<'data> + ?Sized> Scatter<'data> for &mut C {
    fn remaining(&self) -> usize {
        C::remaining(self)
    }

    unsafe fn advance<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
        left: NonZero<usize>,
    ) -> buffer::raw::Chunk<'data, MAX> {
        // SAFETY: the borrowed cursor has the exact state specified by the caller.
        unsafe { C::advance(self, cap, left) }
    }
}

// SAFETY: the slice lifetime is the source lifetime, and advancing can only
// replace it with an exact suffix of the same allocation.
unsafe impl<'data> Scatter<'data> for &'data [u8] {
    fn remaining(&self) -> usize {
        self.len()
    }

    unsafe fn advance<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
        _left: NonZero<usize>,
    ) -> buffer::raw::Chunk<'data, MAX> {
        // SAFETY: the caller proves cap <= left == this slice's length.
        let (bytes, suffix) = unsafe { self.split_at_unchecked(cap.get()) };
        *self = suffix;
        // SAFETY: exact `left` and nonzero `cap` prove a non-empty capped
        // prefix, and `bytes` retains the source slice lifetime.
        unsafe { buffer::raw::Chunk::new(bytes) }
    }
}

mod cursors;
mod queues;
