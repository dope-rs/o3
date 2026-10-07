//! Byte extents representable by a signed 32-bit count.

pub mod raw;

pub use overflow::Overflow;
pub use rejected::Rejected;
pub(in crate::buffer) use source::Source;
pub use value::Value;

/// Largest admitted byte extent.
pub const MAX: usize = i32::MAX as usize;

mod overflow;
mod rejected;

mod value;

mod source;
