//! Allocation accounting tied to a stable generative lifetime root.

pub use domain::{
    Domain,
    budget::{BindError, Budget, Charge},
    errors::AllocError,
    reuse::Reuse,
};
pub(in crate::buffer) use shared::state::State;
pub use shared::{Shared, exact::Exact};

pub mod bound;
pub mod copy;
pub mod staging;
pub mod write;

mod domain;
mod shared;
