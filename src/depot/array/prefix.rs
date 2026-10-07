use std::{mem, num, ops};

use crate::depot::array::{self, Len, Vacancy};

/// Backing storage and exact initialized-prefix ownership shared by array policies.
#[repr(C)]
#[derive(Copy, Debug)]
pub(super) struct Prefix<T, const N: usize> {
    pub(super) len: Len<N>,
    pub(super) entries: [mem::MaybeUninit<T>; N],
}

impl<T, const N: usize> Prefix<T, N> {
    pub(in crate::depot::array) const fn empty() -> Self {
        Self {
            entries: [const { mem::MaybeUninit::uninit() }; N],
            len: Len::ZERO,
        }
    }

    pub(in crate::depot::array) fn single(value: T) -> Self {
        let len = Len::ONE;
        let mut entries = [const { mem::MaybeUninit::uninit() }; N];
        // SAFETY: Len::ONE proves N is nonzero at monomorphization.
        entries[0].write(value);
        Self { entries, len }
    }

    pub(in crate::depot::array) fn fill(len: Len<N>, mut f: impl FnMut(Vacancy<N>) -> T) -> Self {
        let mut entries = [const { mem::MaybeUninit::uninit() }; N];
        // SAFETY: Len bounds this complete initialization range by N.
        let range = len.prefix_mut(&mut entries);
        for (index, entry) in range.iter_mut().enumerate() {
            // SAFETY: this position is below the bounded nonempty prefix end.
            let next = unsafe { num::NonZero::new_unchecked(index as u32 + 1) };
            entry.write(f(Vacancy(next)));
        }
        Self { entries, len }
    }

    pub(in crate::depot::array) fn map_from<'a, U: Copy>(
        &mut self,
        source: &'a Prefix<U, N>,
        mut make: impl FnMut(Vacancy<N>, &'a U) -> T,
    ) {
        self.drop_all();
        let entries = source.len.prefix_mut(&mut self.entries);
        for ((index, value), entry) in source.as_slice().iter().enumerate().zip(entries) {
            // SAFETY: this position is below the bounded nonempty prefix end.
            let next = unsafe { num::NonZero::new_unchecked(index as u32 + 1) };
            entry.write(make(Vacancy(next), value));
        }
        self.len = source.len;
    }

    pub(super) fn vacancy(&mut self) -> Option<array::Vacant<'_, T, N>> {
        let slot = self.len.vacancy()?;
        Some(array::Vacant { raw: self, slot })
    }

    pub(super) fn pop(&mut self) -> Option<T> {
        let last = num::NonZero::new(self.len.0)?;
        let len = Len(last.get() - 1);
        self.len = len;
        Some(unsafe { self.entries.as_ptr().add(len.get()).read().assume_init() })
    }

    pub(in crate::depot::array) fn drop_all(&mut self) {
        crate::depot::raw::Drain::destroy::<false>(self);
    }

    pub(super) fn as_slice(&self) -> &[T] {
        unsafe { self.len.prefix(&self.entries).assume_init_ref() }
    }

    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { self.len.prefix_mut(&mut self.entries).assume_init_mut() }
    }
}

impl<T, const N: usize> Prefix<T, N> {
    pub(in crate::depot::array) fn from_array(values: [T; N]) -> Self {
        Self {
            entries: values.map(mem::MaybeUninit::new),
            len: Len::MAX,
        }
    }
}

impl<T: Copy, const N: usize> Clone for Prefix<T, N> {
    fn clone(&self) -> Self {
        *self
    }
}

// SAFETY: Prefix exclusively owns exactly 0..len, and detaches it before any destruction.
unsafe impl<T, const N: usize> crate::depot::raw::Drain<T> for Prefix<T, N> {
    type Slots = [mem::MaybeUninit<T>; N];

    fn detach(&mut self) -> (&mut Self::Slots, ops::Range<usize>) {
        let end = self.len;
        self.len = Len::ZERO;
        (&mut self.entries, 0..end.get())
    }
}
