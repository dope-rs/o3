/// An index outside this owner's physical allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub index: usize,
    pub capacity: usize,
}
