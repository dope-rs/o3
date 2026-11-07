//! Byte extents representable by a signed 32-bit count.

pub mod raw;

pub use rejected::Rejected;
pub(in crate::buffer) use source::Source;
pub use value::Value;

/// Largest admitted byte extent.
pub const MAX: usize = i32::MAX as usize;

/// Byte extent beyond the signed 32-bit domain.
pub type Overflow = crate::base::num::bounded::Overflow<{ MAX as u32 }>;

const _: () = assert!(size_of::<Overflow>() == size_of::<usize>());

mod rejected;

mod value;

mod source;
