use std::cell::Cell;

use crate::depot::{
    self,
    queue::round::{
        self, phase,
        values::{Head, Snapshot},
    },
};

/// One lane whose owner alone initializes and links its payloads.
pub struct Lane<'a, T: Copy, const G: usize, const N: usize = 0>(
    pub(super) round::Lane<&'a round::Lanes<Cell<Option<T>>, G, N>>,
);

impl<'a, T: Copy, const G: usize, const N: usize> Lane<'a, T, G, N> {
    fn head(&self) -> Option<Head<'a, T, G, N>> {
        // SAFETY: this private admission invokes no callbacks before its immediate consumer.
        depot::raw::Admit::admit(unsafe { depot::raw::Proof::<_, phase::Ready>::new(self.0) })
            .map(Head)
    }

    pub fn front(&self) -> Option<(Snapshot<'a, T, G, round::Front, N>, T)> {
        let head = self.head()?;
        let value = head.get();
        Some((
            Snapshot(round::Snapshot::new(head.0.owner, head.0.position)),
            value,
        ))
    }

    pub fn pop_front(&self) -> Option<T> {
        Some(self.head()?.pop())
    }
}

impl<'a, T: Copy, const G: usize, const N: usize> depot::raw::Admit
    for depot::raw::Proof<Lane<'a, T, G, N>, phase::Ready>
{
    type Output = Option<Head<'a, T, G, N>>;

    fn admit(self) -> Self::Output {
        self.into_inner().head()
    }
}
