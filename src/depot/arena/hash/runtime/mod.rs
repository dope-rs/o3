//! Owned resident tables for keys that must outlive an individual borrow.
//! Generational slab handles retain a distinct [`Domain`] for resident hash identities.

pub use domain::Domain;

/// The retained slot is a member of its resident hash index.
pub enum Indexed {}
pub use map::Map;

pub mod entry;

mod domain;
mod map;
