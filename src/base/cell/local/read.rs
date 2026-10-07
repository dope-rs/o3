use crate::base::cell::region::{self, Brand};

/// Scoped shared access to one generative local region.
#[repr(transparent)]
pub struct Read<'borrow, B: Brand> {
    pub(in crate::base::cell) region: &'borrow region::Token<B>,
}

impl<'borrow, B: Brand> Read<'borrow, B> {
    pub fn new(region: &'borrow region::Token<B>) -> Self {
        Self { region }
    }
}
