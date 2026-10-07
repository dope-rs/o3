/// No available slot remains in this exact pool capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full {
    pub(super) capacity: usize,
}

impl Full {
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
}
