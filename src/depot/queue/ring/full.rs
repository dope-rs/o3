/// The observed occupancy when an exact ring reservation was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full {
    pub(super) used: usize,
    pub(super) capacity: usize,
}

impl Full {
    pub const fn used(self) -> usize {
        self.used
    }

    pub const fn capacity(self) -> usize {
        self.capacity
    }
}
