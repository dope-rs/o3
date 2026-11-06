pub(in crate::buffer::segments) mod sequence;
use std::{marker, mem, num, ops};

use many::Many;
use segments::{Front, Handle, Pool};
use storage::alloc;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Key, handles},
        storage,
        storage::regions,
        write::{self, consume, consume::Consume},
    },
};

pub(in crate::buffer::segments) mod front;
pub(in crate::buffer::segments) mod many;
pub(in crate::buffer::segments) mod single;

impl<T: buffer::raw::Segment> Chain<T> {
    pub fn try_flatten(&self) -> Result<storage::Shared, storage::AllocError> {
        use storage::AllocError;

        let len = self.len();
        consume::Error::check(len, u32::MAX as usize).map_err(AllocError::Capacity)?;
        let Some(capacity) = num::NonZero::<u32>::new(len as u32) else {
            return Ok(storage::Shared::new());
        };
        let source = Flatten {
            capacity,
            sources: self.segments(),
        };
        let init = regions::Initialize::try_init::<()>(source, ())
            .map_err(alloc::Error::into_alloc)
            .map_err(AllocError::Exhausted)?;
        Ok(init.into_buf().into())
    }
}

struct Flatten<'a, T> {
    capacity: num::NonZero<u32>,
    sources: segments::Iter<'a, T>,
}

// SAFETY: try_flatten admits the exact byte sum of this retained immutable chain.
unsafe impl<T: buffer::raw::Segment> regions::Initialize for Flatten<'_, T> {
    fn parts(&self) -> (num::NonZero<u32>, impl buffer::raw::SliceExt) {
        (self.capacity, &self.sources)
    }
}

impl<T: buffer::raw::Segment> Chain<T> {
    /// Moves every incoming owner into this pool, preserving the source on rejection.
    pub fn try_extend(
        &mut self,
        mut source: Many<T>,
    ) -> Result<(), crate::mem::Rejected<Many<T>, segments::Error>> {
        let error = match edit::Relation::new((self, &mut source.0)).try_commit() {
            Ok(()) => return Ok(()),
            Err(error) => error.into_error(),
        };
        Err(crate::mem::Rejected::new(source, error))
    }

    pub fn into_shape(mut self) -> segments::Shape<T> {
        let index = match self.key.state() {
            handles::State::Empty => return segments::Shape::Empty,
            handles::State::Single(index) => index,
            // SAFETY: this decoded multi key owns its exact initialized descriptor.
            handles::State::Multi(_) => return segments::Shape::Many(unsafe { Many::new(self) }),
        };
        self.key = Key::EMPTY;
        let nodes = handles::nodes(&self.handle);
        let owner = unsafe { nodes.detached(index) }.take().0;
        segments::Shape::One(owner)
    }

    /// Joins two chains from this exact pool without touching payload owners.
    pub fn try_append(&mut self, other: &mut Self) -> Result<(), segments::Error> {
        let relation = edit::Relation::new((self, other));
        if let edit::Relation::Other(_) = &relation {
            return Err(segments::Error::DifferentPool);
        }
        relation.try_commit().map_err(|error| error.into_error())
    }
}

impl<T: buffer::raw::Segment, H: segments::Handle<T>> buffer::Residency for Chain<T, H> {
    fn resident(&self) -> usize {
        Chain::resident(self)
    }
}

// SAFETY: the immutable sequence owns both the exact byte total and every node.
unsafe impl<T: buffer::raw::Segment, H: segments::Handle<T>> buffer::raw::Chunks for Chain<T, H> {
    type Cursor<'data>
        = segments::Cursor<'data, T>
    where
        Self: 'data;

    fn len(&self) -> usize {
        Chain::len(self)
    }

    fn start(&self) -> Self::Cursor<'_> {
        segments::Cursor::new(self)
    }
}

impl<T: buffer::raw::Segment> Consume for Chain<T> {
    fn remaining(&self) -> usize {
        self.len()
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        segments::Drain::try_new(self, amount)?.commit();
        Ok(())
    }

    fn consume_max(&mut self, max: usize) -> usize {
        let amount = max.min(self.len());
        segments::Drain {
            target: self,
            amount,
        }
        .commit();
        amount
    }
}

const _: () = {
    assert!(mem::size_of::<Chain<storage::Shared>>() <= 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Chain<storage::Shared>>>() <= 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<&'static mut Chain<storage::Shared>>() == mem::size_of::<usize>());
};

impl<T: buffer::raw::Segment, H: segments::Handle<T>> Chain<T, H> {
    pub fn pop_segment(&mut self) -> Option<T> {
        self.pop_item().map(segments::Item::into_inner)
    }
}

/// Editable chain owning its complete topology.
#[derive(Debug)]
#[repr(C)]
pub struct Chain<T, H: Handle<T> = Pool<T>> {
    pub(in crate::buffer::segments) handle: H,
    pub(in crate::buffer::segments) key: Key,
    pub(in crate::buffer::segments) owner: marker::PhantomData<fn() -> T>,
}

impl<T, H: Handle<T>> Chain<T, H> {
    pub(in crate::buffer::segments) const fn new(handle: H) -> Self {
        Self {
            handle,
            key: Key::EMPTY,
            owner: marker::PhantomData,
        }
    }
}

mod assemble;
pub(in crate::buffer::segments) mod edit;
pub(in crate::buffer::segments) mod prefix;
pub(in crate::buffer::segments) mod view;

impl<T: buffer::raw::Segment> Chain<T> {
    pub fn try_take(&mut self, amount: usize) -> Result<Self, segments::Error> {
        if amount == self.len() {
            let empty = Self::new(self.handle.try_clone().map_err(segments::Error::Retain)?);
            return Ok(mem::replace(self, empty));
        }
        let shape = prefix::Shape::try_new(self, amount)?;
        shape.try_take()
    }

    pub fn try_move_to(&mut self, target: &mut Self, amount: usize) -> Result<(), segments::Error> {
        if amount == self.len() {
            return edit::Relation::new((target, self))
                .try_commit()
                .map_err(|error| match error.into_error() {
                    segments::Error::Total(cause) => segments::Error::component(cause),
                    error => error,
                });
        }
        let shape = prefix::Shape::try_new(self, amount)?;
        shape.try_move_to(target)
    }
}

impl<T: buffer::raw::Segment, H: Handle<T>> Chain<T, H> {
    pub fn front_entry(&mut self) -> Option<segments::Front<'_, T, H>> {
        segments::Front::new(self)
    }

    /// Returns an empty owner unchanged, or None after insertion.
    pub fn try_push(
        &mut self,
        value: T,
    ) -> Result<Option<T>, crate::mem::Rejected<segments::Item<T>, segments::Error>> {
        let item = match segments::Item::new(value) {
            ops::ControlFlow::Continue(item) => item,
            ops::ControlFlow::Break(value) => return Ok(Some(value)),
        };
        self.try_insert(item).map(|_| None)
    }

    pub(in crate::buffer::segments) fn pop_item(&mut self) -> Option<segments::Item<T>> {
        self.front_entry().map(Front::take)
    }
}

impl<T: buffer::raw::Segment, H: segments::Handle<T>> Chain<T, H> {
    /// Copies and releases every segment in order, preserving both sides on rejection.
    /// The caller commits `target` to publish the copied bytes.
    pub fn try_drain(&mut self, target: &mut write::Spare<'_>) -> Result<(), consume::Error> {
        use buffer::raw::SliceExt as _;

        let len = self.len();
        let mut cursor = write::spans::Writer::try_new(target, len)?;
        let (pointer, start) = cursor.suffix();
        // SAFETY: admission covers the retained chain's exact byte sum and
        // disjoint exclusive suffix; traversal retains every source until copying ends.
        unsafe { self.segments().copy_to(pointer.add(start)) };
        self.clear();
        // SAFETY: the complete admitted chain initialized exactly this prefix.
        unsafe { cursor.extend(len) };
        cursor.commit();
        Ok(())
    }
}
