use std::{marker, num};

pub use cursor::{Cursor, fill::Fill, limit::Limit};
pub use growing::Growing;
use layout::NonEmpty;
pub(in crate::buffer) use layout::Tally;
pub use layout::{Layout, fixed::Fixed, width::Width};
pub use span::{
    Span, append::Append, complete::Complete, copy::Copy, frozen::Frozen, lease::Lease,
};

use crate::{
    buffer,
    buffer::{resident, write::consume},
};

pub mod borrowed;
mod growing;
mod layout;
pub mod owners;
mod raw;
pub mod state;

#[repr(transparent)]
pub struct Pool<S: state::State = state::Uninit, K: crate::buffer::raw::Backing = owners::Plain> {
    core: owners::Ptr<K, S>,
}

impl<S: state::State> Pool<S, owners::Plain<Layout>> {
    pub fn try_new(slots: usize, cap: usize) -> Result<Self, CreateError> {
        let layout = Layout::new(slots, cap).map_err(CreateError::Layout)?;
        Self::try_from(layout).map_err(CreateError::Exhausted)
    }
}

impl<L: Into<Layout>, S: state::State> Pool<S, owners::Plain<L>> {
    pub fn try_from(layout: L) -> Result<Self, crate::alloc::Exhausted> {
        Ok(Self {
            core: owners::Block::<owners::Plain<_>, S>::try_new(layout)?,
        })
    }
}

impl<const CAPACITY: usize> Pool<state::Initialized, owners::Plain<Width<CAPACITY>>> {
    /// Copies fixed-width source arrays into one newly acquired complete slot.
    pub fn copy<const PARTS: usize, const WIDTH: usize>(
        &self,
        sources: [&[u8; WIDTH]; PARTS],
    ) -> Option<Span<owners::Plain<Width<CAPACITY>>, Owned, state::Initialized>> {
        const { assert!(PARTS * WIDTH == CAPACITY, "width") }
        let span = self.acquire()?;
        Some(buffer::raw::Copy::copy(CopyIn {
            span,
            sources: sources.map(|source| source.as_slice()),
            len: CAPACITY,
        }))
    }
}

impl<S: state::State, K: crate::buffer::raw::Backing> Pool<S, K> {
    pub fn acquire(&self) -> Option<Span<K, Owned, S>> {
        Span::acquire(self.core)
    }

    /// Acquires a slot whose lifetime is tied to this pool borrow.
    /// Like [`acquire`](Self::acquire), the active slot keeps its allocation alive.
    pub fn borrow(&self) -> Option<Span<K, Borrowed<'_>, S>> {
        Span::borrow(&self.core)
    }

    pub fn capacity(&self) -> num::NonZero<u32> {
        self.core.capacity()
    }

    pub fn slots(&self) -> usize {
        self.core.slots()
    }

    pub fn available(&self) -> usize {
        self.core.available()
    }
}

impl<L> Pool<state::Uninit, owners::Plain<L>> {
    /// Copies one complete slice into a newly acquired slot.
    pub fn try_copy(
        &self,
        bytes: &[u8],
    ) -> Result<Option<Span<owners::Plain<L>, Owned, Frozen>>, consume::Error> {
        consume::Error::check(bytes.len(), self.capacity().get() as usize)?;
        let Some(span) = self.acquire() else {
            return Ok(None);
        };
        let span = buffer::raw::Copy::copy(CopyIn {
            span,
            sources: [bytes],
            len: bytes.len(),
        });
        Ok(Some(span.freeze()))
    }

    #[must_use]
    pub fn cursor(&self) -> Option<Cursor<L>> {
        self.acquire().map(Cursor::from)
    }

    #[must_use]
    pub fn view(&self) -> Option<Cursor<L, Borrowed<'_>>> {
        self.borrow().map(Cursor::from)
    }
}

impl<S: state::State, K: crate::buffer::raw::Backing> Drop for Pool<S, K> {
    fn drop(&mut self) {
        owners::Header::release(self.core);
    }
}

impl<'d> Pool<state::Uninit, owners::Resident<'d>> {
    pub(in crate::buffer) fn try_account(
        tally: Tally,
        budget: resident::Budget<'d>,
    ) -> Result<Self, resident::AllocError> {
        let core = owners::Block::<owners::Accounted, state::Uninit>::try_new(tally, budget)?;
        Ok(Self { core })
    }
}

const _: () =
    assert!(size_of::<Pool<state::Uninit, owners::Resident<'static>>>() == size_of::<usize>());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Capacity(consume::Error),
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrowingError {
    Warm {
        requested: num::NonZero<u32>,
        maximum: u32,
    },
    Exhausted(crate::alloc::Exhausted),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateError {
    Layout(LayoutError),
    Exhausted(crate::alloc::Exhausted),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    ZeroCapacity,
    SlotsTooLarge,
    CapacityTooLarge,
    PlainDataArray {
        slots: u32,
        capacity: u32,
    },
    PlainData {
        slots: u32,
        capacity: u32,
    },
    AccountedData {
        slots: u32,
        capacity: u32,
        tally: u32,
    },
}

const _: () = {
    assert!(size_of::<Pool>() == size_of::<usize>());
    assert!(size_of::<Pool<state::Uninit, owners::Plain<Fixed<1, 1>>>>() == size_of::<usize>());
    assert!(size_of::<Pool<state::Uninit, owners::Plain<Width<1>>>>() == size_of::<usize>());
    assert!(size_of::<LayoutError>() == 16);
};

/// One slot edge that independently retains its pool allocation.
#[derive(Clone, Copy, Debug)]
pub struct Owned;

/// One slot edge whose allocation remains live through an exact pool borrow.
#[derive(Clone, Copy, Debug)]
pub struct Borrowed<'pool>(pub(in crate::buffer::pool) marker::PhantomData<&'pool ()>);

mod cursor;

mod span;

struct CopyIn<K: crate::buffer::raw::Backing, M, S, I> {
    span: Span<K, M, S>,
    sources: I,
    len: usize,
}

// SAFETY: fresh-slot admission proves all sources fit this retained exclusive span.
unsafe impl<K: crate::buffer::raw::Backing, M, S, I: buffer::raw::SliceExt> buffer::raw::Copy
    for CopyIn<K, M, S, I>
{
    type Output = Span<K, M, S>;

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        (&self.sources, self.span.entry().data(), 0)
    }

    unsafe fn publish(mut self) -> Self::Output {
        self.span.len = self.len as u32;
        self.span
    }
}
