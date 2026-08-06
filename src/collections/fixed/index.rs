use std::{iter::from_fn, mem::MaybeUninit};

use crate::collections::ClearGuard;

const WORD_BITS: usize = u64::BITS as usize;

/// Fixed-capacity values addressed directly by caller-provided indices.
pub struct Slots<T> {
    occupied: Box<[u64]>,
    values: Box<[MaybeUninit<T>]>,
    len: usize,
}

impl<T> Slots<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        let words = capacity.div_ceil(WORD_BITS);
        Self {
            occupied: vec![0; words].into_boxed_slice(),
            values: Box::<[T]>::new_uninit_slice(capacity),
            len: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.values.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn contains(&self, index: usize) -> bool {
        self.bit(index)
            .is_some_and(|(word, bit)| self.occupied[word] & bit != 0)
    }

    pub fn vacant(&self, index: usize) -> bool {
        index < self.capacity() && !self.contains(index)
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        if !self.contains(index) {
            return None;
        }
        Some(unsafe { self.values.get_unchecked(index).assume_init_ref() })
    }

    pub fn try_insert(&mut self, index: usize, value: T) -> Result<(), T> {
        let Some((word, bit)) = self.bit(index) else {
            return Err(value);
        };
        if self.occupied[word] & bit != 0 {
            return Err(value);
        }
        // SAFETY: `bit` exists only for an in-bounds value index.
        unsafe { self.values.get_unchecked_mut(index).write(value) };
        self.occupied[word] |= bit;
        self.len += 1;
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        let (word, bit) = self.bit(index)?;
        if self.occupied[word] & bit == 0 {
            return None;
        }
        Some(self.take_occupied(index, word, bit))
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.indices()
            .map(|index| unsafe { self.values.get_unchecked(index).assume_init_ref() })
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let occupied = &self.occupied;
        self.values
            .iter_mut()
            .enumerate()
            .filter(|(index, _)| bit_is_set(occupied, *index))
            .map(|(_, value)| unsafe { value.assume_init_mut() })
    }

    fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.occupied
            .iter()
            .copied()
            .enumerate()
            .flat_map(|(word_index, mut word)| {
                from_fn(move || {
                    if word == 0 {
                        return None;
                    }
                    let bit = word.trailing_zeros() as usize;
                    word &= word - 1;
                    Some(word_index * WORD_BITS + bit)
                })
            })
            .take_while(|index| *index < self.capacity())
    }

    pub fn drain_where(&mut self, mut predicate: impl FnMut(&T) -> bool, mut emit: impl FnMut(T)) {
        for word_index in 0..self.occupied.len() {
            let mut candidates = self.occupied[word_index];
            while candidates != 0 {
                let bit = candidates.trailing_zeros() as usize;
                candidates &= candidates - 1;
                let index = word_index * WORD_BITS + bit;
                if predicate(unsafe { self.values.get_unchecked(index).assume_init_ref() }) {
                    emit(self.take_occupied(index, word_index, 1 << bit));
                }
            }
        }
    }

    fn clear_remaining(&mut self) {
        for word_index in 0..self.occupied.len() {
            while self.occupied[word_index] != 0 {
                let bit = self.occupied[word_index].trailing_zeros() as usize;
                let index = word_index * WORD_BITS + bit;
                drop(self.take_occupied(index, word_index, 1 << bit));
            }
        }
    }

    fn take_occupied(&mut self, index: usize, word: usize, bit: u64) -> T {
        debug_assert!(self.occupied[word] & bit != 0);
        self.occupied[word] &= !bit;
        self.len -= 1;
        // SAFETY: the occupancy bit proves this in-bounds slot is initialized;
        // clearing it transfers the value's drop obligation to the caller.
        unsafe { self.values.get_unchecked(index).assume_init_read() }
    }

    fn bit(&self, index: usize) -> Option<(usize, u64)> {
        (index < self.capacity()).then(|| (index / WORD_BITS, 1 << (index % WORD_BITS)))
    }
}

impl<T> Drop for Slots<T> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear_remaining);
    }
}

fn bit_is_set(occupied: &[u64], index: usize) -> bool {
    occupied[index / WORD_BITS] & (1 << (index % WORD_BITS)) != 0
}
