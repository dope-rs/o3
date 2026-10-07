use crate::depot::queue::links;
#[derive(Clone, Copy)]
pub struct Edge {
    index: links::Index,
    previous: u32,
    next: u32,
}

impl Edge {
    pub(in crate::depot) fn new(index: links::Index, previous: u32, next: u32) -> Self {
        Self {
            index,
            previous,
            next,
        }
    }

    pub fn key(self) -> links::Index {
        self.index
    }
}
impl Edge {
    pub(in crate::depot) fn links(self) -> (u32, u32) {
        (self.previous, self.next)
    }
}
