mod sealed;
pub mod window;

pub(in crate::buffer) use sealed::Raw;
pub use sealed::Snapshot;
