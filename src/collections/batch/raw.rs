use super::{DenseIndex, ENTRIES_PER_WORD, SIDE_MASKS, Set as TypedSet};

pub trait Set<I> {
    /// Inserts an index known to be within capacity and absent from both batches.
    ///
    /// # Safety
    /// `index` must be within this set's capacity and absent from both batches.
    unsafe fn insert_unchecked(&self, index: I);

    /// Restores an index removed from this exact set.
    ///
    /// # Safety
    /// `index` must have been removed from this exact set and remain absent
    /// from both batches.
    unsafe fn restore_unchecked(&self, index: I) {
        unsafe { self.insert_unchecked(index) };
    }

    /// Removes an index known to be within capacity and present in one batch.
    ///
    /// # Safety
    /// `index` must be within this set's capacity and present in exactly one batch.
    unsafe fn remove_unchecked(&self, index: I);
}

impl<I: DenseIndex> Set<I> for TypedSet<I> {
    unsafe fn insert_unchecked(&self, index: I) {
        let index = index.into_usize();
        let set = &self.raw;
        debug_assert!(index < set.capacity.get());

        let side = set.active.get();
        let side_index = side.index();
        let word_index = index / ENTRIES_PER_WORD;
        let shift = (index % ENTRIES_PER_WORD) * 2;
        let pair = 3usize << shift;
        let bit = 1usize << (shift + side_index);
        let word = set.words.word(word_index);
        let current = word.get();
        debug_assert_eq!(current & pair, 0);

        word.set(current | bit);
        if current & SIDE_MASKS[side_index] == 0 {
            unsafe { set.summaries[side_index].insert_unchecked(word_index) };
        }
        set.len[side_index].set(set.len[side_index].get() + 1);
    }

    unsafe fn remove_unchecked(&self, index: I) {
        let index = index.into_usize();
        let set = &self.raw;
        debug_assert!(index < set.capacity.get());

        let word_index = index / ENTRIES_PER_WORD;
        let shift = (index % ENTRIES_PER_WORD) * 2;
        let pair = 3usize << shift;
        let word = set.words.word(word_index);
        let current = word.get();
        let removed = current & pair;
        debug_assert_ne!(removed, 0);

        let next = current & !pair;
        word.set(next);
        for (side, side_mask) in SIDE_MASKS.into_iter().enumerate() {
            if removed & side_mask != 0 {
                if next & side_mask == 0 {
                    set.summaries[side].remove(word_index);
                }
                set.len[side].set(set.len[side].get() - 1);
            }
        }
    }
}
