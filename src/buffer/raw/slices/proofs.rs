use std::{mem::MaybeUninit, slice};

use crate::buffer;

/// # Safety
/// The nonnull pointer spans this owner's initialized prefix, retained in one
/// live allocation when nonempty. The prefix fits `isize`, satisfies the
/// empty-range pointer rules when empty, and stays immutable during shared access.
pub(in crate::buffer) unsafe trait Prefix {
    fn prefix(&self) -> (*mut MaybeUninit<u8>, u32);

    fn as_slice(&self) -> &[u8] {
        let (pointer, len) = self.prefix();
        // SAFETY: the backend retains this exact initialized prefix for this borrow.
        unsafe { buffer::raw::range(pointer.cast(), 0, len as usize) }
    }
}

/// # Safety
/// A nonnull, `isize`-bounded capacity span is disjoint from its length word.
/// The initialized prefix uses the same pointer and length as [`Prefix`] and fits capacity.
/// Each mutable loan retains exclusive access to its selected range.
/// Nonempty spans lie within one live allocation retained for every loan;
/// empty spans retain a valid pointer. Pure projections retain provenance,
/// owner, length and capacity; writes and growth preserve these proofs.
pub(in crate::buffer) unsafe trait Exclusive: Prefix {
    fn as_mut_ptr(&mut self) -> *mut MaybeUninit<u8>;

    fn len_mut(&mut self) -> &mut u32;

    fn capacity(&self) -> u32;

    fn as_mut_slice(&mut self) -> &mut [u8] {
        let pointer = self.as_mut_ptr();
        let len = self.len_mut();
        // SAFETY: this exclusive owner retains its exact initialized prefix.
        unsafe { buffer::raw::range_mut(pointer.cast(), 0, *len as usize) }
    }

    fn spare_parts(&mut self) -> (&mut u32, &mut [MaybeUninit<u8>]) {
        let capacity = self.capacity();
        let pointer = self.as_mut_ptr();
        let len = self.len_mut();
        let used = *len as usize;
        // SAFETY: the backend pairs this length word with its disjoint live
        // allocation, whose remaining suffix is exclusive and unpublished.
        let spare =
            unsafe { slice::from_raw_parts_mut(pointer.add(used), capacity as usize - used) };
        (len, spare)
    }

    fn fill(&mut self, byte: u8) {
        let capacity = self.capacity();
        let pointer = self.as_mut_ptr();
        let len = self.len_mut();
        // SAFETY: this exclusive backend retains every unpublished byte,
        // and writing u8 initializes the complete remaining prefix.
        unsafe {
            pointer
                .add(*len as usize)
                .cast::<u8>()
                .write_bytes(byte, (capacity - *len) as usize);
        }
        *len = capacity;
    }
}
