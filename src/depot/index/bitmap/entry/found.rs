use std::{marker, num::NonZero};

use crate::depot::index::{bitmap, bitmap::entry::Row};

/// A selected nonempty position retaining its exact level.
pub(in crate::depot::index::bitmap) struct Found;

impl<'a, 'l, W: bitmap::Width> Row<&'l bitmap::Level<'a, W>, (), Found, W> {
    /// # Safety
    /// `index` belongs to this exact level and selects a nonempty leaf.
    pub(in crate::depot::index::bitmap) const unsafe fn found(
        level: &'l bitmap::Level<'a, W>,
        index: usize,
    ) -> Self {
        // SAFETY: exact tail bits and this source's u32 domain bound the selection.
        Self {
            source: level,
            index: unsafe { NonZero::new_unchecked(index as u32 + 1) },
            plane: (),
            phase: marker::PhantomData,
        }
    }
}
