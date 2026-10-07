use std::{marker, mem};

use crate::depot::capacity;

/// A validated u32-sized domain for one semantic tag and storage width.
#[repr(transparent)]
pub struct Domain<Tag = (), Raw = usize> {
    capacity: u32,
    index: Identity<Tag, Raw>,
}

impl<Tag, Raw: crate::depot::raw::Word> Domain<Tag, Raw> {
    pub const fn try_new(capacity: usize) -> Result<Self, capacity::Error> {
        let count = match capacity::Count::try_index(
            capacity,
            <Raw as crate::depot::raw::Word>::MAX.get(),
            capacity::Error::new(capacity),
        ) {
            Ok(count) => count,
            Err(error) => return Err(error),
        };
        Ok(Self {
            capacity: count.raw(),
            index: marker::PhantomData,
        })
    }
}

impl<Tag> From<capacity::Count> for Domain<Tag> {
    fn from(capacity: capacity::Count) -> Self {
        Self {
            capacity: capacity.raw(),
            index: marker::PhantomData,
        }
    }
}

impl<Tag, Raw> Domain<Tag, Raw> {
    #[must_use]
    pub const fn capacity(self) -> usize {
        self.capacity as usize
    }

    pub(in crate::depot) const fn raw(self) -> u32 {
        self.capacity
    }
}

impl<Tag, Raw> Clone for Domain<Tag, Raw> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, Raw> Copy for Domain<Tag, Raw> {}

impl<Tag, Raw> PartialEq for Domain<Tag, Raw> {
    fn eq(&self, other: &Self) -> bool {
        self.capacity == other.capacity
    }
}

impl<Tag, Raw> Eq for Domain<Tag, Raw> {}

const _: () = assert!(mem::size_of::<Domain>() == mem::size_of::<u32>());

type Identity<Tag, Raw> = marker::PhantomData<(Tag, Raw, fn(Tag) -> Tag)>;
