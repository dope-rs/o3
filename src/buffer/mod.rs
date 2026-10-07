use std::{convert, num, ops};

use storage::{provided, range};
use write::consume;

use crate::{
    buffer::{pool::owners, storage::regions},
    mem::rc,
};

pub mod raw;

pub mod bytes;
pub mod hold;
pub mod pool;
pub mod resident;
pub mod segments;
pub mod storage;
pub mod write;

/// A range whose bounds were validated against one immutably borrowed source.
/// The source borrow prevents mutation while the view is inspected or retained.
/// Inspection and narrowing do not allocate, copy, or retain an owner.
#[must_use]
#[repr(C)]
pub struct View<'source, S: ?Sized> {
    source: &'source S,
    bytes: &'source [u8],
}

impl Copy for View<'_, storage::Shared> {}

impl Clone for View<'_, storage::Shared> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'source, S: ?Sized> View<'source, S> {
    pub const fn reborrow(&self) -> View<'source, S> {
        View {
            source: self.source,
            bytes: self.bytes,
        }
    }

    pub fn as_slice(&self) -> &'source [u8] {
        self.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn select(&self, bytes: &[u8]) -> Option<View<'source, S>> {
        let range: ops::Range<usize> = self.bytes.subslice_range(bytes)?.into();
        // SAFETY: containment proved this exact range of the retained source;
        // reconstructing its borrow preserves the source lifetime and provenance.
        Some(Self {
            source: self.source,
            bytes: unsafe { self.bytes.get_unchecked(range) },
        })
    }

    pub fn get(&self, range: ops::Range<usize>) -> Option<View<'source, S>> {
        let bytes = self.bytes.get(range)?;
        Some(View {
            source: self.source,
            bytes,
        })
    }

    pub fn try_get(&self, range: ops::Range<usize>) -> Result<Self, range::Error> {
        let bytes = self.bytes.try_get(range)?;
        Ok(Self {
            source: self.source,
            bytes,
        })
    }

    /// Borrows the complete initialized source without retaining or copying it.
    pub fn new(source: &'source S) -> Self
    where
        S: AsRef<[u8]>,
    {
        Self {
            source,
            bytes: source.as_ref(),
        }
    }

    const fn source(&self) -> &'source S {
        self.source
    }
}

impl<S: segments::Seal> View<'_, S> {
    /// Retains this already-validated range without allocating or copying.
    pub fn try_retain(self) -> Result<S, rc::Error> {
        use storage::regions::NonEmpty;

        let Some(view) = NonEmpty::new(&self) else {
            return Ok(S::EMPTY);
        };
        S::try_share(view).map(segments::Item::into_inner)
    }
}

impl View<'_, resident::Shared<'_>> {
    /// Retains this validated range when its existing owner is independent.
    /// A scoped owner returns `None` without retaining or copying its bytes.
    pub fn try_own(self) -> Result<Option<storage::Shared>, rc::Error> {
        use crate::buffer::raw::Retain as _;

        if self.is_empty() {
            return Ok(Some(storage::Shared::new()));
        }
        let owner = match self.source().region.owner() {
            resident::State::Owned(owner) => owner.try_retain()?,
            resident::State::Borrowed(owner) => Some(owner.try_clone()?.into_owned()),
            resident::State::Slice
            | resident::State::Resident(_)
            | resident::State::Accounted(_)
            | resident::State::Provided(_) => return Ok(None),
        };
        // SAFETY: this view retains the exact source borrow and validated slice;
        // the selected independent edge keeps that same immutable extent live.
        Ok(Some(unsafe {
            storage::Shared::assume(self.as_slice(), owner)
        }))
    }
}

impl<L> View<'_, storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'_>>>> {
    /// Retains this range with an independent owner of its pool allocation.
    pub fn try_own(self) -> Result<storage::Shared, rc::Error> {
        let Some(view) = regions::NonEmpty::new(&self) else {
            return Ok(storage::Shared::new());
        };
        view.try_own()
    }
}

const _: () = assert!(size_of::<View<'static, storage::Shared>>() == 3 * size_of::<usize>());

impl<S: ?Sized> consume::Consume for View<'_, S> {
    fn remaining(&self) -> usize {
        self.bytes.len()
    }
    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        raw::Selection::try_new(self, amount).map(raw::Selection::consume)
    }

    fn consume_max(&mut self, max: usize) -> usize {
        raw::Store::advance_max(self, max)
    }
}

impl<S: ?Sized> AsRef<[u8]> for View<'_, S> {
    fn as_ref(&self) -> &[u8] {
        self.bytes
    }
}

impl<'input, S: ?Sized> Read<'input> for View<'input, S> {
    fn as_slice(&self) -> &'input [u8] {
        self.bytes
    }

    fn try_split<const N: usize>(mut self) -> Result<(&'input [u8; N], Self), consume::Error> {
        let (head, tail) = Read::try_split(self.bytes)?;
        self.bytes = tail;
        Ok((head, self))
    }
}

/// Payload backing capacity retained by this owner, excluding metadata and padding.
/// Borrowed and static bytes retain zero bytes.
pub trait Residency {
    fn resident(&self) -> usize;
}

impl<L> Residency for pool::Span<owners::Plain<L>, pool::Owned, pool::Frozen> {
    fn resident(&self) -> usize {
        self.capacity().get() as usize
    }
}

impl<L> Residency for pool::Cursor<L> {
    fn resident(&self) -> usize {
        self.len() + self.spare_len()
    }
}

impl Residency for Vec<u8> {
    fn resident(&self) -> usize {
        self.capacity()
    }
}

impl Residency for &[u8] {
    /// A borrowed slice retains no complete backing allocation of its own.
    fn resident(&self) -> usize {
        0
    }
}

pub const BLOCK_CAPACITY: u32 = 64 * 1024;

fn is_within(range: &ops::Range<usize>, len: usize) -> bool {
    range.start <= range.end && range.end <= len
}

/// One validated byte range retaining its original source borrow.
pub trait Read<'input>: Sized {
    fn as_slice(&self) -> &'input [u8];
    fn try_split<const N: usize>(self) -> Result<(&'input [u8; N], Self), consume::Error>;
}

impl<'input> Read<'input> for convert::Infallible {
    fn as_slice(&self) -> &'input [u8] {
        match *self {}
    }

    fn try_split<const N: usize>(self) -> Result<(&'input [u8; N], Self), consume::Error> {
        match self {}
    }
}

impl<'input> Read<'input> for &'input [u8] {
    fn as_slice(&self) -> &'input [u8] {
        self
    }

    fn try_split<const N: usize>(self) -> Result<(&'input [u8; N], Self), consume::Error> {
        self.split_first_chunk::<N>().ok_or_else(|| {
            consume::Error::new(
                num::NonZero::<usize>::MIN.saturating_add(N.saturating_sub(1)),
                self.len(),
            )
        })
    }
}

pub trait Source: Unpin {
    type View<'input>: Read<'input>
    where
        Self: 'input;

    fn remaining(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.remaining() == 0
    }
}

pub trait Cursor: Source + AsRef<[u8]> {
    /// Validates a range once and retains its original source borrow.
    fn try_get<'input>(
        &'input self,
        range: ops::Range<usize>,
    ) -> Result<Self::View<'input>, range::Error>;
}

impl Source for [u8] {
    type View<'input> = &'input [u8];

    fn remaining(&self) -> usize {
        self.len()
    }
}

impl Cursor for [u8] {
    fn try_get<'input>(
        &'input self,
        range: ops::Range<usize>,
    ) -> Result<Self::View<'input>, range::Error> {
        self.get(range.clone())
            .ok_or_else(|| range::Error::new(range, self.len()))
    }
}

pub trait Complete: Cursor {
    fn view(&self) -> Self::View<'_>;
}

impl<O: Unpin> Source for storage::Shared<O> {
    type View<'input>
        = View<'input, Self>
    where
        Self: 'input;
    fn remaining(&self) -> usize {
        self.region.len()
    }
}
impl<O: Unpin> Cursor for storage::Shared<O> {
    fn try_get<'input>(
        &'input self,
        range: ops::Range<usize>,
    ) -> Result<Self::View<'input>, range::Error> {
        self.view().try_get(range)
    }
}
impl<O: Unpin> Complete for storage::Shared<O> {
    fn view(&self) -> Self::View<'_> {
        View::new(self)
    }
}

impl<'d> View<'_, storage::Shared<Option<provided::Account<'d>>>> {
    /// Retains the proved extent using its existing registration, without admission.
    pub fn try_retain(self) -> Result<storage::Shared<Option<provided::Account<'d>>>, rc::Error> {
        if self.is_empty() {
            return Ok(storage::Shared::<Option<provided::Account<'d>>>::new());
        }
        self.try_clone()
    }
}
const _: () = {
    assert!(
        size_of::<storage::Shared<Option<provided::Account<'static>>>>() == 3 * size_of::<usize>()
    );
    assert!(
        size_of::<Option<storage::Shared<Option<provided::Account<'static>>>>>()
            == 3 * size_of::<usize>()
    );
};
