use std::{mem, ptr};

use crate::depot::arena::linked::slots;

pub(crate) struct Iter<'a, T, I: crate::depot::raw::Exact> {
    store: &'a slots::Store<T>,
    source: I,
}

impl<'a, T, I: crate::depot::raw::Exact> Iter<'a, T, I> {
    pub(super) const fn new(store: &'a slots::Store<T>, source: I) -> Self {
        Self { store, source }
    }

    pub(crate) fn run(
        mut self,
        mut apply: impl FnMut(slots::Vacant<T, &'a slots::Store<T>, ()>, I::Item),
    ) -> I::Output {
        while let Some((claim, value)) = self.next() {
            apply(claim, value);
        }
        // SAFETY: the only exit from the loop is the exact source's terminal None.
        unsafe { self.finish() }
    }

    pub(crate) fn try_run<E>(
        mut self,
        mut apply: impl FnMut(slots::Vacant<T, &'a slots::Store<T>, ()>, T),
    ) -> Result<I::Output, E>
    where
        I: crate::depot::raw::Exact<Item = Result<T, E>>,
    {
        while let Some((claim, value)) = self.next() {
            apply(claim, value?);
        }
        // SAFETY: every input completed; the loop observed the terminal None.
        Ok(unsafe { self.finish() })
    }

    unsafe fn finish(self) -> I::Output {
        let this = mem::ManuallyDrop::new(self);
        unsafe { ptr::read(&this.source).finish() }
    }
    fn next(&mut self) -> Claim<'a, T, I::Item> {
        let value = self.source.next()?;
        // SAFETY: Supply binds this exact iterator to the admitted slot count.
        let inner = unsafe { slots::Vacant::claim(self.store) };
        Some((inner, value))
    }
}

impl<T, I: crate::depot::raw::Exact> Drop for Iter<'_, T, I> {
    fn drop(&mut self) {
        self.store.refund(self.source.remaining().raw());
    }
}

type Claim<'a, T, V> = Option<(slots::Vacant<T, &'a slots::Store<T>, ()>, V)>;
