use crate::buffer::{segments, segments::assembly};

/// Exact overflow while combining two unchanged segment sequences.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Segment cardinality exceeded the metadata index domain.
    Count(assembly::Overflow<segments::Count>),
    /// Logical byte length exceeded the address domain.
    Logical(assembly::Overflow),
    /// Resident byte length exceeded the address domain.
    Resident(assembly::Overflow),
}

impl Error {
    pub(in crate::buffer::segments) fn count(
        current: segments::Count,
        added: segments::Count,
    ) -> Result<segments::Count, Self> {
        current
            .checked_add(added)
            .ok_or(Self::Count(assembly::Overflow::new(current, added)))
    }

    pub(in crate::buffer::segments) fn logical(
        current: usize,
        added: usize,
    ) -> Result<usize, Self> {
        current
            .checked_add(added)
            .ok_or(Self::Logical(assembly::Overflow::new(current, added)))
    }

    pub(in crate::buffer::segments) fn resident(
        current: usize,
        added: usize,
    ) -> Result<usize, Self> {
        current
            .checked_add(added)
            .ok_or(Self::Resident(assembly::Overflow::new(current, added)))
    }
}
