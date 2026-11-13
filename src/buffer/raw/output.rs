use std::mem::MaybeUninit;

use crate::buffer::{pool, raw, resident, storage};

/// # Safety
/// Nonempty fixed capacity retains one stable allocation; its initialized prefix fits.
/// Moves and shared access preserve backing; `spare` exclusively covers its suffix.
/// `truncate` keeps `min(len, current_len)`; `complete` extends by exactly `amount`.
pub unsafe trait Target: AsRef<[u8]> {
    fn capacity(&self) -> usize;
    fn truncate(&mut self, len: usize);

    /// # Safety
    /// Writes may initialize bytes, but must never deinitialize the backing.
    unsafe fn spare(&mut self) -> &mut [MaybeUninit<u8>];

    /// # Safety
    /// Native access ended and initialized `amount` bytes of this unchanged suffix.
    unsafe fn complete(&mut self, amount: usize);
}

// SAFETY: Owned retains its exclusive heap and initialized prefix across moves.
unsafe impl Target for storage::Owned<resident::Budget<'_>> {
    fn capacity(&self) -> usize {
        self.capacity()
    }

    fn truncate(&mut self, len: usize) {
        self.truncate(len);
    }

    unsafe fn spare(&mut self) -> &mut [MaybeUninit<u8>] {
        raw::Exclusive::spare_parts(self).1
    }

    unsafe fn complete(&mut self, amount: usize) {
        // The caller proves the exact initialized extension fits the u32 capacity.
        *raw::Exclusive::len_mut(self) += amount as u32;
    }
}

// SAFETY: Span retains stable slot storage; Exclusive excludes live prefix aliases.
unsafe impl<K: raw::Backing, S: pool::state::State> Target for pool::Span<K, pool::Owned, S>
where
    Self: raw::Exclusive,
{
    fn capacity(&self) -> usize {
        self.capacity().get() as usize
    }

    fn truncate(&mut self, len: usize) {
        self.truncate(len);
    }

    unsafe fn spare(&mut self) -> &mut [MaybeUninit<u8>] {
        raw::Exclusive::spare_parts(self).1
    }

    unsafe fn complete(&mut self, amount: usize) {
        // The caller proves the exact initialized extension fits the u32 capacity.
        *raw::Exclusive::len_mut(self) += amount as u32;
    }
}
