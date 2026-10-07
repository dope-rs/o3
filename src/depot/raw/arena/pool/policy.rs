use crate::depot::arena::pool;
/// Retires this initialized uniquely held value before recycling its slot.
/// # Safety
/// Retiring must consume the exact terminal owner once, end all value loans, and recycle only its own slot after completing the value cleanup.
pub unsafe trait PoolPolicy<T, O: crate::depot::raw::PoolOwner<T>> {
    fn retire(entry: pool::Terminal<'_, T, O>);
}

unsafe impl<T, O: crate::depot::raw::PoolOwner<T>> PoolPolicy<T, O> for () {
    fn retire(entry: pool::Terminal<'_, T, O>) {
        entry.recycle();
    }
}
