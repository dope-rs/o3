use crate::{buffer::storage::alloc, mem::credit};

/// Exact copy failure after the byte capacity has been proven.
pub type Error = alloc::Error<credit::Error>;
