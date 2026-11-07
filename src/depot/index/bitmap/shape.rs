use std::{marker, num::NonZero};

use crate::depot::index::bitmap;

/// Exact storage shape retaining whether its source domain is nonempty.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Shape<N = u32, W = usize> {
    pub(in crate::depot::index::bitmap) bits: N,
    words: N,
    width: marker::PhantomData<fn(W) -> W>,
}

impl<W: bitmap::Width> Shape<u32, W> {
    pub(in crate::depot) const fn capacity(self) -> usize {
        self.bits as usize
    }

    pub const fn new(bits: u32) -> Self {
        let mut words = 0;
        if bits != 0 {
            let shift = W::BITS.trailing_zeros();
            let mut last = (bits - 1) >> shift;
            words = 1;
            while last != 0 {
                words += last + 1;
                last >>= shift;
            }
        }
        Self {
            bits,
            words,
            width: marker::PhantomData,
        }
    }
}

impl<W: bitmap::Width> Shape<NonZero<u32>, W> {
    pub const fn new(bits: NonZero<u32>) -> Self {
        let shape = Shape::<u32, W>::new(bits.get());
        // SAFETY: every nonempty bit domain needs at least its one root word.
        let words = unsafe { NonZero::new_unchecked(shape.words) };
        Self {
            bits,
            words,
            width: marker::PhantomData,
        }
    }
}

impl<N: Copy + Into<u32>, W: bitmap::Width> Shape<N, W> {
    pub fn bits(self) -> usize {
        self.bits.into() as usize
    }

    pub fn words(self) -> usize {
        self.words.into() as usize
    }

    pub fn fill(self, fill: bitmap::Fill) -> impl Iterator<Item = bitmap::Word<W>> {
        let mut bits = self.bits();
        let mut word = 0;
        let mut words = bits.div_ceil(W::BITS as usize);
        std::iter::from_fn(move || {
            if bits == 0 {
                return None;
            }
            if word == words {
                if words == 1 {
                    return None;
                }
                bits = words;
                word = 0;
                words = bits.div_ceil(W::BITS as usize);
            }
            let left = bits - word * W::BITS as usize;
            let value = match fill {
                bitmap::Fill::Empty => 0,
                bitmap::Fill::Full if left >= W::BITS as usize => W::MAX.get(),
                bitmap::Fill::Full => (1usize << left) - 1,
            };
            word += 1;
            // SAFETY: a full word uses W::MAX and a partial word has fewer than W::BITS bits.
            Some(bitmap::Word::new(unsafe { W::from_offset(value) }))
        })
    }
}

const _: () = {
    assert!(size_of::<Shape>() == size_of::<usize>());
    assert!(size_of::<Shape<NonZero<u32>>>() == size_of::<Shape>());
};
