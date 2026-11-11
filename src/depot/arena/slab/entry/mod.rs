use std::marker;

pub struct Slot<B, Tag = (), const ROOT: bool = true> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<B, Tag, const ROOT: bool> Slot<B, Tag, ROOT> {
    pub(in crate::depot::arena::slab) fn new(entry: B) -> Self {
        Self {
            entry,
            tag: marker::PhantomData,
        }
    }
}

pub(super) mod anchor;
pub(super) mod anchored;
pub(super) mod occupied;
pub(super) mod staged;
