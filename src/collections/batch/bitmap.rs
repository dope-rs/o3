use std::cell::{Cell, UnsafeCell};

use crate::ThreadBound;

const WORD_BITS: usize = usize::BITS as usize;

pub(super) struct Bitmap {
    words: UnsafeCell<Words>,
    summary: UnsafeCell<Option<Box<Bitmap>>>,
    capacity: Cell<usize>,
    len: Cell<usize>,
    cursor: Cell<usize>,
    _thread: ThreadBound,
}

pub(super) enum Words {
    Empty,
    Inline(Cell<usize>),
    Heap(Vec<Cell<usize>>),
}

impl Words {
    pub(super) fn zeroed(word_count: usize) -> Self {
        match word_count {
            0 => Self::Empty,
            1 => Self::Inline(Cell::new(0)),
            _ => Self::Heap((0..word_count).map(|_| Cell::new(0)).collect()),
        }
    }

    pub(super) fn as_slice(&self) -> &[Cell<usize>] {
        use std::slice::from_ref;
        match self {
            Self::Empty => &[],
            Self::Inline(word) => from_ref(word),
            Self::Heap(words) => words,
        }
    }
}

impl Bitmap {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        let word_count = capacity.div_ceil(WORD_BITS);
        Self {
            words: UnsafeCell::new(Words::zeroed(word_count)),
            summary: UnsafeCell::new(
                (word_count > 1).then(|| Box::new(Self::with_capacity(word_count))),
            ),
            capacity: Cell::new(capacity),
            len: Cell::new(0),
            cursor: Cell::new(0),
            _thread: ThreadBound::NEW,
        }
    }

    pub(super) fn insert(&self, index: usize) -> bool {
        if index >= self.capacity.get() {
            return false;
        }
        let word_index = index / WORD_BITS;
        let mask = 1usize << (index % WORD_BITS);
        let word = &self.words()[word_index];
        let current = word.get();
        if current & mask != 0 {
            return false;
        }
        word.set(current | mask);
        if current == 0
            && let Some(summary) = self.summary()
        {
            summary.insert(word_index);
        }
        self.len.set(self.len.get() + 1);
        true
    }

    pub(super) fn remove(&self, index: usize) -> bool {
        if index >= self.capacity.get() {
            return false;
        }
        let word_index = index / WORD_BITS;
        let mask = 1usize << (index % WORD_BITS);
        let word = &self.words()[word_index];
        let current = word.get();
        if current & mask == 0 {
            return false;
        }
        let next = current & !mask;
        word.set(next);
        if next == 0
            && let Some(summary) = self.summary()
        {
            summary.remove(word_index);
        }
        self.len.set(self.len.get() - 1);
        true
    }

    pub(super) fn pop_next(&self) -> Option<usize> {
        if self.len.get() == 0 {
            return None;
        }
        let start = self.cursor.get();
        let start_word = start / WORD_BITS;
        let start_bit = start % WORD_BITS;
        let words = self.words();
        let first = words[start_word].get() & (usize::MAX << start_bit);
        if first != 0 {
            return Some(self.take_bit(start_word, first));
        }
        if let Some(summary) = self.summary() {
            if let Some(word) = summary.find_at_or_after(start_word + 1) {
                return Some(self.take_bit(word, words[word].get()));
            }
            if let Some(word) = summary.find_at_or_after(0)
                && word < start_word
            {
                return Some(self.take_bit(word, words[word].get()));
            }
        }
        let low_mask = (1usize << start_bit).wrapping_sub(1);
        let last = words[start_word].get() & low_mask;
        debug_assert!(last != 0);
        Some(self.take_bit(start_word, last))
    }

    pub(super) fn find_at_or_after(&self, start: usize) -> Option<usize> {
        if self.len.get() == 0 || start >= self.capacity.get() {
            return None;
        }
        let word_index = start / WORD_BITS;
        let bits = self.words()[word_index].get() & (usize::MAX << (start % WORD_BITS));
        if bits != 0 {
            return Some(word_index * WORD_BITS + bits.trailing_zeros() as usize);
        }
        let next_word = self.summary()?.find_at_or_after(word_index + 1)?;
        let bits = self.words()[next_word].get();
        debug_assert!(bits != 0);
        Some(next_word * WORD_BITS + bits.trailing_zeros() as usize)
    }

    fn take_bit(&self, word_index: usize, bits: usize) -> usize {
        let bit = bits.trailing_zeros() as usize;
        let index = word_index * WORD_BITS + bit;
        let word = &self.words()[word_index];
        let next = word.get() & !(1usize << bit);
        word.set(next);
        if next == 0
            && let Some(summary) = self.summary()
        {
            summary.remove(word_index);
        }
        self.len.set(self.len.get() - 1);
        self.cursor.set(if index + 1 == self.capacity.get() {
            0
        } else {
            index + 1
        });
        index
    }

    fn summary(&self) -> Option<&Bitmap> {
        unsafe { &*self.summary.get() }.as_deref()
    }

    fn words(&self) -> &[Cell<usize>] {
        unsafe { &*self.words.get() }.as_slice()
    }
}
