use crate::queue;

pub trait Fifo<T> {
    /// # Safety
    /// The queue must have spare capacity.
    unsafe fn push_back_unchecked(&self, value: T);
}

impl<T> Fifo<T> for queue::Fifo<T> {
    unsafe fn push_back_unchecked(&self, value: T) {
        let tail = self.tail.get();
        unsafe {
            (*self.entries.get_unchecked(tail & self.mask()).get()).write(value);
        }
        self.tail.set(tail.wrapping_add(1));
    }
}
