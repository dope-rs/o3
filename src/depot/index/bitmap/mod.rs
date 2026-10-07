//! Owner-bound hierarchical occupancy bits.

pub use error::Error;
pub use fill::Fill;
pub use shape::Shape;
mod width;
pub(in crate::depot::index::bitmap) use entry::Found;
pub(in crate::depot::index::bitmap) use init::Init;
pub(in crate::depot::index::bitmap) use level::Level;
pub use width::Width;

pub mod entry;
mod error;
mod fill;
mod word;
pub use word::Word;

mod init;
mod level;
mod shape;

use std::num::NonZero;

/// The two storage domains used by bitmap owners.
pub trait Size: Copy {
    fn bits(self) -> u32;
}

impl Size for u32 {
    fn bits(self) -> u32 {
        self
    }
}

impl Size for NonZero<u32> {
    fn bits(self) -> u32 {
        self.get()
    }
}

use std::marker;

use bitmap::entry::{Occupied, Removed, Row};

use crate::depot::index::bitmap;

/// Safe traversal of a retained, proven bitmap hierarchy.
pub trait Access<W: Width = usize>: crate::depot::raw::Words<W> {
    fn first(&mut self, plane: Self::Plane) -> Option<Row<&mut Self, Self::Plane, Occupied, W>>
    where
        Self: Sized + crate::depot::raw::Bitmap<W>,
    {
        let row = Access::find(self, plane, 0)?;
        Some(Row {
            source: row.source,
            index: row.index,
            plane,
            phase: marker::PhantomData,
        })
    }

    fn is_empty(&self, plane: Self::Plane) -> bool {
        self.words(plane)
            .last()
            .is_none_or(|word| word.value.get() == W::ZERO)
    }

    fn get(self, index: usize) -> Option<Row<Self, (), (), W>>
    where
        Self: Sized,
    {
        Row::try_new(self, index).ok()
    }

    fn find(self, plane: Self::Plane, start: usize) -> Option<Row<Self, (), (), W>>
    where
        Self: Sized,
    {
        let level = bitmap::Level::new(&self, plane);
        let index = level.find(start)?.0.position();
        Some(Row {
            source: self,
            index,
            plane: (),
            phase: marker::PhantomData,
        })
    }

    fn pop_from(
        self,
        plane: Self::Plane,
        start: usize,
    ) -> Option<Row<Self, Self::Plane, Removed, W>>
    where
        Self: Sized,
    {
        let capacity = NonZero::new(self.shape().bits())?;
        bitmap::pop_from(self, plane, start % capacity.get())
    }
}

impl<W: Width, S: crate::depot::raw::Words<W> + ?Sized> Access<W> for S {}

/// Removes from a retained or normalized start, wrapping to the first membership.
pub(in crate::depot::index) fn pop_from<W: Width, S: crate::depot::raw::Words<W>>(
    source: S,
    plane: S::Plane,
    start: usize,
) -> Option<Row<S, S::Plane, Removed, W>> {
    let level = bitmap::Level::new(&source, plane);
    let (row, current) = level.find(start).or_else(|| level.find(0))?;
    let index = row.position();
    // SAFETY: this exact leaf was just selected; no projection or callback
    // can mutate its retained word before this immediate consumption.
    unsafe { level.write::<false>(index.get() as usize - 1, current) };
    Some(Row {
        source,
        index,
        plane,
        phase: marker::PhantomData,
    })
}
