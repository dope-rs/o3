use std::cell;

use crate::{
    depot::{self, index},
    mem::{self, slot},
};

// SAFETY: the retained cell initialization lasts for the complete output loan.
unsafe impl<'a, T, M> depot::raw::Project<(), T> for &'a slot::Storage<T, M> {
    type Output = &'a T;

    unsafe fn project(self, _: ()) -> Self::Output {
        unsafe { mem::raw::project(self) }
    }
}

// SAFETY: the exclusive initialized cell owns its value and destruction.
unsafe impl<'a, T, M> depot::raw::Project<(), T> for &'a mut slot::Storage<T, M> {
    type Output = mem::raw::Occupied<'a, T>;

    unsafe fn project(self, _: ()) -> Self::Output {
        unsafe { mem::raw::occupy(self) }
    }
}

// SAFETY: the proof retains this exact vacant cell with no intervening writes.
unsafe impl<'a, T, M> depot::raw::Project<(), index::Vacant<T>> for &'a mut slot::Storage<T, M> {
    type Output = mem::raw::Vacant<'a, T>;

    unsafe fn project(self, _: ()) -> Self::Output {
        unsafe { mem::raw::vacancy(self) }
    }
}

// SAFETY: the proof retains an initialized index within this exact array.
unsafe impl<'a, T, const N: usize> depot::raw::Project<usize> for &'a [T; N] {
    type Output = &'a T;

    unsafe fn project(self, index: usize) -> Self::Output {
        unsafe { self.get_unchecked(index) }
    }
}

// SAFETY: the proof retains an exclusive initialized index in this exact array.
unsafe impl<'a, T, const N: usize> depot::raw::Project<usize> for &'a mut [T; N] {
    type Output = &'a mut T;

    unsafe fn project(self, index: usize) -> Self::Output {
        unsafe { self.get_unchecked_mut(index) }
    }
}

// SAFETY: the proof retains an exclusive initialized index in this exact slice.
unsafe impl<'a, T> depot::raw::Project<usize> for &'a mut [T] {
    type Output = &'a mut T;

    unsafe fn project(self, index: usize) -> Self::Output {
        unsafe { self.get_unchecked_mut(index) }
    }
}

// SAFETY: Key preserves each emitted identity and its exact index.
unsafe impl<K: crate::depot::raw::Key> crate::depot::raw::Address for K {
    fn index(self) -> usize {
        crate::depot::raw::Key::index(self)
    }
}

// SAFETY: `None` is absent; `Some` round-trips every item exactly.
unsafe impl<I: Copy + Eq> crate::depot::raw::Link for Option<I> {
    type Item = I;

    const NONE: Self = None;

    fn item(self) -> Option<I> {
        self
    }

    unsafe fn some(item: I) -> Self {
        Some(item)
    }
}

// SAFETY: the maximum primitive value is the reserved sentinel.
unsafe impl crate::depot::raw::Link for u16 {
    type Item = Self;

    const NONE: Self = Self::MAX;

    fn item(self) -> Option<Self> {
        (self != Self::NONE).then_some(self)
    }

    unsafe fn some(item: Self) -> Self {
        item
    }
}

// SAFETY: the maximum primitive value is the reserved sentinel.
unsafe impl crate::depot::raw::Link for u32 {
    type Item = Self;

    const NONE: Self = Self::MAX;

    fn item(self) -> Option<Self> {
        (self != Self::NONE).then_some(self)
    }

    unsafe fn some(item: Self) -> Self {
        item
    }
}

// SAFETY: u32 stores this exact chain's cardinality.
unsafe impl crate::depot::raw::ChainCount for u32 {
    const EMPTY: Self = 0;

    fn storage(count: &cell::Cell<Self>) -> Option<&cell::Cell<u32>> {
        Some(count)
    }
}

// SAFETY: unit deliberately stores no cardinality.
unsafe impl crate::depot::raw::ChainCount for () {
    const EMPTY: Self = ();

    fn storage(_: &cell::Cell<Self>) -> Option<&cell::Cell<u32>> {
        None
    }
}

mod key;
mod rounds;
mod word;

// SAFETY: every u32 is its own complete compact identity; both conversions
// return their scalar input without callbacks, panics, or reentry.
unsafe impl crate::depot::raw::Compact for u32 {
    const MAX: u32 = u32::MAX;

    fn into_raw(self) -> u32 {
        self
    }

    unsafe fn from_raw(raw: u32) -> Self {
        raw
    }
}

mod exacts;

mod tagged;
