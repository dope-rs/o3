use crate::{
    depot,
    depot::index::{bitmap, bitmap::entry::Row},
};

/// # Safety
/// Planes stay stationary and initialized with exact shapes, zero tail bits, and exact nonzero-child summaries.
/// Mutation preserves this; projections return normally without mutation or reentry.
pub unsafe trait Words<W: bitmap::Width = usize> {
    type Plane: Copy;
    type Size: bitmap::Size;

    fn shape(&self) -> bitmap::Shape<Self::Size, W>;
    fn words(&self, plane: Self::Plane) -> &[bitmap::Word<W>];

    fn row(entry: Row<&Self, (), (), W>, plane: Self::Plane) -> &[bitmap::Word<W>]
    where
        Self: Sized,
    {
        (*entry.source()).words(plane)
    }
}

// SAFETY: the borrow retains the same source, planes, and immutable shape.
unsafe impl<W: bitmap::Width, S: Words<W>> Words<W> for &S {
    type Plane = S::Plane;
    type Size = S::Size;

    fn shape(&self) -> bitmap::Shape<Self::Size, W> {
        S::shape(self)
    }

    fn words(&self, plane: Self::Plane) -> &[bitmap::Word<W>] {
        S::words(self, plane)
    }
    fn row(entry: Row<&Self, (), (), W>, plane: Self::Plane) -> &[bitmap::Word<W>] {
        // SAFETY: dereferencing this borrow preserves its exact source and bound.
        S::row(
            unsafe { depot::raw::Rows::map_row(entry, |source| *source) },
            plane,
        )
    }
}

// SAFETY: the exclusive borrow retains the same stationary source and planes.
unsafe impl<W: bitmap::Width, S: Words<W>> Words<W> for &mut S {
    type Plane = S::Plane;
    type Size = S::Size;

    fn shape(&self) -> bitmap::Shape<Self::Size, W> {
        S::shape(self)
    }

    fn words(&self, plane: Self::Plane) -> &[bitmap::Word<W>] {
        S::words(self, plane)
    }
    fn row(entry: Row<&Self, (), (), W>, plane: Self::Plane) -> &[bitmap::Word<W>] {
        // SAFETY: this reborrow retains the original exclusive owner's domain.
        S::row(
            unsafe { depot::raw::Rows::map_row(entry, |source| &**source) },
            plane,
        )
    }
}
