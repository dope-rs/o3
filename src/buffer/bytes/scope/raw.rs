use std::{num::NonZero, ops};

use crate::base::{marker::ThreadBound, num::bounded};

/// # Safety
/// Self must be invariant in `'id` and bind its bounds to that exact source.
/// Every matching view keeps its origin and extent, with `start <= end <= source.len()`.
pub unsafe trait Select<'id>: Sized {
    fn bounds(self) -> ops::Range<usize>;
}

/// One exclusive source with an admitted first byte, fixed window and trailing sample.
pub struct Window<'a, const NUMBER: usize, const SAMPLE: usize> {
    bytes: &'a mut [u8],
    at: NonZero<usize>,
    _thread: ThreadBound,
}

impl<'a, const NUMBER: usize, const SAMPLE: usize> Window<'a, NUMBER, SAMPLE> {
    /// Admits the complete geometry before exposing any mutable part.
    pub fn try_at(bytes: &'a mut [u8], at: NonZero<usize>) -> Option<Self> {
        const { assert!(NUMBER <= usize::MAX - SAMPLE, "bounds") };
        if bytes.get(at.get()..)?.len() < NUMBER + SAMPLE {
            return None;
        }
        Some(Self {
            bytes,
            at,
            _thread: ThreadBound::NEW,
        })
    }

    /// Borrows disjoint parts while retaining the original source and extent.
    pub fn parts(&mut self) -> (&mut u8, &mut [u8; NUMBER], &[u8; SAMPLE]) {
        let source = self.bytes.as_mut_ptr();
        // SAFETY: admission retains the original exclusive slice. The nonzero
        // offset separates its first byte from the fixed window, and the sample
        // follows that window. All three returned borrows end with this loan.
        unsafe {
            (
                &mut *source,
                &mut *source.add(self.at.get()).cast::<[u8; NUMBER]>(),
                &*source.add(self.at.get() + NUMBER).cast::<[u8; SAMPLE]>(),
            )
        }
    }

    /// Splits after a retained position within the admitted fixed window.
    pub fn split<const MAX: u64>(
        self,
        position: bounded::U64<0, MAX>,
    ) -> (&'a mut [u8], &'a mut [u8]) {
        const { assert!((MAX as u128) < NUMBER as u128, "bounds") };
        let len = self.at.get() + position.get() as usize + 1;
        // SAFETY: the retained window admits every position through NUMBER,
        // so this endpoint is within the same exclusively owned byte slice.
        unsafe { self.bytes.split_at_mut_unchecked(len) }
    }
}
