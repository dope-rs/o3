pub mod group;
pub mod indexed;
pub mod links;
pub mod pool;
pub mod resident;
pub mod ring;
pub mod round;
pub mod tagged;

/// Result of removing one in-domain index.
#[derive(Clone, Copy, Eq, PartialEq)]
#[must_use = "the removal outcome must be handled"]
pub enum Removal {
    Removed,
    Absent,
}
