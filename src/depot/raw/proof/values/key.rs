use crate::depot::{
    arena::slab::{external, key::Handle},
    index::Stored,
};

// SAFETY: Parts validates the nonzero generation and retains the exact u64 identity.
unsafe impl<Tag, const MAX: u32> crate::depot::raw::Key for Handle<Tag, MAX> {
    type Raw = u64;

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
}

// SAFETY: copies retain the complete logical identity and its numeric index.
unsafe impl<G: external::Generation, Tag> crate::depot::raw::Address for external::Key<G, Tag> {
    fn index(self) -> usize {
        self.index() as usize
    }
}
