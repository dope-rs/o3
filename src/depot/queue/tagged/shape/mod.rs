use crate::depot::capacity;

/// The exact dimensions that cannot form disjoint compact domains.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<C = capacity::Count, L = capacity::Count> {
    Lanes { capacity: C, lanes: usize },
    Overlap { capacity: C, lanes: L },
}

mod domain;
pub use domain::Domain;
