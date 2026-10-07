pub mod brand;
pub mod local;
pub mod region;

mod sealed;
mod values;

pub(in crate::base::cell) use sealed::Sealed;
