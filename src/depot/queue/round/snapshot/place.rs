use std::mem;

use crate::depot::queue::round;

/// One resident and destination admitted against the same allocation.
#[derive(Debug)]
pub struct Place;

impl<'a, T, const G: usize, const N: usize> round::Snapshot<&'a round::Lanes<T, G, N>, Place> {
    pub fn slot(&self) -> round::Slot<'a, T, G, N> {
        round::Slot {
            owner: self.owner,
            index: self.position.index,
        }
    }

    /// Preserves existing membership in this destination.
    pub fn set(&self) -> round::Insertion {
        let entry = self.snapshot();
        let lane = entry.access().linked_lane(entry.position.index);
        if lane == Some(entry.position.lane) {
            return round::Insertion::Present;
        }
        entry.push_from(lane);
        round::Insertion::Inserted
    }

    /// Moves this resident to its destination's tail.
    pub fn push(&self) {
        let entry = self.snapshot();
        let lane = entry.access().linked_lane(entry.position.index);
        entry.push_from(lane);
    }
}

const _: () = {
    assert!(
        mem::size_of::<round::Snapshot<&'static round::Lanes<(), 1>, Place>>()
            == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<round::Snapshot<&'static round::Lanes<(), 1>, Place>>>()
            == 2 * mem::size_of::<usize>()
    );
};
