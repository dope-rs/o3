pub use owned::{
    Owned,
    bytes::Bytes,
    exact::Exact,
    nonempty::{NonEmpty, Zero},
};
pub use shared::{Shared, str::Str};

use crate::buffer::write::consume;

pub(in crate::buffer) mod alloc;
pub mod backing;
pub mod inline;
mod owned;
pub mod provided;
pub mod range;
pub mod regions;

/// Failure to represent or allocate one storage owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocError {
    Capacity(consume::Error),
    Exhausted(crate::alloc::Exhausted),
}

/// Failure to construct an [`Owned`] buffer within a caller-supplied capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError<E> {
    /// The requested length exceeds the buffer representation.
    Capacity(consume::Error),
    /// The allocator could not materialize the validated capacity.
    Exhausted(crate::alloc::Exhausted),
    /// The encoder returned an error.
    Build(E),
    /// The encoder completed without initializing the exact span.
    Length(consume::Error),
}

const _: () = assert!(size_of::<BuildError<consume::Error>>() == 3 * size_of::<usize>());

impl<E> From<AllocError> for BuildError<E> {
    fn from(error: AllocError) -> Self {
        match error {
            AllocError::Capacity(error) => Self::Capacity(error),
            AllocError::Exhausted(error) => Self::Exhausted(error),
        }
    }
}

mod shared;
