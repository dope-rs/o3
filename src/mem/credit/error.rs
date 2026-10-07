use std::num;

/// A credit request that exceeds the ledger's remaining capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error {
    need: num::NonZero<usize>,
    spare: usize,
}

impl Error {
    pub(crate) const fn new(need: num::NonZero<usize>, spare: usize) -> Self {
        Self { need, spare }
    }

    pub const fn into_parts(self) -> (num::NonZero<usize>, usize) {
        (self.need, self.spare)
    }
}

const _: () = assert!(core::mem::size_of::<Error>() == core::mem::size_of::<usize>() * 2);
const _: () =
    assert!(core::mem::size_of::<Result<(), Error>>() == core::mem::size_of::<usize>() * 2);
