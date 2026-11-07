use crate::depot::arena::pool;
/// Retires this initialized uniquely held value before recycling its slot.
/// # Safety
/// Retiring must consume the exact terminal owner once, end all value loans, and recycle only its own slot after completing the value cleanup.
pub unsafe trait PoolPolicy<T, C> {
    fn retire(entry: pool::Terminal<'_, T, C>);
}

unsafe impl<T, C> PoolPolicy<T, C> for () {
    fn retire(entry: pool::Terminal<'_, T, C>) {
        entry.recycle();
    }
}
