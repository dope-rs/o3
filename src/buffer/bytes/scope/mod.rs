//! Selections tied to their original storage owner.

use std::convert::Infallible;

pub use owner::Owner;
pub use range::Range;
pub use view::View;

mod owner;
mod range;
pub mod raw;
mod view;

/// Selects a byte range while preserving its source's ownership proof.
pub trait Source: Copy {
    type Selection;

    fn select(self, bytes: &[u8]) -> Option<Self::Selection>;
}

impl Source for () {
    type Selection = Infallible;

    fn select(self, _: &[u8]) -> Option<Self::Selection> {
        None
    }
}
