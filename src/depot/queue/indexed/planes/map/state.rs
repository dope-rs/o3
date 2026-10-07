use crate::depot::queue::indexed::planes::{map::Slot, phase};

/// One installed record's exclusive membership in a mutable plane.
pub enum State<'a, K, V, const N: usize, const P: usize> {
    Linked(Slot<'a, K, V, N, phase::Linked<P>>),
    Vacant(Slot<'a, K, V, N, phase::Vacant<P>>),
}

impl<'a, K, V, const N: usize, const P: usize> State<'a, K, V, N, P> {
    pub(in crate::depot::queue::indexed::planes::map) fn new(slot: Slot<'a, K, V, N>) -> Self {
        if slot.is_linked::<P>() {
            Self::Linked(slot.into_phase())
        } else {
            Self::Vacant(slot.into_phase())
        }
    }
    pub fn link_back(self) -> Slot<'a, K, V, N, phase::Linked<P>> {
        match self {
            Self::Linked(slot) => slot,
            Self::Vacant(slot) => slot.link_back(),
        }
    }
    pub fn unlink(self) -> Slot<'a, K, V, N, phase::Vacant<P>> {
        match self {
            Self::Linked(slot) => slot.unlink(),
            Self::Vacant(slot) => slot,
        }
    }
}
