use std::{
    marker, mem,
    num::NonZero,
    ops,
    slice::{from_raw_parts, from_raw_parts_mut},
    str,
};

use crate::{
    base::num,
    buffer,
    buffer::{
        bytes::bounded,
        pool::{self, owners, state},
        resident, segments,
        storage::provided,
    },
    mem::rc,
};
mod slices;
pub use slices::Slices;
mod store;
pub use store::Store;
mod segment;
pub use segment::Segment;
mod scatter;
pub use scatter::Scatter;
pub(in crate::buffer) use slices::{
    Batch, Copy, Exclusive, Indexed, IndexedMut, Linked, LinkedMut, Prefix, SliceExt,
};

/// # Safety
/// The `buffer::raw::Source` contract must hold. `buffer::Cursor` must expose that same range.
/// Consumption must advance exactly the proved prefix and preserve the remaining bytes.
pub unsafe trait Consume: AsRef<[u8]> {
    fn consume<const MAX: u32>(prefix: Consumed<'_, '_, Self, MAX>);
}

// SAFETY: admission retains this exact initialized slice exclusively.
unsafe impl Consume for &mut [u8] {
    fn consume<const MAX: u32>(prefix: Consumed<'_, '_, Self, MAX>) {
        let (target, end) = prefix.into_parts();
        let bytes = mem::take(target);
        // SAFETY: the source-bound end is within this unchanged slice.
        *target = unsafe { bytes.get_unchecked_mut(end.count().get() as usize..) };
    }
}

// SAFETY: the view retains its immutable byte borrow and original source;
// consumption only selects the admitted suffix of that same borrow.
unsafe impl<S: ?Sized> Consume for buffer::View<'_, S> {
    fn consume<const MAX: u32>(prefix: Consumed<'_, '_, Self, MAX>) {
        let (view, end) = prefix.into_parts();
        // SAFETY: the prefix owns the exact view and its validated bound.
        view.bytes = unsafe { view.bytes.get_unchecked(end.count().get() as usize..) };
    }
}

/// One owner edge that can retain the same backing independently.
/// # Safety
/// `try_retain` preserves backing identity, access, and branded lifetimes.
pub unsafe trait Retain: Sized {
    fn try_retain(&self) -> Result<Self, rc::Error>;
}

// SAFETY: Some retains the same owner and lifetime; None retains no backing.
unsafe impl<T: Retain> Retain for Option<T> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        self.as_ref().map(T::try_retain).transpose()
    }
}

/// A complete immutable source whose entire visible range is valid UTF-8.
#[derive(Debug)]
#[repr(transparent)]
pub struct Utf8<O: buffer::raw::Source> {
    owner: O,
}

impl<O: buffer::raw::Source> Utf8<O> {
    /// Validates and retains one complete immutable UTF-8 source.
    pub fn try_new(owner: O) -> Result<Self, str::Utf8Error> {
        str::from_utf8(owner.as_ref())?;
        Ok(Self { owner })
    }

    /// # Safety
    /// The source's entire unchanged visible range is valid UTF-8.
    pub unsafe fn new(owner: O) -> Self {
        Self { owner }
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: construction proves UTF-8 and buffer::raw::Source preserves the complete range.
        unsafe { str::from_utf8_unchecked(self.owner.as_ref()) }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.owner.as_ref()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.owner.as_ref().len()
    }
}

/// # Safety
/// Preserve the owner, visible range, resident capacity, and owner lifetimes.
/// Allocate, copy, and clone nothing; do not introduce sharing.
pub unsafe trait Convert<T> {
    fn into(self) -> T;
}

// SAFETY: identity retains the complete original value and its byte storage.
unsafe impl<T> Convert<T> for T {
    fn into(self) -> T {
        self
    }
}

/// Storage retaining one exact nonempty physical owner.
pub trait Unary<T> {
    fn item(&self) -> &segments::Item<T>;
}

/// # Safety
/// Source and iterator totals remain equal and unchanged through exhaustion: exact nonoverflowing owner count, visible byte sum and resident charge sum.
/// Success yields exactly that many nonempty owners, never more on failure; preserve backing, borrows and `Output` cardinality (`Many` requires at least two).
pub unsafe trait Sequence<T: buffer::raw::Segment>: Sized + Total {
    type Item;
    type Error;
    type Output: Assemble<T>;
    fn owners(self) -> impl Iterator<Item = Self::Item> + Total;

    fn reject(self, error: crate::alloc::Error) -> Self::Error;
}

pub trait Assemble<T: buffer::raw::Segment>: Sized {
    fn finish<S: Sequence<T, Output = Self>>(input: segments::Built<T, S>) -> Self;
}

pub trait Total {
    fn count(&self) -> segments::Count;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn len(&self) -> usize;
    fn resident(&self) -> usize;
}

impl<I, M: Total> Total for buffer::raw::Batch<I, M> {
    fn count(&self) -> segments::Count {
        self.total.count()
    }
    fn len(&self) -> usize {
        self.total.len()
    }
    fn resident(&self) -> usize {
        self.total.resident()
    }
}

/// Immutable scattered bytes with an exact length and borrowed cursor.
///
/// # Safety
/// `start()` has `len()` remaining; offset cursors exist through the end.
pub unsafe trait Chunks {
    type Cursor<'data>: Scatter<'data>
    where
        Self: 'data;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn len(&self) -> usize;
    fn start(&self) -> Self::Cursor<'_>;
    fn cursor(&self, offset: usize) -> Option<Self::Cursor<'_>> {
        Scatter::seek(self.start(), offset)
    }
}

/// An owned admission retaining its operation proof until storage accepts it.
/// # Safety
/// Move only the original owner; the projection must retain its lifetime and exactly `bytes()`.
pub unsafe trait Input<T> {
    fn bytes(&self) -> &[u8];
    fn into_item(self) -> (T, impl for<'a> FnOnce(&'a mut T) -> &'a mut [u8]);
}

/// Projects fixed-residency items admitted by their original native context.
pub trait Fixed<T> {
    fn resident(&self) -> NonZero<u32>;

    /// # Safety
    /// Own this context's unique item; its full retained backing fits `resident()`.
    /// Its prefix is initialized; `limit` is that ceiling and backing/layout live for `'a`.
    unsafe fn project<'a, 'id>(&'a self, item: &'a mut T, limit: Limit<'id>)
    -> Projection<'a, 'id>;

    /// # Safety
    /// Own its context's unique initialized item; full backing fits `resident()` and lives for `'d`.
    /// Its layout and `resident()` match this arena's unchanged layout and resident ceiling.
    unsafe fn insert<'d>(
        vacant: provided::Vacant<'d, T, (), &'d Self>,
        item: T,
    ) -> provided::Unique<'d>
    where
        Self: Sized,
    {
        let context = vacant.context();
        vacant.insert_with(item, |item, limit| {
            // SAFETY: native admission supplied this exact context and item;
            // the arena retains both and issues the context's resident ceiling.
            unsafe { context.project(item, limit) }
        })
    }
}

/// Exact arenas entered together with their external accounting generation.
/// # Safety
/// Roots must borrow their original arenas. The terminal must reclaim every item and charge.
pub unsafe trait Arenas: Unpin {
    type Roots<'d>: 'd
    where
        Self: 'd;

    /// # Safety
    /// Invoke the terminal once after all issued access ends, while every charged budget lives.
    unsafe fn enter(&mut self) -> (Self::Roots<'_>, impl FnOnce() + '_);
}

// SAFETY: disjoint owners retain their exact roots and terminate in reverse order.
unsafe impl<A: Arenas, B: Arenas> Arenas for (A, B) {
    type Roots<'d>
        = (A::Roots<'d>, B::Roots<'d>)
    where
        Self: 'd;

    unsafe fn enter(&mut self) -> (Self::Roots<'_>, impl FnOnce() + '_) {
        // SAFETY: the caller supplies the same generation and terminal ordering for both owners.
        let (first, finish) = unsafe { self.0.enter() };
        let (second, end) = unsafe { self.1.enter() };
        ((first, second), move || {
            end();
            finish();
        })
    }
}

/// A mutable projection proven to fit its resident byte ceiling.
#[repr(transparent)]
pub struct Projection<'a, 'id> {
    bytes: &'a mut [u8],
    operation: marker::PhantomData<fn(&'id mut ()) -> &'id mut ()>,
}

impl<'a> Projection<'a, '_> {
    pub(in crate::buffer) const fn into_inner(self) -> &'a mut [u8] {
        self.bytes
    }
}

/// Immutable UTF-8 in one exact allocation bound to its paying domain.
#[derive(Debug)]
#[repr(transparent)]
pub struct Text<'d> {
    text: buffer::raw::Utf8<resident::Exact<'d>>,
}

impl<'d> Text<'d> {
    /// # Safety
    /// The complete immutable owner must contain valid UTF-8.
    pub unsafe fn from_utf8(owner: resident::Exact<'d>) -> Self {
        // SAFETY: the caller proves UTF-8 for this complete immutable allocation.
        Self {
            text: unsafe { buffer::raw::Utf8::new(owner) },
        }
    }

    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        // SAFETY: retaining the exact owner preserves its complete validated UTF-8 range.
        Ok(unsafe { Self::from_utf8(self.text.owner.try_clone()?) })
    }
}

const _: () = assert!(size_of::<Text<'static>>() == size_of::<usize>());

impl<'d> ops::Deref for Text<'d> {
    type Target = buffer::raw::Utf8<resident::Exact<'d>>;
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

/// Exclusive initialized suffix retained through its output operation.
pub struct Output<'a, L = pool::Layout> {
    target: &'a mut pool::Span<owners::Plain<L>, pool::Owned, state::Initialized>,
}

impl<'a, L> Output<'a, L> {
    pub(in crate::buffer) const fn new(
        target: &'a mut pool::Span<owners::Plain<L>, pool::Owned, state::Initialized>,
    ) -> Self {
        Self { target }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.target.capacity().get() as usize - self.target.len()
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let (data, start) = buffer::raw::Prefix::prefix(&**self.target);
        // SAFETY: Initialized retains every byte of this exact exclusive suffix.
        unsafe { range_mut(data.cast(), start as usize, self.len()) }
    }

    /// # Safety
    /// The foreign operation must prove `len` fits this unchanged output.
    pub unsafe fn complete(self, len: NonZero<usize>) -> pool::Complete<'a, L> {
        unsafe { pool::Complete::new(self.target, len) }
    }
}

const _: () = assert!(size_of::<Output<'_>>() == size_of::<usize>());

/// An adapter exposing its complete unchanged byte extent through one inner owner.
/// # Safety
/// Projections retain the exact backing, lifetime, bytes, and restriction invariants.
pub unsafe trait Project<T: ?Sized>: AsRef<[u8]> {
    fn as_inner(&self) -> &T;
    /// # Safety
    /// Only restrict the extent, preserving its nonempty owner and empty policy.
    unsafe fn as_inner_mut(&mut self) -> &mut T;
}

// SAFETY: the transparent bound exposes the unchanged extent of its original owner.
unsafe impl<T: AsRef<[u8]>> Project<T> for bounded::Value<T> {
    fn as_inner(&self) -> &T {
        &self.0
    }
    unsafe fn as_inner_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

/// # Safety
/// Shared access preserves its exact backing, range, and contents until exclusive mutation.
/// Writes preserve the initialized prefix; successful writes append their complete input.
pub unsafe trait Source: AsRef<[u8]> {}

// SAFETY: the stronger cursor contract requires this stable-source contract.
unsafe impl<T: buffer::raw::Consume> Source for T {}

/// # Safety
/// `AsMut::as_mut` returns the full Source range with its pre-call pointer and length, unchanged after the loan.
/// The call changes no owner, descriptor, extent, or bytes; caller slice writes are allowed, and other exclusive operations retain their own contracts.
pub unsafe trait Mutable: Source + AsMut<[u8]> {}

/// # Safety
/// Every `provided::Mutable::try_take` implementation returns the exact requested prefix
/// and retains its disjoint suffix, contents, owner and lifetimes; failure changes nothing.
pub unsafe trait Split: Mutable {}

/// # Safety
/// Freezing retains the exact backing, initialized range, contents, owner and lifetimes.
/// It allocates, copies and clones nothing and introduces no sharing.
pub unsafe trait Freeze: Source + Sized {
    type Output: Source;

    fn freeze(self) -> Self::Output;
}

/// # Safety
/// Retain source borrows and stable `len`; initialize exactly that prefix once.
/// Never read uninitialized output, retain its access, or invalidate source borrows.
pub unsafe trait Initializer {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// # Safety
    /// `target` exclusively retains at least `self.len()` writable bytes.
    unsafe fn initialize(&self, target: *mut mem::MaybeUninit<u8>);
}

pub(in crate::buffer) use store::{
    Backing, Debit, Growth, Header, Heap, Policy, Rc, Space, Token, Unique,
};

mod length;
pub use length::Length;
mod limit;
pub use limit::Limit;
mod consumed;
pub use consumed::Consumed;
pub(in crate::buffer) use consumed::Selection;
mod slice;
pub use slice::Slice;

/// One proven non-empty span borrowed from its source.
#[repr(transparent)]
pub struct Chunk<'data, const MAX: usize = { usize::MAX }> {
    bytes: &'data [u8],
}

/// Borrows exactly `len` initialized bytes at `start` of one allocation.
/// # Safety
/// `pointer.add(start)` must be valid. A nonempty selected range must contain
/// `len` initialized bytes in one allocation retained for `'a`, without
/// overlapping exclusive access. The nonnull selected pointer and extent must
/// satisfy [`from_raw_parts`], including its `isize` bound and empty-range rules.
pub(in crate::buffer) unsafe fn range<'a>(
    pointer: *const u8,
    start: usize,
    len: usize,
) -> &'a [u8] {
    // SAFETY: the caller proved this exact initialized subrange of one allocation.
    unsafe { from_raw_parts(pointer.add(start), len) }
}

/// Borrows the same initialized subrange exclusively.
///
/// # Safety
/// As [`range`], with exclusive access to the selected range for `'a`.
/// Other accesses during that loan must be disjoint; the selected pointer and
/// extent must satisfy [`from_raw_parts_mut`], including its empty-range rules.
pub(in crate::buffer) unsafe fn range_mut<'a>(
    pointer: *mut u8,
    start: usize,
    len: usize,
) -> &'a mut [u8] {
    // SAFETY: the caller proved this exact initialized subrange and its exclusive retention.
    unsafe { from_raw_parts_mut(pointer.add(start), len) }
}

impl<'data, const MAX: usize> Chunk<'data, MAX> {
    /// Retains one exact initialized span from its original immutable borrow.
    ///
    /// # Safety
    /// `bytes` is non-empty and `bytes.len() <= MAX`.
    pub unsafe fn new(bytes: &'data [u8]) -> Self {
        Self { bytes }
    }

    pub const fn bytes(&self) -> &'data [u8] {
        self.bytes
    }

    pub const fn len(&self) -> num::bounded::NonZeroUsize<1, MAX> {
        // SAFETY: construction proves the retained span is non-empty and bounded by MAX.
        unsafe { num::bounded::raw::nonzero_usize(self.bytes.len()) }
    }
}

const _: () = {
    assert!(mem::size_of::<Chunk<'static>>() == mem::size_of::<&[u8]>());
    assert!(mem::size_of::<Option<Chunk<'static>>>() == mem::size_of::<&[u8]>());
};

pub(in crate::buffer) unsafe fn copy_slices<'a>(
    sources: impl IntoIterator<Item = &'a [u8]>,
    target: *mut u8,
) {
    let mut offset = 0;
    for source in sources {
        // SAFETY: the caller retains all sources and their complete disjoint target.
        unsafe { source.copy_to(target.add(offset)) };
        offset += source.len();
    }
}
