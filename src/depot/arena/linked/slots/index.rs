use std::{marker, mem};

/// Compact position in one linked-slot domain.
#[repr(transparent)]
#[derive(Debug)]
pub struct Index<Tag = ()> {
    raw: u32,
    domain: marker::PhantomData<fn(Tag) -> Tag>,
}

impl<Tag> Index<Tag> {
    /// The reserved absent link: every other value preserves its domain tag.
    pub(crate) const NONE: Self = Self::new(u32::MAX);

    pub(crate) const fn new(raw: u32) -> Self {
        Self {
            raw,
            domain: marker::PhantomData,
        }
    }

    pub(crate) const fn get(self) -> Option<u32> {
        if self.raw == u32::MAX {
            None
        } else {
            Some(self.raw)
        }
    }

    pub(crate) const fn raw(self) -> u32 {
        self.raw
    }
}

impl<Tag> Clone for Index<Tag> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag> Copy for Index<Tag> {}

impl<Tag> PartialEq for Index<Tag> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<Tag> Eq for Index<Tag> {}

const _: () = {
    assert!(mem::size_of::<Index>() == mem::size_of::<u32>());
    assert!(mem::align_of::<Index>() == mem::align_of::<u32>());
};
