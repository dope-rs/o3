use std::{mem, ptr};

use crate::buffer;
/// # Safety
/// Shared access preserves the visible range, backing and contents until mutation.
/// Projection applies the proved range and preserves the owner's empty-storage policy.
pub unsafe trait Store: AsRef<[u8]> + Sized {
    fn advance<const MAX: u32>(prefix: buffer::raw::Consumed<'_, '_, Self, MAX>) {
        buffer::raw::Selection::from(prefix).consume();
    }

    fn advance_max(&mut self, max: usize) -> usize
    where
        Self: buffer::raw::Source,
    {
        let prefix = buffer::raw::Selection::new(self, max);
        let len = prefix.len();
        prefix.consume();
        len
    }

    /// Returns a canonical empty owner, or `None` to restrict the current owner
    /// according to its own empty-storage policy.
    /// A returned owner must expose no bytes and require no allocation.
    fn empty() -> Option<Self>;

    fn project(slice: buffer::raw::Slice<'_, '_, Self>) {
        slice.apply();
    }

    /// Applies the selected extent. Nonempty selections must retain their original
    /// backing; empty selections preserve this owner's empty-storage policy.
    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>);
}

// SAFETY: the exclusive slice retains its initialized range and original borrow.
unsafe impl Store for &mut [u8] {
    fn empty() -> Option<Self> {
        Some(&mut [])
    }

    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>) {
        let (target, range) = slice.into_parts();
        let bytes = mem::take(target);
        // SAFETY: this admitted range belongs to the unchanged exclusive slice.
        *target = unsafe { bytes.get_unchecked_mut(range.into_inner()) };
    }
}

// SAFETY: View retains its immutable source borrow and only narrows its selected bytes.
unsafe impl<S: ?Sized> Store for buffer::View<'_, S> {
    fn empty() -> Option<Self> {
        None
    }

    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>) {
        let (target, range) = slice.into_parts();
        // SAFETY: this selection retains the view and bounds within its unchanged bytes.
        target.bytes = unsafe { target.bytes.get_unchecked(range.into_inner()) };
    }
}

// SAFETY: shared Vec access preserves its bytes; projection keeps its allocation.
unsafe impl Store for Vec<u8> {
    fn empty() -> Option<Self> {
        None
    }

    #[inline]
    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>) {
        let (target, range) = slice.into_parts();
        let len = range.len();
        let bounds = range.into_inner();
        if bounds.start != 0 {
            let pointer = target.as_mut_ptr();
            // SAFETY: the retained selection proves this initialized range;
            // copying to its prefix stays in the same allocation and may overlap.
            unsafe { ptr::copy(pointer.add(bounds.start), pointer, len) };
        }
        target.truncate(len);
    }
}

mod headers;
mod loans;
mod tags;

mod alloc;
pub(in crate::buffer) use alloc::{Debit, Growth, Header, Heap, Policy, Rc, Token, Unique};
mod writing;
pub(in crate::buffer) use writing::Space;
mod pooling;
pub(in crate::buffer) use pooling::Backing;
