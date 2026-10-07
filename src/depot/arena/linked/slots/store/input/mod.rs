use std::{mem, ptr};

pub(super) use iter::Iter;

use crate::depot::arena::linked::slots;

mod iter;

/// Capacity held for one unchanged, owned input operation.
pub(crate) struct Input<'a, T, S: crate::depot::raw::Input<T>> {
    store: &'a slots::Store<T>,
    source: S,
}

impl<'a, T, S: crate::depot::raw::Input<T>> Input<'a, T, S> {
    pub(crate) fn try_new(store: &'a slots::Store<T>, source: S) -> Result<Self, S::Error> {
        let count = source.count();
        if let Err(error) = store.try_reserve(count) {
            return Err(source.reject(error));
        }
        Ok(Self { store, source })
    }

    pub(crate) fn try_start(
        self,
    ) -> Started<'a, T, S, impl crate::depot::raw::Exact<Item = S::Item, Output = S::Output>>
    where
        S: crate::depot::raw::TryStart<T>,
    {
        let this = mem::ManuallyDrop::new(self);
        let count = this.source.count();
        let source = unsafe { ptr::read(&this.source) };
        let (source, context) = match source.try_start() {
            Ok(parts) => parts,
            Err(error) => {
                this.store.refund(count.raw());
                return Err(error);
            }
        };
        Ok((Iter::new(this.store, source), context))
    }

    pub(crate) fn start(
        self,
    ) -> (
        Iter<'a, T, impl crate::depot::raw::Exact<Item = S::Item, Output = S::Output>>,
        S::Context,
    )
    where
        S: crate::depot::raw::Start<T>,
    {
        let this = mem::ManuallyDrop::new(self);
        let (source, context) = unsafe { ptr::read(&this.source) }.start();
        (Iter::new(this.store, source), context)
    }
}

impl<T, S: crate::depot::raw::Input<T>> Drop for Input<'_, T, S> {
    fn drop(&mut self) {
        self.store.refund(self.source.count().raw());
    }
}

/// Started traversal retains its slot store and unchanged operation context.
type Started<'a, T, S, I> = Result<
    (Iter<'a, T, I>, <S as crate::depot::raw::Input<T>>::Context),
    <S as crate::depot::raw::Input<T>>::Error,
>;
