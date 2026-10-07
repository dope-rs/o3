use crate::depot::capacity;

/// An index outside the exact heap domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error<C = capacity::Count> {
    pub(in crate::depot::heap::indexed) index: usize,
    pub(in crate::depot::heap::indexed) capacity: C,
}

impl<C: Copy> Error<C> {
    pub const fn index(&self) -> usize {
        self.index
    }

    pub const fn capacity(&self) -> C {
        self.capacity
    }
}
