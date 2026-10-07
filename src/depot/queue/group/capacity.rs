use crate::depot::capacity;

/// A requested count outside this tagged-lane index domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<const LANES: usize> {
    Domain(capacity::Error),
    Tagged { requested: usize },
}

impl<const LANES: usize> Error<LANES> {
    pub const fn requested(self) -> usize {
        match self {
            Self::Domain(error) => error.requested(),
            Self::Tagged { requested: value } => value,
        }
    }
}

/// A count representable by this fixed number of tagged lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Count<const LANES: usize>(capacity::Count);

impl Count<1> {
    pub const fn new(count: capacity::Count) -> Self {
        Self(count)
    }
}

impl<const LANES: usize> Count<LANES> {
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

// SAFETY: every copy retains the immutable count and its original tagged-lane bound.
unsafe impl<const LANES: usize> crate::depot::raw::Count for Count<LANES> {
    const MAX: u32 = u32::MAX.saturating_sub(LANES.saturating_sub(1) as u32);

    fn count(self) -> capacity::Count {
        self.0
    }
}

impl<const LANES: usize> TryFrom<usize> for Count<LANES> {
    type Error = Error<LANES>;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        let count = capacity::Count::try_from(value).map_err(Self::Error::Domain)?;
        Self::try_from(count)
    }
}

impl<const LANES: usize> TryFrom<capacity::Count> for Count<LANES> {
    type Error = Error<LANES>;

    fn try_from(count: capacity::Count) -> Result<Self, Self::Error> {
        let value = count.get();
        if LANES > u8::MAX as usize
            || u64::from(count.raw()) + LANES as u64 > u64::from(u32::MAX) + 1
        {
            return Err(Self::Error::Tagged { requested: value });
        }
        Ok(Self(count))
    }
}
