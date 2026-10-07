use std::{marker, ops};

use crate::buffer::{self, bytes::scope};

/// Bounds selected from the immutable owner with this invariant identity.
pub struct Range<'id> {
    pub(in crate::buffer::bytes::scope) bounds: ops::Range<usize>,
    pub(in crate::buffer::bytes::scope) brand: marker::PhantomData<*mut &'id ()>,
}

impl<'id> Range<'id> {
    pub(in crate::buffer::bytes::scope) fn new(
        source: scope::View<'_, 'id>,
        bounds: ops::Range<usize>,
    ) -> Self {
        Self {
            bounds,
            brand: source.brand,
        }
    }

    /// Selects one side of the unchanged source's admitted prefix boundary.
    pub(in crate::buffer) fn select<const SUFFIX: bool, S: buffer::raw::Store>(
        selected: buffer::raw::Selection<'id, S>,
    ) -> buffer::raw::Slice<'id, 'id, S> {
        let (source, len) = selected.into_parts();
        let bounds = if SUFFIX {
            len..source.as_ref().len()
        } else {
            0..len
        };
        let range = Self {
            bounds,
            brand: marker::PhantomData,
        };
        // SAFETY: the selected prefix retains this exact source and its admitted bounds.
        unsafe { buffer::raw::Slice::from_parts(source, range) }
    }
}

impl Range<'_> {
    pub(in crate::buffer) fn into_inner(self) -> ops::Range<usize> {
        self.bounds
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.bounds.end - self.bounds.start
    }
}

const _: () = assert!(size_of::<Range<'static>>() == 2 * size_of::<usize>());
