use std::marker;

use crate::depot::queue::round::{self, phase};

/// One group resolved against its exact owner.
pub struct Group<'a, T, const G: usize, const N: usize = 0> {
    pub(in crate::depot::queue::round) owner: &'a round::Lanes<T, G, N>,
    pub(in crate::depot::queue::round) index: usize,
    pub(super) item: marker::PhantomData<fn(T) -> T>,
}

impl<'a, T, const G: usize, const N: usize> Group<'a, T, G, N> {
    pub(in crate::depot::queue::round) fn select(
        &self,
    ) -> Option<round::Location<&'a round::Lanes<T, G, N>, phase::Head>> {
        // SAFETY: Group retains the owner and admitted group index.
        let ring = unsafe { self.owner.groups.get_unchecked(self.index) };
        let lane = ring.first()?;
        let track = unsafe { self.owner.block.tail().get_unchecked(lane.get()) };
        // SAFETY: this group's live lane is non-empty until the immediate commit.
        let index = unsafe { track.entries.front_key() };
        Some(round::Location::new(
            self.owner,
            round::Position { lane, index },
        ))
    }

    pub fn active(&self) -> Option<round::Snapshot<&'a round::Lanes<T, G, N>, round::Turn>> {
        let entry = self.select()?;
        Some(round::Snapshot::new(entry.owner, entry.position))
    }

    pub fn next(&self) -> Option<(usize, &'a T)> {
        let head = self.select()?;
        let lane = head.lane();
        // SAFETY: group just admitted its exact active front; no callback intervenes.
        let value = round::Location::take_active(unsafe { crate::depot::raw::Proof::new(head) });
        Some((lane, value))
    }
}
