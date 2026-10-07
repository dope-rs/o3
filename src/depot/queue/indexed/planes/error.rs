use crate::{alloc, depot::heap};

#[derive(Debug)]
pub enum Error<C> {
    Values { capacity: C, error: alloc::Error },
    Heap(heap::Error<C>),
}
