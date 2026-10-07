use std::cell;

use crate::common::Counted;

#[derive(Debug)]
pub struct Block<'a> {
    pub bytes: [u8; 4096],
    pub drops: Counted<&'a cell::Cell<Option<usize>>>,
}
