/// A requested identity outside this queue's allocation.
#[derive(Debug, PartialEq, Eq)]
pub struct Bounds<I> {
    pub index: I,
    pub capacity: usize,
}
