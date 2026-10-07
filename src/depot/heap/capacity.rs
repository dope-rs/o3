use std::marker;

use crate::{
    alloc,
    depot::{capacity, heap, index::Position},
};

#[repr(transparent)]
pub(crate) struct Capacity<E, I, R, C = capacity::Count> {
    count: C,
    shape: Domain<E, I, R>,
}

impl<E, I, R, C: Copy> Clone for Capacity<E, I, R, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E, I, R, C: Copy> Copy for Capacity<E, I, R, C> {}

impl<E, I, R, C: alloc::raw::Extent> Capacity<E, I, R, C> {
    pub(in crate::depot) const fn count(self) -> C {
        self.count
    }

    fn overflow(capacity: C, plane: heap::Plane, error: alloc::Overflow) -> heap::Error<C> {
        use std::alloc::Layout;

        heap::Error::Layout {
            capacity,
            plane,
            entries: Layout::new::<E>(),
            positions: Layout::new::<I>(),
            residents: Layout::new::<R>(),
            error,
        }
    }
}

impl<E, I: Position, R, C: alloc::raw::Extent> Capacity<E, I, R, C> {
    pub(in crate::depot) fn try_new(count: C) -> Result<Self, heap::Error<C>> {
        use alloc::raw::Rows;

        let maximum = <I as crate::depot::raw::Word>::MAX;
        if count.len() > maximum.get() {
            return Err(heap::Error::Capacity {
                requested: count,
                maximum,
            });
        }
        if count.len() > Self::ROOT_MAX {
            return Err(Self::overflow(count, heap::Plane::Entries, alloc::Overflow));
        }
        if count.len() > Self::MAX {
            return Err(Self::overflow(
                count,
                heap::Plane::Positions,
                alloc::Overflow,
            ));
        }
        if count.len() > Self::EXTRA_MAX {
            return Err(Self::overflow(
                count,
                heap::Plane::Residents,
                alloc::Overflow,
            ));
        }
        Ok(Self {
            count,
            shape: marker::PhantomData,
        })
    }

    pub(in crate::depot) fn allocate(self) -> Result<alloc::raw::Block<Self, u32>, heap::Error<C>> {
        alloc::raw::Block::try_new(self, 0, I::NONE).map_err(move |error| heap::Error::Allocation {
            capacity: self.count,
            layout: alloc::raw::Shape::layout(&self),
            error,
        })
    }
}

unsafe impl<E, I: Copy, R, C: alloc::raw::Extent> alloc::raw::Shape for Capacity<E, I, R, C> {
    type Root = E;
    type Tail = I;
    type Extra = R;
    type Last = ();

    fn len(&self) -> usize {
        self.count.len()
    }
}

unsafe impl<E, I: Copy, R, C: alloc::raw::Extent> alloc::raw::Rows for Capacity<E, I, R, C> {}

type Domain<E, I, R> = marker::PhantomData<fn() -> (E, I, R)>;
