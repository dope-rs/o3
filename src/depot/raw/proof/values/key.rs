use std::num::NonZero;

use crate::depot::{
    arena::slab::key::{Handle, Parts},
    index::Stored,
};

// SAFETY: Parts validates the nonzero generation and retains the exact u64 identity.
unsafe impl<Tag, const MAX: u32> crate::depot::raw::Key for Handle<Tag, MAX> {
    type Raw = u64;
    type Live = Self;

    const VACANT: Self::Raw = 0;

    fn index(self) -> usize {
        self.index() as usize
    }

    fn into_raw(self) -> Self::Raw {
        self.parts().into_raw().get()
    }

    fn from_raw(raw: Stored<Self>) -> Self {
        Self::from_stored(raw)
    }

    fn decode(raw: Self::Raw) -> Option<Self> {
        NonZero::<u64>::new(raw)
            .and_then(Parts::from_raw)
            .map(Self::from_parts)
    }

    fn into_live(self) -> Self::Live {
        self
    }

    fn from_live(live: Self::Live) -> Self {
        live
    }
}
