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
        for value in &mut self.source {
            // SAFETY: this exact input retains one reservation per emitted item.
            let claim = unsafe { slots::Vacant::claim(self.store) };
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
        for value in &mut self.source {
            // The failed item is already excluded from the remaining count.
            let value = value.inspect_err(|_| self.store.refund(1))?;
            // SAFETY: this successful item retains its pre-reserved slot.
            let claim = unsafe { slots::Vacant::claim(self.store) };
            apply(claim, value);
        }
        // SAFETY: every input completed; the loop observed the terminal None.
        Ok(unsafe { self.finish() })
    }

    unsafe fn finish(self) -> I::Output {
        let this = mem::ManuallyDrop::new(self);
        unsafe { ptr::read(&this.source).finish() }
    }
}

impl<T, I: crate::depot::raw::Exact> Drop for Iter<'_, T, I> {
    fn drop(&mut self) {
        self.store.refund(self.source.remaining().raw());
    }
}
