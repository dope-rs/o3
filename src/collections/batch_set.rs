use std::cell::{Cell, UnsafeCell};

use crate::marker::ThreadBound;

use super::CellBitmap;

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
/// Duplicate inserts coalesce across both the draining and pending batches.
/// Once an index is removed from a drain, inserting it again defers it to the
/// next batch.
pub struct BatchSet {
    words: UnsafeCell<Words>,
    summaries: [CellBitmap; 2],
    capacity: Cell<usize>,
    len: [Cell<usize>; 2],
    cursor: [Cell<usize>; 2],
    active: Cell<Side>,
    draining: Cell<bool>,
    _thread: ThreadBound,
}

enum Words {
    Empty,
    Inline(Cell<usize>),
    Heap(Vec<Cell<usize>>),
}

impl Words {
    fn zeroed(word_count: usize) -> Self {
        match word_count {
            0 => Self::Empty,
            1 => Self::Inline(Cell::new(0)),
            _ => Self::Heap((0..word_count).map(|_| Cell::new(0)).collect()),
        }
    }

    fn grow(&mut self, word_count: usize) {
        match self {
            Self::Empty => *self = Self::zeroed(word_count),
            Self::Inline(word) if word_count > 1 => {
                let first = word.get();
                let mut words = Vec::with_capacity(word_count);
                words.push(Cell::new(first));
                words.resize_with(word_count, || Cell::new(0));
                *self = Self::Heap(words);
            }
            Self::Heap(words) => words.resize_with(word_count, || Cell::new(0)),
            Self::Inline(_) => {}
        }
    }

    fn as_slice(&self) -> &[Cell<usize>] {
        match self {
            Self::Empty => &[],
            Self::Inline(word) => std::slice::from_ref(word),
            Self::Heap(words) => words,
        }
    }
}

impl BatchSet {
    pub fn with_capacity(capacity: usize) -> Self {
        let word_count = capacity.div_ceil(ENTRIES_PER_WORD);
        Self {
            words: UnsafeCell::new(Words::zeroed(word_count)),
            summaries: [
                CellBitmap::with_capacity(word_count),
                CellBitmap::with_capacity(word_count),
            ],
            capacity: Cell::new(capacity),
            len: [Cell::new(0), Cell::new(0)],
            cursor: [Cell::new(0), Cell::new(0)],
            active: Cell::new(Side::A),
            draining: Cell::new(false),
            _thread: ThreadBound::NEW,
        }
    }

    /// Grows both batch generations while preserving their contents.
    ///
    /// Insertion never grows the set implicitly.
    pub fn grow_to(&self, capacity: usize) {
        if capacity <= self.capacity.get() {
            return;
        }
        let word_count = capacity.div_ceil(ENTRIES_PER_WORD);
        unsafe { &mut *self.words.get() }.grow(word_count);
        self.summaries[0].grow_to(word_count);
        self.summaries[1].grow_to(word_count);
        self.capacity.set(capacity);
    }

    /// Inserts an index into the pending batch.
    ///
    /// Returns `false` when the index is outside the set's capacity or already
    /// present in either the batch being drained or the pending batch. This
    /// method never grows the set.
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

    /// Starts draining a stable batch.
    ///
    /// Returns `None` while another batch drain is live. Inserts made through
    /// the set during a drain are deferred to the next batch.
    pub fn drain_batch(&self) -> Option<BatchDrain<'_>> {
        if self.draining.replace(true) {
            return None;
        }
        let side = self.active.get();
        self.active.set(side.other());
        Some(BatchDrain { set: self, side })
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
        let side_index = side.index();
        if self.len[side_index].get() == 0 {
            return None;
        }
        let start = self.cursor[side_index].get();
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

    fn word(&self, index: usize) -> &Cell<usize> {
        let words = unsafe { &*self.words.get() }.as_slice();
        debug_assert!(index < words.len());
        // Every caller derives `index` either from an element below `capacity`
        // or from a summary whose capacity is kept equal to `words.len()`.
        unsafe { words.get_unchecked(index) }
    }
}

/// A consuming iterator over one stable [`BatchSet`] batch.
pub struct BatchDrain<'a> {
    set: &'a BatchSet,
    side: Side,
}

impl Iterator for BatchDrain<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        self.set.pop_side(self.side)
    }
}

impl Drop for BatchDrain<'_> {
    fn drop(&mut self) {
        self.set.return_remaining(self.side);
        self.set.draining.set(false);
    }
}
