use crate::depot::capacity;

/// An index outside the original queue's retained capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error<C = capacity::Count> {
    pub index: usize,
    pub capacity: C,
}
