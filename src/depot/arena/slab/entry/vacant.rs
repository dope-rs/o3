use std::marker;

use crate::depot::arena::slab::external;

/// One reserved inactive slot awaiting its first initialized value.
pub struct Vacant<B, Tag = (), const ROOT: bool = true> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<B, Tag, const ROOT: bool> Vacant<B, Tag, ROOT> {
    pub(in crate::depot::arena::slab) fn with_state<S>(
        self,
        state: S,
    ) -> external::Vacant<B, Tag, ROOT, S> {
        external::Vacant {
            entry: self.entry,
            state,
            tag: self.tag,
        }
    }
}
