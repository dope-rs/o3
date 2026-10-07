//! Lifetime-bound identities for keyed storage tests.

use std::{marker, num};

use o3::{depot, depot::index::Stored};

/// A nonzero identity with a 16-bit slot index and 48-bit generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Id<'d>(num::NonZero<u64>, marker::PhantomData<fn(&'d ()) -> &'d ()>);

/// The component rejected by the identity domain.
#[derive(Debug)]
pub enum Error {
    Index(usize),
    Generation(u64),
}

impl Id<'_> {
    /// Packs one bounded slot index and logical generation into an identity.
    pub fn try_new(index: usize, epoch: u64) -> Result<Self, Error> {
        if index > u16::MAX as usize {
            return Err(Error::Index(index));
        }
        if epoch == 0 || epoch >= 1_u64 << 48 {
            return Err(Error::Generation(epoch));
        }
        let raw = (epoch << 16) | index as u64;
        // SAFETY: the checked generation is nonzero and occupies bits 16..64.
        Ok(Self(
            unsafe { num::NonZero::<u64>::new_unchecked(raw) },
            marker::PhantomData,
        ))
    }
}

// SAFETY: `try_new` reserves zero, confines the index to the low 16 bits, and
// preserves the remaining nonzero bits as the logical generation. Callers of
// decoding maps only the reserved raw identity to vacancy.
unsafe impl depot::raw::Key for Id<'_> {
    type Raw = u64;
    type Live = Self;

    const VACANT: Self::Raw = 0;

    fn index(self) -> usize {
        self.0.get() as u16 as usize
    }

    fn into_raw(self) -> Self::Raw {
        self.0.get()
    }

    fn from_raw(raw: Stored<Self>) -> Self {
        // SAFETY: Stored proves this raw was emitted by the exact Id domain,
        // whose complete live representation is NonZero<u64>.
        Self(
            unsafe { num::NonZero::<u64>::new_unchecked(raw.into_raw()) },
            marker::PhantomData,
        )
    }

    fn decode(raw: Self::Raw) -> Option<Self> {
        num::NonZero::<u64>::new(raw).map(|raw| Self(raw, marker::PhantomData))
    }

    fn into_live(self) -> Self::Live {
        self
    }

    fn from_live(live: Self::Live) -> Self {
        live
    }
}
