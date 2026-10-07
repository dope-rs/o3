mod error;

pub mod scope;

pub use error::Error;

/// Validates and consumes at the actual storage owner.
pub trait Consume {
    fn remaining(&self) -> usize;
    fn try_consume(&mut self, amount: usize) -> Result<(), Error>;
    fn consume_max(&mut self, max: usize) -> usize;
}

impl Consume for &mut [u8] {
    fn remaining(&self) -> usize {
        self.len()
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), Error> {
        crate::buffer::raw::Selection::try_new(self, amount)
            .map(crate::buffer::raw::Selection::consume)
    }

    fn consume_max(&mut self, max: usize) -> usize {
        crate::buffer::raw::Selection::new(self, max).take().len()
    }
}
