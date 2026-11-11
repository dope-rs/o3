use std::marker;

use crate::{
    depot::arena::slab::{self, entries, external},
    mem::lease,
};

/// A logical identity projection of its native vacant phase.
pub struct Vacant<B, Tag = (), const ROOT: bool = true, S = ()> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) state: S,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<'a, T, G: external::Generation, Tag, M: entries::Mode, const N: usize>
    Vacant<entries::Pending<'a, external::Stamped<T, G>, 1, M, N>, Tag, true, G>
{
    pub fn key(&self) -> external::Key<G, Tag> {
        external::Key::new(self.entry.index().raw(), self.state)
    }

    pub fn stage(
        self,
        value: T,
    ) -> external::Staged<crate::mem::Lease<external::Busy<'a, T, G, M, N>, lease::Open>, Tag> {
        slab::Vacant::new(self.entry)
            .stage(external::Stamped {
                generation: self.state,
                value,
            })
            .into()
    }
}
