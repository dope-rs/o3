pub trait Fifo<T> {
    /// # Safety
    /// The queue must have spare capacity.
    unsafe fn push_back_unchecked(&self, value: T);
}
