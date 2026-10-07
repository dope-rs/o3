use std::num::NonZero;

use crate::buffer::write::spans;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    required: NonZero<usize>,
    available: usize,
}

impl Error {
    /// Classifies a nonempty request against a destination with no remaining capacity.
    pub const fn empty(required: NonZero<usize>) -> Self {
        Self::new(required, 0)
    }

    pub const fn new(required: NonZero<usize>, room: usize) -> Self {
        Self {
            required,
            available: room,
        }
    }

    pub fn check(required: usize, room: usize) -> Result<(), Self> {
        if required <= room {
            return Ok(());
        }
        Err(Self::new(
            NonZero::<usize>::MIN.saturating_add(required - 1),
            room,
        ))
    }

    pub const fn into_parts(self) -> (NonZero<usize>, usize) {
        (self.required, self.available)
    }

    pub(in crate::buffer) fn checked_end<const N: usize>(
        start: usize,
        capacity: usize,
        slices: &[&[u8]; N],
    ) -> Result<usize, Self> {
        Self::check(start, capacity)?;
        let Some((last, prefix)) = slices.split_last() else {
            return Ok(start);
        };
        let mut room = capacity - start;
        for slice in prefix {
            Self::check(slice.len(), room)?;
            room -= slice.len();
        }
        Self::check(last.len(), room)?;
        Ok(capacity - room + last.len())
    }
}

impl From<spans::Gap> for Error {
    fn from(gap: spans::Gap) -> Self {
        let (expected, actual) = gap.into_parts();
        Self::new(
            NonZero::<usize>::MIN.saturating_add(expected.get() as usize - 1),
            actual as usize,
        )
    }
}

const _: () = assert!(size_of::<Error>() == 2 * size_of::<usize>());
