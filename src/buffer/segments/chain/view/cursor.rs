use std::num::NonZero;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Chain, Handle, chain::view, handles, scatter},
    },
    depot::arena::{linked, linked::slots},
};

/// Exact byte traversal borrowing one immutable retained chain.
pub struct Cursor<'a, T> {
    nodes: linked::Iter<'a, segments::Item<T>>,
    current: &'a [u8],
    remaining: usize,
}

impl<'a, T: buffer::raw::Segment> Cursor<'a, T> {
    /// # Safety
    /// The checkpoint belongs to this unchanged source, and offset < source.len().
    pub(in crate::buffer) unsafe fn resume<H: Handle<T>>(
        source: &'a Chain<T, H>,
        position: &mut scatter::raw::Position,
        offset: usize,
    ) -> Self {
        let nodes = handles::nodes(&source.handle);
        // SAFETY: offset < source.len() proves this unchanged owner is nonempty.
        let (head, len) = match unsafe { source.occupied() } {
            view::Occupied::Single(index, item) => (index, item.len().get()),
            view::Occupied::Multi(desc) => (desc.links.head_raw(), desc.len),
        };
        if position.index == slots::Index::NONE || offset < position.start {
            position.index = head;
            position.start = 0;
        }
        loop {
            // SAFETY: the checkpoint retains this source's live current node;
            // advancing below its exact total always reaches another live node.
            let item = unsafe { nodes.value(position.index) };
            let bytes = item.chunk().bytes();
            let skip = offset - position.start;
            let next = unsafe { nodes.next(position.index) };
            if skip < bytes.len() {
                return Self {
                    nodes: unsafe { linked::Iter::new(nodes, next) },
                    current: unsafe { bytes.get_unchecked(skip..) },
                    remaining: len - offset,
                };
            }
            position.start += bytes.len();
            position.index = next;
        }
    }

    pub(in crate::buffer) fn remaining(&self) -> usize {
        self.remaining
    }

    /// # Safety
    /// Positive remaining and cap/left bounds belong to this unchanged borrowed chain.
    pub(in crate::buffer) unsafe fn next_chunk<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
        _left: NonZero<usize>,
    ) -> buffer::raw::Chunk<'a, MAX> {
        if self.current.is_empty() {
            // SAFETY: positive remaining bytes and an exhausted current span
            // prove that this exact borrowed topology has a live next item.
            let item = unsafe { self.nodes.next_live() };
            self.current = item.chunk().bytes();
        }
        // SAFETY: the retained current span or newly loaded item is nonempty.
        let len = unsafe { NonZero::new_unchecked(self.current.len()) }.min(cap);
        let (bytes, suffix) = self.current.split_at(len.get());
        self.current = suffix;
        self.remaining -= len.get();
        // SAFETY: the retained item proves nonemptiness; cap and left preserve
        // the caller's byte bound and the source's exact progress.
        unsafe { buffer::raw::Chunk::new(bytes) }
    }

    pub(in crate::buffer::segments) fn new<H: Handle<T>>(source: &'a Chain<T, H>) -> Self {
        let sequence = source;
        let nodes = handles::nodes(&sequence.handle);
        let (head, len) = match sequence.state() {
            view::State::Empty => (slots::Index::NONE, 0),
            view::State::Single(index, item) => (index, item.len().get()),
            view::State::Multi(desc) => (desc.links.head_raw(), desc.len),
        };
        Self {
            // SAFETY: this sequence borrow retains the exact immutable live chain.
            nodes: unsafe { linked::Iter::new(nodes, head) },
            current: &[],
            remaining: len,
        }
    }
}

const _: () = {
    assert!(size_of::<Cursor<'static, ()>>() == 5 * size_of::<usize>());
    assert!(size_of::<Option<Cursor<'static, ()>>>() == size_of::<Cursor<'static, ()>>());
};
