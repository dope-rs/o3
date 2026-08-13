use crate::collections::batch::{self, set};

pub trait Set<I> {
    /// # Safety
    /// `index` must be within this set's capacity and absent from both batches.
    unsafe fn insert_unchecked(&self, index: I);

    /// # Safety
    /// The index must have been removed here and remain absent.
    unsafe fn restore_unchecked(&self, index: I) {
        unsafe { self.insert_unchecked(index) };
    }

    /// # Safety
    /// `index` must be within this set's capacity and present in exactly one batch.
    unsafe fn remove_unchecked(&self, index: I);
}

impl<I: set::DenseIndex> Set<I> for set::Set<I> {
    unsafe fn insert_unchecked(&self, index: I) {
        let index = index.into_usize();
        let set = &self.raw;
        debug_assert!(index < set.capacity.get());

        let side = set.active.get();
        let side_index = side.index();
        let word_index = index / batch::ENTRIES_PER_WORD;
        let shift = (index % batch::ENTRIES_PER_WORD) * 2;
        let pair = 3usize << shift;
        let bit = 1usize << (shift + side_index);
        let word = set.words.word(word_index);
        let current = word.get();
        debug_assert_eq!(current & pair, 0);

        word.set(current | bit);
        if current & batch::SIDE_MASKS[side_index] == 0 {
            set.summaries[side_index].insert_absent(word_index);
        }
        set.len[side_index].set(set.len[side_index].get() + 1);
    }

    unsafe fn remove_unchecked(&self, index: I) {
        let index = index.into_usize();
        let set = &self.raw;
        debug_assert!(index < set.capacity.get());

        let word_index = index / batch::ENTRIES_PER_WORD;
        let shift = (index % batch::ENTRIES_PER_WORD) * 2;
        let pair = 3usize << shift;
        let word = set.words.word(word_index);
        let current = word.get();
        let removed = current & pair;
        debug_assert_ne!(removed, 0);

        let next = current & !pair;
        word.set(next);
        for (side, side_mask) in batch::SIDE_MASKS.into_iter().enumerate() {
            if removed & side_mask != 0 {
                if next & side_mask == 0 {
                    set.summaries[side].remove(word_index);
                }
                set.len[side].set(set.len[side].get() - 1);
            }
        }
    }
}
