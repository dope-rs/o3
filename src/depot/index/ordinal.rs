use std::{marker, mem};

/// A dense ordinal carrying its semantic domain and exact storage width.
#[repr(transparent)]
pub struct Ordinal<Tag = (), Raw = usize> {
    raw: Raw,
    tag: Identity<Tag>,
}

impl<Tag, Raw: crate::depot::raw::Word> Ordinal<Tag, Raw> {
    #[must_use]
    pub const fn new(raw: Raw) -> Self {
        Self {
            raw,
            tag: marker::PhantomData,
        }
    }

    #[must_use]
    pub const fn raw(self) -> Raw {
        self.raw
    }

    pub(in crate::depot) fn offset(self) -> usize {
        <Raw as crate::depot::raw::Word>::offset(self.raw)
    }
}

impl<Tag, Raw: crate::depot::raw::Word> Clone for Ordinal<Tag, Raw> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, Raw: crate::depot::raw::Word> Copy for Ordinal<Tag, Raw> {}

const _: () = {
    assert!(mem::size_of::<Ordinal<(), u8>>() == mem::size_of::<u8>());
    assert!(mem::size_of::<Ordinal<(), u16>>() == mem::size_of::<u16>());
    assert!(mem::size_of::<Ordinal<(), u32>>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Ordinal>() == mem::size_of::<usize>());
};

type Identity<Tag> = marker::PhantomData<(Tag, fn(Tag) -> Tag)>;
