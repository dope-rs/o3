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
pub struct Id(NonZero<u64>);

impl Id {
    pub fn new(index: u32, epoch: NonZero<u32>) -> Self {
        let raw = u64::from(epoch.get()) << 32 | u64::from(index);
        // SAFETY: a nonzero generation occupies the high half of `raw`.
        Self(unsafe { NonZero::<u64>::new_unchecked(raw) })
    }
}

unsafe impl depot::raw::Key for Id {
    type Raw = u64;
    type Live = Self;

    const VACANT: Self::Raw = 0;

    fn index(self) -> usize {
        self.0.get() as u32 as usize
    }

    fn into_raw(self) -> Self::Raw {
        self.0.get()
    }

    fn from_raw(raw: Stored<Self>) -> Self {
        let raw = raw.into_raw();
        // SAFETY: Stored retains a live Id representation with a nonzero generation.
        Self(unsafe { NonZero::<u64>::new_unchecked(raw) })
    }

    fn decode(raw: Self::Raw) -> Option<Self> {
        NonZero::<u64>::new(raw).map(Self)
    }

    fn into_live(self) -> Self::Live {
        self
    }

    fn from_live(live: Self::Live) -> Self {
        live
    }
}

/// Exact owned planes for bitmap ownership and hierarchy contracts.
pub struct Bitmap<N: bitmap::Size = u32, W: bitmap::Width = usize> {
    shape: bitmap::Shape<N, W>,
    words: Vec<bitmap::Word<W>>,
}

impl<N: bitmap::Size, W: bitmap::Width> Bitmap<N, W> {
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
unsafe impl<N: bitmap::Size, W: bitmap::Width> depot::raw::Words<W> for Bitmap<N, W> {
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
unsafe impl<N: bitmap::Size, W: bitmap::Width> depot::raw::Bitmap<W> for Bitmap<N, W> {}

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
