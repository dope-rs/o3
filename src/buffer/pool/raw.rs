use crate::buffer::{
    pool::{Span, owners},
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
