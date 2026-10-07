use std::{mem, ptr};

pub(in crate::depot) struct Position<'a, T> {
    pub(in crate::depot::heap::raw) slot: &'a mut mem::MaybeUninit<T>,
    pub(in crate::depot::heap::raw) index: usize,
}

impl<T> Position<'_, T> {
    pub(in crate::depot) const fn offset(&self) -> usize {
        self.index
    }

    pub(in crate::depot::heap::raw) fn write(self, value: T) {
        self.slot.write(value);
    }

    pub(in crate::depot::heap::raw) fn copy(self, value: &T) {
        unsafe { ptr::copy_nonoverlapping(value, self.slot.as_mut_ptr(), 1) };
    }
}
