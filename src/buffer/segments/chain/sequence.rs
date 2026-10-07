use std::{marker, mem};

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{
            Handle, Key, Pool,
            chain::{view, view::iter::Iter},
            handles,
        },
    },
    depot::arena::linked::{self, fifo::ends, slots},
};

/// The complete state of one compact chain handle.
#[derive(Debug)]
#[repr(C)]
pub struct Sequence<T, H: Handle<T> = Pool<T>> {
    pub(in crate::buffer::segments) handle: H,
    pub(in crate::buffer::segments) key: Key,
    pub(in crate::buffer::segments) owner: marker::PhantomData<fn() -> T>,
}

struct Node<'a, T, H: Handle<T>> {
    source: &'a Sequence<T, H>,
    index: slots::Index,
}

struct Links<'a, T, H: Handle<T>> {
    source: &'a Sequence<T, H>,
    index: slots::Index,
}

// SAFETY: state constructs Node only from this borrowed sequence's own single key.
unsafe impl<'a, T: 'a, H: Handle<T>> buffer::raw::Indexed<'a> for Node<'a, T, H> {
    type Item = segments::Item<T>;

    fn parts(self) -> (&'a slots::Store<Self::Item>, slots::Index) {
        (handles::nodes(&self.source.handle), self.index)
    }
}

// SAFETY: state selects this sequence's own multi key and retains its exact stores.
unsafe impl<'a, T: 'a, H: Handle<T>> buffer::raw::Linked<'a> for Links<'a, T, H> {
    type Item = segments::Item<T>;
    type Metadata = handles::Descriptor;

    fn parts(
        self,
    ) -> (
        &'a slots::Store<Self::Item>,
        &'a slots::Store<Self::Metadata>,
        slots::Index,
    ) {
        (
            handles::nodes(&self.source.handle),
            handles::descriptors(&self.source.handle),
            self.index,
        )
    }

    fn head(metadata: &Self::Metadata) -> slots::Index {
        metadata.links.head_raw()
    }
}

impl<T, H: Handle<T>> Sequence<T, H> {
    pub fn clear(&mut self) {
        let key = mem::replace(&mut self.key, Key::EMPTY);
        Self::release(&self.handle, &key);
    }

    fn release(handle: &H, key: &Key) {
        let nodes = handles::nodes(handle);
        let index = match key.state() {
            handles::State::Empty => return,
            handles::State::Single(index) => {
                // SAFETY: this detached key retains its exact initialized node.
                unsafe { nodes.detached(index) }.clear().release();
                return;
            }
            handles::State::Multi(index) => index,
        };
        let descs = handles::descriptors(handle);
        // SAFETY: this detached multi key uniquely owns its exact descriptor.
        let (mut desc, retired) = unsafe { descs.detached(index) }.take();
        // SAFETY: the descriptor owns this exact tracked node-store partition.
        let mut links = unsafe { ends::Bound::new(nodes, &mut desc.links) };
        links.clear();
        retired.release();
    }
}

impl<T, H: Handle<T>> Drop for Sequence<T, H> {
    fn drop(&mut self) {
        Self::release(&self.handle, &self.key);
    }
}

impl<T: buffer::raw::Segment, H: Handle<T>> Sequence<T, H> {
    pub(in crate::buffer::segments) fn state(&self) -> view::State<'_, T> {
        if self.key.is_empty() {
            return view::State::Empty;
        }
        // SAFETY: this unchanged owner has a nonempty key.
        match unsafe { self.occupied() } {
            view::Occupied::Single(index, item) => view::State::Single(index, item),
            view::Occupied::Multi(desc) => view::State::Multi(desc),
        }
    }

    /// # Safety
    /// This unchanged sequence owns a nonempty chain.
    pub(in crate::buffer::segments) unsafe fn occupied(&self) -> view::Occupied<'_, T> {
        // SAFETY: the caller's nonempty owner proof excludes the empty key.
        match unsafe { self.key.occupied() } {
            handles::Occupied::Single(index) => {
                let node = buffer::raw::Indexed::get(Node {
                    source: self,
                    index,
                });
                view::Occupied::Single(index, node)
            }
            handles::Occupied::Multi(index) => {
                let desc = buffer::raw::Linked::metadata(Links {
                    source: self,
                    index,
                });
                view::Occupied::Multi(desc)
            }
        }
    }
    pub(in crate::buffer::segments) fn totals(&self) -> (segments::Count, usize, usize) {
        match self.state() {
            view::State::Empty => (segments::Count::EMPTY, 0, 0),
            view::State::Single(_, item) => (
                segments::Count::ONE,
                item.len().get(),
                buffer::Residency::resident(item),
            ),
            view::State::Multi(desc) => (
                segments::Count::new(desc.count.get()),
                desc.len,
                desc.resident_bytes,
            ),
        }
    }

    pub fn len(&self) -> usize {
        match self.state() {
            view::State::Empty => 0,
            view::State::Single(_, item) => item.len().get(),
            view::State::Multi(desc) => desc.len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.key.is_empty()
    }

    pub fn count(&self) -> segments::Count {
        match self.state() {
            view::State::Empty => segments::Count::EMPTY,
            view::State::Single(_, _) => segments::Count::ONE,
            view::State::Multi(desc) => segments::Count::new(desc.count.get()),
        }
    }

    pub fn resident(&self) -> usize {
        match self.state() {
            view::State::Empty => 0,
            view::State::Single(_, item) => buffer::Residency::resident(item),
            view::State::Multi(desc) => desc.resident_bytes,
        }
    }

    pub fn segments(&self) -> Iter<'_, T> {
        let (head, left) = match self.state() {
            view::State::Empty => (slots::Index::NONE, segments::Count::EMPTY),
            view::State::Single(index, _) => (index, segments::Count::ONE),
            view::State::Multi(desc) => (
                desc.links.head_raw(),
                segments::Count::new(desc.count.get()),
            ),
        };
        // SAFETY: this sequence borrow retains the exact immutable live chain.
        let cursor = unsafe { linked::Iter::new(handles::nodes(&self.handle), head) };
        Iter::new(cursor, left)
    }

    pub fn contiguous(&self) -> Option<&[u8]> {
        match self.state() {
            view::State::Empty => Some(&[]),
            view::State::Single(_, item) => Some(item.as_ref().as_ref()),
            view::State::Multi(_) => None,
        }
    }

    pub(in crate::buffer::segments) fn is_same(&self, other: &Self) -> bool {
        self.handle.is_same(&other.handle)
    }
}
