use std::mem;

pub(in crate::buffer) use built::Built;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Chain, Key, Pool, handles},
        storage,
    },
    depot::arena::linked::{fifo::ends, slots},
};

mod built;

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
        let handle = Pool::try_new(segments::Plan {
            segments: count.into_raw() as usize,
            chains: usize::from(count.is_many()),
        });
        Self::try_build(source, handle)
    }

    pub(in crate::buffer::segments) fn try_reserve(
        source: S,
        current: segments::Count,
    ) -> Result<Self, S::Error> {
        let handle = Pool::try_reserve(current, source.count());
        Self::try_build(source, handle)
    }

    fn try_build(
        source: S,
        handle: Result<Pool<T>, crate::alloc::Error>,
    ) -> Result<Self, S::Error> {
        let handle = match handle {
            Ok(handle) => handle,
            Err(error) => return Err(source.reject(error)),
        };
        Ok(Self { handle, source })
    }
}

impl<T: buffer::raw::Segment, S: buffer::raw::Sequence<T>> Exact<T, S> {
    pub fn collect(self) -> S::Output
    where
        S: buffer::raw::Sequence<T, Item = segments::Item<T>>,
    {
        let Self { handle, source } = self;
        let mut source = source.owners();
        let nodes = handles::nodes(&handle);
        let mut writer = Writer::new(nodes);
        for value in source.by_ref() {
            let claim = unsafe { slots::Vacant::reserve(nodes) };
            unsafe { writer.push_claim(claim, value) };
        }
        let links = writer.into_links();
        Self::finish(handle, links, source)
    }

    pub fn try_collect<E>(self) -> Result<S::Output, E>
    where
        S: buffer::raw::Sequence<T, Item = Result<segments::Item<T>, E>>,
    {
        let Self { handle, source } = self;
        let mut source = source.owners();
        let nodes = handles::nodes(&handle);
        let mut writer = Writer::new(nodes);
        for value in source.by_ref() {
            let value = value?;
            let claim = unsafe { slots::Vacant::reserve(nodes) };
            unsafe { writer.push_claim(claim, value) };
        }
        let links = writer.into_links();
        Ok(Self::finish(handle, links, source))
    }

    fn finish(
        handle: Pool<T>,
        links: ends::Exclusive<slots::Index>,
        total: impl buffer::raw::Total,
    ) -> S::Output {
        let key = match buffer::raw::Total::count(&total).kind() {
            segments::Kind::Empty => Key::EMPTY,
            segments::Kind::One => Key::single(links.head_raw()),
            segments::Kind::Many(count) => {
                let descs = handles::descriptors(&handle);
                // SAFETY: `Exact` is constructed with one private descriptor
                // whenever its admitted source contains at least two owners;
                // Sequence preserves that total throughout iteration.
                let claim =
                    unsafe { slots::Vacant::reserve(descs) }.insert(handles::Descriptor::new(
                        links,
                        count.nonzero(),
                        buffer::raw::Total::len(&total),
                        buffer::raw::Total::resident(&total),
                    ));
                let key = Key::multi(claim.index());
                unsafe { claim.publish() };
                key
            }
        };
        let mut sequence = Chain::new(handle);
        sequence.key = key;
        buffer::raw::Assemble::finish::<S>(Built::new(sequence))
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
        let mut links = unsafe { ends::Bound::new(self.nodes, &mut self.links) };
        links.clear();
    }
}

const _: () = assert!(
    size_of::<Exact<storage::Shared, &segments::Many<storage::Shared>>>() == 2 * size_of::<usize>()
);
