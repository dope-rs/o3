//! One-word identities for completion sources with narrow echo fields.
//!
//! Unlike [`super::Arena`], this adds no fields, branches, or lookup work.

pub mod raw;
mod sealed;

pub use sealed::{Arena, Drain, Echo, Key, Lease, Reservation, Resolved, Slots};
