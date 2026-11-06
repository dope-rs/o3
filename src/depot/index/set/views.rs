use std::{cell, marker, slice};

use crate::depot::index::{bitmap, bitmap::entry::Row, set};

pub(in crate::depot) struct View<S, Tag, Raw, const N: usize> {
    pub(in crate::depot::index::set) source: S,
    tag: Domain<Tag, Raw>,
}

impl<S, Tag, Raw, const N: usize> View<S, Tag, Raw, N> {
    pub(in crate::depot) const fn new(source: S) -> Self {
        Self {
            source,
            tag: marker::PhantomData,
        }
    }
}

impl<S: Copy, Tag, Raw, const N: usize> Clone for View<S, Tag, Raw, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Copy, Tag, Raw, const N: usize> Copy for View<S, Tag, Raw, N> {}

/// Retains one nonempty packed set, including its cursor header and all planes.
pub(in crate::depot::index::set) struct NonEmpty<'a, Tag, Raw, const N: usize, M> {
    state: &'a set::State<Tag, Raw, N, M>,
}

impl<'a, Tag, Raw, const N: usize, M> NonEmpty<'a, Tag, Raw, N, M> {
    pub(in crate::depot::index::set) fn new(state: &'a set::State<Tag, Raw, N, M>) -> Option<Self> {
        if state.block.extent().shape().bits() == 0 {
            return None;
        }
        Some(Self { state })
    }

    /// Advances the original source plane from its retained bounded row.
    pub(in crate::depot::index::set) fn advance<S>(row: &'a Row<View<S, Tag, Raw, N>, set::Id<N>>)
    where
        S: crate::depot::raw::SetSource<Tag, Raw, N, Mode = M>,
    {
        let state = row.source().source.planes();
        let next = row.index() + 1;
        Self { state }
            .cursor(row.plane())
            .set(if next == state.block.extent().shape().bits() {
                0
            } else {
                next
            });
    }

    /// The retained nonempty allocation includes all N initialized cursors.
    pub(in crate::depot::index::set) fn cursor(&self, plane: set::Id<N>) -> &'a cell::Cell<usize> {
        // SAFETY: Extent::try_allocate initializes N cursor cells before the
        // planes whenever bits != 0. NonEmpty retains that exact allocation,
        // and Id can only be constructed for an offset strictly below N.
        unsafe { &*self.state.block.as_ptr().add(plane.offset()) }
    }

    fn words(&self, plane: set::Id<N>) -> &'a [bitmap::Word] {
        let shape = self.state.block.extent().shape();
        // SAFETY: this exact nonempty packed allocation contains every bounded plane.
        unsafe {
            slice::from_raw_parts(
                self.state
                    .block
                    .as_ptr()
                    .add(N + plane.offset() * shape.words())
                    .cast(),
                shape.words(),
            )
        }
    }
}

// SAFETY: the retained source owns every initialized plane in one immutable domain.
unsafe impl<S, Tag, Raw, const N: usize> crate::depot::raw::Words for View<S, Tag, Raw, N>
where
    S: crate::depot::raw::SetSource<Tag, Raw, N>,
{
    type Plane = set::Id<N>;
    type Size = u32;

    fn shape(&self) -> bitmap::Shape {
        self.source.planes().block.extent().shape()
    }

    fn words(&self, plane: Self::Plane) -> &[bitmap::Word] {
        let Some(view) = NonEmpty::new(self.source.planes()) else {
            return &[];
        };
        view.words(plane)
    }
    fn row(entry: Row<&Self>, plane: Self::Plane) -> &[bitmap::Word] {
        NonEmpty {
            state: entry.source().source.planes(),
        }
        .words(plane)
    }
}

type Domain<Tag, Raw> = marker::PhantomData<fn(Tag, Raw) -> (Tag, Raw)>;
