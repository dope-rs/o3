use std::{array, cell};

use o3::{
    base::num::bounded,
    depot::{
        self, capacity,
        queue::{indexed::planes, links, round::values},
    },
};

pub mod indexed;
pub mod keyed;

#[derive(Clone, Copy)]
pub struct Reentry(pub *const values::Values<Self, 1, 1>);

impl PartialEq for Reentry {
    fn eq(&self, _: &Self) -> bool {
        // SAFETY: the test keeps this owner alive and unmoved through every comparison.
        let owner = unsafe { &*self.0 };
        owner.try_slot(0).is_ok_and(|slot| slot.take().is_some())
    }
}

pub struct RingHooks<const N: usize>(pub [links::Hook; N]);

unsafe impl<const N: usize> depot::raw::Hooks for RingHooks<N> {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        // SAFETY: the test store contains every admitted index.
        unsafe { self.0.get_unchecked(index.get()) }
    }
}

pub struct Sequence(bounded::Usize<{ usize::MAX / <links::Index as depot::raw::Word>::MAX.get() }>);

impl Sequence {
    pub fn try_new(factor: usize) -> Result<Self, bounded::Error> {
        Ok(Self(bounded::Usize::ZERO.try_add(factor)?))
    }
}

impl planes::Seed<usize> for Sequence {
    fn seed(&self, index: links::Index) -> usize {
        index.get() * self.0.get()
    }
}

#[derive(Debug)]
pub struct Source<'a, T, const N: usize> {
    pub items: [T; N],
    started: &'a cell::Cell<Option<usize>>,
}

#[derive(Debug)]
pub struct Error<'a, T, const N: usize> {
    pub input: Source<'a, T, N>,
    pub cause: capacity::Shortage,
}

pub struct Iter<T, const N: usize>(array::IntoIter<T, N>);

impl<'a, T, const N: usize> Source<'a, T, N> {
    pub fn try_new(
        items: [T; N],
        started: &'a cell::Cell<Option<usize>>,
    ) -> Result<Self, capacity::Error> {
        capacity::Count::try_from(N)?;
        Ok(Self { items, started })
    }
}

unsafe impl<'a, T, const N: usize> depot::raw::Input<T> for Source<'a, T, N> {
    type Item = T;
    type Context = ();
    type Output = array::IntoIter<T, N>;
    type Error = Error<'a, T, N>;

    fn count(&self) -> capacity::Count {
        capacity::Count::new(N as u32)
    }

    fn reject(self, cause: capacity::Shortage) -> Self::Error {
        Error { input: self, cause }
    }
}

unsafe impl<T, const N: usize> depot::raw::Start<T> for Source<'_, T, N> {
    fn start(self) -> (impl depot::raw::Exact<Item = T, Output = Self::Output>, ()) {
        self.started
            .set(self.started.get().and_then(|count| count.checked_add(1)));
        (Iter(self.items.into_iter()), ())
    }
}

impl<T, const N: usize> Iterator for Iter<T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

unsafe impl<T, const N: usize> depot::raw::Exact for Iter<T, N> {
    type Output = array::IntoIter<T, N>;

    fn remaining(&self) -> capacity::Count {
        capacity::Count::new(self.0.len() as u32)
    }

    unsafe fn finish(self) -> Self::Output {
        self.0
    }
}
