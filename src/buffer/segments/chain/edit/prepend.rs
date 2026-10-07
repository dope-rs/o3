use std::iter;

use segments::assembly;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::chain::{
            Assemble,
            edit::{Cause, Growth},
        },
    },
};

pub(in crate::buffer::segments) struct Prepend<T: buffer::raw::Segment, C: Assemble<T>> {
    source: (segments::Item<T>, C),
    growth: Growth,
    cause: Cause,
}

impl<T: buffer::raw::Segment, C: Assemble<T>> Prepend<T, C> {
    pub(in crate::buffer::segments) fn new(
        source: (segments::Item<T>, C),
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
unsafe impl<T: buffer::raw::Segment, C: Assemble<T>> buffer::raw::Sequence<T> for Prepend<T, C> {
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(segments::Item<T>, C), assembly::Error>;
    type Output = C;
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

impl<T: buffer::raw::Segment, C: Assemble<T>> buffer::raw::Total for Prepend<T, C> {
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
