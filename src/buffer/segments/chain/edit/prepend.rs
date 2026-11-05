use std::iter;

use segments::assembly;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::chain::edit::{Cause, Growth},
    },
};

pub(in crate::buffer::segments) struct Prepend<T: buffer::raw::Segment> {
    source: (segments::Item<T>, segments::Many<T>),
    growth: Growth,
    cause: Cause,
}

impl<T: buffer::raw::Segment> Prepend<T> {
    pub(in crate::buffer::segments) fn new(
        source: (segments::Item<T>, segments::Many<T>),
        growth: Growth,
        cause: Cause,
    ) -> Self {
        Self {
            source,
            growth,
            cause,
        }
    }
}

// SAFETY: this prepared input owns the original source and its checked growth.
unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T> for Prepend<T> {
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(segments::Item<T>, segments::Many<T>), assembly::Error>;
    type Output = segments::Many<T>;
    fn owners(self) -> impl Iterator<Item = Self::Item> + buffer::raw::Total {
        let (item, chain) = self.source;
        let mut chain: segments::Chain<T> = chain.into();
        buffer::raw::Batch {
            owners: iter::once(item).chain(iter::from_fn(move || chain.pop_item())),
            total: self.growth,
        }
    }
    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        crate::mem::Rejected::new(self.source, self.cause.allocation(error))
    }
}

impl<T: buffer::raw::Segment> buffer::raw::Total for Prepend<T> {
    fn count(&self) -> segments::Count {
        self.growth.count
    }
    fn len(&self) -> usize {
        self.growth.len
    }
    fn resident(&self) -> usize {
        self.growth.resident
    }
}
