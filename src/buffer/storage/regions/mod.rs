use std::{num::NonZero, ops, ptr, slice};

pub(in crate::buffer) use nonempty::NonEmpty;
pub(in crate::buffer) use prefix::Prefix;
pub(in crate::buffer) use tag::Header;
pub use tag::Tag;
pub(in crate::buffer) use value::{Complete, Init};

use crate::{
    buffer,
    buffer::{
        is_within, pool,
        pool::owners,
        resident,
        storage::{alloc, provided, range, regions},
    },
    mem::rc,
};

/// # Safety
/// The sources retain immutable bytes whose complete sum equals the capacity.
/// Their copy operation initializes that complete admitted destination exactly once.
pub(in crate::buffer) unsafe trait Initialize: Sized {
    fn parts(&self) -> (NonZero<u32>, impl crate::buffer::raw::SliceExt);

    fn try_init<P: crate::buffer::raw::Policy>(
        self,
        seed: P::Seed,
    ) -> Result<regions::Init<P>, alloc::Error<P::Failure>> {
        use crate::buffer::raw::SliceExt as _;

        let (capacity, sources) = self.parts();
        let mut heap = crate::buffer::raw::Heap::try_new(seed, capacity)?;
        // SAFETY: the retained sources exactly fill this fresh disjoint allocation.
        unsafe { sources.copy_to(heap.as_mut_ptr().cast()) };
        // SAFETY: copying every retained source initialized the complete allocation.
        Ok(unsafe { regions::Init::new(heap.into_shared()) })
    }
}

// SAFETY: admission pairs every retained slice with their exact nonzero byte sum.
unsafe impl<const N: usize> Initialize for crate::buffer::raw::Slices<'_, N> {
    fn parts(&self) -> (NonZero<u32>, impl crate::buffer::raw::SliceExt) {
        (self.len(), self.slices())
    }
}

mod nonempty;
mod prefix;
mod tag;
mod value;

/// Opaque byte coordinates, usable only under an exact backing-owner proof.
/// Public raw contracts may borrow this representation; it exposes no safe views.
#[derive(Debug)]
#[repr(transparent)]
pub struct Extent(ptr::NonNull<[u8]>);

/// Immutable initialized bytes inseparably paired with their retaining owner.
#[derive(Debug)]
#[repr(C)]
pub struct Value<O> {
    owner: O,
    bytes: Bytes,
}

/// An immutable view borrowed from the owner retaining its initialized extent.
#[derive(Debug)]
#[repr(transparent)]
pub struct Bytes {
    extent: regions::Extent,
}

/// Completes consumption according to the actual backing's empty-owner policy.
pub trait End: Sized {
    const EMPTY: Option<Self>;
}

impl regions::Extent {
    pub(in crate::buffer) const EMPTY: Self = Self(ptr::NonNull::slice_from_raw_parts(
        ptr::NonNull::dangling(),
        0,
    ));

    /// # Safety
    /// The inclusive address range from `ptr` through `ptr + len` must not wrap.
    /// The allocation need not remain live; coordinates alone expose no bytes.
    pub(in crate::buffer) const unsafe fn new(ptr: ptr::NonNull<u8>, len: usize) -> Self {
        Self(ptr::NonNull::slice_from_raw_parts(ptr, len))
    }

    pub(in crate::buffer) fn from_bytes(bytes: &[u8]) -> Self {
        Self(ptr::NonNull::from(bytes))
    }

    pub(in crate::buffer) const fn len(&self) -> usize {
        self.0.len()
    }

    pub(in crate::buffer) const fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr().cast::<u8>()
    }

    pub(in crate::buffer) fn duplicate(&self) -> Self {
        Self(self.0)
    }

    fn offset(&self, amount: usize) -> ptr::NonNull<u8> {
        let pointer = self.0.cast::<u8>().as_ptr().wrapping_add(amount);
        // SAFETY: constructors establish nonwrapping addresses; callers keep amount
        // within the stored extent, so the resulting address remains nonzero.
        unsafe { ptr::NonNull::new_unchecked(pointer) }
    }

    pub(in crate::buffer) fn get(&self, range: impl ops::RangeBounds<usize>) -> Option<Self> {
        use ops::Bound;

        let start = match range.start_bound() {
            Bound::Included(&value) => value,
            Bound::Excluded(&value) => value.checked_add(1)?,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&value) => value.checked_add(1)?,
            Bound::Excluded(&value) => value,
            Bound::Unbounded => self.len(),
        };
        let range = start..end;
        is_within(&range, self.len()).then(|| {
            // SAFETY: this checked subrange preserves the original nonwrapping addresses.
            unsafe { Self::new(self.offset(range.start), range.len()) }
        })
    }

    /// # Safety
    /// The range must be ordered and contained in this exact initialized extent,
    /// proving its length cannot underflow.
    pub(in crate::buffer) unsafe fn restrict(&self, range: ops::Range<usize>) -> Self {
        let len = range.end - range.start;
        // SAFETY: the caller proves this subrange of the original nonwrapping extent.
        unsafe { Self::new(self.offset(range.start), len) }
    }
}

const _: () = assert!(size_of::<regions::Extent>() == 2 * size_of::<usize>());

const _: () = {
    assert!(size_of::<regions::Bytes>() == 2 * size_of::<usize>());
    assert!(size_of::<regions::Value<Option<regions::Tag>>>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<regions::Value<Option<regions::Tag>>>>() == 3 * size_of::<usize>());
};

const _: () = assert!(usize::BITS >= 64);

impl<O> regions::Value<O> {
    pub(in crate::buffer) const fn empty(owner: O) -> Self {
        Self {
            owner,
            bytes: regions::Bytes {
                extent: regions::Extent::EMPTY,
            },
        }
    }
    /// # Safety
    /// The extent must form one valid initialized immutable slice retained by
    /// the transferred owner for every region borrow, including after owner moves.
    pub(in crate::buffer) const unsafe fn new(owner: O, extent: regions::Extent) -> Self {
        Self {
            owner,
            bytes: regions::Bytes { extent },
        }
    }
    pub(in crate::buffer) const fn owner(&self) -> &O {
        &self.owner
    }

    /// # Safety
    /// Every replacement must remain an initialized subrange of this exact
    /// owner's backing; the owner and its lifetime must remain unchanged.
    pub(in crate::buffer) unsafe fn extent_mut(&mut self) -> &mut regions::Extent {
        &mut self.bytes.extent
    }
    /// # Safety
    /// The bounds must select a subrange of the unchanged current extent.
    pub(in crate::buffer) unsafe fn restrict(&mut self, bounds: ops::Range<usize>) {
        // SAFETY: the caller proves the range; the paired owner remains unchanged.
        self.bytes.extent = unsafe { self.bytes.extent.restrict(bounds) };
    }
    pub fn try_restrict(&mut self, visible: ops::Range<usize>) -> Result<(), range::Error>
    where
        O: regions::End,
    {
        self.bytes.extent = self
            .bytes
            .extent
            .get(visible.clone())
            .ok_or_else(|| range::Error::new(visible, self.len()))?;
        if self.is_empty()
            && let Some(owner) = O::EMPTY
        {
            self.owner = owner;
        }
        Ok(())
    }
}

impl<O: buffer::raw::Retain> regions::Value<O> {
    pub(in crate::buffer) fn try_clone(&self) -> Result<Self, rc::Error> {
        let extent = self.bytes.extent.duplicate();
        Ok(Self {
            owner: self.owner.try_retain()?,
            bytes: regions::Bytes { extent },
        })
    }
    /// # Safety
    /// The slice must be an initialized subrange of this exact region, preserved
    /// by the source borrow until retaining its owner completes.
    pub(in crate::buffer) unsafe fn try_view(&self, bytes: &[u8]) -> Result<Self, rc::Error> {
        Ok(Self {
            owner: self.owner.try_retain()?,
            bytes: regions::Bytes {
                extent: regions::Extent::from_bytes(bytes),
            },
        })
    }
}

impl regions::Value<Option<regions::Tag>> {
    pub(in crate::buffer) fn try_get(
        &self,
        range: impl ops::RangeBounds<usize>,
    ) -> Result<Option<Self>, rc::Error> {
        use buffer::raw::Retain as _;
        let Some(extent) = self.bytes.extent.get(range) else {
            return Ok(None);
        };
        if extent.len() == 0 {
            return Ok(Some(Self::empty(None)));
        }
        Ok(Some(Self {
            owner: self.owner.try_retain()?,
            bytes: regions::Bytes { extent },
        }))
    }
}

impl<O> ops::Deref for regions::Value<O> {
    type Target = regions::Bytes;
    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

impl From<regions::Value<crate::buffer::raw::Rc>> for regions::Value<Option<regions::Tag>> {
    fn from(region: regions::Value<crate::buffer::raw::Rc>) -> Self {
        Self {
            owner: Some(regions::Tag::allocation(region.owner)),
            bytes: region.bytes,
        }
    }
}

impl<L> From<regions::Value<owners::Edge<owners::Plain<L>>>>
    for regions::Value<Option<regions::Tag>>
{
    fn from(region: regions::Value<owners::Edge<owners::Plain<L>>>) -> Self {
        Self {
            owner: regions::Tag::pool(region.owner),
            bytes: region.bytes,
        }
    }
}

impl<'d, L> From<regions::Value<owners::Edge<owners::Plain<L>, pool::Borrowed<'d>>>>
    for regions::Value<owners::Edge<owners::Plain<L>>>
{
    fn from(region: regions::Value<owners::Edge<owners::Plain<L>, pool::Borrowed<'d>>>) -> Self {
        Self {
            owner: region.owner.into_owned(),
            bytes: region.bytes,
        }
    }
}

impl<'a: 'd, 'd> From<&'a [u8]> for regions::Value<resident::State<'d>> {
    fn from(bytes: &'a [u8]) -> Self {
        Self {
            owner: resident::State::Slice,
            bytes: regions::Bytes {
                extent: regions::Extent::from_bytes(bytes),
            },
        }
    }
}

impl<'d> From<regions::Value<Option<regions::Tag>>> for regions::Value<resident::State<'d>> {
    fn from(region: regions::Value<Option<regions::Tag>>) -> Self {
        Self {
            owner: resident::State::Owned(region.owner),
            bytes: region.bytes,
        }
    }
}

impl<'d> From<regions::Value<crate::buffer::raw::Rc<resident::Lease<'d>>>>
    for regions::Value<resident::State<'d>>
{
    fn from(region: regions::Value<crate::buffer::raw::Rc<resident::Lease<'d>>>) -> Self {
        Self {
            owner: resident::State::Resident(region.owner),
            bytes: region.bytes,
        }
    }
}

impl<'d, L> From<regions::Value<owners::Edge<owners::Plain<L>, pool::Borrowed<'d>>>>
    for regions::Value<resident::State<'d>>
{
    fn from(region: regions::Value<owners::Edge<owners::Plain<L>, pool::Borrowed<'d>>>) -> Self {
        match regions::Tag::pool(region.owner) {
            Some(owner) => Self {
                owner: resident::State::Borrowed(owner),
                bytes: region.bytes,
            },
            None => Self::empty(resident::State::Slice),
        }
    }
}

impl<'d> From<regions::Value<Option<provided::Account<'d>>>>
    for regions::Value<resident::State<'d>>
{
    fn from(region: regions::Value<Option<provided::Account<'d>>>) -> Self {
        match region.owner {
            Some(owner) => Self {
                owner: resident::State::Provided(owner),
                bytes: region.bytes,
            },
            None => Self::empty(resident::State::Slice),
        }
    }
}

impl<'d> regions::Value<resident::State<'d>> {
    pub(in crate::buffer) fn try_own(
        mut self,
    ) -> Result<regions::Value<Option<regions::Tag>>, Self> {
        if let resident::State::Resident(owner) = self.owner {
            self.owner = match owner.try_own() {
                Ok(owner) => resident::State::Owned(Some(regions::Tag::allocation(owner))),
                Err(owner) => resident::State::Resident(owner),
            };
        }
        self.try_detach()
    }

    pub(in crate::buffer) fn try_detach(
        self,
    ) -> Result<regions::Value<Option<regions::Tag>>, Self> {
        let empty = self.is_empty();
        match self.owner.try_own(empty) {
            Ok(owner) => Ok(regions::Value {
                owner,
                bytes: self.bytes,
            }),
            Err(owner) => Err(Self {
                owner,
                bytes: self.bytes,
            }),
        }
    }
}

impl regions::Bytes {
    pub const fn len(&self) -> usize {
        self.extent.len()
    }
    pub const fn is_empty(&self) -> bool {
        self.extent.len() == 0
    }
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: regions::Bytes can only be borrowed from its paired retaining regions::Value.
        unsafe { slice::from_raw_parts(self.extent.as_ptr(), self.extent.len()) }
    }
}

impl ops::Deref for regions::Bytes {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}
