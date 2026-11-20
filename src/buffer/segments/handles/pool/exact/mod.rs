use std::num::NonZero;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Chain, Key, Pool, handles},
        storage,
    },
    depot::arena::linked::{fifo::ends, slots},
};

/// Fresh, unaliased metadata capacity owned by one prepared build.
pub struct Exact<T, S> {
    handle: Pool<T>,
    source: S,
}

impl<T: buffer::raw::Segment, S: buffer::raw::Sequence<T>> Exact<T, S> {
    pub fn try_new(source: S) -> Result<Self, S::Error> {
        let count = source.count();
        let parts = handles::Parts::<T>::try_new(segments::Plan {
            segments: count.into_raw() as usize,
            chains: 1,
        });
        Self::try_build(source, parts)
    }

    #[inline(always)]
    pub(in crate::buffer::segments) fn try_reserve(
        source: S,
        current: segments::Count,
    ) -> Result<Self, S::Error> {
        let parts = handles::Parts::<T>::try_reserve(current, source.count());
        Self::try_build(source, parts)
    }

    #[inline(always)]
    fn try_build(
        source: S,
        parts: Result<handles::Parts<T>, crate::alloc::Overflow>,
    ) -> Result<Self, S::Error> {
        let handle = match parts
            .map_err(crate::alloc::Error::from)
            .and_then(Pool::allocate)
        {
            Ok(handle) => handle,
            Err(error) => return Err(source.reject(error)),
        };
        Ok(Self { handle, source })
    }

    #[inline(always)]
    pub fn collect(self) -> segments::Many<T>
    where
        S: buffer::raw::Sequence<T, Item = segments::Item<T>>,
    {
        let Self { handle, source } = self;
        let mut source = source.owners();
        let nodes = handles::nodes(&handle);
        let mut links = ends::Exclusive::empty();
        source.0.by_ref().for_each(|value| {
            // SAFETY: the private pool has capacity for this sequence and owns all these links.
            unsafe { slots::Vacant::insert_one(nodes, value).append(&mut links) };
        });
        Self::finish(handle, links, &source.1)
    }

    pub fn try_collect<E>(self) -> Result<segments::Many<T>, E>
    where
        S: buffer::raw::Sequence<T, Item = Result<segments::Item<T>, E>>,
    {
        let mut source = self.source.owners();
        // The private pool drops its initialized prefix in insertion order before the iterator.
        let handle = self.handle;
        let nodes = handles::nodes(&handle);
        let mut links = ends::Exclusive::empty();
        for value in source.0.by_ref() {
            let value = value?;
            // SAFETY: the admitted sequence yields at most the private pool's capacity.
            unsafe { slots::Vacant::insert_one(nodes, value).append(&mut links) };
        }
        Ok(Self::finish(handle, links, &source.1))
    }

    fn finish(
        handle: Pool<T>,
        links: ends::Exclusive<slots::Index>,
        total: &impl buffer::raw::Total,
    ) -> segments::Many<T> {
        // SAFETY: Sequence preserves its admitted count of at least two owners.
        let count = unsafe { NonZero::new_unchecked(total.count().into_raw()) };
        let descs = handles::descriptors(&handle);
        let descriptor = handles::Descriptor {
            links,
            count,
            len: total.len(),
            resident_bytes: total.resident(),
        };
        // SAFETY: Exact owns one available descriptor for this completed sequence.
        let claim = unsafe { slots::Vacant::insert_one(descs, descriptor) };
        let key = Key::multi(claim.index());
        unsafe { claim.publish() };
        let mut sequence = Chain::new(handle);
        sequence.key = key;
        // SAFETY: the published descriptor owns the Sequence's at least two owners.
        unsafe { segments::Many::new(sequence) }
    }
}

const _: () = assert!(
    size_of::<Exact<storage::Shared, &segments::Many<storage::Shared>>>() == 2 * size_of::<usize>()
);
