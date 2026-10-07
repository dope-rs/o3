use std::cell;

use crate::depot::arena::pool;
/// Owns initialized disjoint slots; owner and slots remain pinned during loans.
/// # Safety
/// The free list belongs to this exact pinned owner and contains only its vacant, disjoint slots. Live loans must exclude recycling their slots.
pub unsafe trait PoolOwner<T>: Sized {
    unsafe fn free(&self) -> &cell::Cell<*mut pool::Entry<T, Self>>;
}
