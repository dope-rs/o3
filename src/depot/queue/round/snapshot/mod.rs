use std::marker;

use crate::depot::queue::round::{self, phase};

pub(super) mod front;
pub(super) mod place;
pub(super) mod turn;

/// An admitted position whose shared membership must be resolved at each use.
#[derive(Clone, Copy, Debug)]
pub struct Snapshot<O, P> {
    pub(in crate::depot::queue::round) owner: O,
    pub(in crate::depot::queue::round) position: round::Position,
    phase: marker::PhantomData<P>,
}

impl<O, P> Snapshot<O, P> {
    pub(in crate::depot::queue::round) fn new(owner: O, position: round::Position) -> Self {
        Self {
            owner,
            position,
            phase: marker::PhantomData,
        }
    }

    pub const fn index(&self) -> usize {
        self.position.index.get()
    }

    pub const fn lane(&self) -> usize {
        self.position.lane.get()
    }
}

impl<'a, T, const G: usize, const N: usize, P> Snapshot<&'a round::Lanes<T, G, N>, P> {
    pub(in crate::depot::queue::round) fn snapshot(
        &self,
    ) -> round::Location<&'a round::Lanes<T, G, N>> {
        round::Location::new(self.owner, self.position)
    }

    fn resolve(&self) -> Option<round::Location<&'a round::Lanes<T, G, N>, phase::Head>> {
        let entry = self.snapshot();
        if entry.access().track.entries.first() != Some(entry.position.index) {
            return None;
        }
        Some(entry.phase())
    }

    fn turn(&self) -> Option<round::Location<&'a round::Lanes<T, G, N>, phase::Head>> {
        let head = self.resolve()?;
        // SAFETY: the retained FIFO front proves this lane's group remains nonempty.
        let first = unsafe { head.access().group.first_live() };
        (first == self.position.lane).then_some(head)
    }

    pub(in crate::depot::queue::round) fn filter<const FAIR: bool>(
        &self,
        test: impl FnOnce(round::Location<&'a round::Lanes<T, G, N>, phase::Head>) -> bool,
    ) -> Option<round::Location<&'a round::Lanes<T, G, N>, phase::Head>> {
        let entry = if FAIR { self.turn()? } else { self.resolve()? };
        if !test(entry) {
            return None;
        }
        if FAIR { self.turn() } else { self.resolve() }
    }
}
