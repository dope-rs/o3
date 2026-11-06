/// Pinned mutation preserves the retirement policy's initialized invariant.
/// # Safety
/// Every mutation admitted through this policy must leave the initialized value valid for its retirement operation without moving any pinned fields.
pub unsafe trait PoolMutable<T, C>: crate::depot::raw::PoolPolicy<T, C> {}

unsafe impl<T, C> PoolMutable<T, C> for () {}
