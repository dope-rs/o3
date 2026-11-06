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
        let lane = self.slot().linked_lane();
        if lane == Some(self.position.lane) {
            return round::Insertion::Present;
        }
        let round::Lanes { block, groups } = self.owner;
        // SAFETY: this snapshot admits both indices to these same-owner fields;
        // membership was just resolved without an intervening callback.
        unsafe { round::lanes::push_from(block, groups, self.position, lane) };
        round::Insertion::Inserted
    }

    /// Moves this resident to its destination's tail.
    pub fn push(&self) {
        let lane = self.slot().linked_lane();
        let round::Lanes { block, groups } = self.owner;
        // SAFETY: this snapshot admits both indices to these same-owner fields;
        // membership was just resolved without an intervening callback.
        unsafe { round::lanes::push_from(block, groups, self.position, lane) };
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
