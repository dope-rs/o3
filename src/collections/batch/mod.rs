use std::{cell, marker, mem, pin};

use crate::{collections, mem::quota};

mod bitmap;
mod inline;
pub mod raw;

pub use inline::{Fill, FrontVacant, Inline, Vacant};

const WORD_BITS: usize = usize::BITS as usize;
const ENTRIES_PER_WORD: usize = WORD_BITS / 2;
const LOW_SIDE_MASK: usize = usize::MAX / 3;
const SIDE_MASKS: [usize; 2] = [LOW_SIDE_MASK, LOW_SIDE_MASK << 1];

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Side {
    A,
    B,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DrainState {
    Idle,
    Active(Side),
    Paused(Side),
}

impl Side {
    const fn index(self) -> usize {
        self as usize
    }

    const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

/// A single-threaded set of indices drained in isolated batches.
///
/// Duplicates coalesce; reinserting a drained index defers it to the next batch.
#[repr(transparent)]
pub struct Set<I = usize> {
    raw: RawSet,
    index: marker::PhantomData<fn(I) -> I>,
}

/// A located front entry which leaves its set unchanged until consumed.
#[must_use]
pub struct Front<'a, I = usize> {
    set: &'a mut RawSet,
    side: Side,
    raw: usize,
    index: marker::PhantomData<fn(I) -> I>,
}

/// A stable dense index accepted and yielded by a [`Set`].
///
/// # Safety
/// `from_usize_unchecked(value.into_usize())` must reproduce `value`, and the
/// conversion must remain valid for every copy of `value`.
pub unsafe trait DenseIndex: Copy {
    fn into_usize(self) -> usize;

    /// Reconstructs an index previously returned by [`into_usize`](Self::into_usize).
    ///
    /// # Safety
    /// `raw` must have been produced by `into_usize` for this exact index type.
    unsafe fn from_usize_unchecked(raw: usize) -> Self;
}

macro_rules! dense_integer {
    ($($index:ty),+ $(,)?) => {$(
        unsafe impl DenseIndex for $index {
            #[inline]
            fn into_usize(self) -> usize {
                self as usize
            }

            #[inline]
            unsafe fn from_usize_unchecked(raw: usize) -> Self {
                raw as Self
            }
        }
    )+};
}

dense_integer!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// Type-erased storage retained by heterogeneous pinned queue infrastructure.
///
/// Safe users should use [`Set`]. A typed set can expose this storage only
/// through [`Set::erase`], whose caller preserves its index invariant.
#[doc(hidden)]
pub struct RawSet {
    words: bitmap::Words,
    summaries: [bitmap::Tree; 2],
    capacity: cell::Cell<usize>,
    len: [cell::Cell<usize>; 2],
    cursor: [cell::Cell<usize>; 2],
    active: cell::Cell<Side>,
    draining: cell::Cell<DrainState>,
    _thread: crate::ThreadBound,
}

impl RawSet {
    fn try_with_capacity(capacity: usize) -> Result<Self, collections::AllocationError> {
        use crate::ThreadBound;
        let word_count = capacity.div_ceil(ENTRIES_PER_WORD);
        Ok(Self {
            words: bitmap::Words::try_zeroed(word_count)?,
            summaries: [
                bitmap::Tree::try_with_capacity(word_count)?,
                bitmap::Tree::try_with_capacity(word_count)?,
            ],
            capacity: cell::Cell::new(capacity),
            len: [cell::Cell::new(0), cell::Cell::new(0)],
            cursor: [cell::Cell::new(0), cell::Cell::new(0)],
            active: cell::Cell::new(Side::A),
            draining: cell::Cell::new(DrainState::Idle),
            _thread: ThreadBound::NEW,
        })
    }

    /// Inserts an index into the pending batch.
    ///
    /// Returns `false` outside capacity or when either batch already contains it.
    pub fn insert(&self, index: usize) -> bool {
        if index >= self.capacity.get() {
            return false;
        }

        let side = self.active.get();
        let side_index = side.index();
        let word_index = index / ENTRIES_PER_WORD;
        let shift = (index % ENTRIES_PER_WORD) * 2;
        let pair = 3usize << shift;
        let bit = 1usize << (shift + side_index);
        let word = self.words.word(word_index);
        let current = word.get();
        if current & pair != 0 {
            return false;
        }

        word.set(current | bit);
        if current & SIDE_MASKS[side_index] == 0 {
            self.summaries[side_index].insert(word_index);
        }
        self.len[side_index].set(self.len[side_index].get() + 1);
        true
    }

    /// Returns whether either batch contains `index`.
    pub fn contains(&self, index: usize) -> bool {
        if index >= self.capacity.get() {
            return false;
        }

        let word_index = index / ENTRIES_PER_WORD;
        let shift = (index % ENTRIES_PER_WORD) * 2;
        self.words.word(word_index).get() & (3usize << shift) != 0
    }

    /// Removes an index from either batch.
    pub fn remove(&self, index: usize) -> bool {
        if index >= self.capacity.get() {
            return false;
        }

        let word_index = index / ENTRIES_PER_WORD;
        let shift = (index % ENTRIES_PER_WORD) * 2;
        let pair = 3usize << shift;
        let word = self.words.word(word_index);
        let current = word.get();
        let removed = current & pair;
        if removed == 0 {
            return false;
        }

        let next = current & !pair;
        word.set(next);
        for (side, side_mask) in SIDE_MASKS.into_iter().enumerate() {
            if removed & side_mask != 0 {
                if next & side_mask == 0 {
                    self.summaries[side].remove(word_index);
                }
                self.len[side].set(self.len[side].get() - 1);
            }
        }
        true
    }

    /// Removes the next index from the pending batch.
    pub fn pop(&self) -> Option<usize> {
        self.pop_side(self.active.get())
    }

    /// Removes the first pending index at or after `start`, wrapping once.
    /// Lookup is bounded by bitmap depth, not skipped occupancy.
    pub fn pop_from(&self, start: usize) -> Option<usize> {
        self.pop_side_from(self.active.get(), start)
    }

    /// Starts or resumes draining a stable batch.
    ///
    /// Returns `None` during another drain; concurrent inserts enter the next batch.
    fn drain_batch(&self) -> Option<RawDrain<'_>> {
        let side = match self.draining.get() {
            DrainState::Active(_) => return None,
            DrainState::Paused(side) if self.len[side.index()].get() != 0 => side,
            DrainState::Paused(_) | DrainState::Idle => {
                let side = self.active.get();
                self.active.set(side.other());
                side
            }
        };
        self.draining.set(DrainState::Active(side));
        Some(RawDrain { set: self, side })
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity.get()
    }

    pub fn len(&self) -> usize {
        self.len[0].get() + self.len[1].get()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn pop_side(&self, side: Side) -> Option<usize> {
        let index = self.peek_side(side)?;
        self.take(side, index);
        Some(index)
    }

    fn peek_side(&self, side: Side) -> Option<usize> {
        let side_index = side.index();
        if self.len[side_index].get() == 0 {
            return None;
        }
        let capacity = self.capacity.get();
        debug_assert!(capacity != 0);
        let start = self.cursor[side_index].get() % capacity;
        self.find_at_or_after(side, start)
            .or_else(|| self.find_at_or_after(side, 0))
    }

    fn pop_side_from(&self, side: Side, start: usize) -> Option<usize> {
        let side_index = side.index();
        if self.len[side_index].get() == 0 {
            return None;
        }
        let capacity = self.capacity.get();
        debug_assert!(capacity != 0);
        let start = start % capacity;
        let index = self
            .find_at_or_after(side, start)
            .or_else(|| self.find_at_or_after(side, 0))?;
        self.take(side, index);
        Some(index)
    }

    fn find_at_or_after(&self, side: Side, start: usize) -> Option<usize> {
        if start >= self.capacity.get() {
            return None;
        }

        let side = side.index();
        let word_index = start / ENTRIES_PER_WORD;
        let shift = (start % ENTRIES_PER_WORD) * 2 + side;
        let bits = self.words.word(word_index).get() & SIDE_MASKS[side] & (usize::MAX << shift);
        if bits != 0 {
            return Some(word_index * ENTRIES_PER_WORD + bits.trailing_zeros() as usize / 2);
        }

        let word_index = self.summaries[side].find_at_or_after(word_index + 1)?;
        let bits = self.words.word(word_index).get() & SIDE_MASKS[side];
        debug_assert!(bits != 0);
        Some(word_index * ENTRIES_PER_WORD + bits.trailing_zeros() as usize / 2)
    }

    fn take(&self, side: Side, index: usize) {
        let side = side.index();
        let word_index = index / ENTRIES_PER_WORD;
        let shift = (index % ENTRIES_PER_WORD) * 2 + side;
        let bit = 1usize << shift;
        let word = self.words.word(word_index);
        let next = word.get() & !bit;
        word.set(next);
        if next & SIDE_MASKS[side] == 0 {
            self.summaries[side].remove(word_index);
        }
        self.len[side].set(self.len[side].get() - 1);
        self.cursor[side].set(if index + 1 == self.capacity.get() {
            0
        } else {
            index + 1
        });
    }
}

struct RawDrain<'a> {
    set: &'a RawSet,
    side: Side,
}

impl RawDrain<'_> {
    fn peek(&self) -> Option<usize> {
        self.set.peek_side(self.side)
    }

    fn pause(self) {
        let this = mem::ManuallyDrop::new(self);
        let state = if this.set.len[this.side.index()].get() == 0 {
            DrainState::Idle
        } else {
            DrainState::Paused(this.side)
        };
        this.set.draining.set(state);
    }

    fn return_remaining(&self) {
        let source_index = self.side.index();
        if self.set.len[source_index].get() == 0 {
            return;
        }
        self.return_remaining_slow();
    }

    fn return_remaining_slow(&self) {
        let source = self.side;
        let set = self.set;
        let destination = source.other();
        debug_assert!(set.active.get() == destination);
        let source_index = source.index();
        let destination_index = destination.index();
        let moved_len = set.len[source_index].get();
        if moved_len == 0 {
            return;
        }

        let destination_was_empty = set.len[destination_index].get() == 0;
        while let Some(word_index) = set.summaries[source_index].pop_next() {
            let word = set.words.word(word_index);
            let current = word.get();
            let source_bits = current & SIDE_MASKS[source_index];
            debug_assert!(source_bits != 0);
            let moved = match source {
                Side::A => source_bits << 1,
                Side::B => source_bits >> 1,
            };
            debug_assert_eq!(current & moved, 0);
            let next = (current & !source_bits) | moved;
            word.set(next);
            if current & SIDE_MASKS[destination_index] == 0 {
                set.summaries[destination_index].insert(word_index);
            }
        }

        set.len[source_index].set(0);
        set.len[destination_index].set(set.len[destination_index].get() + moved_len);
        if destination_was_empty {
            set.cursor[destination_index].set(set.cursor[source_index].get());
        }
        set.cursor[source_index].set(0);
    }
}

impl Iterator for RawDrain<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        self.set.pop_side(self.side)
    }
}

impl Drop for RawDrain<'_> {
    fn drop(&mut self) {
        debug_assert!(self.set.draining.get() == DrainState::Active(self.side));
        self.return_remaining();
        self.set.draining.set(DrainState::Idle);
    }
}

impl<I> Set<I> {
    pub fn with_capacity(capacity: usize) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(set) => set,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(capacity: usize) -> Result<Self, collections::AllocationError> {
        Ok(Self {
            raw: RawSet::try_with_capacity(capacity)?,
            index: marker::PhantomData,
        })
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.raw.capacity()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Exposes the type-erased pinned storage used by heterogeneous queues.
    ///
    /// # Safety
    /// Every value inserted through the returned storage must have been
    /// produced by `DenseIndex::into_usize` for `I`. The storage must remain
    /// structurally pinned for as long as the returned reference can be used.
    ///
    /// ```compile_fail
    /// use std::pin::Pin;
    /// use o3::collections::batch::{RawSet, Set};
    ///
    /// fn widen<'short, 'long>(set: Pin<&'short Set>) -> Pin<&'long RawSet> {
    ///     unsafe { Set::erase(set) }
    /// }
    /// ```
    #[inline]
    pub unsafe fn erase(self: pin::Pin<&Self>) -> pin::Pin<&RawSet> {
        unsafe { self.map_unchecked(|set| &set.raw) }
    }
}

impl<I: DenseIndex> Set<I> {
    /// Inserts an index into the pending batch.
    ///
    /// Returns `false` outside capacity or when either batch already contains it.
    #[inline]
    pub fn insert(&self, index: I) -> bool {
        self.raw.insert(index.into_usize())
    }

    /// Returns whether either batch contains `index`.
    #[inline]
    pub fn contains(&self, index: I) -> bool {
        self.raw.contains(index.into_usize())
    }

    /// Removes an index from either batch.
    #[inline]
    pub fn remove(&self, index: I) -> bool {
        self.raw.remove(index.into_usize())
    }

    /// Removes the next index from the pending batch.
    #[inline]
    pub fn pop(&self) -> Option<I> {
        self.raw.pop().map(|raw| {
            // SAFETY: safe insertion accepts only an `I`; erased insertion is
            // restricted by `erase` to raw values produced by that same type.
            unsafe { I::from_usize_unchecked(raw) }
        })
    }

    pub fn front(&mut self) -> Option<Front<'_, I>> {
        let side = self.raw.active.get();
        let raw = self.raw.peek_side(side)?;
        Some(Front {
            set: &mut self.raw,
            side,
            raw,
            index: marker::PhantomData,
        })
    }

    /// Removes the first pending index at or after `start`, wrapping once.
    /// Lookup is bounded by bitmap depth, not skipped occupancy.
    #[inline]
    pub fn pop_from(&self, start: I) -> Option<I> {
        self.raw.pop_from(start.into_usize()).map(|raw| {
            // SAFETY: the returned raw value was previously inserted under
            // this set's typed or erased insertion contract.
            unsafe { I::from_usize_unchecked(raw) }
        })
    }

    /// Starts or resumes draining a stable batch.
    ///
    /// Returns `None` during another drain; concurrent inserts enter the next batch.
    #[inline]
    pub fn drain_batch(&self) -> Option<Drain<'_, I>> {
        Some(Drain {
            raw: self.raw.drain_batch()?,
            index: marker::PhantomData,
        })
    }
}

impl<I: DenseIndex> Front<'_, I> {
    pub fn get(&self) -> I {
        // SAFETY: `raw` was located in this typed set and remains unchanged.
        unsafe { I::from_usize_unchecked(self.raw) }
    }

    pub fn take(self) -> I {
        self.set.take(self.side, self.raw);
        // SAFETY: `raw` was located in this typed set and removed unchanged.
        unsafe { I::from_usize_unchecked(self.raw) }
    }
}

/// A consuming iterator over one stable [`Set`] batch.
#[repr(transparent)]
pub struct Drain<'a, I = usize> {
    raw: RawDrain<'a>,
    index: marker::PhantomData<fn(I) -> I>,
}

#[must_use = "the result determines whether the ready index was consumed"]
pub enum Next<I> {
    Item(I),
    Empty,
    Exhausted(I),
}

impl<I: DenseIndex> Drain<'_, I> {
    /// Returns the next index without removing it from this batch.
    #[inline]
    pub fn peek(&self) -> Option<I> {
        self.raw.peek().map(|raw| {
            // SAFETY: every raw entry was inserted under the owning set's
            // typed or erased insertion contract.
            unsafe { I::from_usize_unchecked(raw) }
        })
    }

    pub fn next_with_quota<Tag>(&mut self, quota: &quota::Ledger<Tag>) -> Next<I> {
        let Some(raw) = self.raw.peek() else {
            return Next::Empty;
        };
        if !quota.take() {
            // SAFETY: `raw` was read from this typed drain and remains present.
            let index = unsafe { I::from_usize_unchecked(raw) };
            return Next::Exhausted(index);
        }
        self.raw.set.take(self.raw.side, raw);
        // SAFETY: `raw` was removed from this typed drain.
        let index = unsafe { I::from_usize_unchecked(raw) };
        Next::Item(index)
    }

    /// Preserves the unconsumed part of this batch for the next drain.
    /// Pausing performs no bitmap transfer.
    #[inline]
    pub fn pause(self) {
        self.raw.pause();
    }
}

impl<I: DenseIndex> Iterator for Drain<'_, I> {
    type Item = I;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.raw.next().map(|raw| {
            // SAFETY: every raw entry was inserted under the owning set's
            // typed or erased insertion contract.
            unsafe { I::from_usize_unchecked(raw) }
        })
    }
}

const _: () = {
    assert!(mem::size_of::<Set<usize>>() == mem::size_of::<RawSet>());
    assert!(mem::align_of::<Set<usize>>() == mem::align_of::<RawSet>());
    assert!(mem::size_of::<Drain<'static, usize>>() == mem::size_of::<RawDrain<'static>>());
    assert!(mem::align_of::<Drain<'static, usize>>() == mem::align_of::<RawDrain<'static>>());
};
