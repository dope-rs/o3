use std::mem;

use crate::depot::queue::indexed::planes::{Entry, phase};

/// One plane's exact membership, held exclusively until consumed.
#[must_use]
pub enum State<'a, T, const N: usize, const P: usize> {
    Linked(Entry<'a, T, N, phase::Linked<P>>),
    Vacant(Entry<'a, T, N, phase::Vacant<P>>),
}

impl<'a, T, const N: usize, const P: usize> State<'a, T, N, P> {
    pub(in crate::depot::queue::indexed::planes) fn new(entry: Entry<'a, T, N>) -> Self {
        if entry.is_linked::<P>() {
            Self::Linked(entry.into_phase())
        } else {
            Self::Vacant(entry.into_phase())
        }
    }
    pub fn link_back(self) -> Entry<'a, T, N, phase::Linked<P>> {
        match self {
            Self::Linked(entry) => entry,
            Self::Vacant(entry) => entry.link_back(),
        }
    }
    pub fn unlink(self) -> Entry<'a, T, N, phase::Vacant<P>> {
        match self {
            Self::Linked(entry) => entry.unlink(),
            Self::Vacant(entry) => entry,
        }
    }
}

const _: () = {
    assert!(
        mem::size_of::<Entry<'static, (), 3, phase::Linked<0>>>() == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Entry<'static, (), 3, phase::Vacant<0>>>() == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<Entry<'static, (), 3, phase::Linked<0>>>>()
            == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<Entry<'static, (), 3, phase::Vacant<0>>>>()
            == 2 * mem::size_of::<usize>()
    );
};
