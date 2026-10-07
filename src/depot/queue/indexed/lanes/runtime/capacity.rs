use std::{marker, mem};

use crate::{
    alloc,
    depot::queue::{indexed::lanes, tagged},
};

#[repr(transparent)]
pub(in crate::depot) struct Capacity<
    T,
    C: crate::depot::raw::Count,
    L: crate::depot::raw::Count,
    H: tagged::raw::Head,
> {
    shape: tagged::Domain<[H], C, L>,
    ownership: marker::PhantomData<lanes::Value<T>>,
}

impl<T, C: crate::depot::raw::Count, L: crate::depot::raw::Count, H: tagged::raw::Head>
    Capacity<T, C, L, H>
{
    pub(in crate::depot::queue::indexed::lanes::runtime) fn try_new(
        shape: tagged::Domain<[H], C, L>,
    ) -> Result<Self, alloc::Overflow> {
        const {
            assert!(mem::align_of::<lanes::Value<T>>() >= mem::align_of::<H>());
        }
        alloc::raw::Bundle::try_layout::<Self>(
            shape.capacity().count().get(),
            shape.lanes().count().get(),
        )?;
        Ok(Self {
            shape,
            ownership: marker::PhantomData,
        })
    }

    pub(in crate::depot) const fn shape(&self) -> tagged::Domain<[H], C, L> {
        self.shape
    }
}

// SAFETY: construction proves both disjoint tagged domains and the complete
// typed allocation, including final padding. The immutable shape is retained.
unsafe impl<T, C: crate::depot::raw::Count, L: crate::depot::raw::Count, H: tagged::raw::Head>
    alloc::raw::Shape for Capacity<T, C, L, H>
{
    type Root = lanes::Value<T>;
    type Tail = H;
    type Extra = ();
    type Last = ();

    fn len(&self) -> usize {
        self.shape.capacity().count().get()
    }

    fn tail_len(&self) -> usize {
        self.shape.lanes().count().get()
    }
}
