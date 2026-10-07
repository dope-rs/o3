use std::{cell, marker};

use crate::{
    alloc::{self, array},
    depot::{index, index::bitmap},
};

pub(in crate::depot::index::set) struct Extent<Tag, Raw, const N: usize> {
    shape: bitmap::Shape,
    domain: marker::PhantomData<index::Domain<Tag, Raw>>,
}

impl<Tag, Raw, const N: usize> Extent<Tag, Raw, N> {
    pub(in crate::depot::index::set) fn try_new(
        domain: index::Domain<Tag, Raw>,
    ) -> Result<Self, alloc::Overflow> {
        const { assert!(N != 0, "a planar set requires at least one plane") };
        let shape = bitmap::Shape::<u32>::new(domain.raw());
        if N > u32::MAX as usize || (shape.bits() != 0 && shape.words() > (usize::MAX - N) / N) {
            return Err(alloc::Overflow);
        }
        Ok(Self {
            shape,
            domain: marker::PhantomData,
        })
    }

    pub(in crate::depot::index::set) const fn shape(&self) -> bitmap::Shape {
        self.shape
    }
}

impl<Tag, Raw, const N: usize> Clone for Extent<Tag, Raw, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, Raw, const N: usize> Copy for Extent<Tag, Raw, N> {}

// SAFETY: try_new validates the exact immutable packed cell count.
unsafe impl<Tag, Raw, const N: usize> alloc::raw::Extent for Extent<Tag, Raw, N> {
    fn len(&self) -> usize {
        let shape = self.shape;
        if shape.bits() == 0 {
            0
        } else {
            N + N * shape.words()
        }
    }
}

impl<Tag, Raw, const N: usize> Extent<Tag, Raw, N> {
    pub(in crate::depot::index::set) fn try_allocate(
        self,
        fill: [bitmap::Fill; N],
    ) -> Result<array::Block<cell::Cell<usize>, Self>, alloc::Error> {
        let shape = self.shape();
        let mut block = array::Block::try_new(self, ())?;
        if shape.bits() != 0 {
            for cursor in &mut block[..N] {
                cursor.write(cell::Cell::new(0));
            }
            for (plane, fill) in fill.into_iter().enumerate() {
                for (word, value) in shape.fill(fill).enumerate() {
                    // SAFETY: plane < N and shape.fill yields exactly shape.words() cells.
                    // The admitted N + N * words extent bounds this index and its arithmetic.
                    unsafe { block.get_unchecked_mut(N + plane * shape.words() + word) }
                        .write(cell::Cell::new(value.get()));
                }
            }
        }
        // SAFETY: the empty extent has no elements; otherwise the N cursors and
        // each plane's exact shape fill initialize every element once.
        Ok(unsafe { alloc::raw::AssumeInit::assume_init(block) })
    }
}
