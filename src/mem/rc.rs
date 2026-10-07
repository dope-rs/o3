/// The fixed-width reference domain has no room for another owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Exhausted,
}

/// Result of consuming one locally reference-counted owner edge.
#[derive(Eq, PartialEq)]
#[must_use = "the last owner must run its domain finalizer"]
pub enum Release {
    /// At least one owner remains live.
    Shared,
    /// The count reached zero and the domain finalizer now owns retirement.
    Last,
}

const _: () = assert!(size_of::<Release>() == size_of::<u8>());
