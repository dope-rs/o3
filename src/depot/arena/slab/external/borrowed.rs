use std::marker;

use crate::depot::arena::slab;

/// A logical identity projection of its native borrowed phase.
pub struct Borrowed<B, Tag = (), const ROOT: bool = true> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<B, Tag, const ROOT: bool> Borrowed<B, Tag, ROOT> {
    pub(in crate::depot::arena::slab) fn into_native(self) -> slab::Borrowed<B, Tag, ROOT> {
        slab::Borrowed {
            entry: self.entry,
            tag: self.tag,
        }
    }
}

const _: () = {
    type Slot =
        Borrowed<slab::entries::Busy<'static, u64, { u32::MAX }, slab::entries::Interior<true>>>;
    assert!(size_of::<Slot>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Slot>>() == size_of::<Slot>());
};
