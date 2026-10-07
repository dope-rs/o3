use std::{marker, num::NonZero};

use crate::depot::{
    capacity::Physical,
    index::bitmap::{self, entry},
};

/// One checked position retaining its original source owner.
pub struct Row<S, P = (), M = (), W = usize> {
    pub(in crate::depot::index::bitmap) phase: Phase<M, W>,
    pub(in crate::depot::index::bitmap) plane: P,
    pub(in crate::depot) source: S,
    pub(in crate::depot::index::bitmap) index: NonZero<u32>,
}

impl<S: Copy, W> Clone for Row<S, (), (), W> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Copy, W> Copy for Row<S, (), (), W> {}

impl<W: bitmap::Width, S: crate::depot::raw::Words<W>> Row<S, (), (), W> {
    /// Projects an admitted one-based row position in its exact source domain.
    /// This retains a physical bound, without claiming current membership.
    pub fn from_source(source: crate::depot::raw::Proof<(S, NonZero<u32>), Physical>) -> Self {
        let (source, index) = source.into_inner();
        Self {
            source,
            index,
            plane: (),
            phase: marker::PhantomData,
        }
    }

    pub fn try_new(source: S, index: usize) -> Result<Self, bitmap::Error> {
        let capacity = source.shape().bits();
        if index >= capacity {
            return Err(bitmap::Error { index, capacity });
        }
        // SAFETY: admission bounds this index in the exact retained source.
        Ok(unsafe { Self::new(source, index) })
    }

    /// # Safety
    /// The index belongs to this exact retained source's immutable domain.
    pub(in crate::depot) unsafe fn new(source: S, index: usize) -> Self {
        // SAFETY: the retained u32 domain bounds the nonzero encoded position.
        let index = unsafe { NonZero::new_unchecked(index as u32 + 1) };
        // SAFETY: the same retained domain admits this one-based row position.
        Self::from_source(unsafe { crate::depot::raw::Proof::new((source, index)) })
    }
}

impl<W: bitmap::Width, S: crate::depot::raw::Words<W>, P, M> Row<S, P, M, W> {
    pub(in crate::depot) const fn plane(&self) -> P
    where
        P: Copy,
    {
        self.plane
    }

    pub(in crate::depot::index::bitmap) fn borrow(&self) -> Row<&S, (), (), W> {
        Row {
            source: &self.source,
            index: self.index,
            plane: (),
            phase: marker::PhantomData,
        }
    }

    pub const fn source(&self) -> &S {
        &self.source
    }

    pub const fn index(&self) -> usize {
        self.index.get() as usize - 1
    }

    pub fn contains(&self, plane: S::Plane) -> bool {
        let index = self.index();
        let words = S::row(self.borrow(), plane);
        // SAFETY: this row retains the checked position in its exact plane.
        let word = unsafe { words.get_unchecked(index / W::BITS as usize) };
        word.get() & (1usize << (index % W::BITS as usize)) != 0
    }
}

impl<W: bitmap::Width, S: crate::depot::raw::Words<W>, P> Row<S, P, (), W> {
    /// # Safety
    /// The projection must retain the exact source, immutable domain, and owner lifetime.
    pub(in crate::depot) unsafe fn map<T: crate::depot::raw::Words<W>>(
        self,
        map: impl FnOnce(S) -> T,
    ) -> Row<T, (), (), W> {
        Row {
            source: map(self.source),
            index: self.index,
            plane: (),
            phase: marker::PhantomData,
        }
    }
}

impl<'a, W: bitmap::Width, S: crate::depot::raw::Bitmap<W>> Row<&'a mut S, (), (), W> {
    pub fn occupied(
        self,
        plane: S::Plane,
    ) -> Result<Mutable<'a, S, entry::Occupied, W>, Mutable<'a, S, entry::Absent, W>> {
        let entry: Row<_, _, (), W> = Row {
            source: self.source,
            index: self.index,
            plane,
            phase: marker::PhantomData,
        };
        if entry.contains(plane) {
            Ok(entry.with_phase())
        } else {
            Err(entry.with_phase())
        }
    }
}

impl<W: bitmap::Width, S: crate::depot::raw::Words<W, Size = NonZero<u32>>> Row<S, (), (), W> {
    /// Selects the requested row, saturating at this nonempty source's final row.
    pub fn clamp(source: S, index: usize) -> Self {
        let capacity = source.shape().bits;
        let index = index.min(capacity.get() as usize - 1);
        // SAFETY: the nonzero exact domain and clamp bound this encoded position.
        unsafe { Self::new(source, index) }
    }
}

impl<S, P, M, W> Row<S, P, M, W> {
    /// The compact one-based position retained by this row's source proof.
    pub const fn position(&self) -> NonZero<u32> {
        self.index
    }

    pub(in crate::depot) fn with_phase<N>(self) -> Row<S, P, N, W> {
        Row {
            source: self.source,
            index: self.index,
            plane: self.plane,
            phase: marker::PhantomData,
        }
    }
}

/// An exclusive row retains its bitmap owner and selected plane across phase changes.
type Mutable<'a, S, P, W> = Row<&'a mut S, <S as crate::depot::raw::Words<W>>::Plane, P, W>;

type Phase<M, W> = marker::PhantomData<fn(M, W) -> (M, W)>;
