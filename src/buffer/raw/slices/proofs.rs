use std::{mem::MaybeUninit, slice};

use crate::{buffer, depot::arena::linked::slots};

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

/// # Safety
/// Sources stay disjoint from the retained exclusive suffix, which fits their total length.
/// Parts and publication refer to that same suffix.
pub(in crate::buffer) unsafe trait Copy: Sized {
    type Output;

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize);

    /// # Safety
    /// The exact returned suffix has received all source bytes once.
    unsafe fn publish(self) -> Self::Output;

    fn copy(mut self) -> Self::Output {
        use buffer::raw::SliceExt as _;

        {
            let (sources, target, start) = self.parts();
            // SAFETY: Copy retains the admitted exclusive suffix and disjoint sources.
            let target = unsafe { target.add(start) };
            unsafe { sources.copy_to(target) };
        }
        // SAFETY: every admitted byte was initialized in this exact suffix.
        unsafe { self.publish() }
    }
}

/// # Safety
/// The consumed owner retains this exact initialized node for `'a`.
/// Shared projection prevents mutation or release throughout the returned borrow.
pub(in crate::buffer) unsafe trait Indexed<'a>: Sized {
    type Item: 'a;

    fn parts(self) -> (&'a slots::Store<Self::Item>, slots::Index);

    fn get(self) -> &'a Self::Item {
        let (store, index) = self.parts();
        // SAFETY: the consumed native owner retains this exact initialized slot.
        unsafe { store.value(index) }
    }
}

/// # Safety
/// The consumed owner exclusively retains its selected node for `'a`.
pub(in crate::buffer) unsafe trait IndexedMut<'a>: Indexed<'a> {
    fn get_mut(self) -> &'a mut Self::Item {
        let (store, index) = self.parts();
        // SAFETY: this native owner lends its exact live node exclusively.
        unsafe { &mut *store.value_ptr(index) }
    }
}

/// # Safety
/// The owner retains this live descriptor and its initialized node partition for `'a`.
/// Its head names a live node in that partition throughout shared projection.
pub(in crate::buffer) unsafe trait Linked<'a>: Sized {
    type Item: 'a;
    type Metadata: 'a;

    fn parts(
        self,
    ) -> (
        &'a slots::Store<Self::Item>,
        &'a slots::Store<Self::Metadata>,
        slots::Index,
    );

    fn head(metadata: &Self::Metadata) -> slots::Index;

    fn metadata(self) -> &'a Self::Metadata {
        let (_, descs, index) = self.parts();
        // SAFETY: this owner retains its exact initialized descriptor.
        unsafe { descs.value(index) }
    }

    fn front(self) -> &'a Self::Item {
        let (nodes, descs, index) = self.parts();
        // SAFETY: the descriptor and its initialized head belong to this same owner.
        let desc = unsafe { descs.value(index) };
        unsafe { nodes.value(Self::head(desc)) }
    }
}

/// # Safety
/// The owner exclusively retains its descriptor and disjoint node partition for `'a`.
pub(in crate::buffer) unsafe trait LinkedMut<'a>: Linked<'a> {
    fn metadata_mut(self) -> &'a mut Self::Metadata {
        let (_, descs, index) = self.parts();
        // SAFETY: this native owner lends its live descriptor exclusively.
        unsafe { &mut *descs.value_ptr(index) }
    }

    fn front_mut(self) -> (&'a mut Self::Metadata, &'a mut Self::Item) {
        let (nodes, descs, index) = self.parts();
        // SAFETY: the original owner exclusively retains both live values in
        // distinct stores, and its descriptor selects only its own node partition.
        unsafe {
            let desc = &mut *descs.value_ptr(index);
            let item = &mut *nodes.value_ptr(Self::head(desc));
            (desc, item)
        }
    }
}

pub(in crate::buffer) struct Batch<I, M> {
    pub(in crate::buffer) owners: I,
    pub(in crate::buffer) total: M,
}

impl<I: Iterator, M> Iterator for Batch<I, M> {
    type Item = I::Item;
    fn next(&mut self) -> Option<Self::Item> {
        self.owners.next()
    }
}
