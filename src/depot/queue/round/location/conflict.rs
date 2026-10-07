use crate::depot::queue::{
    links,
    round::{self, Idle, Linked, Location},
};

/// Linked membership and an admitted destination in the same exclusive owner.
#[derive(Debug)]
pub struct Conflict<O> {
    pub(super) member: Location<O, Linked>,
    pub(super) requested: links::Index,
}

impl<'a, T, const G: usize, const N: usize> Conflict<&'a mut round::Lanes<T, G, N>> {
    pub fn index(&self) -> usize {
        self.member.index()
    }
    pub fn lane(&self) -> usize {
        self.requested.get()
    }
    pub fn actual(&self) -> usize {
        self.member.lane()
    }
    pub fn remove(self) -> Location<&'a mut round::Lanes<T, G, N>, Idle<true>> {
        let mut entry = self.member.remove();
        entry.position.lane = self.requested;
        entry.phase()
    }
}
