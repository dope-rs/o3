use std::marker;

use crate::depot::arena::slab;

/// An exclusive logical identity projection of an initialized native slot.
pub struct Occupied<B, Tag = (), const ROOT: bool = true> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<B, Tag, const ROOT: bool> From<slab::Occupied<B, Tag, ROOT>> for Occupied<B, Tag, ROOT> {
    fn from(entry: slab::Occupied<B, Tag, ROOT>) -> Self {
        Self {
            entry: entry.entry,
            tag: entry.tag,
        }
    }
}

impl<B, Tag, const ROOT: bool> Occupied<B, Tag, ROOT> {
    pub(in crate::depot::arena::slab) fn into_native(self) -> slab::Occupied<B, Tag, ROOT> {
        slab::Occupied {
            entry: self.entry,
            tag: self.tag,
        }
    }
}
