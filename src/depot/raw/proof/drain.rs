use std::{mem, ops};

/// Detaches exactly the initialized range from its original exclusive owner.
/// # Safety
/// The returned bounded initialized range belongs exclusively to the backing; detachment clears its ownership state.
pub(in crate::depot) unsafe trait Drain<T> {
    type Slots: AsMut<[mem::MaybeUninit<T>]> + ?Sized;

    fn detach(&mut self) -> (&mut Self::Slots, ops::Range<usize>);

    fn destroy<const REVERSE: bool>(&mut self) {
        let (slots, range) = self.detach();
        // SAFETY: detachment transfers this exact initialized range before user drop glue runs.
        let entries = unsafe { slots.as_mut().get_unchecked_mut(range) };
        if REVERSE {
            for entry in entries.iter_mut().rev() {
                unsafe { entry.assume_init_drop() };
            }
        } else {
            unsafe { entries.assume_init_drop() };
        }
    }
}
