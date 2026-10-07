use std::mem;

use crate::depot::queue::ring;

/// # Safety
/// Thread-confined projections/publication return normally without reentry; shared mutation lends no residents.
pub unsafe trait RingCell: crate::depot::raw::RingStore {
    /// # Safety
    /// The state describes this exact backing's initialized range.
    unsafe fn publish(&self, state: ring::State<Self::Word>);

    fn take_front(&self) -> Option<Self::Item> {
        let mut state = self.state();
        if state.is_empty() {
            return None;
        }
        let index = unsafe { state.pop_front(self.capacity()) };
        let value = unsafe { self.entries().add(index).read().assume_init() };
        unsafe { self.publish(state) };
        Some(value)
    }

    /// # Safety
    /// This operation owns one spare slot with no running in-place destructor
    /// and cannot be reentered before publication.
    unsafe fn insert<const FRONT: bool>(&self, value: Self::Item) {
        let mut state = self.state();
        let index = unsafe { state.push::<FRONT>(self.capacity()) };
        unsafe {
            self.entries()
                .cast_mut()
                .add(index)
                .write(mem::MaybeUninit::new(value))
        };
        unsafe { self.publish(state) };
    }
}

mod rings;
pub(crate) use rings::{RingDestroy, RingRead, drop_ring};
