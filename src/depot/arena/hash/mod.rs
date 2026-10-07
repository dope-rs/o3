//! Generation-aware resident values indexed by the fixed hash directory.
//! Values stay stable while compact positions form the directory.

use std::{marker, mem};

pub(in crate::depot) use borrows::Position;
pub use plan::Plan;

use crate::{alloc, base::num::bounded, depot::map};
pub mod errors;
pub mod runtime;

pub(in crate::depot) use cores::Core;
pub(in crate::depot) use entries::{Bucket, Entry, Meta};
pub(in crate::depot) use slot::Slot;

mod borrows;
mod cores;
mod entries;
mod plan;
mod slot;

type Fields<T> = alloc::raw::Tail<
    alloc::raw::Tail<
        alloc::raw::Tail<
            <map::raw::Layout<bounded::U32<0, { u32::MAX - 1 }>> as alloc::raw::Recipe>::Fields,
            Meta,
        >,
        bounded::U32<0, { u32::MAX - 1 }>,
    >,
    mem::MaybeUninit<T>,
>;

/// A value-bound resident hash layout proof.
#[repr(transparent)]
pub struct Layout<T> {
    table: map::raw::Shape<bounded::U32<0, { u32::MAX - 1 }>>,
    value: marker::PhantomData<fn() -> T>,
}

pub(in crate::depot) struct Parts<T> {
    pub(in crate::depot) bundle: alloc::raw::Bundle<Fields<T>>,
    pub(in crate::depot) buckets: usize,
    pub(in crate::depot) shape: Layout<T>,
}

impl<T> Parts<T> {
    pub(in crate::depot) fn try_new(
        table: map::raw::Shape<bounded::U32<0, { u32::MAX - 1 }>>,
    ) -> Result<Self, alloc::Overflow> {
        let cap = table.capacity().get();
        let dir = table.layout();
        let buckets = dir.buckets();
        let bundle = alloc::raw::Recipe::try_build(Self::recipe(dir, cap))?;
        Ok(Self {
            bundle,
            buckets,
            shape: Layout {
                table,
                value: marker::PhantomData,
            },
        })
    }

    pub(in crate::depot) fn from(layout: Layout<T>) -> Self {
        let cap = layout.capacity();
        let dir = layout.table.layout();
        let buckets = dir.buckets();
        // SAFETY: the value-bound layout proves this exact extension.
        let bundle = unsafe { alloc::raw::Recipe::build(Self::recipe(dir, cap)) };
        Self {
            bundle,
            buckets,
            shape: layout,
        }
    }

    fn recipe(
        table: map::raw::Layout<bounded::U32<0, { u32::MAX - 1 }>>,
        capacity: usize,
    ) -> impl alloc::raw::Recipe<Fields = Fields<T>> {
        use alloc::raw::Recipe as _;

        alloc::raw::Recipe::tail::<Meta>(table, capacity)
            .tail::<bounded::U32<0, { u32::MAX - 1 }>>(capacity)
            .tail::<mem::MaybeUninit<T>>(capacity)
    }
}

impl<T> Layout<T> {
    pub const fn capacity(self) -> usize {
        self.table.capacity().get()
    }
}

impl<T> Copy for Layout<T> {}

impl<T> Clone for Layout<T> {
    fn clone(&self) -> Self {
        *self
    }
}

const _: () = {
    assert!(mem::size_of::<Layout<u64>>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Option<Layout<u64>>>() == mem::size_of::<Option<u32>>());
};
