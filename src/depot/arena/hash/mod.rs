//! Generation-aware resident values indexed by the fixed hash directory.
//! Values stay stable while compact positions form the directory.

use std::{marker, mem};

pub use plan::Plan;

use crate::{alloc, base::num::bounded, depot::map};
pub mod errors;
pub mod runtime;

pub(in crate::depot) use cores::Core;
pub(in crate::depot) use entries::{Bucket, Meta};
pub use slot::Vacant;

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

impl<T> Layout<T> {
    pub const fn capacity(self) -> usize {
        self.table.capacity().get()
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
