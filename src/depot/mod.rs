pub mod arena;
pub mod array;
pub mod capacity;
pub mod heap;
pub mod index;
pub mod map;
pub mod queue;

pub mod raw;

/// A closed family of exact ownership and state predicates retained by raw proofs.
pub trait Predicate: raw::Sealed {}
