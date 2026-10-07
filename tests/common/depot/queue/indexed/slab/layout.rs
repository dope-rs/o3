use std::cell::Cell;

use crate::common::Counted;

#[repr(align(128))]
pub struct Aligned<'a>(pub Counted<&'a Cell<Option<usize>>>);
