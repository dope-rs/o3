//! Compact sets retaining their native index domain.

use std::marker;

use crate::{depot, depot::array};

/// At most sixteen members, retaining the native domain through every operation.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Set<I: depot::raw::BitIndex<N>, const N: usize> {
    bits: u16,
    index: marker::PhantomData<fn() -> I>,
}

impl<I: depot::raw::BitIndex<N>, const N: usize> Set<I, N> {
    pub const EMPTY: Self = {
        assert!(N <= u16::BITS as usize);
        Self {
            bits: 0,
            index: marker::PhantomData,
        }
    };

    pub fn insert(&mut self, index: I) {
        self.bits |= 1 << index.index();
    }

    pub fn remove(&mut self, index: I) {
        self.bits &= !(1 << index.index());
    }

    pub fn contains(self, index: I) -> bool {
        self.bits & (1 << index.index()) != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
            index: marker::PhantomData,
        }
    }

    pub const fn intersect(self, other: Self) -> Self {
        Self {
            bits: self.bits & other.bits,
            index: marker::PhantomData,
        }
    }

    pub const fn without(self, other: Self) -> Self {
        Self {
            bits: self.bits & !other.bits,
            index: marker::PhantomData,
        }
    }

    pub const fn complement(self) -> Self {
        Self {
            bits: !self.bits & ((1_u32 << N) - 1) as u16,
            index: marker::PhantomData,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub const fn len(self) -> array::Len<N> {
        array::Len(self.bits.count_ones())
    }

    pub fn first(self) -> Option<I> {
        let index = self.bits.lowest_one()?;
        // SAFETY: insertion admits only I positions below N; set operations
        // preserve that domain, and lowest_one returns only a set-bit index.
        Some(unsafe { I::from_index(index as usize) })
    }
}
