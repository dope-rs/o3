mod error;
pub mod total;

pub use error::Error;

/// Two observed values whose sum is outside their exact domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Overflow<T = usize> {
    pub(in crate::buffer::segments) current: T,
    pub(in crate::buffer::segments) added: T,
}

impl<T> Overflow<T> {
    pub(in crate::buffer::segments) const fn new(current: T, added: T) -> Self {
        Self { current, added }
    }
}
