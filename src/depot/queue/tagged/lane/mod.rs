use crate::depot::capacity;

/// A lane outside the selected owner's immutable domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error<L = capacity::Count> {
    pub lane: usize,
    pub lanes: L,
}

const _: () = {
    assert!(size_of::<Error<std::num::NonZero<u32>>>() == size_of::<Error>());
    assert!(
        size_of::<Option<Error<std::num::NonZero<u32>>>>()
            == size_of::<Error<std::num::NonZero<u32>>>()
    );
};

mod endpoint;
pub use endpoint::Endpoint;
