//! Fixed metadata chains for independently retained byte owners.
//! Detached chains own their pool core; scoped chains borrow it. Neither form
//! copies payload bytes.

use std::{mem, num::NonZero};

use assembly::total;
pub(in crate::buffer::segments) use chain::edit::Consume;
pub use chain::{
    Chain,
    edit::drain::Drain,
    front::Front,
    many::{Many, flatten::Flatten},
    sequence::Sequence,
    single::Single,
    view::{cursor::Cursor, iter::Iter},
};
use handles::Key;
pub(in crate::buffer) use handles::pool::exact::Built;
pub use handles::{
    handle::Handle,
    header::Header,
    pool::{Pool, exact::Exact},
};
pub use item::Item;
pub(in crate::buffer) use item::seal::Seal;
pub use shape::{Shape, nonempty::Nonempty, owners::Owners};

use crate::{buffer, buffer::write::consume, depot::capacity, mem::rc};

pub mod assembly;
pub mod queue;

pub mod scatter;
mod shape;

/// A fixed metadata pool cannot admit the requested transition.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// No segment slot is available.
    SegmentCapacity(capacity::Shortage),
    /// No multi-chain descriptor is available.
    ChainCapacity(capacity::Shortage),
    /// The handles do not belong to the same pool.
    DifferentPool,
    /// Segment cardinality exceeded the metadata index domain.
    Count(assembly::Overflow<Count>),
    /// Logical byte length exceeded the address domain.
    Logical(assembly::Overflow),
    /// Resident byte length exceeded the address domain.
    Resident(assembly::Overflow),
    /// Combined unchanged-sequence accounting exceeded its exact domain.
    Total(total::Error),
    /// The requested byte prefix exceeds the source length.
    Boundary(consume::Error),
    /// A payload or metadata owner has no remaining reference capacity.
    Retain(rc::Error),
}

impl Error {
    pub(in crate::buffer::segments) const fn component(error: total::Error) -> Self {
        match error {
            total::Error::Count(error) => Self::Count(error),
            total::Error::Logical(error) => Self::Logical(error),
            total::Error::Resident(error) => Self::Resident(error),
        }
    }
}

impl From<total::Error> for Error {
    fn from(error: total::Error) -> Self {
        Self::Total(error)
    }
}

impl<T: buffer::raw::Segment> From<crate::mem::Rejected<Item<T>, Error>>
    for crate::mem::Rejected<T, Error>
{
    fn from(rejected: crate::mem::Rejected<Item<T>, Error>) -> Self {
        let (item, error) = rejected.into_parts();
        Self::new(item.into_inner(), error)
    }
}

/// Independent capacities for segment nodes and simultaneous multi-chains.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Plan {
    pub segments: usize,
    pub chains: usize,
}

impl Plan {
    pub(crate) const fn max_segments() -> usize {
        u32::MAX as usize
    }
}

const _: () = {
    assert!(mem::size_of::<Error>() == 4 * mem::size_of::<usize>());
    assert!(mem::size_of::<Plan>() == 2 * mem::size_of::<usize>());
};

mod chain;
mod handles;
mod item;

/// Exact segment cardinality in the metadata index domain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Count(u32);

/// Zero-sized proof of one-segment cardinality.
#[derive(Clone, Copy, Debug)]
pub struct One(());

impl Default for One {
    fn default() -> Self {
        Self::new()
    }
}

impl One {
    pub const fn new() -> Self {
        Self(())
    }

    pub const fn count(&self) -> Count {
        Count::ONE
    }
}

#[repr(transparent)]
pub(in crate::buffer::segments) struct Plural(NonZero<u32>);

impl Plural {
    pub(in crate::buffer::segments) const fn raw(&self) -> u32 {
        self.0.get()
    }

    pub(in crate::buffer::segments) const fn nonzero(&self) -> NonZero<u32> {
        self.0
    }

    pub(in crate::buffer::segments) const fn tail(&self) -> NonZero<u32> {
        // SAFETY: every Plural constructor proves a value of at least two.
        unsafe { NonZero::new_unchecked(self.raw().wrapping_sub(1)) }
    }
}

pub(in crate::buffer::segments) enum Kind {
    Empty,
    One,
    Many(Plural),
}

impl Count {
    pub const EMPTY: Self = Self(0);
    pub const ONE: Self = Self(1);
    pub const TWO: Self = Self(2);

    #[must_use]
    pub const fn new(count: u32) -> Self {
        Self(count)
    }

    #[must_use]
    pub const fn into_raw(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn is_many(self) -> bool {
        self.0 >= 2
    }

    #[must_use]
    pub fn checked_add(self, added: Self) -> Option<Self> {
        self.0.checked_add(added.0).map(Self)
    }

    #[must_use]
    pub fn checked_sub(self, removed: Self) -> Option<Self> {
        self.0.checked_sub(removed.0).map(Self)
    }

    pub(in crate::buffer::segments) const fn kind(self) -> Kind {
        match self.0 {
            0 => Kind::Empty,
            1 => Kind::One,
            // SAFETY: the prior arms exclude zero and one.
            count => Kind::Many(Plural(unsafe { NonZero::new_unchecked(count) })),
        }
    }
}

const _: () = {
    assert!(mem::size_of::<Count>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Plural>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Chain<(), Pool<()>>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Chain<(), &'static Header<()>>>() == 2 * mem::size_of::<usize>());
};

mod tail;
pub(in crate::buffer) use tail::Tail;

/// Classification of one complete owner admitted by [`Chain::try_push`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Push<T> {
    /// The owner became one new segment.
    Loaded,
    /// The owner was empty, contributed no segment, and is returned unchanged.
    Discarded(T),
}
