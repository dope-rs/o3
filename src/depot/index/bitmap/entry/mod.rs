mod occupied;

/// Failed occupancy admission retaining the exact checked source and index.
pub struct Absent;
pub use occupied::Occupied;

mod row;
pub use row::Row;
/// A completed removal retaining its original bounded row and source plane.
pub struct Removed;

mod found;
pub(in crate::depot::index::bitmap) use found::Found;
