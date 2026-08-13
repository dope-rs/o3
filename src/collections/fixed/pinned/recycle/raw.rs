use std::ptr;

use crate::collections::fixed::pinned::recycle;

/// # Safety
/// The pool must stay live and thread-exclusive for every branded handle.
pub unsafe trait PoolOwner<'owner, T: recycle::Recycle> {
    fn pool(self) -> ptr::NonNull<recycle::Pool<T>>;
}
