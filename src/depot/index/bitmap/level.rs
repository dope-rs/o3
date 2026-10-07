use std::num::NonZero;

use crate::depot::index::{bitmap, bitmap::entry::Row};

pub(in crate::depot::index::bitmap) struct Level<'a, W: bitmap::Width> {
    words: &'a [bitmap::Word<W>],
    bits: usize,
}

impl<'a, W: bitmap::Width> Level<'a, W> {
    pub(in crate::depot::index::bitmap) fn new<S: crate::depot::raw::Words<W>>(
        source: &'a S,
        plane: S::Plane,
    ) -> Self {
        Self {
            words: source.words(plane),
            bits: source.shape().bits(),
        }
    }

    pub(in crate::depot::index::bitmap) fn find(
        &self,
        mut start: usize,
    ) -> Option<(Found<'_, Self, W>, W)> {
        const {
            assert!(size_of::<Option<Found<'_, Self, W>>>() == 2 * size_of::<usize>());
        }
        let mut count = NonZero::new(self.bits)?;
        let mut offset = 0;
        let mut shift = 0;
        let radix = NonZero::<usize>::MIN.saturating_add(W::BITS as usize - 1);
        loop {
            if start >= count.get() {
                return None;
            }
            let mut index = start / radix.get();
            // SAFETY: the checked position belongs to this exact level; each
            // preceding level contributes exactly its word count to offset.
            let word = unsafe { self.words.get_unchecked(offset + index) };
            let mut current = word.value.get();
            let bits = (current & (!W::ZERO << (start % radix.get()))).offset();
            if let Some(mut bits) = NonZero::new(bits) {
                loop {
                    if shift == 0 {
                        let position = index * radix.get() + bits.trailing_zeros() as usize;
                        // SAFETY: descent retains this exact nonempty leaf and
                        // its current word without intervening mutation.
                        return Some((unsafe { Row::found(self, position) }, current));
                    }
                    index = index * radix.get() + bits.trailing_zeros() as usize;
                    offset -= ((self.bits - 1) >> shift) + 1;
                    shift -= W::BITS.trailing_zeros();
                    // SAFETY: offset subtracts this preceding level's exact length;
                    // the set summary bit proves this exact child is nonempty.
                    let word = unsafe { self.words.get_unchecked(offset + index) };
                    current = word.value.get();
                    bits = unsafe { NonZero::new_unchecked(current.offset()) };
                }
            }
            let words = count.div_ceil(radix);
            if words.get() == 1 {
                return None;
            }
            offset += words.get();
            count = words;
            start = index + 1;
            shift += W::BITS.trailing_zeros();
        }
    }

    /// # Safety
    /// The index belongs to this level's exact retained source and plane.
    unsafe fn update<const SET: bool>(&self, index: usize) {
        // SAFETY: the retained row supplies this exact first leaf position.
        let word = unsafe { self.words.get_unchecked(index / W::BITS as usize) };
        // SAFETY: observation and update retain this source without intervening mutation.
        unsafe { self.write::<SET>(index, word.value.get()) };
    }

    /// # Safety
    /// `index` belongs to this exact source and `current` is its unchanged leaf word.
    pub(in crate::depot::index::bitmap) unsafe fn write<const SET: bool>(
        &self,
        mut index: usize,
        mut current: W,
    ) {
        let mut words = self.words;
        // SAFETY: a retained row is strictly below bits, so the domain is nonempty.
        let mut count = unsafe { NonZero::new_unchecked(self.bits) };
        let radix = NonZero::<usize>::MIN.saturating_add(W::BITS as usize - 1);
        count = count.div_ceil(radix);
        loop {
            let offset = index / W::BITS as usize;
            let word = unsafe { words.get_unchecked(offset) };
            // SAFETY: both sealed primitive widths represent the bit value one.
            let mask = unsafe { W::from_offset(1) } << (index % W::BITS as usize);
            let next = if SET { current | mask } else { current & !mask };
            word.value.set(next);
            let stable = if SET {
                current != W::ZERO
            } else {
                next != W::ZERO
            };
            if stable || count.get() == 1 {
                return;
            }
            words = unsafe { words.get_unchecked(count.get()..) };
            count = count.div_ceil(radix);
            index = offset;
            // SAFETY: the exact summary hierarchy contains this parent word;
            // prior propagation changed only its child, not the parent itself.
            current = unsafe { words.get_unchecked(index / W::BITS as usize) }
                .value
                .get();
        }
    }
}

impl<W: bitmap::Width, S: crate::depot::raw::Words<W>, P> Row<S, P, (), W> {
    pub fn update<const SET: bool>(&self, plane: S::Plane) {
        let level = Level {
            words: S::row(self.borrow(), plane),
            bits: self.source.shape().bits(),
        };
        // SAFETY: this retained row bounds its position in the exact selected plane.
        unsafe { level.update::<SET>(self.index()) };
    }
}

type Found<'l, S, W> = Row<&'l S, (), bitmap::Found, W>;
