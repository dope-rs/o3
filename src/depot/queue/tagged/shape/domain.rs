use std::{marker, mem, num::NonZero};

use crate::{
    alloc,
    depot::{
        capacity,
        queue::{links, tagged::shape::Error},
    },
};

/// Immutable, disjoint compact node and endpoint domains.
#[repr(C)]
pub struct Domain<
    H: ?Sized = [links::Chain],
    C: crate::depot::raw::Count = capacity::Count,
    L: crate::depot::raw::Count = capacity::Count,
> {
    capacity: C,
    lanes: L,
    heads: marker::PhantomData<fn(&H) -> &H>,
}

impl<H: ?Sized, C: crate::depot::raw::Count, L: crate::depot::raw::Count> Copy for Domain<H, C, L> {}

impl<H: ?Sized, C: crate::depot::raw::Count, L: crate::depot::raw::Count> Clone
    for Domain<H, C, L>
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<H: ?Sized, C: crate::depot::raw::Count, L: crate::depot::raw::Count> Domain<H, C, L> {
    pub(in crate::depot::queue) fn build(capacity: C, lanes: L) -> Result<Self, Error<C, L>> {
        if u64::from(capacity.count().raw()) + u64::from(lanes.count().raw())
            > u64::from(u32::MAX) + 1
        {
            return Err(Error::Overlap { capacity, lanes });
        }
        Ok(Self {
            capacity,
            lanes,
            heads: marker::PhantomData,
        })
    }

    pub const fn capacity(self) -> C {
        self.capacity
    }

    pub(in crate::depot) const fn lanes(self) -> L {
        self.lanes
    }
}

impl<H, C: crate::depot::raw::Count, L: crate::depot::raw::Count> Domain<[H], C, L> {
    pub const fn bounded(capacity: C, lanes: L) -> Self {
        const { assert!(C::MAX as u64 + L::MAX as u64 <= u32::MAX as u64 + 1) };
        Self {
            capacity,
            lanes,
            heads: marker::PhantomData,
        }
    }
}

impl<H, const N: usize, C: crate::depot::raw::Count> Domain<[H; N], C> {
    pub(in crate::depot) fn new(capacity: C) -> Self {
        const {
            assert!(N <= u32::MAX as usize);
            assert!(C::MAX as u64 + N as u64 <= u32::MAX as u64 + 1);
        }
        Self {
            capacity,
            lanes: capacity::Count::new(N as u32),
            heads: marker::PhantomData,
        }
    }
}

impl<H, const N: usize> Domain<[H; N]> {
    pub fn try_from(capacity: capacity::Count) -> Result<Self, Error> {
        let lanes = capacity::Count::try_from(N).map_err(|error| Error::Lanes {
            capacity,
            lanes: error.requested(),
        })?;
        Self::build(capacity, lanes)
    }
}

// SAFETY: Domain retains the immutable count consumed by its allocation.
unsafe impl<H: ?Sized, C: crate::depot::raw::Count, L: crate::depot::raw::Count> alloc::raw::Extent
    for Domain<H, C, L>
{
    fn len(&self) -> usize {
        self.capacity.count().get()
    }
}

const _: () = {
    assert!(mem::size_of::<Domain>() == 2 * mem::size_of::<u32>());
    assert!(mem::size_of::<Domain<[links::Chain; 2]>>() == mem::size_of::<Domain>());
    assert!(
        mem::size_of::<Domain<[links::Chain], NonZero<u32>, NonZero<u32>>>()
            == mem::size_of::<Domain>()
    );
    assert!(
        mem::size_of::<Option<Domain<[links::Chain], NonZero<u32>, NonZero<u32>>>>()
            == mem::size_of::<Domain>()
    );
};
