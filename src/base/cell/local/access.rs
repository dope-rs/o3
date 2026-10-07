use crate::base::cell::{
    local::Read,
    region::{self, Brand},
};

/// Scoped mutable access to one generative local region.
#[repr(transparent)]
pub struct Access<'borrow, B: Brand> {
    pub(in crate::base::cell) region: &'borrow mut region::Token<B>,
}

impl<'borrow, B: Brand> Access<'borrow, B> {
    pub fn new(region: &'borrow mut region::Token<B>) -> Self {
        Self { region }
    }

    pub fn reborrow(&mut self) -> Access<'_, B> {
        Access {
            region: self.region,
        }
    }

    pub fn read(&self) -> Read<'_, B> {
        Read::new(self.region)
    }

    /// Runs one region operation without allowing its token borrow to escape.
    pub fn with_region<R>(self, f: impl for<'a> FnOnce(&'a mut region::Token<B>) -> R) -> R {
        f(self.region)
    }
}
