use std::cell;

mod bitmap;

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
pub struct Set {
    words: bitmap::Words,
    summaries: [bitmap::Bitmap; 2],
    capacity: cell::Cell<usize>,
    len: [cell::Cell<usize>; 2],
    cursor: [cell::Cell<usize>; 2],
    active: cell::Cell<Side>,
    draining: cell::Cell<DrainState>,
    _thread: crate::ThreadBound,
}

impl Set {
    pub fn with_capacity(capacity: usize) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(set) => set,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(capacity: usize) -> Result<Self, crate::collections::AllocationError> {
        use crate::{
            ThreadBound,
            collections::batch::bitmap::{Bitmap, Words},
        };
        let word_count = capacity.div_ceil(ENTRIES_PER_WORD);
        Ok(Self {
            words: Words::try_zeroed(word_count)?,
            summaries: [
                Bitmap::try_with_capacity(word_count)?,
                Bitmap::try_with_capacity(word_count)?,
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
        let word = self.word(word_index);
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
        self.word(word_index).get() & (3usize << shift) != 0
    }

    /// Removes an index from either batch.
    pub fn remove(&self, index: usize) -> bool {
        if index >= self.capacity.get() {
            return false;
        }

        let word_index = index / ENTRIES_PER_WORD;
        let shift = (index % ENTRIES_PER_WORD) * 2;
        let pair = 3usize << shift;
        let word = self.word(word_index);
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
    ///
    /// The hierarchical summaries keep lookup bounded by the bitmap depth,
    /// independent of the number of occupied indices skipped.
    pub fn pop_from(&self, start: usize) -> Option<usize> {
        self.pop_side_from(self.active.get(), start)
    }

    /// Starts or resumes draining a stable batch.
    ///
    /// Returns `None` during another drain; concurrent inserts enter the next batch.
    pub fn drain_batch(&self) -> Option<Drain<'_>> {
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
        Some(Drain {
            set: self,
            side,
            active: true,
        })
    }

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
        let bits = self.word(word_index).get() & SIDE_MASKS[side] & (usize::MAX << shift);
        if bits != 0 {
            return Some(word_index * ENTRIES_PER_WORD + bits.trailing_zeros() as usize / 2);
        }

        let word_index = self.summaries[side].find_at_or_after(word_index + 1)?;
        let bits = self.word(word_index).get() & SIDE_MASKS[side];
        debug_assert!(bits != 0);
        Some(word_index * ENTRIES_PER_WORD + bits.trailing_zeros() as usize / 2)
    }

    fn take(&self, side: Side, index: usize) {
        let side = side.index();
        let word_index = index / ENTRIES_PER_WORD;
        let shift = (index % ENTRIES_PER_WORD) * 2 + side;
        let bit = 1usize << shift;
        let word = self.word(word_index);
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

    fn return_remaining(&self, source: Side) {
        let source_index = source.index();
        if self.len[source_index].get() == 0 {
            return;
        }
        self.return_remaining_slow(source);
    }

    fn return_remaining_slow(&self, source: Side) {
        let destination = source.other();
        debug_assert!(self.active.get() == destination);
        let source_index = source.index();
        let destination_index = destination.index();
        let moved_len = self.len[source_index].get();
        if moved_len == 0 {
            return;
        }

        let destination_was_empty = self.len[destination_index].get() == 0;
        while let Some(word_index) = self.summaries[source_index].pop_next() {
            let word = self.word(word_index);
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
                self.summaries[destination_index].insert(word_index);
            }
        }

        self.len[source_index].set(0);
        self.len[destination_index].set(self.len[destination_index].get() + moved_len);
        if destination_was_empty {
            self.cursor[destination_index].set(self.cursor[source_index].get());
        }
        self.cursor[source_index].set(0);
    }

    fn word(&self, index: usize) -> &cell::Cell<usize> {
        let words = self.words.as_slice();
        debug_assert!(index < words.len());
        unsafe { words.get_unchecked(index) }
    }
}

/// A consuming iterator over one stable [`Set`] batch.
pub struct Drain<'a> {
    set: &'a Set,
    side: Side,
    active: bool,
}

impl Drain<'_> {
    /// Returns the next index without removing it from this batch.
    pub fn peek(&self) -> Option<usize> {
        self.set.peek_side(self.side)
    }

    /// Preserves the unconsumed part of this batch for the next drain.
    ///
    /// Unlike dropping a partial drain, pausing performs no bitmap transfer.
    pub fn pause(mut self) {
        let state = if self.set.len[self.side.index()].get() == 0 {
            DrainState::Idle
        } else {
            DrainState::Paused(self.side)
        };
        self.set.draining.set(state);
        self.active = false;
    }
}

impl Iterator for Drain<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        self.set.pop_side(self.side)
    }
}

impl Drop for Drain<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        debug_assert!(self.set.draining.get() == DrainState::Active(self.side));
        self.set.return_remaining(self.side);
        self.set.draining.set(DrainState::Idle);
    }
}
