use std::mem;

use crate::{
    base::num::bounded,
    depot::queue::{links, round},
};

/// A resident resolved against one exact allocation.
pub struct Slot<'a, T, const G: usize, const N: usize = 0> {
    pub(in crate::depot::queue::round) owner: &'a round::Lanes<T, G, N>,
    pub(in crate::depot::queue::round) index: links::Index,
}

impl<'a, T, const G: usize, const N: usize> Slot<'a, T, G, N> {
    pub fn get(&self) -> &'a T {
        // SAFETY: this slot retains the allocation against which its index was admitted.
        unsafe {
            &self
                .owner
                .block
                .as_ref()
                .get_unchecked(self.index.get())
                .value
        }
    }

    pub const fn index(&self) -> usize {
        self.index.get()
    }

    pub fn is_linked(&self) -> bool {
        let node = unsafe { self.owner.block.as_ref().get_unchecked(self.index.get()) };
        self.index.is_linked(&node.hook)
    }

    /// Removes the resident from its current lane, if any.
    pub fn clear(self) -> &'a T {
        let node = unsafe { self.owner.block.as_ref().get_unchecked(self.index.get()) };
        if self.index.is_linked(&node.hook) {
            let lane = unsafe { self.owner.block.values().get_unchecked(self.index.get()) }.get();
            return round::Location::<_, round::Linked>::new(
                self.owner,
                round::Position {
                    lane,
                    index: self.index,
                },
            )
            .remove()
            .get();
        }
        self.get()
    }

    /// Admits a destination while retaining this resident's original owner.
    pub fn try_lane(
        &self,
        index: usize,
    ) -> Result<round::Snapshot<&'a round::Lanes<T, G, N>, round::Place>, round::Bounds> {
        let lane = round::Lane::try_new(self.owner, index)?;
        Ok(round::Snapshot::new(
            lane.owner,
            round::Position {
                lane: lane.index,
                index: self.index,
            },
        ))
    }

    /// Keeps the allocation's static lane extent through destination selection.
    pub fn lane<const MAX: u32>(
        &self,
        index: bounded::U32<0, MAX>,
    ) -> round::Snapshot<&'a round::Lanes<T, G, N>, round::Place> {
        let lane = self.owner.lane(index);
        round::Snapshot::new(
            lane.owner,
            round::Position {
                lane: lane.index,
                index: self.index,
            },
        )
    }
}

const _: () = assert!(mem::size_of::<Slot<'static, (), 1>>() == 2 * mem::size_of::<usize>());
