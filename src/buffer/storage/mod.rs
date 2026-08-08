use std::{error::Error, fmt};

use crate::buffer;

pub mod inline;
mod owned;
pub(in crate::buffer) mod raw;
pub mod shared;

pub use owned::Owned;

/// Failure to construct an exact-length [`Owned`] buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError<E> {
    /// The requested length exceeds the buffer representation.
    Capacity(buffer::CapacityError),
    /// The encoder returned an error.
    Build(E),
    /// The encoder completed without initializing the requested number of bytes.
    LengthMismatch { expected: usize, actual: usize },
}

impl<E: fmt::Display> fmt::Display for BuildError<E> {
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

impl<E> Error for BuildError<E>
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
