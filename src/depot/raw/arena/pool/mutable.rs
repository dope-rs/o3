/// Pinned mutation preserves the retirement policy's initialized invariant.
/// # Safety
/// Every mutation admitted through this policy must leave the initialized value valid for its retirement operation without moving any pinned fields.
pub unsafe trait PoolMutable<T, O: crate::depot::raw::PoolOwner<T>>:
    crate::depot::raw::PoolPolicy<T, O>
{
}

unsafe impl<T, O: crate::depot::raw::PoolOwner<T>> PoolMutable<T, O> for () {}
