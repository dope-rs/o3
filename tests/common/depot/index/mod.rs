use std::{cell::Cell, collections, num::NonZero};

use o3::depot::{
    self,
    index::{Stored, bitmap},
};

pub mod capacity;
pub mod key;
pub mod width;

/// The singleton numeric domain containing only offset zero.
#[derive(Clone, Copy, Debug)]
pub struct Zero;

// SAFETY: the sole value and offset zero form an exact bijection; both
// conversions return constants without callbacks, panics, or reentry.
unsafe impl depot::raw::Compact for Zero {
    const MAX: u32 = 0;

    fn into_raw(self) -> u32 {
        0
    }

    unsafe fn from_raw(_: u32) -> Self {
        Self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Id(NonZero<u32>, u32);

impl Id {
    pub fn new(index: u32, epoch: NonZero<u32>) -> Self {
        Self(epoch, index)
    }
}

unsafe impl depot::raw::Key for Id {
    type Raw = u64;

    const VACANT: Self::Raw = 0;

    fn index(self) -> usize {
        self.1 as usize
    }

    fn into_raw(self) -> Self::Raw {
        (u64::from(self.0.get()) << 32) | u64::from(self.1)
    }

    fn from_raw(raw: Stored<Self>) -> Self {
        let raw = raw.into_raw();
        // SAFETY: Stored retains this exact domain's nonzero high-half generation.
        Self(
            unsafe { NonZero::<u32>::new_unchecked((raw >> 32) as u32) },
            raw as u32,
        )
    }
}

/// Exact owned planes for bitmap ownership and hierarchy contracts.
pub struct Bitmap<N: Copy + Into<u32> = u32, W: bitmap::Width = usize> {
    shape: bitmap::Shape<N, W>,
    words: Vec<bitmap::Word<W>>,
}

impl<N: Copy + Into<u32>, W: bitmap::Width> Bitmap<N, W> {
    pub fn try_new(
        shape: bitmap::Shape<N, W>,
        fill: bitmap::Fill,
    ) -> Result<Self, collections::TryReserveError> {
        let mut words = Vec::new();
        words.try_reserve_exact(shape.words())?;
        words.extend(shape.fill(fill));
        Ok(Self { shape, words })
    }
}

// SAFETY: initialization and every mutation preserve the immutable exact shape.
unsafe impl<N: Copy + Into<u32>, W: bitmap::Width> depot::raw::Words<W> for Bitmap<N, W> {
    type Plane = ();
    type Size = N;

    fn shape(&self) -> bitmap::Shape<N, W> {
        self.shape
    }

    fn words(&self, (): ()) -> &[bitmap::Word<W>] {
        &self.words
    }
}

// SAFETY: this owner uniquely owns its words and exposes no independent mutation handle.
unsafe impl<N: Copy + Into<u32>, W: bitmap::Width> depot::raw::Bitmap<W> for Bitmap<N, W> {}

pub struct Borrowed<'a> {
    shape: bitmap::Shape,
    cells: &'a [Cell<usize>],
}

unsafe impl depot::raw::Words for Borrowed<'_> {
    type Plane = ();
    type Size = u32;

    fn shape(&self) -> bitmap::Shape {
        self.shape
    }

    fn words(&self, (): ()) -> &[bitmap::Word] {
        bitmap::Word::from_cells(self.cells)
    }
}

impl<'a> Borrowed<'a> {
    /// # Safety
    /// The cells must retain the exact initialized hierarchy and tail bits for
    /// this shape. Only bitmap operations may mutate them.
    pub unsafe fn new(shape: bitmap::Shape, cells: &'a [Cell<usize>]) -> Self {
        Self { shape, cells }
    }
}
