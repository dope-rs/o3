use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CapacityError {
    attempted: usize,
    capacity: usize,
}

impl CapacityError {
    pub(crate) const fn new(attempted: usize, capacity: usize) -> Self {
        Self {
            attempted,
            capacity,
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

/// Failure to construct an exact-length [`Owned`] buffer.
///
/// [`Owned`]: crate::buffer::Owned
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExactBuildError<E> {
    /// The requested length exceeds the buffer representation.
    Capacity(CapacityError),
    /// The encoder returned an error.
    Build(E),
    /// The encoder completed without initializing the requested number of bytes.
    LengthMismatch { expected: usize, actual: usize },
}

impl<E: fmt::Display> fmt::Display for ExactBuildError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity(error) => error.fmt(f),
            Self::Build(error) => error.fmt(f),
            Self::LengthMismatch { expected, actual } => {
                write!(
                    f,
                    "exact buffer length mismatch: expected {expected}, wrote {actual}"
                )
            }
        }
    }
}

impl<E> Error for ExactBuildError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Capacity(error) => Some(error),
            Self::Build(error) => Some(error),
            Self::LengthMismatch { .. } => None,
        }
    }
}
