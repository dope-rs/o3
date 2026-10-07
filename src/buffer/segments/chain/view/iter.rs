use std::iter;

use crate::{buffer::segments, depot::arena::linked};

/// Immutable traversal over the concrete retained owners in one chain.
pub struct Iter<'a, T> {
    cursor: linked::Iter<'a, segments::Item<T>>,
    remaining: segments::Count,
}

impl<'a, T> Iter<'a, T> {
    pub(in crate::buffer::segments::chain) fn new(
        cursor: linked::Iter<'a, segments::Item<T>>,
        left: segments::Count,
    ) -> Self {
        Self {
            cursor,
            remaining: left,
        }
    }
}

impl<'a, T: 'a> Iter<'a, T> {
    pub(in crate::buffer::segments) fn next_item(&mut self) -> Option<&'a segments::Item<T>> {
        let left = self.remaining.into_raw();
        if left == 0 {
            return None;
        }
        self.remaining = segments::Count::new(left - 1);
        // SAFETY: `remaining` is the exact live topology cardinality.
        Some(unsafe { self.cursor.next_live() })
    }
}

impl<'a, T: 'a> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_item().map(AsRef::as_ref)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.remaining.into_raw() as usize;
        (left, Some(left))
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> iter::FusedIterator for Iter<'_, T> {}

impl<T> Clone for Iter<'_, T> {
    fn clone(&self) -> Self {
        Self {
            cursor: self.cursor,
            remaining: self.remaining,
        }
    }
}

const _: () = {
    assert!(size_of::<Iter<'static, ()>>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<Iter<'static, ()>>>() == size_of::<Iter<'static, ()>>());
};

impl<'a, T: crate::buffer::raw::Segment + 'a> crate::buffer::raw::SliceExt for Iter<'a, T> {
    unsafe fn copy_to(&self, target: *mut u8) {
        // SAFETY: cloning traversal preserves the retained immutable chain;
        // the caller admits its complete byte sum and disjoint target.
        unsafe {
            crate::buffer::raw::copy_slices(self.clone().map(AsRef::as_ref), target);
        }
    }
}
