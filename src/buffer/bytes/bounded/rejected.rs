use std::num::NonZero;

use crate::{buffer, buffer::bytes::bounded};

/// A stable owner whose byte extent exceeds the signed count domain.
#[repr(transparent)]
pub struct Rejected<T: buffer::raw::Source> {
    source: T,
}

impl<T: buffer::raw::Source> Rejected<T> {
    pub(in crate::buffer::bytes::bounded) const fn new(source: T) -> Self {
        Self { source }
    }

    pub fn into_parts(self) -> (T, bounded::Overflow) {
        let excess = NonZero::<usize>::MIN
            .saturating_add(self.source.as_ref().len().saturating_sub(bounded::MAX + 1));
        (self.source, bounded::Overflow::new(excess))
    }
}
