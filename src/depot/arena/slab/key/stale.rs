use crate::depot::{
    arena::slab::{Refusal, key},
    capacity,
};

/// A bounded identity whose resident state or generation no longer matches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stale<'h, Tag = (), const MAX: u32 = { u32::MAX }> {
    Free {
        key: capacity::Key<'h, key::Handle<Tag, MAX>>,
        actual: key::Generation<MAX>,
    },
    Reserved {
        key: capacity::Key<'h, key::Handle<Tag, MAX>>,
        actual: key::Generation<MAX>,
    },
    Retired {
        key: capacity::Key<'h, key::Handle<Tag, MAX>>,
        actual: key::Generation<MAX>,
    },
    Generation {
        key: capacity::Key<'h, key::Handle<Tag, MAX>>,
        actual: key::Generation<MAX>,
    },
}

impl<'h, Tag, const MAX: u32> Stale<'h, Tag, MAX> {
    pub(in crate::depot::arena::slab) fn refused(
        key: capacity::Key<'h, key::Handle<Tag, MAX>>,
        actual: key::Generation<MAX>,
        refusal: Refusal<()>,
    ) -> Self {
        match refusal {
            Refusal::Free => Self::Free { key, actual },
            Refusal::Reserved => Self::Reserved { key, actual },
            Refusal::Retired(()) => Self::Retired { key, actual },
            Refusal::Stale => Self::Generation { key, actual },
        }
    }
}

const _: () = {
    assert!(size_of::<Stale<'static>>() == size_of::<key::Error>());
    assert!(size_of::<Option<Stale<'static>>>() == size_of::<Stale<'static>>());
};
