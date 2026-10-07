use std::cell::Cell;

use crate::depot::{
    self,
    queue::round::{
        self,
        values::{Head, Snapshot},
    },
};

/// One fair group whose live membership proves its payloads initialized.
pub struct Group<'a, T: Copy, const G: usize, const N: usize = 0>(
    pub(super) round::Group<'a, Cell<Option<T>>, G, N>,
);

impl<'a, T: Copy, const G: usize, const N: usize> Group<'a, T, G, N> {
    pub fn active(&self) -> Option<(Snapshot<'a, T, G, round::Turn, N>, T)> {
        let head = Head(self.0.select()?);
        let value = head.get();
        Some((
            Snapshot(round::Snapshot::new(head.0.owner, head.0.position)),
            value,
        ))
    }

    pub fn next(&self) -> Option<T> {
        let head = Head(self.0.select()?);
        // SAFETY: selection retains this exact active front until immediate consumption.
        Some(Head::consume(unsafe { depot::raw::Proof::new(head) }))
    }
}
