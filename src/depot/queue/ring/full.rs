use crate::depot::capacity;

/// The observed occupancy when an exact ring reservation was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full {
    pub(super) capacity: capacity::Count,
}

impl Full {
    pub const fn used(self) -> usize {
        self.capacity.get()
    }

    pub const fn capacity(self) -> usize {
        self.capacity.get()
    }
}
