use std::marker;

use crate::depot::index::bitmap;

pub(in crate::depot::index::bitmap) struct Init<W> {
    bits: usize,
    word: usize,
    words: usize,
    fill: bitmap::Fill,
    width: marker::PhantomData<fn(W) -> W>,
}

impl<W: bitmap::Width> Init<W> {
    pub(in crate::depot::index::bitmap) const fn new(bits: usize, fill: bitmap::Fill) -> Self {
        Self {
            bits,
            word: 0,
            words: bits.div_ceil(W::BITS as usize),
            fill,
            width: marker::PhantomData,
        }
    }
}

impl<W: bitmap::Width> Iterator for Init<W> {
    type Item = bitmap::Word<W>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.bits == 0 {
            return None;
        }
        if self.word == self.words {
            if self.words == 1 {
                return None;
            }
            self.bits = self.words;
            self.word = 0;
            self.words = self.bits.div_ceil(W::BITS as usize);
        }
        let left = self.bits - self.word * W::BITS as usize;
        let value = match self.fill {
            bitmap::Fill::Empty => 0,
            bitmap::Fill::Full if left >= W::BITS as usize => W::MAX.get(),
            bitmap::Fill::Full => (1usize << left) - 1,
        };
        self.word += 1;
        // SAFETY: a full word uses W::MAX and a partial word has fewer than W::BITS bits.
        Some(bitmap::Word::new(unsafe { W::from_offset(value) }))
    }
}
