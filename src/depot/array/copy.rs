//! Fixed-capacity arrays specialized for [`Copy`] elements.

use std::{mem, ops, ptr, slice};

use crate::depot::array::{self, Len, compact, prefix::Prefix};

/// An inline vector without drop glue.
#[repr(transparent)]
#[derive(Copy, Debug)]
pub struct Vec<T, const N: usize> {
    pub(in crate::depot::array) raw: Prefix<T, N>,
}

impl<T: Copy, const N: usize> Clone for Vec<T, N> {
    fn clone(&self) -> Self {
        *self
    }
}

/// A consuming iterator without drop glue.
#[derive(Debug)]
#[repr(C)]
pub struct IntoIter<T, const N: usize> {
    pub(in crate::depot::array) entries: [mem::MaybeUninit<T>; N],
    pub(in crate::depot::array) front: Len<N>,
    pub(in crate::depot::array) back: Len<N>,
}

impl<T: Copy, const N: usize> Default for Vec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy, const N: usize> Vec<T, N> {
    /// Copies only the initialized source prefix, retaining the destination allocation.
    pub fn copy_from(&mut self, source: &Self) {
        // SAFETY: unique destination and shared source are disjoint; the source length bounds its initialized prefix.
        unsafe {
            ptr::copy_nonoverlapping(
                source.raw.entries.as_ptr(),
                self.raw.entries.as_mut_ptr(),
                source.raw.len.get(),
            );
        }
        self.raw.len = source.raw.len;
    }

    pub const fn new() -> Self {
        Self {
            raw: Prefix::empty(),
        }
    }

    pub fn from_one(value: T) -> Self {
        Self {
            raw: Prefix::single(value),
        }
    }

    /// Maps one bounded source into this destination while retaining its borrow.
    pub fn map_from<'a, U: Copy>(
        &mut self,
        source: &'a Vec<U, N>,
        make: impl FnMut(array::Vacancy<N>, &'a U) -> T,
    ) {
        self.raw.map_from(&source.raw, make);
    }

    /// Returns the initialized-prefix length with its fixed capacity attached.
    pub const fn extent(&self) -> Len<N> {
        self.raw.len
    }

    pub fn filter_from(&mut self, source: &Self, mut keep: impl FnMut(&T) -> bool) {
        compact::Compact::from_source(&mut self.raw, &source.raw).filter(&mut keep);
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        compact::Compact::retain(&mut self.raw).filter(&mut keep);
    }

    pub fn vacancy(&mut self) -> Option<array::Vacant<'_, T, N>> {
        self.raw.vacancy()
    }

    pub fn pop(&mut self) -> Option<T> {
        self.raw.pop()
    }

    pub fn as_slice(&self) -> &[T] {
        self.raw.as_slice()
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.raw.as_mut_slice()
    }
}

impl<T: Copy, const N: usize> From<[T; N]> for Vec<T, N> {
    fn from(values: [T; N]) -> Self {
        Self {
            raw: Prefix::from_array(values),
        }
    }
}

impl<T: Copy, const N: usize> AsRef<[T]> for Vec<T, N> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: Copy, const N: usize> ops::Deref for Vec<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T: Copy, const N: usize> ops::DerefMut for Vec<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<'a, T: Copy, const N: usize> IntoIterator for &'a Vec<T, N> {
    type IntoIter = slice::Iter<'a, T>;
    type Item = &'a T;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<T: Copy, const N: usize> IntoIterator for Vec<T, N> {
    type IntoIter = IntoIter<T, N>;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter::new(self.raw)
    }
}

impl<T, const N: usize> IntoIter<T, N> {
    pub(super) fn new(prefix: Prefix<T, N>) -> Self {
        Self {
            entries: prefix.entries,
            front: Len::ZERO,
            back: prefix.len,
        }
    }
}

impl<T, const N: usize> Iterator for IntoIter<T, N> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if self.front == self.back {
            return None;
        }
        let index = self.front.get();
        self.front = Len(self.front.0.wrapping_add(1));
        Some(unsafe { self.entries.get_unchecked(index).assume_init_read() })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.back.get() - self.front.get();
        (len, Some(len))
    }
}

impl<T, const N: usize> ExactSizeIterator for IntoIter<T, N> {}

// SAFETY: the iterator owns exactly its unread suffix; detachment exhausts it before destruction.
unsafe impl<T, const N: usize> crate::depot::raw::Drain<T> for IntoIter<T, N> {
    type Slots = [mem::MaybeUninit<T>; N];

    fn detach(&mut self) -> (&mut Self::Slots, ops::Range<usize>) {
        let start = self.front;
        let end = self.back;
        self.front = self.back;
        (&mut self.entries, start.get()..end.get())
    }
}
