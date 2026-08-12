use std::process;

use crate::collections::queue::fixed;

const WORD_BITS: usize = 64;

pub struct Fifo {
    pending: fixed::Fifo<u32>,
    occupied: Box<[u64]>,
    capacity: usize,
}

pub struct Front<'a> {
    queue: &'a mut Fifo,
    index: u32,
}

impl Fifo {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            pending: fixed::Fifo::with_capacity(capacity),
            occupied: vec![0; capacity.div_ceil(WORD_BITS)].into_boxed_slice(),
            capacity,
        }
    }

    pub fn schedule(&mut self, index: u32) -> Result<(), u32> {
        let Ok(position) = usize::try_from(index) else {
            return Err(index);
        };
        if position >= self.capacity {
            return Err(index);
        }
        let word_index = position / WORD_BITS;
        let bit = 1_u64 << (position % WORD_BITS);
        let word = &mut self.occupied[word_index];
        if *word & bit != 0 {
            return Ok(());
        }
        *word |= bit;
        if let Err(index) = self.pending.push_back(index) {
            *word &= !bit;
            return Err(index);
        }
        Ok(())
    }

    pub fn front_entry(&mut self) -> Option<Front<'_>> {
        let index = *self.pending.front()?;
        Some(Front { queue: self, index })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

impl Front<'_> {
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn remove(self) {
        let Some(index) = self.queue.pending.pop_front() else {
            process::abort();
        };
        let Ok(position) = usize::try_from(index) else {
            process::abort();
        };
        let word_index = position / WORD_BITS;
        let bit = 1_u64 << (position % WORD_BITS);
        let Some(word) = self.queue.occupied.get_mut(word_index) else {
            process::abort();
        };
        if *word & bit == 0 {
            process::abort();
        }
        *word &= !bit;
    }
}
