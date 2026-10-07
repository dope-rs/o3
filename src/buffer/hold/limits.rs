use std::mem;

/// Resident byte limit proven by an exact capacity calculation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Bound(usize);

/// Exact capacity operation and operands that overflowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    Scale { logical: usize, factor: usize },
    Add { left: Bound, right: usize },
    Multiply { left: Bound, right: usize },
}

impl Bound {
    pub const fn new(bytes: usize) -> Self {
        Self(bytes)
    }

    pub const fn bytes(&self) -> usize {
        self.0
    }

    pub const fn max(self, other: usize) -> Self {
        if self.0 < other { Self(other) } else { self }
    }

    pub const fn try_add(self, right: usize) -> Result<Self, Error> {
        match self.0.checked_add(right) {
            Some(bytes) => Ok(Self(bytes)),
            None => Err(Error::Add { left: self, right }),
        }
    }

    pub const fn try_mul(self, right: usize) -> Result<Self, Error> {
        match self.0.checked_mul(right) {
            Some(bytes) => Ok(Self(bytes)),
            None => Err(Error::Multiply { left: self, right }),
        }
    }
}

const _: () = assert!(mem::size_of::<Bound>() == mem::size_of::<usize>());
const _: () = assert!(mem::size_of::<Error>() == 3 * mem::size_of::<usize>());
