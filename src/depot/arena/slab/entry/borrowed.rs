use std::marker;

use crate::depot::arena::slab::external;

/// An initialized interior slot retaining its reentrancy exclusion.
pub struct Borrowed<B, Tag = (), const ROOT: bool = true> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<B, Tag, const ROOT: bool> Borrowed<B, Tag, ROOT> {
    pub(in crate::depot::arena::slab) fn into_external(self) -> external::Borrowed<B, Tag, ROOT> {
        external::Borrowed {
            entry: self.entry,
            tag: self.tag,
        }
    }
}
