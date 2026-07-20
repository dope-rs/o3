use std::error::Error;
use std::fmt;

use crate::marker::ThreadBound;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CapacityError {
    attempted: usize,
    capacity: usize,
    _thread: ThreadBound,
}

impl CapacityError {
    pub(crate) const fn new(attempted: usize, capacity: usize) -> Self {
        Self {
            attempted,
            capacity,
            _thread: ThreadBound::NEW,
        }
    }

    pub const fn attempted(self) -> usize {
        self.attempted
    }

    pub const fn capacity(self) -> usize {
        self.capacity
    }
}

impl fmt::Debug for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapacityError")
            .field("attempted", &self.attempted)
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl fmt::Display for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capacity exceeded: attempted {}, capacity {}",
            self.attempted, self.capacity
        )
    }
}

impl Error for CapacityError {}
