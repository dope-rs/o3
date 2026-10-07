pub(in crate::buffer::segments) mod sequence;
use std::{marker, mem, num, ops};

pub(in crate::buffer::segments) use assemble::Assemble;
use many::Many;
use segments::{Front, Handle, Pool, Push, Sequence, assembly};
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

impl<T: buffer::raw::Segment, H: Handle<T>> Chain<T, H> {
    pub fn try_insert(
        &mut self,
        item: segments::Item<T>,
    ) -> Result<Front<'_, T, H>, crate::mem::Rejected<segments::Item<T>, segments::Error>> {
        edit::Insert::try_new(self, item, edit::Back)?.commit();
        Ok(Front { sequence: self })
    }
}

impl<T: buffer::raw::Segment> Assemble<T> for Chain<T> {
    fn sequence(&mut self) -> &mut segments::Chain<T, segments::Pool<T>> {
        self
    }
}

impl<T: buffer::raw::Segment> buffer::raw::Assemble<T> for Chain<T> {
    fn finish<S: buffer::raw::Sequence<T, Output = Self>>(input: segments::Built<T, S>) -> Self {
        input.chain
    }
}

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

impl<T: buffer::raw::Segment> TryFrom<(Chain<T>, segments::Item<T>)> for Chain<T> {
    type Error = crate::mem::Rejected<(Chain<T>, segments::Item<T>), assembly::Error>;

    fn try_from(source: (Chain<T>, segments::Item<T>)) -> Result<Self, Self::Error> {
        Assemble::try_append(source.0, source.1)
    }
}

impl<T: buffer::raw::Segment> TryFrom<(segments::Item<T>, Chain<T>)> for Chain<T> {
    type Error = crate::mem::Rejected<(segments::Item<T>, Chain<T>), assembly::Error>;

    fn try_from(source: (segments::Item<T>, Chain<T>)) -> Result<Self, Self::Error> {
        Assemble::try_prepend(source.1, source.0)
    }
}

impl<T: buffer::raw::Segment> TryFrom<(Chain<T>, Chain<T>)> for Chain<T> {
    type Error = crate::mem::Rejected<(Chain<T>, Chain<T>), assembly::Error>;

    fn try_from(source: (Chain<T>, Chain<T>)) -> Result<Self, Self::Error> {
        let (first, second) = source;
        if first.is_empty() {
            return Ok(second);
        }
        if second.is_empty() {
            return Ok(first);
        }
        edit::Join::new(first, second).try_commit()
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
        source: Many<T>,
    ) -> Result<(), crate::mem::Rejected<Many<T>, segments::Error>> {
        let mut source: Chain<T> = source.into();
        let error = match edit::Relation::new((self, &mut source)).try_commit() {
            Ok(()) => return Ok(()),
            Err(error) => error.into_error(),
        };
        // SAFETY: rejected preflight preserves this operation's original Many source.
        let source = unsafe { Many::new(source) };
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
        self.sequence.resident()
    }
}

// SAFETY: the immutable sequence owns both the exact byte total and every node.
unsafe impl<T: buffer::raw::Segment, H: segments::Handle<T>> buffer::raw::Chunks for Chain<T, H> {
    type Cursor<'data>
        = segments::Cursor<'data, T>
    where
        Self: 'data;

    fn len(&self) -> usize {
        self.sequence.len()
    }

    fn start(&self) -> Self::Cursor<'_> {
        segments::Cursor::new(self)
    }
}

impl<T: buffer::raw::Segment> Consume for Chain<T> {
    fn remaining(&self) -> usize {
        self.sequence.len()
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

/// Editable chain whose complete topology is owned by its sequence.
#[derive(Debug)]
#[repr(transparent)]
pub struct Chain<T, H: Handle<T> = Pool<T>> {
    pub(in crate::buffer::segments) sequence: Sequence<T, H>,
}

impl<T, H: Handle<T>> ops::Deref for Chain<T, H> {
    type Target = Sequence<T, H>;
    fn deref(&self) -> &Self::Target {
        &self.sequence
    }
}

impl<T, H: Handle<T>> ops::DerefMut for Chain<T, H> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sequence
    }
}

impl<T, H: Handle<T>> Chain<T, H> {
    pub(in crate::buffer::segments) const fn new(handle: H) -> Self {
        Self {
            sequence: Sequence {
                handle,
                key: Key::EMPTY,
                owner: marker::PhantomData,
            },
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

    pub fn try_push(
        &mut self,
        value: T,
    ) -> Result<Push<T>, crate::mem::Rejected<T, segments::Error>> {
        let item = match segments::Item::new(value) {
            ops::ControlFlow::Continue(item) => item,
            ops::ControlFlow::Break(value) => return Ok(Push::Discarded(value)),
        };
        self.try_insert(item)?;
        Ok(Push::Loaded)
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
        let mut cursor = write::Cursor::try_new(target, len)?;
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
