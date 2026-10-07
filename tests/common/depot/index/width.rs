use std::num::NonZero;

use o3::depot::{self, index::bitmap};

/// The inline route layout with eight leaves, one summary and a byte cursor.
pub struct Inline<const BITS: u32 = 256> {
    words: [bitmap::Word<u32>; 9],
    pub cursor: u8,
}

impl<const BITS: u32> Default for Inline<BITS> {
    fn default() -> Self {
        const {
            assert!(BITS > 224 && BITS <= 256);
        }
        Self {
            words: [const { bitmap::Word::zero() }; 9],
            cursor: 0,
        }
    }
}

// SAFETY: construction initializes this exact immutable domain to empty; only
// the bitmap kernel can access the words and their summary afterward.
unsafe impl<const BITS: u32> depot::raw::Words<u32> for Inline<BITS> {
    type Plane = ();
    type Size = NonZero<u32>;

    fn shape(&self) -> bitmap::Shape<Self::Size, u32> {
        const { bitmap::Shape::<NonZero<u32>, u32>::new(NonZero::<u32>::MIN.saturating_add(BITS - 1)) }
    }

    fn words(&self, (): ()) -> &[bitmap::Word<u32>] {
        &self.words
    }
}

// SAFETY: this owner exposes no independent access to its private plane.
unsafe impl<const BITS: u32> depot::raw::Bitmap<u32> for Inline<BITS> {}
