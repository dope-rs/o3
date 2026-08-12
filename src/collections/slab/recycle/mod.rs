mod sealed;

pub use sealed::{Lease, Pool, VacantEntry};

/// Converts a leased value back into the seed used to build its replacement.
pub trait Recycle: Sized {
    type Seed;

    fn into_seed(self) -> Self::Seed;
}
