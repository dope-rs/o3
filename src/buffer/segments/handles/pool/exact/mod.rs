use std::{mem, num::NonZero};

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

pub(in crate::buffer) struct Writer<'a, T> {
    nodes: &'a slots::Store<segments::Item<T>>,
    links: ends::Exclusive<slots::Index>,
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
}

impl<T: buffer::raw::Segment, S: buffer::raw::Sequence<T>> Exact<T, S> {
    #[inline(always)]
    pub fn collect(self) -> segments::Many<T>
    where
        S: buffer::raw::Sequence<T, Item = segments::Item<T>>,
    {
        let Self { handle, source } = self;
        let mut source = source.owners();
        let nodes = handles::nodes(&handle);
        let mut writer = Writer::new(nodes);
        source.0.by_ref().for_each(|value| {
            let claim = unsafe { slots::Vacant::reserve(nodes) };
            unsafe { writer.push_claim(claim, value) };
        });
        let links = writer.into_links();
        Self::finish(handle, links, &source.1)
    }

    pub fn try_collect<E>(self) -> Result<segments::Many<T>, E>
    where
        S: buffer::raw::Sequence<T, Item = Result<segments::Item<T>, E>>,
    {
        let Self { handle, source } = self;
        let mut source = source.owners();
        let nodes = handles::nodes(&handle);
        let mut writer = Writer::new(nodes);
        for value in source.0.by_ref() {
            let value = value?;
            let claim = unsafe { slots::Vacant::reserve(nodes) };
            unsafe { writer.push_claim(claim, value) };
        }
        let links = writer.into_links();
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
        // SAFETY: Exact reserves one private descriptor for its admitted Sequence.
        let claim = unsafe { slots::Vacant::reserve(descs) }.insert(handles::Descriptor {
            links,
            count,
            len: total.len(),
            resident_bytes: total.resident(),
        });
        let key = Key::multi(claim.index());
        unsafe { claim.publish() };
        let mut sequence = Chain::new(handle);
        sequence.key = key;
        // SAFETY: the published descriptor owns the Sequence's at least two owners.
        unsafe { segments::Many::new(sequence) }
    }
}

impl<T: buffer::raw::Segment> Writer<'_, T> {
    /// # Safety
    /// `index` is an outstanding vacancy in this writer's exact node store.
    pub(in crate::buffer::segments::handles::pool) unsafe fn push_claim(
        &mut self,
        claim: slots::Vacant<segments::Item<T>, &'_ slots::Store<segments::Item<T>>>,
        value: segments::Item<T>,
    ) {
        unsafe { claim.insert(value).append(&mut self.links) };
    }

    pub(in crate::buffer) fn into_links(mut self) -> ends::Exclusive<slots::Index> {
        mem::replace(&mut self.links, ends::Exclusive::empty())
    }
}

impl<'a, T> Writer<'a, T> {
    pub(in crate::buffer::segments::handles::pool) fn new(
        nodes: &'a slots::Store<segments::Item<T>>,
    ) -> Self {
        Self {
            nodes,
            links: ends::Exclusive::empty(),
        }
    }
}

impl<T> Drop for Writer<'_, T> {
    fn drop(&mut self) {
        // SAFETY: this writer owns its exact initialized node-store partition.
        unsafe { self.links.clear(self.nodes) };
    }
}

const _: () = assert!(
    size_of::<Exact<storage::Shared, &segments::Many<storage::Shared>>>() == 2 * size_of::<usize>()
);
