use crate::buffer::{
    pool::{Cursor, Span, owners, state},
    storage,
};

/// # Safety
/// `start..start + len` is initialized within the consumed span's retained
/// capacity, with no conflicting live loans. The span owns the range's slot edge.
pub(in crate::buffer::pool) unsafe fn into_shared<K: crate::buffer::raw::Backing, M, S>(
    span: Span<K, M, S>,
    start: usize,
    len: usize,
) -> storage::Shared<owners::Edge<K, M>> {
    use std::ptr;

    if len == 0 {
        return storage::Shared::empty(owners::Edge::NONE);
    }
    // SAFETY: the consumed span retains this exact nonempty in-bounds range.
    let data = unsafe { span.entry().data().add(start) };
    // SAFETY: pool allocation data is non-null.
    let data = unsafe { ptr::NonNull::new_unchecked(data) };
    let edge = span.into_edge();
    // SAFETY: edge retains the exact live slot containing this initialized
    // range and is installed atomically with its extent.
    unsafe { storage::Shared::from_raw(data, len, edge) }
}

/// # Safety
/// Projection retains the exact writer and extent that admitted the append.
pub(in crate::buffer) unsafe trait Write: Sized {
    type Backing: crate::buffer::raw::Backing;
    type Root;

    /// # Safety
    /// Only the admitted append may extend the projected initialized prefix.
    unsafe fn writer(&mut self) -> &mut Span<Self::Backing, Self::Root>;
}

// SAFETY: Cursor retains its original writer across admission and publication.
unsafe impl<L, M> Write for Cursor<L, M> {
    type Backing = owners::Plain<L>;
    type Root = M;

    unsafe fn writer(&mut self) -> &mut Span<Self::Backing, Self::Root> {
        &mut self.lease
    }
}

// SAFETY: a lease projects its exact exclusive initialized-prefix writer.
unsafe impl<L, M> Write for Span<owners::Plain<L>, M, state::Uninit> {
    type Backing = owners::Plain<L>;
    type Root = M;

    unsafe fn writer(&mut self) -> &mut Span<Self::Backing, Self::Root> {
        self
    }
}
