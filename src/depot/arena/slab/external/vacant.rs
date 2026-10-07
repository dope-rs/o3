use std::marker;

use crate::depot::arena::slab;

/// A logical identity projection of its native vacant phase.
pub struct Vacant<B, Tag = (), const ROOT: bool = true, S = ()> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) state: S,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<B, Tag, const ROOT: bool, S> Vacant<B, Tag, ROOT, S> {
    pub(in crate::depot::arena::slab) fn into_native(self) -> slab::Vacant<B, Tag, ROOT> {
        slab::Vacant {
            entry: self.entry,
            tag: self.tag,
        }
    }
}
