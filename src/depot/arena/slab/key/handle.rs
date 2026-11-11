use std::{marker, num::NonZero};

use super::{Generation, Parts};
use crate::depot::{index::Stored, queue::links};

#[derive(Debug)]
#[repr(transparent)]
pub struct Handle<Tag, const MAX: u32, const COMPACT: bool> {
    parts: Parts<MAX>,
    marker: marker::PhantomData<*mut Tag>,
}

impl<Tag, const MAX: u32> Handle<Tag, MAX, false> {
    pub(crate) const fn new(index: u32, epoch: Generation<MAX>) -> Self {
        Self::from_parts(Parts::new(index, epoch))
    }

    #[doc(hidden)]
    pub const fn from_parts(parts: Parts<MAX>) -> Self {
        Self {
            parts,
            marker: marker::PhantomData,
        }
    }

    pub(in crate::depot) fn from_stored(raw: Stored<Self>) -> Self {
        // SAFETY: Stored retains a complete identity emitted by this Handle domain.
        let raw = unsafe { NonZero::<u64>::new_unchecked(raw.into_raw()) };
        Self::from_parts(Parts { raw })
    }

    pub const fn parts(self) -> Parts<MAX> {
        self.parts
    }
}

impl<Tag, const MAX: u32> From<Handle<Tag, MAX, false>> for Parts<MAX> {
    fn from(key: Handle<Tag, MAX, false>) -> Self {
        key.parts
    }
}

impl<Tag, const MAX: u32> Handle<Tag, MAX, true> {
    pub(in crate::depot) const fn new(index: links::Index, epoch: Generation<MAX>) -> Self {
        Self {
            parts: Parts::new(index.raw(), epoch),
            marker: marker::PhantomData,
        }
    }
}

impl<Tag, const MAX: u32, const COMPACT: bool> Handle<Tag, MAX, COMPACT> {
    pub const fn index(self) -> u32 {
        self.parts.index()
    }

    pub const fn generation(self) -> Generation<MAX> {
        self.parts.generation()
    }
}

impl<Tag, const MAX: u32, const COMPACT: bool> Clone for Handle<Tag, MAX, COMPACT> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, const MAX: u32, const COMPACT: bool> Copy for Handle<Tag, MAX, COMPACT> {}

impl<Tag, const MAX: u32, const COMPACT: bool> PartialEq for Handle<Tag, MAX, COMPACT> {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl<Tag, const MAX: u32, const COMPACT: bool> Eq for Handle<Tag, MAX, COMPACT> {}
