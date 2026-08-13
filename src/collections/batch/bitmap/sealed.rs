use std::cell;

use crate::collections;

const WORD_BITS: usize = usize::BITS as usize;

pub(in crate::collections::batch) struct Tree {
    words: Words,
    summary: Option<Box<Tree>>,
    capacity: cell::Cell<usize>,
    len: cell::Cell<usize>,
    cursor: cell::Cell<usize>,
    _thread: crate::ThreadBound,
}

pub(in crate::collections::batch) enum Words {
    Empty,
    Inline(cell::Cell<usize>),
    Heap(Vec<cell::Cell<usize>>),
}

impl Words {
    pub(in crate::collections::batch) fn try_zeroed(
        word_count: usize,
    ) -> Result<Self, collections::AllocationError> {
        match word_count {
            0 => Ok(Self::Empty),
            1 => Ok(Self::Inline(cell::Cell::new(0))),
            _ => {
                let mut words: Vec<cell::Cell<usize>> =
                    collections::VecExt::try_vec_with_capacity(word_count)?;
                for _ in 0..word_count {
                    words.push(cell::Cell::new(0));
                }
                Ok(Self::Heap(words))
            }
        }
    }

    pub(in crate::collections::batch) fn as_slice(&self) -> &[cell::Cell<usize>] {
        use std::slice::from_ref;
        match self {
            Self::Empty => &[],
            Self::Inline(word) => from_ref(word),
            Self::Heap(words) => words,
        }
    }

    pub(in crate::collections::batch) fn word(&self, index: usize) -> &cell::Cell<usize> {
        let words = self.as_slice();
        debug_assert!(index < words.len());
        unsafe { words.get_unchecked(index) }
    }
}

impl Tree {
    pub(in crate::collections::batch) fn try_with_capacity(
        capacity: usize,
    ) -> Result<Self, collections::AllocationError> {
        use crate::ThreadBound;
        let word_count = capacity.div_ceil(WORD_BITS);
        let words = Words::try_zeroed(word_count)?;
        let summary = if word_count > 1 {
            Some(collections::BoxExt::try_box(Self::try_with_capacity(
                word_count,
            )?)?)
        } else {
            None
        };
        Ok(Self {
            words,
            summary,
            capacity: cell::Cell::new(capacity),
            len: cell::Cell::new(0),
            cursor: cell::Cell::new(0),
            _thread: ThreadBound::NEW,
        })
    }

    pub(in crate::collections::batch) fn insert(&self, index: usize) -> bool {
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

    pub(in crate::collections::batch) unsafe fn insert_unchecked(&self, index: usize) {
        debug_assert!(index < self.capacity.get());
        let word_index = index / WORD_BITS;
        let mask = 1usize << (index % WORD_BITS);
        let word = self.words.word(word_index);
        let current = word.get();
        debug_assert_eq!(current & mask, 0);
        word.set(current | mask);
        if current == 0
            && let Some(summary) = self.summary()
        {
            unsafe { summary.insert_unchecked(word_index) };
        }
        self.len.set(self.len.get() + 1);
    }

    pub(in crate::collections::batch) fn remove(&self, index: usize) -> bool {
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

    pub(in crate::collections::batch) fn pop_next(&self) -> Option<usize> {
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

    pub(in crate::collections::batch) fn find_at_or_after(&self, start: usize) -> Option<usize> {
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

    fn summary(&self) -> Option<&Tree> {
        self.summary.as_deref()
    }

    fn words(&self) -> &[cell::Cell<usize>] {
        self.words.as_slice()
    }
}
