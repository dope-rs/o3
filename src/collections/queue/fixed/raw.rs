use crate::collections::queue::fixed;

/// # Safety
/// `index` must be a pure, stable projection for every copied value.
pub unsafe trait Index: Copy {
    /// Returns the stable dense position shared by every copy of this value.
    fn index(self) -> u32;
}

unsafe impl Index for u32 {
    fn index(self) -> u32 {
        self
    }
}

pub trait Coalescing<T, I = u32> {
    /// # Safety
    /// `index` must be less than the queue's capacity.
    unsafe fn schedule_unchecked(&mut self, index: I, value: T);
}

pub trait Fifo<T> {
    /// # Safety
    /// The queue must have spare capacity.
    unsafe fn vacant_entry_unchecked<'queue>(&'queue mut self) -> fixed::Vacant<'queue, T>;
}

impl<T, I> Coalescing<T, I> for fixed::Coalescing<T, I>
where
    I: Index,
{
    unsafe fn schedule_unchecked(&mut self, index: I, value: T) {
        let entry = unsafe { self.entries.get_unchecked_mut(index.index() as usize) };
        if entry.is_none() {
            let ring = &mut self.pending.ring;
            unsafe { ring.push_back_unchecked(index) };
        }
        *entry = Some(value);
    }
}

impl<T> Fifo<T> for fixed::Fifo<T> {
    unsafe fn vacant_entry_unchecked<'queue>(&'queue mut self) -> fixed::Vacant<'queue, T> {
        fixed::Vacant { queue: self }
    }
}
